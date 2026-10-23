//! MiniMax Code 写回器：`~/.minimax/v2/sqlite/runtime-state.sqlite`。
//!
//! 纯增量 INSERT（`columnar_version=3` 对齐真实库全量分布，触发器会自动
//! 维护 project 关联表；测试库无触发器时同样成立）。

use crate::transcript::build_transcript;
use crate::{
    effective_cwd, ts_ms, HandoffError, HandoffInput, HandoffOptions, HandoffOutcome, HandoffResult,
};
use ch_domain::Role;
use rusqlite::Connection;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

/// 写入侧使用的 agent 名（MiniMax Code 的编码 agent；真实库 greeting 为 Coder）。
const AGENT_NAME: &str = "coder";

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

/// 写入一条 MiniMax Code 会话（sessions + message rows）。
pub fn write(input: &HandoffInput, opts: &HandoffOptions) -> HandoffResult<HandoffOutcome> {
    let items = build_transcript(input, opts);
    if items.is_empty() {
        return Err(HandoffError::Empty);
    }

    let session_id = format!("mvs_{}", Uuid::new_v4().simple());
    let cwd = effective_cwd(input, opts);
    let db_path: PathBuf = crate::resolve_dest(crate::HandoffTarget::MinimaxCode, opts);
    let started = input.started_at();
    let now_ms = ts_ms(ch_domain::now_utc());
    let created_ms = ts_ms(started);
    let title = input.title().to_string();

    if opts.dry_run {
        return Ok(HandoffOutcome {
            target: "minimax-code".into(),
            conversation_id: input.conversation.id.clone(),
            title,
            session_id,
            destination: db_path.to_string_lossy().into_owned(),
            items_written: items.len(),
            dry_run: true,
        });
    }

    let conn = open_target(&db_path)?;
    crate::ensure_tables(
        &conn,
        &["local_runtime_sessions", "local_runtime_message_rows"],
    )?;
    let tx = conn.unchecked_transaction()?;

    let record_json = serde_json::json!({
        "sessionId": session_id,
        "agentName": AGENT_NAME,
        "workspaceDir": cwd,
        "createdAtMs": created_ms,
        "updatedAtMs": now_ms,
        "parentSessionId": serde_json::Value::Null,
        "title": title,
        "visibility": "visible",
        "archived": false,
    })
    .to_string();
    let is_default_ws = i64::from(cwd == opts.home_dir.to_string_lossy());
    {
        let mut stmt = tx.prepare(
            "INSERT INTO local_runtime_sessions \
             (session_id, record_json, updated_at_ms, columnar_version, agent_name, \
              runtime, session_type, status, archived, visibility, session_kind, \
              parent_session_id, workspace_dir, project_workspace_dir, is_default_workspace, \
              title, created_at_ms) \
             VALUES (?1, ?2, ?3, 3, ?4, 'pi-agent', NULL, 'idle', 0, 'visible', 'conversation', \
                     NULL, ?5, ?5, ?6, ?7, ?8)",
        )?;
        stmt.execute(rusqlite::params![
            session_id,
            record_json,
            now_ms,
            AGENT_NAME,
            cwd,
            is_default_ws,
            title,
            created_ms,
        ])?;
    }

    for item in &items {
        let msg_id = Uuid::new_v4().to_string();
        let role = if item.role == Role::Assistant {
            "assistant"
        } else {
            "user"
        };
        let item_ms = ts_ms(item.at);
        let data_json = serde_json::json!({
            "msg_id": msg_id,
            "timestamp": item_ms,
            "role": role,
            "msg_type": 1,
            "msg_content": item.text,
        })
        .to_string();
        let mut stmt = tx.prepare(
            "INSERT INTO local_runtime_message_rows \
             (session_id, msg_id, role, created_at_ms, data_json) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        stmt.execute(rusqlite::params![
            session_id, msg_id, role, item_ms, data_json,
        ])?;
    }

    tx.commit()?;

    Ok(HandoffOutcome {
        target: "minimax-code".into(),
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

    /// 最小 schema：覆盖写回器涉及的全部列 + adapter 读取路径。
    const DDL: &str = r"
CREATE TABLE local_runtime_sessions (
    session_id TEXT PRIMARY KEY, record_json TEXT NOT NULL, updated_at_ms INTEGER NOT NULL,
    columnar_version INTEGER NOT NULL DEFAULT 0, agent_name TEXT, runtime TEXT,
    session_type TEXT, status TEXT, archived INTEGER NOT NULL DEFAULT 0,
    visibility TEXT NOT NULL DEFAULT 'visible',
    session_kind TEXT NOT NULL DEFAULT 'unknown', parent_session_id TEXT,
    workspace_dir TEXT, project_workspace_dir TEXT,
    is_default_workspace INTEGER NOT NULL DEFAULT 0 CHECK (is_default_workspace IN (0, 1)),
    title TEXT, created_at_ms INTEGER
);
CREATE TABLE local_runtime_message_rows (
    id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL, msg_id TEXT NOT NULL,
    role TEXT, turn_id TEXT, created_at_ms INTEGER NOT NULL, data_json TEXT NOT NULL,
    UNIQUE(session_id, msg_id)
);
";

    #[test]
    fn writes_session_and_roundtrips() {
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let db = dir.path().join("runtime-state.sqlite");
        let conn = Connection::open(&db).expect("db creation failed");
        conn.execute_batch(DDL).expect("ddl failed");
        drop(conn);

        let input = make_input();
        let opts = HandoffOptions {
            dest_override: Some(db.clone()),
            ..HandoffOptions::new("/nonexistent-home")
        };
        let out = write(&input, &opts).expect("write failed");
        assert!(out.session_id.starts_with("mvs_"));

        let raw = ch_adapter_minimax::parse_session(&db, &out.session_id)
            .expect("roundtrip parse failed");
        assert_eq!(raw.provider, ch_domain::Provider::MinimaxCode);
        assert_eq!(raw.source_conversation_id, out.session_id);
        assert!(raw.messages.len() >= 3, "preamble + dialog");
        assert!(raw.messages.iter().any(|m| m
            .text
            .as_deref()
            .is_some_and(|t| t.contains("[threadock 交接存档]"))));
    }

    #[test]
    fn missing_db_errors() {
        let input = make_input();
        let opts = HandoffOptions {
            dest_override: Some(PathBuf::from("/nonexistent/x.sqlite")),
            ..HandoffOptions::new("/nonexistent-home")
        };
        assert!(write(&input, &opts).is_err());
    }
}
