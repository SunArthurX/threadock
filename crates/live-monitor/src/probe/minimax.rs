//! MiniMax Code 实时探测：`~/.minimax/v2/sqlite/runtime-state.sqlite`（WAL）。
//! `local_runtime_sessions.status` 有源侧显式状态（running/idle/error/…），
//! 进度取 `local_runtime_token_usage`（每 turn token）与消息行数。

use crate::types::{LiveActivity, LiveProgress, LiveSource, LiveState, LiveTask};
use crate::{classify, home_dir, snippet, today_start_ms};
use ch_adapter_sdk::open_readonly;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::path::Path;

pub fn scan(now_ms: i64) -> Result<Vec<LiveTask>, String> {
    scan_window(
        &home_dir()?.join(".minimax/v2/sqlite/runtime-state.sqlite"),
        now_ms,
        today_start_ms(now_ms),
    )
}

/// 指定库 + 窗口起点扫描（测试注入用；`since_ms` 通常为本地零点）。
pub fn scan_window(db_path: &Path, now_ms: i64, since_ms: i64) -> Result<Vec<LiveTask>, String> {
    if !db_path.is_file() {
        return Ok(vec![]);
    }
    let conn = open_readonly(db_path).map_err(|e| format!("open minimax db: {e}"))?;
    let cutoff = since_ms;
    let mut stmt = conn
        .prepare(
            "SELECT session_id,
                    COALESCE(title, json_extract(record_json, '$.title')),
                    status, session_kind, error_message,
                    COALESCE(workspace_dir, json_extract(record_json, '$.workspaceDir')),
                    COALESCE(created_at_ms, json_extract(record_json, '$.createdAtMs')),
                    updated_at_ms
             FROM local_runtime_sessions
             WHERE updated_at_ms > ?1
               AND COALESCE(parent_session_id,
                            json_extract(record_json, '$.parentSessionId')) IS NULL
             ORDER BY updated_at_ms DESC
             LIMIT 200",
        )
        .map_err(|e| format!("prepare minimax: {e}"))?;
    let rows = stmt
        .query_map(params![cutoff], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, i64>(7)?,
            ))
        })
        .map_err(|e| format!("query minimax: {e}"))?;
    let mut tasks = Vec::new();
    for row in rows {
        let (session_id, title, status, session_kind, error_message, workspace, created, updated) =
            row.map_err(|e| format!("minimax row: {e}"))?;
        let state = explicit_state(status.as_deref(), now_ms, updated);
        let progress = session_progress(&conn, &session_id);
        let activity = last_activity(&conn, &session_id).or_else(|| {
            // 无消息时用错误信息兜底（error/aborted 的会话至少要有解释）
            error_message
                .filter(|m| !m.trim().is_empty())
                .map(|m| LiveActivity {
                    kind: "status".into(),
                    role: None,
                    snippet: snippet(&m, 160),
                    at_ms: updated,
                })
        });
        tasks.push(LiveTask {
            source: LiveSource::MinimaxCode,
            session_id,
            title: title.filter(|t| !t.is_empty()),
            workspace,
            originator: session_kind,
            started_at_ms: created,
            last_activity_ms: updated,
            state,
            activity,
            progress: Some(progress),
        });
    }
    Ok(tasks)
}

/// 源侧显式状态优先于时间启发式。
#[must_use]
fn explicit_state(status: Option<&str>, now_ms: i64, updated_ms: i64) -> LiveState {
    match status.unwrap_or("") {
        s if s.contains("run") || s == "working" || s == "executing" => LiveState::Running,
        s if s.contains("wait") || s == "approval" => LiveState::Waiting,
        // idle / error / aborted / interrupted / 空值 → 时间启发式
        _ => classify(now_ms, updated_ms),
    }
}

/// 最近 token 用量 + 消息行数。
#[must_use]
fn session_progress(conn: &Connection, session_id: &str) -> LiveProgress {
    let mut p = LiveProgress::default();
    if let Ok((input, output)) = conn.query_row(
        "SELECT input_tokens, output_tokens FROM local_runtime_token_usage
         WHERE session_id = ?1 ORDER BY ts DESC, id DESC LIMIT 1",
        params![session_id],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
    ) {
        p.total_tokens = Some(input + output);
    }
    if let Ok(n) = conn.query_row(
        "SELECT count(*) FROM local_runtime_message_rows WHERE session_id = ?1",
        params![session_id],
        |r| r.get::<_, i64>(0),
    ) {
        p.message_count = Some(n);
    }
    p
}

/// 最近消息行 → 活动摘要（从新到旧找第一条可展示内容）。
fn last_activity(conn: &Connection, session_id: &str) -> Option<LiveActivity> {
    let mut stmt = conn
        .prepare(
            "SELECT role, created_at_ms, data_json FROM local_runtime_message_rows
             WHERE session_id = ?1 ORDER BY id DESC LIMIT 5",
        )
        .ok()?;
    let rows: Vec<(Option<String>, i64, String)> = stmt
        .query_map(params![session_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .ok()?
        .filter_map(std::result::Result::ok)
        .collect();
    for (role, created_ms, data_json) in &rows {
        if let Some(act) = message_row_activity(role.as_deref(), data_json) {
            return Some(LiveActivity {
                at_ms: *created_ms,
                ..act
            });
        }
    }
    None
}

/// transcript：最近 `limit` 条活动（时间升序）。
#[must_use]
pub fn transcript(db_path: &Path, session_id: &str, limit: usize) -> Vec<LiveActivity> {
    let Ok(conn) = open_readonly(db_path) else {
        return vec![];
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT role, created_at_ms, data_json FROM local_runtime_message_rows
         WHERE session_id = ?1 ORDER BY id DESC LIMIT 60",
    ) else {
        return vec![];
    };
    let Ok(rows) = stmt.query_map(params![session_id], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
        ))
    }) else {
        return vec![];
    };
    let mut out: Vec<LiveActivity> = Vec::new(); // 新→旧
    for row in rows.filter_map(std::result::Result::ok) {
        if out.len() >= limit {
            break;
        }
        let (role, created_ms, data_json) = row;
        if let Some(act) = message_row_activity(role.as_deref(), &data_json) {
            out.push(LiveActivity {
                at_ms: created_ms,
                ..act
            });
        }
    }
    out.reverse();
    let start = out.len().saturating_sub(limit);
    out[start..].to_vec()
}

/// 一条消息行 → 可展示活动；内部事件行（msg_type=3 / eventType 协议）与装饰内容跳过。
#[must_use]
fn message_row_activity(role: Option<&str>, data_json: &str) -> Option<LiveActivity> {
    let data: Value = serde_json::from_str(data_json).ok()?;
    if data.get("msg_type").and_then(Value::as_i64) == Some(3) {
        return None;
    }
    let content = data
        .get("msg_content")
        .and_then(Value::as_str)
        .unwrap_or("");
    let content = content.trim();
    // 内部事件协议 JSON / 问候装饰 / 空内容 → 跳过
    let is_event_payload = serde_json::from_str::<Value>(content)
        .ok()
        .is_some_and(|v| v.get("eventType").is_some());
    if content.is_empty() || content.starts_with('<') || is_event_payload {
        // 思考过程兜底：正文空但 thinking 有内容
        let thinking = data
            .get("thinking_content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if thinking.is_empty() {
            return None;
        }
        return Some(LiveActivity {
            kind: "thinking".into(),
            role: Some("assistant".into()),
            snippet: snippet(thinking, 160),
            at_ms: 0,
        });
    }
    Some(LiveActivity {
        kind: "message".into(),
        role: Some(role.unwrap_or("user"))
            .filter(|r| !r.is_empty())
            .map(String::from),
        snippet: snippet(content, 160),
        at_ms: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_db(dir: &Path) -> Connection {
        let db = dir.join("runtime-state.sqlite");
        let conn = Connection::open(&db).expect("open");
        conn.execute_batch(
            r"
            CREATE TABLE local_runtime_sessions (
                session_id TEXT PRIMARY KEY,
                record_json TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                status TEXT, session_kind TEXT, error_message TEXT,
                parent_session_id TEXT, workspace_dir TEXT, title TEXT,
                created_at_ms INTEGER, archived INTEGER NOT NULL DEFAULT 0,
                visibility TEXT NOT NULL DEFAULT 'visible'
            );
            CREATE TABLE local_runtime_message_rows (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL, msg_id TEXT, role TEXT,
                turn_id TEXT, created_at_ms INTEGER DEFAULT 0, data_json TEXT NOT NULL
            );
            CREATE TABLE local_runtime_token_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL, agent_name TEXT DEFAULT '',
                framework_type TEXT DEFAULT '', turn_id TEXT, model TEXT,
                ts INTEGER NOT NULL, input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0
            );
            ",
        )
        .expect("schema");
        conn
    }

    #[test]
    fn running_status_beats_freshness() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = 1_793_618_000_000i64;
        {
            let conn = create_db(dir.path());
            // status=running 但 3 分钟没写入 → 仍判 Running
            conn.execute(
                "INSERT INTO local_runtime_sessions
                 (session_id, record_json, updated_at_ms, status, session_kind, title, workspace_dir, created_at_ms)
                 VALUES ('m1','{}', ?1, 'running', 'conversation', '跑批任务', '/tmp/mm', ?2)",
                params![now - 180_000, now - 600_000],
            )
            .expect("insert");
            conn.execute(
                "INSERT INTO local_runtime_token_usage
                 (session_id, ts, input_tokens, output_tokens) VALUES ('m1', ?1, 900, 110)",
                params![now - 60_000],
            )
            .expect("insert usage");
            conn.execute(
                "INSERT INTO local_runtime_message_rows (session_id, role, created_at_ms, data_json)
                 VALUES ('m1','assistant', ?1, '{\"msg_type\":0,\"msg_content\":\"正在处理第 3 批\"}')",
                params![now - 30_000],
            )
            .expect("insert msg");
        }
        let tasks = scan_window(
            &dir.path().join("runtime-state.sqlite"),
            now,
            now - 30 * 60_000,
        )
        .expect("scan");
        assert_eq!(tasks.len(), 1);
        let t = &tasks[0];
        assert_eq!(t.state, LiveState::Running, "显式 running 优先");
        assert_eq!(t.title.as_deref(), Some("跑批任务"));
        assert_eq!(t.workspace.as_deref(), Some("/tmp/mm"));
        let act = t.activity.as_ref().expect("activity");
        assert!(act.snippet.contains("第 3 批"));
        assert_eq!(
            t.progress.as_ref().expect("progress").total_tokens,
            Some(1010)
        );
    }

    #[test]
    fn skips_internal_event_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = 1_793_618_000_000i64;
        {
            let conn = create_db(dir.path());
            conn.execute(
                "INSERT INTO local_runtime_sessions
                 (session_id, record_json, updated_at_ms, status, title)
                 VALUES ('m2','{}', ?1, 'idle', '事件行过滤')",
                params![now - 60_000],
            )
            .expect("insert");
            conn.execute_batch(
                "INSERT INTO local_runtime_message_rows (session_id, role, created_at_ms, data_json) VALUES
                 ('m2','system',100,'{\"msg_type\":3,\"msg_content\":\"{\\\"eventType\\\":\\\"todo_updated\\\"}\"}'),
                 ('m2','assistant',200,'{\"msg_type\":0,\"msg_content\":\"真实回复\"}');",
            )
            .expect("insert rows");
        }
        let tasks = scan_window(
            &dir.path().join("runtime-state.sqlite"),
            now,
            now - 30 * 60_000,
        )
        .expect("scan");
        let act = tasks[0].activity.as_ref().expect("activity");
        assert!(act.snippet.contains("真实回复"), "{:?}", act.snippet);
    }

    #[test]
    fn transcript_ascending() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = 1_793_618_000_000i64;
        {
            let conn = create_db(dir.path());
            conn.execute(
                "INSERT INTO local_runtime_sessions (session_id, record_json, updated_at_ms)
                 VALUES ('m3','{}', ?1)",
                params![now],
            )
            .expect("insert");
            conn.execute_batch(
                "INSERT INTO local_runtime_message_rows (session_id, role, created_at_ms, data_json) VALUES
                 ('m3','user',100,'{\"msg_type\":0,\"msg_content\":\"问\"}'),
                 ('m3','assistant',200,'{\"msg_type\":0,\"msg_content\":\"答\"}');",
            )
            .expect("insert");
        }
        let items = transcript(&dir.path().join("runtime-state.sqlite"), "m3", 10);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].snippet, "问");
        assert_eq!(items[1].snippet, "答");
    }

    #[test]
    fn missing_db_is_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tasks = scan_window(&dir.path().join("nope.sqlite"), 0, 0).expect("scan");
        assert!(tasks.is_empty());
    }
}
