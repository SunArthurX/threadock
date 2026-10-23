//! ZCode 写回器：`~/.zcode/cli/db/db.sqlite`（session/message/part 三表）。
//!
//! 纯增量 INSERT（新 `sess_<uuid>`），不动存量行。message/part 的 sequence
//! 显式填充（真实库另有 autofill 触发器，显式值两边都成立）。

use crate::transcript::build_transcript;
use crate::{
    effective_cwd, ts_ms, HandoffError, HandoffInput, HandoffOptions, HandoffOutcome, HandoffResult,
};
use ch_domain::Role;
use rusqlite::Connection;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

/// 打开目标库（必须已存在；不上锁时等待 5s 再失败）。
fn open_target(path: &std::path::Path) -> HandoffResult<Connection> {
    if !path.exists() {
        return Err(HandoffError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "target store not found: {} (目标 Agent 是否安装/初始化？)",
                path.display()
            ),
        )));
    }
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    Ok(conn)
}

/// project_id：`/Users/x/proj` → `proj_Users-x-proj`（对齐真实库格式）。
fn project_id(cwd: &str) -> String {
    let encoded: String = cwd
        .trim_start_matches('/')
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("proj_{encoded}")
}

/// 写入一条 ZCode 会话（session + 每 transcript 条目一组 message/part）。
#[allow(clippy::too_many_lines)] // 五个写回器中唯一的三表装配，拆分反而割裂事务
pub fn write(input: &HandoffInput, opts: &HandoffOptions) -> HandoffResult<HandoffOutcome> {
    let items = build_transcript(input, opts);
    if items.is_empty() {
        return Err(HandoffError::Empty);
    }

    let session_id = format!("sess_{}", Uuid::new_v4());
    let cwd = effective_cwd(input, opts);
    let db_path: PathBuf = crate::resolve_dest(crate::HandoffTarget::ZCode, opts);
    let started = input.started_at();
    let now_ms = ts_ms(ch_domain::now_utc());
    let created_ms = ts_ms(started);

    if opts.dry_run {
        return Ok(HandoffOutcome {
            target: "zcode".into(),
            conversation_id: input.conversation.id.clone(),
            title: input.title().to_string(),
            session_id,
            destination: db_path.to_string_lossy().into_owned(),
            items_written: items.len(),
            dry_run: true,
        });
    }

    let conn = open_target(&db_path)?;
    crate::ensure_tables(&conn, &["session", "message", "part"])?;
    let tx = conn.unchecked_transaction()?;

    {
        let title = input.title();
        let mut stmt = tx.prepare(
            "INSERT INTO session (id, project_id, workspace_id, parent_id, slug, directory, \
             path, title, version, time_created, time_updated) \
             VALUES (?1, ?2, NULL, '', ?3, ?4, ?4, ?5, '', ?6, ?7)",
        )?;
        stmt.execute(rusqlite::params![
            session_id,
            project_id(&cwd),
            session_id,
            cwd,
            title,
            created_ms,
            now_ms,
        ])?;
    }

    for (idx, item) in items.iter().enumerate() {
        let msg_id = format!("msg_handoff_{}_{}", idx, Uuid::new_v4().simple());
        let part_id = format!("part_handoff_{}", Uuid::new_v4().simple());
        let item_ms = ts_ms(item.at);
        let role = if item.role == Role::Assistant {
            "assistant"
        } else {
            "user"
        };
        {
            let mut stmt = tx.prepare(
                "INSERT INTO message (id, session_id, time_created, time_updated, data, sequence) \
                 VALUES (?1, ?2, ?3, ?3, ?4, ?5)",
            )?;
            stmt.execute(rusqlite::params![
                msg_id,
                session_id,
                item_ms,
                serde_json::json!({ "role": role }).to_string(),
                idx as i64,
            ])?;
        }
        {
            let data = serde_json::json!({
                "type": "text",
                "text": item.text,
                "time": { "start": item_ms, "end": item_ms },
            })
            .to_string();
            let mut stmt = tx.prepare(
                "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data, sequence) \
                 VALUES (?1, ?2, ?3, ?4, ?4, ?5, 0)",
            )?;
            stmt.execute(rusqlite::params![
                part_id, msg_id, session_id, item_ms, data,
            ])?;
        }
    }

    tx.commit()?;

    Ok(HandoffOutcome {
        target: "zcode".into(),
        conversation_id: input.conversation.id.clone(),
        title: input.title().to_string(),
        session_id,
        destination: db_path.to_string_lossy().into_owned(),
        items_written: items.len(),
        dry_run: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::make_input;

    /// 最小 schema（对齐真实库核心列；新增列在真实库有默认值）。
    const DDL: &str = r"
CREATE TABLE session (
    id TEXT PRIMARY KEY, project_id TEXT NOT NULL DEFAULT '', workspace_id TEXT,
    parent_id TEXT, slug TEXT NOT NULL DEFAULT '', directory TEXT NOT NULL DEFAULT '',
    path TEXT, title TEXT NOT NULL DEFAULT '', version TEXT NOT NULL DEFAULT '',
    share_url TEXT, summary_additions INTEGER, summary_deletions INTEGER,
    summary_files INTEGER, summary_diffs TEXT, revert TEXT, permission TEXT,
    time_created INTEGER NOT NULL DEFAULT 0, time_updated INTEGER NOT NULL DEFAULT 0,
    time_compacting INTEGER, time_archived INTEGER
);
CREATE TABLE message (
    id TEXT PRIMARY KEY, session_id TEXT NOT NULL, time_created INTEGER NOT NULL DEFAULT 0,
    time_updated INTEGER NOT NULL DEFAULT 0, data TEXT NOT NULL, sequence INTEGER
);
CREATE TABLE part (
    id TEXT PRIMARY KEY, message_id TEXT NOT NULL, session_id TEXT NOT NULL,
    time_created INTEGER NOT NULL DEFAULT 0, time_updated INTEGER NOT NULL DEFAULT 0,
    data TEXT NOT NULL, sequence INTEGER
);
";

    fn make_db(dir: &std::path::Path) -> PathBuf {
        let path = dir.join("db.sqlite");
        let conn = Connection::open(&path).expect("db creation failed");
        conn.execute_batch(DDL).expect("ddl failed");
        path
    }

    #[test]
    fn writes_session_and_roundtrips() {
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let db = make_db(dir.path());
        let input = make_input();
        let opts = HandoffOptions {
            dest_override: Some(db.clone()),
            ..HandoffOptions::new("/nonexistent-home")
        };
        let out = write(&input, &opts).expect("write failed");
        assert!(out.session_id.starts_with("sess_"));

        let raw =
            ch_adapter_zcode::parse_session(&db, &out.session_id).expect("roundtrip parse failed");
        assert_eq!(raw.provider, ch_domain::Provider::ZCode);
        assert_eq!(raw.source_conversation_id, out.session_id);
        assert!(raw.messages.len() >= 3, "preamble + dialog");
        assert!(raw.messages.iter().any(|m| m
            .text
            .as_deref()
            .is_some_and(|t| t.contains("[threadock 交接存档]"))));
        assert!(raw
            .messages
            .iter()
            .any(|m| m.text.as_deref().is_some_and(|t| t.contains("继续开发"))));
    }

    #[test]
    fn missing_db_errors() {
        let input = make_input();
        let opts = HandoffOptions {
            dest_override: Some(PathBuf::from("/nonexistent/db.sqlite")),
            ..HandoffOptions::new("/nonexistent-home")
        };
        assert!(write(&input, &opts).is_err());
    }

    #[test]
    fn dry_run_skips_db() {
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let db = make_db(dir.path());
        let before: i64 = {
            let conn = Connection::open(&db).expect("db open failed");
            conn.query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0))
                .expect("count failed")
        };
        let input = make_input();
        let opts = HandoffOptions {
            dest_override: Some(db),
            dry_run: true,
            ..HandoffOptions::new("/nonexistent-home")
        };
        let out = write(&input, &opts).expect("dry-run failed");
        assert!(out.dry_run);
        let after: i64 = {
            let conn = Connection::open(dir.path().join("db.sqlite")).expect("db open failed");
            conn.query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0))
                .expect("count failed")
        };
        assert_eq!(before, after, "dry-run 不得写入任何行");
    }
    #[test]
    fn schema_guard_rejects_foreign_db() {
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let db = dir.path().join("foreign.sqlite");
        let conn = Connection::open(&db).expect("db creation failed");
        conn.execute_batch("CREATE TABLE other (x TEXT);")
            .expect("ddl failed");
        drop(conn);
        let input = make_input();
        let opts = HandoffOptions {
            dest_override: Some(db),
            ..HandoffOptions::new("/nonexistent-home")
        };
        let err = write(&input, &opts).expect_err("must reject");
        assert!(err.to_string().contains("缺少表"), "got: {err}");
    }
}
