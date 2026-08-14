//! MiniMax Code Adapter，对应 plan §10.5「MiniMax Code」。
//!
//! ## 数据源
//!
//! MiniMax Code（Mavis agent runtime）把会话存在
//! `~/.minimax/v2/sqlite/runtime-state.sqlite`，核心表：
//!
//! - `local_runtime_sessions(session_id, record_json, updated_at_ms)`
//!   - `record_json` 含 `sessionId`/`agentName`/`workspaceDir`/`title`/
//!     `createdAtMs`/`updatedAtMs`/`status`
//! - `local_runtime_message_rows(id, session_id, msg_id, role, turn_id,
//!   created_at_ms, data_json)`
//!   - `data_json` 含 `msg_id`/`timestamp`/`role`/`msg_type`/`msg_content`/
//!     `thinking_content`
//!
//! ## 能力
//!
//! - `discover_sessions`：列出所有会话（按 updated_at_ms 降序）
//! - `parse_session`：读取单条会话的所有消息行，解析为 RawConversation

use ch_domain::{Provider, Role};
use ch_normalization::{RawConversation, RawMessage};
use rusqlite::{params, Connection, OpenFlags};
use std::path::Path;
use thiserror::Error;

pub type AdapterResult<T> = std::result::Result<T, MinimaxError>;

#[derive(Debug, Error)]
pub enum MinimaxError {
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

pub const ADAPTER_ID: &str = "minimax";
pub const PROVIDER: Provider = Provider::MinimaxCode;

/// 发现的 MiniMax 会话。
#[derive(Debug, Clone)]
pub struct DiscoveredSession {
    pub session_id: String,
    pub title: String,
    pub agent_name: String,
    pub workspace_dir: String,
    pub message_count: i64,
    /// Unix 毫秒
    pub created_at_ms: i64,
    /// Unix 毫秒
    pub updated_at_ms: i64,
    /// 子任务数量（主任务的派生分支数）。
    pub child_count: i64,
    /// 来源侧父会话 ID（修复旧数据主子链路用）。
    pub parent_session_id: Option<String>,
}

/// 只读打开 MiniMax runtime-state.sqlite（plan §10.1：只读快照）。
fn open_db(db_path: impl AsRef<Path>) -> AdapterResult<Connection> {
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    Ok(conn)
}

/// 列出 MiniMax 所有**主任务**会话（parentSessionId 为空），按更新时间降序。
///
/// MiniMax 的 session 有层级：主任务（`parentSessionId` 为 null）下面挂多个子任务。
/// 只在左侧列表展示主任务，避免子任务刷屏（plan §10.5：会话归并）。
/// updated_at 取主任务自身与所有子任务中的最大值，反映真实活跃时间。
pub fn discover_sessions(db_path: impl AsRef<Path>) -> AdapterResult<Vec<DiscoveredSession>> {
    let conn = open_db(&db_path)?;
    // 只选 parentSessionId 为 null 且有标题的主任务（过滤 runtime 无标题残根）
    let mut stmt = conn.prepare(
        "SELECT s.session_id, s.record_json, s.updated_at_ms,
                (SELECT count(*) FROM local_runtime_message_rows m WHERE m.session_id = s.session_id) AS msg_count,
                (SELECT count(*) FROM local_runtime_sessions c
                 WHERE json_extract(c.record_json, '$.parentSessionId') = s.session_id) AS child_count,
                (SELECT COALESCE(MAX(c.updated_at_ms), s.updated_at_ms) FROM local_runtime_sessions c
                 WHERE json_extract(c.record_json, '$.parentSessionId') = s.session_id) AS max_child_updated
         FROM local_runtime_sessions s
         WHERE json_extract(s.record_json, '$.parentSessionId') IS NULL
           AND json_extract(s.record_json, '$.title') IS NOT NULL
         ORDER BY max_child_updated DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        let session_id: String = r.get(0)?;
        let record_json: String = r.get(1)?;
        let self_updated_at_ms: i64 = r.get(2)?;
        let msg_count: i64 = r.get(3)?;
        let child_count: i64 = r.get(4)?;
        let max_child_updated: i64 = r.get(5).unwrap_or(self_updated_at_ms);
        let obj: serde_json::Value = serde_json::from_str(&record_json).unwrap_or_default();
        let title = obj
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("(无标题)")
            .to_string();
        let agent_name = obj
            .get("agentName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let workspace_dir = obj
            .get("workspaceDir")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let created_at_ms = obj
            .get("createdAtMs")
            .and_then(|v| v.as_i64())
            .unwrap_or(self_updated_at_ms);
        // 真实更新时间 = max(自身, 所有子任务)
        let updated_at_ms = self_updated_at_ms.max(max_child_updated);
        Ok(DiscoveredSession {
            session_id,
            title,
            agent_name,
            workspace_dir,
            message_count: msg_count,
            created_at_ms,
            updated_at_ms,
            child_count,
            parent_session_id: None, // discover_sessions 只返回主任务
        })
    })?;
    let mut v = Vec::new();
    for r in rows {
        v.push(r?);
    }
    Ok(v)
}

/// 列出 MiniMax **所有**会话（主任务 + 子任务），按更新时间降序。
/// 用于 auto_sync：主任务先导入（source_parent_id=null），子任务后导入（source_parent_id=父ID）。
/// 过滤 runtime 内部残留：MiniMax 的子任务 visibility=hidden 是正常形态（保留），
/// 仅排除 record_json 无 title 字段的空残根（__local_runtime_v2__ 生成）。
pub fn discover_all_sessions(db_path: impl AsRef<Path>) -> AdapterResult<Vec<DiscoveredSession>> {
    let conn = open_db(&db_path)?;
    let mut stmt = conn.prepare(
        "SELECT s.session_id, s.record_json, s.updated_at_ms,
                (SELECT count(*) FROM local_runtime_message_rows m WHERE m.session_id = s.session_id) AS msg_count,
                COALESCE(
                    (SELECT count(*) FROM local_runtime_sessions c
                     WHERE json_extract(c.record_json, '$.parentSessionId') = s.session_id), 0
                ) AS child_count,
                s.updated_at_ms AS effective_updated,
                json_extract(s.record_json, '$.parentSessionId') AS parent
         FROM local_runtime_sessions s
         WHERE json_extract(s.record_json, '$.title') IS NOT NULL
         ORDER BY effective_updated DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        let session_id: String = r.get(0)?;
        let record_json: String = r.get(1)?;
        let self_updated_at_ms: i64 = r.get(2)?;
        let msg_count: i64 = r.get(3)?;
        let child_count: i64 = r.get(4)?;
        let updated_at_ms: i64 = r.get(5).unwrap_or(self_updated_at_ms);
        let parent: Option<String> = r.get(6)?;
        let obj: serde_json::Value = serde_json::from_str(&record_json).unwrap_or_default();
        let title = obj
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("(无标题)")
            .to_string();
        let agent_name = obj
            .get("agentName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let workspace_dir = obj
            .get("workspaceDir")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let created_at_ms = obj
            .get("createdAtMs")
            .and_then(|v| v.as_i64())
            .unwrap_or(self_updated_at_ms);
        Ok(DiscoveredSession {
            session_id,
            title,
            agent_name,
            workspace_dir,
            message_count: msg_count,
            created_at_ms,
            updated_at_ms,
            child_count,
            parent_session_id: parent.filter(|s| !s.is_empty()),
        })
    })?;
    let mut v = Vec::new();
    for r in rows {
        v.push(r?);
    }
    Ok(v)
}

/// 解析单条 MiniMax 会话。
pub fn parse_session(db_path: impl AsRef<Path>, session_id: &str) -> AdapterResult<RawConversation> {
    let conn = open_db(&db_path)?;

    // 1. 会话元信息
    let (record_json,): (String,) = conn
        .query_row(
            "SELECT record_json FROM local_runtime_sessions WHERE session_id = ?1",
            params![session_id],
            |r| Ok((r.get(0)?,)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => MinimaxError::NotFound(session_id.to_string()),
            other => other.into(),
        })?;
    let sess: serde_json::Value = serde_json::from_str(&record_json)?;
    let title = sess
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let agent_name = sess
        .get("agentName")
        .and_then(|v| v.as_str())
        .unwrap_or("minimax");
    let created_at_ms = sess
        .get("createdAtMs")
        .and_then(|v| v.as_i64())
        .and_then(|ms| time::OffsetDateTime::from_unix_timestamp_nanos((ms as i128) * 1_000_000).ok());
    // 主子任务链路：parentSessionId 为 null 表示顶层主任务
    let source_parent_id = sess
        .get("parentSessionId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 2. 消息行
    let mut stmt = conn.prepare(
        "SELECT msg_id, role, created_at_ms, data_json
         FROM local_runtime_message_rows
         WHERE session_id = ?1
         ORDER BY id",
    )?;
    let rows = stmt.query_map(params![session_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;

    let mut messages = Vec::new();
    for row in rows {
        let (msg_id, role_str, created_at_ms, data_json) = row?;
        let data: serde_json::Value = serde_json::from_str(&data_json).unwrap_or_default();

        let role = match role_str.as_deref().unwrap_or("user") {
            "assistant" => Role::Assistant,
            "system" => Role::System,
            "tool" => Role::Tool,
            _ => Role::User,
        };

        // 正文：msg_content；过滤 <greeting-message /> 等装饰
        let content = data
            .get("msg_content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let text = clean_content(content);

        // 优先用 data.timestamp（消息自身时间），否则用行的 created_at_ms
        let ts_ms = data
            .get("timestamp")
            .and_then(|v| v.as_i64())
            .unwrap_or(created_at_ms);
        let created_at =
            time::OffsetDateTime::from_unix_timestamp_nanos((ts_ms as i128) * 1_000_000).ok();

        if !text.is_empty() {
            messages.push(RawMessage {
                role,
                text: Some(text),
                content_json: None,
                source_message_id: Some(msg_id),
                created_at,
            });
        }
    }

    if messages.is_empty() {
        return Err(MinimaxError::Empty(session_id.to_string()));
    }

    Ok(RawConversation {
        provider: PROVIDER,
        source_conversation_id: session_id.to_string(),
        title,
        model: Some(agent_name.to_string()),
        started_at: created_at_ms,
        messages,
        events: Vec::new(),
        source_parent_id,
    })
}

/// 清理 MiniMax 消息正文里的装饰标签（如 `<greeting-message />`、`<done />`）。
/// 只过滤「整行就是一个 XML/自闭合标签」的装饰行，保留含标签的正常代码/文本。
fn clean_content(s: &str) -> String {
    s.lines()
        .filter(|l| {
            let t = l.trim();
            if !t.starts_with('<') {
                return true; // 普通行
            }
            // 整行是一个标签：以 `<...>` 或 `<... />` 结尾，且中间没有中文/长文本
            // 用「是否只含 ASCII + 标签字符」判定装饰标签
            let inner = t
                .trim_start_matches('<')
                .trim_end_matches('>')
                .trim();
            // 装饰标签特征：不含引号、不含中文、长度短、以 / 结尾或是单个标识符
            let is_decorator = !inner.contains('"')
                && !inner.chars().any(|c| c as u32 > 0x4DFF) // 非中文/特殊符号
                && inner.len() < 40;
            !is_decorator
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_db(dir: &Path) -> std::path::PathBuf {
        let db = dir.join("runtime-state.sqlite");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            r#"CREATE TABLE local_runtime_sessions (
                session_id TEXT PRIMARY KEY,
                record_json TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            CREATE TABLE local_runtime_message_rows (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                msg_id TEXT NOT NULL,
                role TEXT,
                turn_id TEXT,
                created_at_ms INTEGER NOT NULL,
                data_json TEXT NOT NULL,
                UNIQUE(session_id, msg_id)
            );"#,
        )
        .unwrap();

        let sess = serde_json::json!({
            "sessionId": "mvs_test1",
            "title": "测试会话",
            "agentName": "coder",
            "workspaceDir": "/tmp/proj",
            "createdAtMs": 1784560908466_i64,
            "updatedAtMs": 1785594356394_i64
        });
        conn.execute(
            "INSERT INTO local_runtime_sessions (session_id, record_json, updated_at_ms) VALUES ('mvs_test1', ?1, 1785594356394)",
            params![sess.to_string()],
        )
        .unwrap();

        let m1 = serde_json::json!({
            "msg_id": "msg-1",
            "timestamp": 1784560910000_i64,
            "role": "user",
            "msg_type": 1,
            "msg_content": "帮我写个排序算法"
        });
        conn.execute(
            "INSERT INTO local_runtime_message_rows (session_id, msg_id, role, created_at_ms, data_json)
             VALUES ('mvs_test1', 'msg-1', 'user', 1784560910000, ?1)",
            params![m1.to_string()],
        )
        .unwrap();

        let m2 = serde_json::json!({
            "msg_id": "msg-2",
            "timestamp": 1784560912000_i64,
            "role": "assistant",
            "msg_type": 1,
            "msg_content": "<greeting-message />\n\n好的，这是快速排序：\n```python\ndef qs(a): pass\n```",
            "thinking_content": "用户要排序算法"
        });
        conn.execute(
            "INSERT INTO local_runtime_message_rows (session_id, msg_id, role, created_at_ms, data_json)
             VALUES ('mvs_test1', 'msg-2', 'assistant', 1784560912000, ?1)",
            params![m2.to_string()],
        )
        .unwrap();
        drop(conn);
        db
    }

    #[test]
    fn discover_lists_session() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = make_test_db(dir.path());
        let sessions = discover_sessions(&db).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "mvs_test1");
        assert_eq!(sessions[0].title, "测试会话");
        assert_eq!(sessions[0].agent_name, "coder");
        assert_eq!(sessions[0].message_count, 2);
    }

    #[test]
    fn discover_keeps_hidden_titled_drops_untitled_stubs() {
        // MiniMax 子任务 visibility=hidden 是正常形态（有标题，保留）；
        // 仅排除无 title 的 runtime 残根（__local_runtime_v2__）
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join("h.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE local_runtime_sessions (session_id TEXT PRIMARY KEY, record_json TEXT NOT NULL, updated_at_ms INTEGER NOT NULL);
             CREATE TABLE local_runtime_message_rows (id INTEGER PRIMARY KEY, session_id TEXT, msg_id TEXT, role TEXT, turn_id TEXT, created_at_ms INTEGER, data_json TEXT);",
        ).unwrap();
        // 有标题的隐藏子任务 → 保留
        conn.execute("INSERT INTO local_runtime_sessions VALUES ('c1', '{\"sessionId\":\"c1\",\"title\":\"真实子任务\",\"parentSessionId\":\"p1\",\"visibility\":\"hidden\"}', 1000)", []).unwrap();
        // 无标题残根 → 排除
        conn.execute("INSERT INTO local_runtime_sessions VALUES ('s1', '{\"sessionId\":\"s1\",\"parentSessionId\":null,\"visibility\":\"hidden\",\"archived\":true}', 2000)", []).unwrap();
        drop(conn);

        let all = discover_all_sessions(&db).unwrap();
        assert!(all.iter().any(|s| s.session_id == "c1"), "隐藏但有标题的子任务应保留");
        assert!(!all.iter().any(|s| s.session_id == "s1"), "无标题残根应排除");
        assert_eq!(all[0].parent_session_id.as_deref(), Some("p1"));
    }

    #[test]
    fn parse_extracts_messages() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = make_test_db(dir.path());
        let raw = parse_session(&db, "mvs_test1").unwrap();
        assert_eq!(raw.provider, Provider::MinimaxCode);
        assert_eq!(raw.title.as_deref(), Some("测试会话"));
        assert_eq!(raw.model.as_deref(), Some("coder"));
        assert_eq!(raw.messages.len(), 2);
        assert_eq!(raw.messages[0].role, Role::User);
        assert!(raw.messages[0].text.as_deref().unwrap().contains("排序算法"));
        assert_eq!(raw.messages[1].role, Role::Assistant);
        // greeting 装饰被清理掉
        assert!(!raw.messages[1].text.as_deref().unwrap().contains("<greeting"));
        assert!(raw.messages[1].text.as_deref().unwrap().contains("快速排序"));
        assert!(raw.started_at.is_some());
        assert!(raw.messages[0].created_at.is_some());
    }

    #[test]
    fn not_found_errors() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = make_test_db(dir.path());
        assert!(matches!(
            parse_session(&db, "nope"),
            Err(MinimaxError::NotFound(_))
        ));
    }

    #[test]
    fn empty_session_errors() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join("empty.sqlite");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE local_runtime_sessions (session_id TEXT PRIMARY KEY, record_json TEXT, updated_at_ms INTEGER);
             CREATE TABLE local_runtime_message_rows (id INTEGER PRIMARY KEY, session_id TEXT, msg_id TEXT, role TEXT, turn_id TEXT, created_at_ms INTEGER, data_json TEXT);",
        ).unwrap();
        conn.execute(
            "INSERT INTO local_runtime_sessions VALUES ('s', '{\"title\":\"x\"}', 0)",
            [],
        )
        .unwrap();
        drop(conn);
        assert!(matches!(
            parse_session(&db, "s"),
            Err(MinimaxError::Empty(_))
        ));
    }

    #[test]
    fn discover_only_lists_root_sessions() {
        // 主任务 + 2 个子任务：discover 只应返回主任务
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join("hier.sqlite");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE local_runtime_sessions (session_id TEXT PRIMARY KEY, record_json TEXT NOT NULL, updated_at_ms INTEGER NOT NULL);
             CREATE TABLE local_runtime_message_rows (id INTEGER PRIMARY KEY, session_id TEXT, msg_id TEXT, role TEXT, turn_id TEXT, created_at_ms INTEGER, data_json TEXT);",
        )
        .unwrap();
        // 主任务（updated 1000），下面挂 2 个子任务（updated 5000、6000）
        conn.execute(
            "INSERT INTO local_runtime_sessions VALUES ('parent', '{\"sessionId\":\"parent\",\"title\":\"主任务\",\"parentSessionId\":null}', 1000)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO local_runtime_sessions VALUES ('child1', '{\"sessionId\":\"child1\",\"title\":\"子任务1\",\"parentSessionId\":\"parent\"}', 5000)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO local_runtime_sessions VALUES ('child2', '{\"sessionId\":\"child2\",\"title\":\"子任务2\",\"parentSessionId\":\"parent\"}', 6000)",
            [],
        )
        .unwrap();
        drop(conn);

        let sessions = discover_sessions(&db).unwrap();
        assert_eq!(sessions.len(), 1, "只应返回主任务");
        assert_eq!(sessions[0].session_id, "parent");
        assert_eq!(sessions[0].title, "主任务");
        assert_eq!(sessions[0].child_count, 2, "应有 2 个子任务");
        // updated_at 应取子任务最大值 6000，而非自身 1000
        assert_eq!(sessions[0].updated_at_ms, 6000);
    }
}
