//! Claude Code Adapter，对应 plan §10.5「Claude Code」。
//!
//! ## 数据源
//!
//! Claude Code 把会话存在 `~/.claude/projects/<encoded-path>/<session-uuid>.jsonl`。
//! 每行一个 JSON 事件，type 含 user/assistant/system/attachment 等。
//! 消息体在 `message.content`，可能是 string 或 `[{type:text/tool_use/tool_result}]`。
//!
//! ## 能力
//!
//! - `discover_sessions`：扫描 `~/.claude/projects/` 找到所有 .jsonl 会话文件。
//! - `parse_session`：把单个 JSONL 文件解析为 `RawConversation`（含消息 + tool_use 事件）。

use ch_domain::{EventType, Provider, Role};
use ch_normalization::{RawConversation, RawEvent, RawMessage};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub type AdapterResult<T> = std::result::Result<T, ClaudeCodeError>;

#[derive(Debug, Error)]
pub enum ClaudeCodeError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid utf-8")]
    InvalidUtf8,

    #[error("no messages found in session")]
    Empty,
}

pub const ADAPTER_ID: &str = "claude-code";
pub const PROVIDER: Provider = Provider::ClaudeCode;

/// 发现的会话文件。
#[derive(Debug, Clone)]
pub struct DiscoveredSession {
    pub session_id: String,
    pub file_path: PathBuf,
    pub project_dir: String,
    pub size_bytes: u64,
    /// 文件修改时间（Unix 毫秒）——「已导入」新鲜度判定用。
    pub mtime_ms: Option<i64>,
}

/// 扫描 Claude Code 数据目录，返回所有会话文件。
///
/// `claude_home` 通常为 `~/.claude`。
pub fn discover_sessions(claude_home: impl AsRef<Path>) -> AdapterResult<Vec<DiscoveredSession>> {
    let projects_dir = claude_home.as_ref().join("projects");
    if !projects_dir.exists() {
        return Ok(Vec::new());
    }
    let mut sessions = Vec::new();
    for entry in std::fs::read_dir(&projects_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let project_dir = entry.file_name().to_string_lossy().into_owned();
        for f in std::fs::read_dir(entry.path())? {
            let f = f?;
            let path = f.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                let session_id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                let meta = f.metadata();
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                let mtime_ms = meta
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64);
                sessions.push(DiscoveredSession {
                    session_id,
                    file_path: path,
                    project_dir: project_dir.clone(),
                    size_bytes: size,
                    mtime_ms,
                });
            }
        }
    }
    // 按大小降序（大文件通常是对话更多的）
    sessions.sort_by_key(|s| std::cmp::Reverse(s.size_bytes));
    Ok(sessions)
}

/// 解析单个 Claude Code 会话 JSONL 文件。
pub fn parse_session(file_path: impl AsRef<Path>) -> AdapterResult<RawConversation> {
    let path_ref = file_path.as_ref();
    let bytes = std::fs::read(path_ref)?;
    let content = std::str::from_utf8(&bytes).map_err(|_| ClaudeCodeError::InvalidUtf8)?;
    let session_id = path_ref
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    parse_str(content, &session_id)
}

/// 从字符串解析（测试用）。
pub fn parse_str(content: &str, session_id: &str) -> AdapterResult<RawConversation> {
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let mut title: Option<String> = None;
    let mut model: Option<String> = None;
    let mut first_timestamp: Option<time::OffsetDateTime> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let obj: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // 跳过无法解析的行
        };

        let event_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match event_type {
            "user" | "assistant" => {
                if let Some(msg) = obj.get("message").and_then(|m| m.as_object()) {
                    let role = msg
                        .get("role")
                        .and_then(|r| r.as_str())
                        .unwrap_or(event_type);
                    let role = match role {
                        "assistant" => Role::Assistant,
                        "system" => Role::System,
                        "tool" => Role::Tool,
                        _ => Role::User,
                    };

                    // 提取 model
                    if model.is_none() {
                        if let Some(m) = msg.get("model").and_then(|m| m.as_str()) {
                            model = Some(m.to_string());
                        }
                    }

                    let content_val = msg.get("content");

                    // 提取 timestamp（ISO 8601 → OffsetDateTime）
                    let msg_ts = obj
                        .get("timestamp")
                        .and_then(|t| t.as_str())
                        .and_then(parse_iso_timestamp);
                    if let Some(ts) = msg_ts {
                        if first_timestamp.is_none() {
                            first_timestamp = Some(ts);
                        }
                    }

                    // 判断是否是纯 tool_result（不含 text）——这些不是真正的用户消息
                    let is_pure_tool_result = content_val
                        .and_then(|c| c.as_array())
                        .map(|arr| {
                            arr.iter().all(|item| {
                                item.get("type").and_then(|t| t.as_str())
                                    == Some("tool_result")
                            })
                        })
                        .unwrap_or(false);

                    if !is_pure_tool_result {
                        if let Some(text) = extract_text(content_val) {
                            if !text.trim().is_empty() {
                                messages.push(RawMessage {
                                    role,
                                    text: Some(text),
                                    content_json: None,
                                    source_message_id: obj
                                        .get("uuid")
                                        .and_then(|u| u.as_str())
                                        .map(String::from),
                                    created_at: msg_ts,
                                });
                            }
                        }
                    }

                    // 提取 tool_use 作为事件
                    if let Some(arr) = content_val.and_then(|c| c.as_array()) {
                        for item in arr {
                            let item_type =
                                item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            match item_type {
                                "tool_use" => {
                                    let tool_name = item
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("unknown");
                                    events.push(RawEvent {
                                        event_type: EventType::ToolCallStarted,
                                        summary: Some(format!("Tool: {tool_name}")),
                                        payload_json: Some(item.clone()),
                                        source_event_id: item
                                            .get("id")
                                            .and_then(|i| i.as_str())
                                            .map(String::from),
                                        created_at: None,
                                    });
                                }
                                "tool_result" => {
                                    events.push(RawEvent {
                                        event_type: EventType::ToolCallCompleted,
                                        summary: Some("Tool result".into()),
                                        payload_json: Some(item.clone()),
                                        source_event_id: item
                                            .get("tool_use_id")
                                            .and_then(|i| i.as_str())
                                            .map(String::from),
                                        created_at: None,
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            "ai-title" | "summary"
                // Claude Code 用 aiTitle 字段（不是 title）
                if title.is_none() => {
                    let t = obj
                        .get("aiTitle")
                        .or_else(|| obj.get("title"))
                        .or_else(|| obj.get("summary"))
                        .and_then(|t| t.as_str());
                    if let Some(t) = t {
                        title = Some(t.to_string());
                    }
                }
            _ => {}
        }
    }

    if messages.is_empty() && events.is_empty() {
        return Err(ClaudeCodeError::Empty);
    }

    Ok(RawConversation {
        provider: PROVIDER,
        source_conversation_id: session_id.to_string(),
        title,
        model,
        started_at: first_timestamp,
        messages,
        events,
        source_parent_id: None,
    })
}

/// 解析 ISO 8601 时间戳（如 `2026-07-28T00:45:13.191Z`）。
fn parse_iso_timestamp(s: &str) -> Option<time::OffsetDateTime> {
    // time crate 的 parse 需要 Format
    // 简单方案：手动解析 ISO 8601 的常见格式
    let formats = [time::format_description::well_known::Rfc3339];
    for fmt in &formats {
        if let Ok(dt) = time::OffsetDateTime::parse(s, fmt) {
            return Some(dt);
        }
    }
    None
}

/// 从 content 字段提取纯文本。
/// content 可能是 string，也可能是 [{type:text, text:...}, {type:tool_use, ...}]。
/// 过滤 thinking 类型（内部思考，不是正文）。
fn extract_text(content: Option<&serde_json::Value>) -> Option<String> {
    let val = content?;
    if let Some(s) = val.as_str() {
        return Some(s.to_string());
    }
    if let Some(arr) = val.as_array() {
        let mut texts = Vec::new();
        for item in arr {
            let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
            // 只提取 text 类型，跳过 thinking/tool_use/tool_result/image
            if item_type == "text" {
                if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                    texts.push(t.to_string());
                }
            }
        }
        if !texts.is_empty() {
            return Some(texts.join("\n"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"type":"ai-title","aiTitle":"PDF 添加书签"}
{"type":"user","message":{"role":"user","content":"给文件加上书签"}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"我应该先查看文件"},{"type":"text","text":"我来帮你处理这个 PDF。"},{"type":"tool_use","id":"tu1","name":"Bash","input":{"command":"ls"}}]}}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu1","content":"file.pdf"}]}}
{"type":"assistant","message":{"role":"assistant","model":"claude-sonnet-4","content":"完成了。"}}
"#;

    #[test]
    fn parses_user_and_assistant() {
        let raw = parse_str(SAMPLE, "test-session").unwrap();
        assert_eq!(raw.title.as_deref(), Some("PDF 添加书签"));
        // user + assistant(文字) + assistant(完成) = 3 条
        // 纯 tool_result 的 user 不计入消息
        assert_eq!(raw.messages.len(), 3);
        assert_eq!(raw.messages[0].role, Role::User);
        assert!(raw.messages[0].text.as_deref().unwrap().contains("书签"));
    }

    #[test]
    fn thinking_filtered_out() {
        let raw = parse_str(SAMPLE, "test-session").unwrap();
        // thinking 内容不应出现在任何消息中
        assert!(raw.messages.iter().all(|m| !m
            .text
            .as_deref()
            .unwrap_or("")
            .contains("我应该先查看")));
    }

    #[test]
    fn tool_result_user_filtered() {
        let raw = parse_str(SAMPLE, "test-session").unwrap();
        // 纯 tool_result 的 user 消息不应出现
        assert!(raw
            .messages
            .iter()
            .all(|m| !m.text.as_deref().unwrap_or("").contains("file.pdf")));
    }

    #[test]
    fn extracts_tool_use_and_result_as_events() {
        let raw = parse_str(SAMPLE, "test-session").unwrap();
        assert!(!raw.events.is_empty());
        assert!(raw
            .events
            .iter()
            .any(|e| e.summary.as_deref().unwrap_or("").contains("Bash")));
        assert!(raw
            .events
            .iter()
            .any(|e| e.event_type == EventType::ToolCallCompleted));
    }

    #[test]
    fn extracts_model() {
        let with_model = r#"{"type":"assistant","message":{"role":"assistant","model":"claude-sonnet-4","content":"hi"}}"#;
        let raw = parse_str(with_model, "s").unwrap();
        assert_eq!(raw.model.as_deref(), Some("claude-sonnet-4"));
    }

    #[test]
    fn handles_string_content() {
        let raw = parse_str(
            r#"{"type":"user","message":{"role":"user","content":"hello"}}"#,
            "s",
        )
        .unwrap();
        assert_eq!(raw.messages[0].text.as_deref(), Some("hello"));
    }

    #[test]
    fn handles_array_content() {
        let raw = parse_str(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"line1"},{"type":"text","text":"line2"}]}}"#,
            "s",
        )
        .unwrap();
        assert!(raw.messages[0].text.as_deref().unwrap().contains("line1"));
        assert!(raw.messages[0].text.as_deref().unwrap().contains("line2"));
    }

    #[test]
    fn empty_session_errors() {
        assert!(parse_str("", "s").is_err());
    }

    #[test]
    fn invalid_json_lines_skipped() {
        let raw = parse_str(
            "not json\n{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"ok\"}}\n",
            "s",
        )
        .unwrap();
        assert_eq!(raw.messages.len(), 1);
    }

    #[test]
    fn provider_is_claude_code() {
        let raw = parse_str(SAMPLE, "s").unwrap();
        assert_eq!(raw.provider, Provider::ClaudeCode);
    }
}
