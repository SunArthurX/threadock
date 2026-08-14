//! ZCode Adapter，对应 plan §10.5「ZCode」。
//!
//! ## 数据源
//!
//! ZCode 把会话存在 `~/.zcode/cli/db/db.sqlite`，表结构：
//! - `session`：会话元信息（id, title, directory, time_created 等）
//! - `message`：消息（id, session_id, data 含 role/model）
//! - `part`：消息内容片段（message_id, data 含 type:text/tool 等）
//!
//! ## 能力
//!
//! - `discover_sessions`：从 db.sqlite 列出会话（id/title/时间/目录）
//! - `parse_session`：读取单条会话的所有 message+part，解析为 RawConversation

use ch_domain::{EventType, Provider, Role};
use ch_normalization::{RawConversation, RawEvent, RawMessage};
use rusqlite::{params, Connection, OpenFlags};
use std::path::Path;
use thiserror::Error;

pub type AdapterResult<T> = std::result::Result<T, ZCodeError>;

#[derive(Debug, Error)]
pub enum ZCodeError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("session not found: {0}")]
    NotFound(String),

    #[error("session {0} has no messages")]
    Empty(String),
}

pub const ADAPTER_ID: &str = "zcode";
pub const PROVIDER: Provider = Provider::ZCode;

/// 发现的会话。
#[derive(Debug, Clone)]
pub struct DiscoveredSession {
    pub session_id: String,
    pub title: String,
    pub directory: String,
    pub message_count: i64,
    pub time_created: i64,
    pub time_updated: i64,
    /// 子任务数量（主任务的派生分支数）。
    pub child_count: i64,
    /// 来源侧父会话 ID（修复旧数据主子链路用）。
    pub parent_id: Option<String>,
}

/// 只读打开 ZCode 数据库（plan §10.1：只读快照原则）。
fn open_db(db_path: impl AsRef<Path>) -> AdapterResult<Connection> {
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    Ok(conn)
}

/// 列出 ZCode 所有**主任务**会话（parent_id 为空），按更新时间降序。
///
/// ZCode 的 session 有层级：主任务（`parent_id` 为 null/空）下面挂多个子任务
/// （`task_type='subagent_child'`）。只在左侧列表展示主任务，避免子任务刷屏。
/// updated_at 取主任务自身与所有子任务中的最大值，反映真实活跃时间。
pub fn discover_sessions(db_path: impl AsRef<Path>) -> AdapterResult<Vec<DiscoveredSession>> {
    let conn = open_db(db_path)?;
    // 只选 parent_id 为空的主任务，子任务数和时间取子查询
    let mut stmt = conn.prepare(
        "SELECT s.id, s.title, s.directory, s.time_created, s.time_updated,
                (SELECT count(*) FROM message m WHERE m.session_id = s.id) as msg_count,
                (SELECT count(*) FROM session c WHERE c.parent_id = s.id) AS child_count,
                (SELECT COALESCE(MAX(c.time_updated), s.time_updated) FROM session c WHERE c.parent_id = s.id) AS max_child_updated
         FROM session s
         WHERE s.parent_id IS NULL OR s.parent_id = ''
         ORDER BY max_child_updated DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        let time_updated: i64 = r.get(4)?;
        let max_child: i64 = r.get(7).unwrap_or(time_updated);
        Ok(DiscoveredSession {
            session_id: r.get(0)?,
            title: r.get(1)?,
            directory: r.get(2)?,
            time_created: r.get(3)?,
            time_updated: time_updated.max(max_child),
            message_count: r.get(5)?,
            child_count: r.get(6)?,
            parent_id: None, // discover_sessions 只返回主任务
        })
    })?;
    let mut v = Vec::new();
    for r in rows {
        v.push(r?);
    }
    Ok(v)
}

/// 列出 ZCode **所有**会话（主任务 + 子任务），按更新时间降序。
/// 用于 auto_sync：主任务先导入（source_parent_id=null），子任务后导入（source_parent_id=父ID）。
pub fn discover_all_sessions(db_path: impl AsRef<Path>) -> AdapterResult<Vec<DiscoveredSession>> {
    let conn = open_db(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT s.id, s.title, s.directory, s.time_created, s.time_updated,
                (SELECT count(*) FROM message m WHERE m.session_id = s.id) as msg_count,
                COALESCE((SELECT count(*) FROM session c WHERE c.parent_id = s.id), 0) AS child_count,
                s.time_updated AS effective_updated,
                NULLIF(s.parent_id, '') AS parent
         FROM session s
         ORDER BY effective_updated DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(DiscoveredSession {
            session_id: r.get(0)?,
            title: r.get(1)?,
            directory: r.get(2)?,
            time_created: r.get(3)?,
            time_updated: r.get(4)?,
            message_count: r.get(5)?,
            child_count: r.get(6)?,
            parent_id: r.get(8)?,
        })
    })?;
    let mut v = Vec::new();
    for r in rows {
        v.push(r?);
    }
    Ok(v)
}

/// 解析单条 ZCode 会话。
pub fn parse_session(db_path: impl AsRef<Path>, session_id: &str) -> AdapterResult<RawConversation> {
    let conn = open_db(db_path)?;

    // 1. 会话元信息（含时间戳 + parent_id 主子链路）
    let (title, _directory, model, time_created, time_updated, parent_id): (
        String, String, Option<String>, i64, i64, Option<String>,
    ) = conn
        .query_row(
            "SELECT title, directory, NULL, time_created, time_updated, parent_id
             FROM session WHERE id = ?1",
            params![session_id],
            |r| Ok((
                r.get(0)?,
                r.get(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get::<_, Option<String>>(5)?,
            )),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => ZCodeError::NotFound(session_id.to_string()),
            other => other.into(),
        })?;

    // parent_id 为空字符串时视为无父级（顶层主任务）
    let source_parent_id = parent_id
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let started_at = time::OffsetDateTime::from_unix_timestamp(time_created / 1000).ok();
    let _updated_at = time::OffsetDateTime::from_unix_timestamp(time_updated / 1000).ok();

    // 2. 消息 + parts
    let mut messages = Vec::new();
    let mut events = Vec::new();

    // 查所有消息（按 time_created 排序）
    let mut msg_stmt = conn.prepare(
        "SELECT id, data, time_created FROM message
         WHERE session_id = ?1
         ORDER BY time_created, id",
    )?;
    let msg_rows = msg_stmt.query_map(params![session_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
    })?;

    for msg_row in msg_rows {
        let (msg_id, msg_data, msg_time) = msg_row?;
        let msg_obj: serde_json::Value = serde_json::from_str(&msg_data).unwrap_or_default();
        let role_str = msg_obj.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        let role = match role_str {
            "assistant" => Role::Assistant,
            "system" => Role::System,
            "tool" => Role::Tool,
            _ => Role::User,
        };
        let msg_created_at = time::OffsetDateTime::from_unix_timestamp(msg_time / 1000).ok();

        // 查这条消息的 parts
        let mut part_stmt = conn.prepare(
            "SELECT data FROM part
             WHERE message_id = ?1
             ORDER BY sequence, time_created, id",
        )?;
        let part_rows = part_stmt.query_map(params![msg_id], |r| r.get::<_, String>(0))?;

        let mut text_parts = Vec::new();
        for part_row in part_rows {
            let part_data = part_row?;
            let part_obj: serde_json::Value = serde_json::from_str(&part_data).unwrap_or_default();
            let part_type = part_obj.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match part_type {
                "text" => {
                    if let Some(t) = part_obj.get("text").and_then(|t| t.as_str()) {
                        // 过滤 system-reminder 噪声
                        if !t.starts_with("<system-reminder>") {
                            text_parts.push(t.to_string());
                        }
                    }
                }
                "tool_use" | "tool-call" => {
                    let tool_name = part_obj
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("tool");
                    events.push(RawEvent {
                        event_type: EventType::ToolCallStarted,
                        summary: Some(format!("Tool: {tool_name}")),
                        payload_json: Some(part_obj),
                        source_event_id: None,
                        created_at: None,
                    });
                }
                "tool_result" | "tool-result" => {
                    events.push(RawEvent {
                        event_type: EventType::ToolCallCompleted,
                        summary: Some("Tool result".into()),
                        payload_json: Some(part_obj),
                        source_event_id: None,
                        created_at: None,
                    });
                }
                "command" => {
                    let cmd = part_obj
                        .get("command")
                        .and_then(|c| c.as_str())
                        .unwrap_or("");
                    events.push(RawEvent {
                        event_type: EventType::CommandStarted,
                        summary: Some(cmd.to_string()),
                        payload_json: Some(part_obj),
                        source_event_id: None,
                        created_at: None,
                    });
                }
                _ => {}
            }
        }

        if !text_parts.is_empty() {
            messages.push(RawMessage {
                role,
                text: Some(text_parts.join("\n")),
                content_json: None,
                source_message_id: Some(msg_id),
                created_at: msg_created_at,
            });
        }
    }

    if messages.is_empty() && events.is_empty() {
        return Err(ZCodeError::Empty(session_id.to_string()));
    }

    Ok(RawConversation {
        provider: PROVIDER,
        source_conversation_id: session_id.to_string(),
        title: Some(title),
        model,
        started_at,
        messages,
        events,
        source_parent_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_db() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join("test.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE session (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL DEFAULT '',
                workspace_id TEXT,
                parent_id TEXT,
                slug TEXT NOT NULL DEFAULT '',
                directory TEXT NOT NULL DEFAULT '',
                path TEXT,
                title TEXT NOT NULL DEFAULT '',
                version TEXT NOT NULL DEFAULT '',
                share_url TEXT,
                summary_additions INTEGER,
                summary_deletions INTEGER,
                summary_files INTEGER,
                summary_diffs TEXT,
                revert TEXT,
                permission TEXT,
                time_created INTEGER NOT NULL DEFAULT 0,
                time_updated INTEGER NOT NULL DEFAULT 0,
                time_compacting INTEGER,
                time_archived INTEGER
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL DEFAULT 0,
                time_updated INTEGER NOT NULL DEFAULT 0,
                data TEXT NOT NULL,
                sequence INTEGER
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL DEFAULT 0,
                time_updated INTEGER NOT NULL DEFAULT 0,
                data TEXT NOT NULL,
                sequence INTEGER
            );
            "#,
        )
        .unwrap();

        // 插入测试数据
        conn.execute(
            "INSERT INTO session (id, title, directory, time_created, time_updated)
             VALUES ('sess1', '测试会话', '/tmp/proj', 1000, 2000)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, data, time_created)
             VALUES ('msg1', 'sess1', '{\"role\":\"user\"}', 1100)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, data, time_created)
             VALUES ('msg2', 'sess1', '{\"role\":\"assistant\"}', 1200)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, data, sequence)
             VALUES ('p1', 'msg1', 'sess1', '{\"type\":\"text\",\"text\":\"你好\"}', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, data, sequence)
             VALUES ('p2', 'msg2', 'sess1', '{\"type\":\"text\",\"text\":\"你好啊\"}', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, data, sequence)
             VALUES ('p3', 'msg2', 'sess1', '{\"type\":\"tool_use\",\"name\":\"Bash\"}', 1)",
            [],
        )
        .unwrap();
        drop(conn);
        dir
    }

    #[test]
    fn discover_sessions() {
        let dir = create_test_db();
        let db = dir.path().join("test.db");
        let sessions = super::discover_sessions(&db).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "sess1");
        assert_eq!(sessions[0].title, "测试会话");
        assert_eq!(sessions[0].message_count, 2);
    }

    #[test]
    fn parse_session_with_messages_and_events() {
        let dir = create_test_db();
        let db = dir.path().join("test.db");
        let raw = super::parse_session(&db, "sess1").unwrap();
        assert_eq!(raw.title.as_deref(), Some("测试会话"));
        assert_eq!(raw.provider, Provider::ZCode);
        assert_eq!(raw.messages.len(), 2);
        assert_eq!(raw.messages[0].role, Role::User);
        assert!(raw.messages[0].text.as_deref().unwrap().contains("你好"));
        assert_eq!(raw.messages[1].role, Role::Assistant);
        assert!(!raw.events.is_empty());
        assert!(raw.events[0].summary.as_deref().unwrap().contains("Bash"));
    }

    #[test]
    fn filters_system_reminder() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join("t.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY, title TEXT NOT NULL, directory TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, parent_id TEXT);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL, sequence INTEGER);
             CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT NOT NULL, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL, sequence INTEGER);",
        ).unwrap();
        conn.execute("INSERT INTO session VALUES ('s','t','d',0,0,NULL)", []).unwrap();
        conn.execute("INSERT INTO message VALUES ('m','s',0,0,'{\"role\":\"user\"}',0)", []).unwrap();
        conn.execute(
            "INSERT INTO part VALUES ('p1','m','s',0,0,'{\"type\":\"text\",\"text\":\"<system-reminder>noise</system-reminder>\"}',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO part VALUES ('p2','m','s',0,0,'{\"type\":\"text\",\"text\":\"真实内容\"}',1)",
            [],
        )
        .unwrap();
        drop(conn);
        let raw = super::parse_session(&db, "s").unwrap();
        assert_eq!(raw.messages.len(), 1);
        assert_eq!(raw.messages[0].text.as_deref(), Some("真实内容"));
    }

    #[test]
    fn session_not_found() {
        let dir = create_test_db();
        let db = dir.path().join("test.db");
        assert!(matches!(
            super::parse_session(&db, "nope"),
            Err(ZCodeError::NotFound(_))
        ));
    }

    #[test]
    fn empty_session_errors() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join("t.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY, title TEXT NOT NULL, directory TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, parent_id TEXT);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL, sequence INTEGER);
             CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT NOT NULL, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL, sequence INTEGER);",
        ).unwrap();
        conn.execute("INSERT INTO session VALUES ('s','t','d',0,0,NULL)", []).unwrap();
        drop(conn);
        assert!(matches!(
            super::parse_session(&db, "s"),
            Err(ZCodeError::Empty(_))
        ));
    }

    #[test]
    fn discover_only_lists_root_sessions() {
        // 主任务 + 2 个子任务：discover 只应返回主任务
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join("hier.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT '', directory TEXT NOT NULL DEFAULT '', time_created INTEGER NOT NULL DEFAULT 0, time_updated INTEGER NOT NULL DEFAULT 0, parent_id TEXT);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, time_created INTEGER NOT NULL DEFAULT 0, time_updated INTEGER NOT NULL DEFAULT 0, data TEXT NOT NULL, sequence INTEGER);
             CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT NOT NULL, session_id TEXT NOT NULL, time_created INTEGER NOT NULL DEFAULT 0, time_updated INTEGER NOT NULL DEFAULT 0, data TEXT NOT NULL, sequence INTEGER);",
        )
        .unwrap();
        // 主任务（updated 1000），下面挂 2 个子任务（updated 5000、6000）
        conn.execute("INSERT INTO session (id, title, time_updated, parent_id) VALUES ('parent', '主任务', 1000, NULL)", []).unwrap();
        conn.execute("INSERT INTO session (id, title, time_updated, parent_id) VALUES ('child1', '子任务1', 5000, 'parent')", []).unwrap();
        conn.execute("INSERT INTO session (id, title, time_updated, parent_id) VALUES ('child2', '子任务2', 6000, 'parent')", []).unwrap();
        drop(conn);

        let sessions = super::discover_sessions(&db).unwrap();
        assert_eq!(sessions.len(), 1, "只应返回主任务");
        assert_eq!(sessions[0].session_id, "parent");
        assert_eq!(sessions[0].title, "主任务");
        assert_eq!(sessions[0].child_count, 2, "应有 2 个子任务");
        // updated_at 应取子任务最大值 6000，而非自身 1000
        assert_eq!(sessions[0].time_updated, 6000);
    }

    #[test]
    fn parse_session_extracts_parent_id() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join("parent.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT '', directory TEXT NOT NULL DEFAULT '', time_created INTEGER NOT NULL DEFAULT 0, time_updated INTEGER NOT NULL DEFAULT 0, parent_id TEXT);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, time_created INTEGER NOT NULL DEFAULT 0, time_updated INTEGER NOT NULL DEFAULT 0, data TEXT NOT NULL, sequence INTEGER);
             CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT NOT NULL, session_id TEXT NOT NULL, time_created INTEGER NOT NULL DEFAULT 0, time_updated INTEGER NOT NULL DEFAULT 0, data TEXT NOT NULL, sequence INTEGER);",
        )
        .unwrap();
        // 子任务，parent_id 指向 'parent-session'
        conn.execute("INSERT INTO session (id, title, parent_id) VALUES ('child', '子任务', 'parent-session')", []).unwrap();
        conn.execute("INSERT INTO message (id, session_id, data) VALUES ('m1', 'child', '{\"role\":\"user\"}')", []).unwrap();
        conn.execute("INSERT INTO part (id, message_id, session_id, data, sequence) VALUES ('p1', 'm1', 'child', '{\"type\":\"text\",\"text\":\"子任务内容\"}', 0)", []).unwrap();
        drop(conn);

        let raw = super::parse_session(&db, "child").unwrap();
        assert_eq!(raw.source_parent_id.as_deref(), Some("parent-session"));
    }
}
