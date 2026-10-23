//! Cursor 写回器：`state.vscdb` 的 `cursorDiskKV`（bubble 行 + composerData 索引）
//! 与 `composerHeaders`（会话头）。
//!
//! 字段集镜像 2026-10 真实库观测（`_v`、grouping 标志、空数组占位），
//! value 按 TEXT 写入（新版 Cursor 的存储形态）。纯增量：全新 composerId，
//! 绝不触碰既有 key。Cursor 桌面端正持有该库时可能 busy，5s 超时后报错。

use crate::transcript::{build_transcript, truncate_chars, TranscriptItem};
use crate::{ts_ms, HandoffError, HandoffInput, HandoffOptions, HandoffOutcome, HandoffResult};
use ch_domain::{Role, Timestamp};
use rusqlite::Connection;
use std::path::PathBuf;
use std::time::Duration;
use time::macros::format_description;
use uuid::Uuid;

/// 真实 bubble 行里随 _v:3 出现的空数组字段（照抄以最大化兼容）。
const BUBBLE_EMPTY_ARRAYS: &[&str] = &[
    "approximateLintErrors",
    "lints",
    "codebaseContextChunks",
    "commits",
    "pullRequests",
    "attachedCodeChunks",
    "assistantSuggestedDiffs",
    "gitDiffs",
    "interpreterResults",
    "images",
    "attachedFolders",
    "attachedFoldersNew",
    "userResponsesToSuggestedCodeBlocks",
    "suggestedCodeBlocks",
    "diffsForCompressingFiles",
    "relevantFiles",
    "toolResults",
    "notepads",
    "capabilities",
    "multiFileLinterErrors",
    "diffHistories",
    "recentLocationsHistory",
    "recentlyViewedFiles",
];

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

fn iso_millis(t: Timestamp) -> String {
    let fmt =
        format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");
    t.format(&fmt).unwrap_or_default()
}

/// 真实 bubble 行 JSON（_v:3 + 空数组占位 + text）。
fn bubble_json(item: &TranscriptItem, bubble_id: &str) -> String {
    let mut obj = serde_json::json!({
        "_v": 3,
        "type": if item.role == Role::Assistant { 2 } else { 1 },
        "isAgentic": false,
        "bubbleId": bubble_id,
        "text": item.text,
    });
    if let Some(map) = obj.as_object_mut() {
        for key in BUBBLE_EMPTY_ARRAYS {
            map.insert((*key).to_string(), serde_json::json!([]));
        }
    }
    obj.to_string()
}

/// composerData 索引里的单条 header（grouping 对齐真实形态）。
fn header_json(item: &TranscriptItem, bubble_id: &str) -> serde_json::Value {
    let bubble_type = if item.role == Role::Assistant { 2 } else { 1 };
    let mut grouping = serde_json::json!({
        "isRenderable": true,
        "hasText": true,
        "isShortPlainText": true,
    });
    if item.role == Role::Assistant {
        grouping["textPreview"] = serde_json::json!(truncate_chars(&item.text, 60));
    }
    serde_json::json!({
        "bubbleId": bubble_id,
        "type": bubble_type,
        "grouping": grouping,
        "createdAt": iso_millis(item.at),
        "startedAtMs": ts_ms(item.at),
    })
}

/// 写入一条 Cursor 会话（KV 行 + composerHeaders 头）。
#[allow(clippy::too_many_lines)] // KV 行 / 索引 / 头三段装配一体，拆分反而难对账
pub fn write(input: &HandoffInput, opts: &HandoffOptions) -> HandoffResult<HandoffOutcome> {
    let items = build_transcript(input, opts);
    if items.is_empty() {
        return Err(HandoffError::Empty);
    }

    let composer_id = Uuid::new_v4().to_string();
    let db_path: PathBuf = crate::resolve_dest(crate::HandoffTarget::Cursor, opts);
    let now_ms = ts_ms(ch_domain::now_utc());
    let title = input.title().to_string();

    if opts.dry_run {
        return Ok(HandoffOutcome {
            target: "cursor".into(),
            conversation_id: input.conversation.id.clone(),
            title,
            session_id: composer_id.clone(),
            destination: db_path.to_string_lossy().into_owned(),
            items_written: items.len(),
            dry_run: true,
        });
    }

    let conn = open_target(&db_path)?;
    crate::ensure_tables(&conn, &["cursorDiskKV", "composerHeaders"])?;
    let tx = conn.unchecked_transaction()?;

    // 1. bubble 行
    let mut headers = Vec::with_capacity(items.len());
    for item in &items {
        let bubble_id = Uuid::new_v4().to_string();
        {
            let key = format!("bubbleId:{composer_id}:{bubble_id}");
            let mut stmt = tx.prepare("INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)")?;
            stmt.execute(rusqlite::params![key, bubble_json(item, &bubble_id)])?;
        }
        headers.push(header_json(item, &bubble_id));
    }

    // 2. composerData 索引行
    {
        let index = serde_json::json!({
            "_v": 18,
            "composerId": composer_id,
            "richText": "",
            "hasLoaded": true,
            "text": "",
            "fullConversationHeadersOnly": headers,
            "conversationMarkdown": "",
        })
        .to_string();
        let key = format!("composerData:{composer_id}");
        let mut stmt = tx.prepare("INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)")?;
        stmt.execute(rusqlite::params![key, index])?;
    }

    // 3. composerHeaders 会话头
    {
        let head = serde_json::json!({
            "type": "head",
            "composerId": composer_id,
            "name": title,
            "createdAt": now_ms,
            "unifiedMode": "agent",
            "forceMode": "edit",
        })
        .to_string();
        let mut stmt = tx.prepare(
            "INSERT INTO composerHeaders \
             (composerId, workspaceId, createdAt, lastUpdatedAt, isArchived, isSubagent, \
              recency, checkpointAt, subagentTypeName, value) \
             VALUES (?1, NULL, ?2, ?2, 0, 0, ?2, NULL, NULL, ?3)",
        )?;
        stmt.execute(rusqlite::params![composer_id, now_ms, head])?;
    }

    tx.commit()?;

    Ok(HandoffOutcome {
        target: "cursor".into(),
        conversation_id: input.conversation.id.clone(),
        title: input.title().to_string(),
        session_id: composer_id,
        destination: db_path.to_string_lossy().into_owned(),
        items_written: items.len(),
        dry_run: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::make_input;

    const DDL: &str = r"
CREATE TABLE cursorDiskKV (key TEXT PRIMARY KEY, value BLOB);
CREATE TABLE composerHeaders (
    composerId TEXT PRIMARY KEY, workspaceId TEXT, createdAt INTEGER,
    lastUpdatedAt INTEGER, isArchived INTEGER, isSubagent INTEGER, recency INTEGER,
    checkpointAt INTEGER, subagentTypeName TEXT, value TEXT
);
";

    #[test]
    fn writes_kv_and_roundtrips() {
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let db = dir.path().join("state.vscdb");
        let conn = Connection::open(&db).expect("db creation failed");
        conn.execute_batch(DDL).expect("ddl failed");
        drop(conn);

        let input = make_input();
        let opts = HandoffOptions {
            dest_override: Some(db.clone()),
            ..HandoffOptions::new("/nonexistent-home")
        };
        let out = write(&input, &opts).expect("write failed");

        let raw =
            ch_adapter_cursor::parse_session(&db, &out.session_id).expect("roundtrip parse failed");
        assert_eq!(raw.provider, ch_domain::Provider::Cursor);
        assert_eq!(raw.source_conversation_id, out.session_id);
        assert!(raw.title.as_deref().is_some_and(|t| t.contains("修登录")));
        assert!(raw.messages.len() >= 3, "preamble + dialog");
        assert!(raw.messages.iter().any(|m| m
            .text
            .as_deref()
            .is_some_and(|t| t.contains("[threadock 交接存档]"))));
    }

    #[test]
    fn bubble_json_carries_empty_array_fields() {
        let item = TranscriptItem {
            role: Role::User,
            text: "hi".into(),
            at: Timestamp::UNIX_EPOCH,
        };
        let json = bubble_json(&item, "b1");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse failed");
        assert_eq!(v["type"], 1);
        assert_eq!(v["text"], "hi");
        assert!(v["toolResults"].is_array(), "空数组占位字段必须在");
    }

    #[test]
    fn missing_db_errors() {
        let input = make_input();
        let opts = HandoffOptions {
            dest_override: Some(PathBuf::from("/nonexistent/vscdb")),
            ..HandoffOptions::new("/nonexistent-home")
        };
        assert!(write(&input, &opts).is_err());
    }
}
