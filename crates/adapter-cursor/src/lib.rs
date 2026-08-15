//! Cursor Adapter，对应 plan §10.5「Cursor」。
//!
//! ## 数据源
//!
//! Cursor 把会话存在 VS Code 风格的 leveldb 兼容 SQLite 库
//! `~/Library/Application Support/Cursor/User/globalStorage/state.vscdb`，
//! 表 `cursorDiskKV(key TEXT PRIMARY KEY, value BLOB)`。
//!
//! 关键 key 体系：
//! - `composerData:<conversationId>` → 对话索引 JSON，含字段
//!   `composerId`、`fullConversationHeadersOnly[]`（每个 bubble 有
//!   `bubbleId`、`type`（1=用户 / 2=助手）、`grouping`）
//! - `bubbleId:<conversationId>:<bubbleId>` → 单条消息 JSON，含
//!   `text`（正文）、`createdAt`（ISO 8601）、`type`
//!
//! ## 能力
//!
//! - `discover_sessions`：列出所有 composerData 对应的会话
//! - `parse_session`：读取单条会话的所有 bubble，解析为 RawConversation

use ch_domain::{EventType, Provider, Role};
use ch_normalization::{RawConversation, RawEvent, RawMessage};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use thiserror::Error;

pub type AdapterResult<T> = std::result::Result<T, CursorError>;

#[derive(Debug, Error)]
pub enum CursorError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("conversation not found: {0}")]
    NotFound(String),

    #[error("conversation {0} has no messages")]
    Empty(String),
}

pub const ADAPTER_ID: &str = "cursor";
pub const PROVIDER: Provider = Provider::Cursor;

/// 发现的 Cursor 会话（composer）。
#[derive(Debug, Clone)]
pub struct DiscoveredSession {
    pub session_id: String,
    /// 对话标题：Cursor 没有显式 title，用首条用户消息前 60 字符。
    pub title: String,
    /// bubble 数量。
    pub message_count: usize,
    /// 首条消息时间（ISO 字符串）。
    pub created_at: Option<String>,
    pub file_path: String,
}

/// 只读打开 Cursor 的 state.vscdb（plan §10.1：只读快照原则）。
fn open_db(db_path: impl AsRef<Path>) -> AdapterResult<Connection> {
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    Ok(conn)
}

/// 列出 Cursor 所有会话（按首条消息时间降序，无时间的排最后）。
pub fn discover_sessions(db_path: impl AsRef<Path>) -> AdapterResult<Vec<DiscoveredSession>> {
    let conn = open_db(&db_path)?;
    // 取所有 composerData:<id> key
    let mut stmt = conn.prepare(
        "SELECT key, value FROM cursorDiskKV WHERE key LIKE 'composerData:%' AND key != 'composerData:empty-state-draft'",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
    })?;

    let mut sessions = Vec::new();
    for row in rows {
        let (key, value) = row?;
        let session_id = key
            .strip_prefix("composerData:")
            .unwrap_or(&key)
            .to_string();
        let obj: serde_json::Value = serde_json::from_slice(&value).unwrap_or_default();
        let headers = obj
            .get("fullConversationHeadersOnly")
            .and_then(|v| v.as_array());
        let bubble_count = headers.map(|a| a.len()).unwrap_or(0);

        // 首条用户消息的 text 作为 title 候选；时间从 headers[0] 推断需读 bubble，这里先用首 bubble 的 createdAt（若 composerData 内联了）。
        // 多数 composerData 不含 text，title 留空，由 parse 时填充；此处先给个粗标题。
        let title = format!("Cursor 会话 ({} 条消息)", bubble_count);
        sessions.push(DiscoveredSession {
            session_id,
            title,
            message_count: bubble_count,
            created_at: None,
            file_path: db_path.as_ref().to_string_lossy().into_owned(),
        });
    }
    // 按消息数降序（粗排，没有时间信息）
    sessions.sort_by_key(|s| std::cmp::Reverse(s.message_count));
    Ok(sessions)
}

/// 解析单条 Cursor 会话：读取它的所有 bubble，提取文本与工具事件。
pub fn parse_session(
    db_path: impl AsRef<Path>,
    conversation_id: &str,
) -> AdapterResult<RawConversation> {
    let conn = open_db(&db_path)?;

    // 1. 读 composerData 拿 bubble 列表
    let composer_key = format!("composerData:{conversation_id}");
    let value: Vec<u8> = conn
        .query_row(
            "SELECT value FROM cursorDiskKV WHERE key = ?1",
            rusqlite::params![&composer_key],
            |r| r.get::<_, Vec<u8>>(0),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                CursorError::NotFound(conversation_id.to_string())
            }
            other => other.into(),
        })?;
    let composer: serde_json::Value = serde_json::from_slice(&value)?;
    let headers = composer
        .get("fullConversationHeadersOnly")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if headers.is_empty() {
        return Err(CursorError::Empty(conversation_id.to_string()));
    }

    // 2. 逐个 bubble 读内容
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let mut first_created_at: Option<time::OffsetDateTime> = None;

    for h in &headers {
        let bubble_id = h.get("bubbleId").and_then(|v| v.as_str()).unwrap_or("");
        if bubble_id.is_empty() {
            continue;
        }
        let type_num = h.get("type").and_then(|v| v.as_i64()).unwrap_or(0);
        let bubble_key = format!("bubbleId:{conversation_id}:{bubble_id}");

        let bubble_value: Vec<u8> = match conn.query_row(
            "SELECT value FROM cursorDiskKV WHERE key = ?1",
            rusqlite::params![&bubble_key],
            |r| r.get::<_, Vec<u8>>(0),
        ) {
            Ok(v) => v,
            Err(rusqlite::Error::QueryReturnedNoRows) => continue,
            Err(e) => return Err(e.into()),
        };
        let bubble: serde_json::Value = serde_json::from_slice(&bubble_value).unwrap_or_default();

        // 时间
        let created_at = bubble
            .get("createdAt")
            .and_then(|v| v.as_str())
            .and_then(parse_iso);
        if first_created_at.is_none() {
            first_created_at = created_at;
        }

        // 文本
        let text = bubble.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let grouping = h.get("grouping");

        // role: type 1 = user, 2 = assistant
        let role = if type_num == 1 {
            Role::User
        } else {
            Role::Assistant
        };

        // 只在 hasText / isShortPlainText 时把文本当作正文，避免纯 thinking / tool 噪声
        let has_text = grouping
            .and_then(|g| g.get("hasText"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let is_plain = grouping
            .and_then(|g| g.get("isShortPlainText"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if !text.is_empty() && (has_text || is_plain || type_num == 1) {
            messages.push(RawMessage {
                role,
                text: Some(text.to_string()),
                content_json: None,
                source_message_id: Some(bubble_id.to_string()),
                created_at,
            });
        }

        // 工具调用 → 事件
        let cap_type = grouping
            .and_then(|g| g.get("capabilityType"))
            .and_then(|v| v.as_i64());
        if let Some(ct) = cap_type {
            if ct == 15 {
                // toolFormerTool
                let tool = grouping
                    .and_then(|g| g.get("toolFormerTool"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                events.push(RawEvent {
                    event_type: EventType::ToolCallStarted,
                    summary: Some(format!("Cursor tool #{tool}")),
                    payload_json: Some(bubble.clone()),
                    source_event_id: Some(bubble_id.to_string()),
                    created_at,
                });
            }
        }
    }

    if messages.is_empty() && events.is_empty() {
        return Err(CursorError::Empty(conversation_id.to_string()));
    }

    // 标题：取首条用户消息前 60 字
    let title = messages
        .iter()
        .find(|m| m.role == Role::User)
        .and_then(|m| m.text.as_deref())
        .map(|t| {
            let t = t.trim();
            if t.chars().count() <= 60 {
                t.to_string()
            } else {
                t.chars().take(60).collect::<String>() + "…"
            }
        });

    Ok(RawConversation {
        provider: PROVIDER,
        source_conversation_id: conversation_id.to_string(),
        title,
        model: Some("cursor".into()),
        started_at: first_created_at,
        messages,
        events,
        source_parent_id: None,
    })
}

/// 解析 ISO 8601 时间戳（如 `2026-04-28T08:41:38.027Z`）。
fn parse_iso(s: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// 构造一个模拟的 Cursor state.vscdb。
    fn make_test_db(dir: &Path) -> std::path::PathBuf {
        let db = dir.join("state.vscdb");
        let conn = Connection::open(&db).expect("database connection failed");
        conn.execute_batch("CREATE TABLE cursorDiskKV (key TEXT PRIMARY KEY, value BLOB);")
            .expect("unexpected None");

        // 一个 composerData + 2 个 bubble（用户 + 助手）
        let composer = serde_json::json!({
            "composerId": "conv1",
            "fullConversationHeadersOnly": [
                {"bubbleId": "b1", "type": 1, "grouping": {"hasText": true}},
                {"bubbleId": "b2", "type": 2, "grouping": {"hasText": true, "isShortPlainText": true}}
            ]
        });
        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES ('composerData:conv1', ?1)",
            rusqlite::params![serde_json::to_vec(&composer).expect("unexpected None")],
        )
        .expect("unexpected None");

        let b1 = serde_json::json!({
            "bubbleId": "b1",
            "type": 1,
            "text": "帮我写个函数",
            "createdAt": "2026-04-28T08:41:38.027Z"
        });
        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES ('bubbleId:conv1:b1', ?1)",
            rusqlite::params![serde_json::to_vec(&b1).expect("unexpected None")],
        )
        .expect("unexpected None");

        let b2 = serde_json::json!({
            "bubbleId": "b2",
            "type": 2,
            "text": "好的，这是函数实现：fn foo() {}",
            "createdAt": "2026-04-28T08:41:40.000Z"
        });
        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES ('bubbleId:conv1:b2', ?1)",
            rusqlite::params![serde_json::to_vec(&b2).expect("unexpected None")],
        )
        .expect("unexpected None");
        drop(conn);
        db
    }

    #[test]
    fn discover_finds_composer() {
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let db = make_test_db(dir.path());
        let sessions = discover_sessions(&db).expect("unexpected None");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "conv1");
        assert_eq!(sessions[0].message_count, 2);
    }

    #[test]
    fn parse_extracts_messages_and_time() {
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let db = make_test_db(dir.path());
        let raw = parse_session(&db, "conv1").expect("parse failed");
        assert_eq!(raw.provider, Provider::Cursor);
        assert_eq!(raw.messages.len(), 2);
        assert_eq!(raw.messages[0].role, Role::User);
        assert!(raw.messages[0]
            .text
            .as_deref()
            .expect("unexpected None")
            .contains("帮我写个函数"));
        assert_eq!(raw.messages[1].role, Role::Assistant);
        assert!(raw.started_at.is_some());
        // 标题取自首条用户消息
        assert_eq!(raw.title.as_deref(), Some("帮我写个函数"));
    }

    #[test]
    fn not_found_errors() {
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let db = make_test_db(dir.path());
        assert!(matches!(
            parse_session(&db, "nope"),
            Err(CursorError::NotFound(_))
        ));
    }
}
