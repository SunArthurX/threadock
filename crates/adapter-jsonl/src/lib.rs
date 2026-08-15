//! 通用 JSONL Adapter，对应 plan §10.5「OpenCode、Hermes 等」与 §22 `adapters/generic-jsonl/`。
//!
//! ## JSONL 约定（v0.1）
//!
//! 每行一个 JSON 对象，`type` 字段决定类别：
//!
//! ```jsonl
//! {"type":"meta","title":"会话标题","model":"gpt-4"}
//! {"type":"message","role":"user","text":"你好"}
//! {"type":"message","role":"assistant","text":"你好啊","content_json":{...}}
//! {"type":"event","event_type":"command_started","summary":"cargo build"}
//! {"type":"event","event_type":"diff_generated","summary":"main.rs 改动"}
//! ```
//!
//! - 首个 `meta` 行可选，提供 title/model。
//! - `role`：user / assistant / system / tool（缺省 user）。
//! - `event_type`：对应 ch_domain::EventType 的 snake_case 名（见 §12.2）。
//! - 未知 `type` 行被忽略（容错，plan §11.6 不猜测字段）。

use ch_domain::{EventType, Provider, Role};
use ch_normalization::{RawConversation, RawEvent, RawMessage};
use std::path::Path;
use thiserror::Error;

pub type AdapterResult<T> = std::result::Result<T, JsonlAdapterError>;

#[derive(Debug, Error)]
pub enum JsonlAdapterError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid utf-8")]
    InvalidUtf8,

    #[error("json parse error at line {line}: {source}")]
    Json {
        line: usize,
        #[source]
        source: serde_json::Error,
    },

    #[error("jsonl has no message or event records")]
    Empty,

    #[error("unknown event_type: {0}")]
    UnknownEventType(String),
}

pub const ADAPTER_ID: &str = "generic-jsonl";
pub const ADAPTER_VERSION: &str = "0.1.0";
pub const PROVIDER: Provider = Provider::Generic;

/// 单行的可识别结构（宽松解析，未知字段忽略）。
#[derive(Debug, Clone, serde::Deserialize)]
struct Record {
    #[serde(rename = "type")]
    kind: String,
    // meta
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    model: Option<String>,
    // message
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    content_json: Option<serde_json::Value>,
    // event
    #[serde(default)]
    event_type: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    // 通用
    #[serde(default)]
    source_id: Option<String>,
}

/// 从文件解析。
pub fn parse_file(path: impl AsRef<Path>) -> AdapterResult<RawConversation> {
    let path_ref = path.as_ref();
    let bytes = std::fs::read(path_ref)?;
    let content = std::str::from_utf8(&bytes).map_err(|_| JsonlAdapterError::InvalidUtf8)?;
    parse_str(content, &path_ref.to_string_lossy())
}

/// 从字符串解析。`source_id` 用作 source_conversation_id。
pub fn parse_str(content: &str, source_id: &str) -> AdapterResult<RawConversation> {
    let mut title: Option<String> = None;
    let mut model: Option<String> = None;
    let mut messages: Vec<RawMessage> = Vec::new();
    let mut events: Vec<RawEvent> = Vec::new();

    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let rec: Record = serde_json::from_str(trimmed).map_err(|e| JsonlAdapterError::Json {
            line: i + 1,
            source: e,
        })?;

        match rec.kind.as_str() {
            "meta" => {
                if title.is_none() {
                    title = rec.title;
                }
                if model.is_none() {
                    model = rec.model;
                }
            }
            "message" => {
                let role = parse_role(rec.role.as_deref());
                messages.push(RawMessage {
                    role,
                    text: rec.text,
                    content_json: rec.content_json,
                    source_message_id: rec.source_id,
                    created_at: None,
                });
            }
            "event" => {
                let et = rec
                    .event_type
                    .as_deref()
                    .ok_or_else(|| JsonlAdapterError::UnknownEventType("(missing)".into()))?;
                let event_type = parse_event_type(et)
                    .ok_or_else(|| JsonlAdapterError::UnknownEventType(et.to_string()))?;
                events.push(RawEvent {
                    event_type,
                    summary: rec.summary,
                    payload_json: rec.content_json,
                    source_event_id: rec.source_id,
                    created_at: None,
                });
            }
            // 未知 type 容错忽略（plan §11.6）
            other => {
                tracing::debug!(line = i + 1, kind = other, "skipping unknown record type");
            }
        }
    }

    if messages.is_empty() && events.is_empty() {
        return Err(JsonlAdapterError::Empty);
    }

    Ok(RawConversation {
        provider: PROVIDER,
        source_conversation_id: source_id.to_string(),
        title,
        model,
        started_at: None,
        messages,
        events,
        source_parent_id: None,
    })
}

fn parse_role(s: Option<&str>) -> Role {
    match s.map(|x| x.to_lowercase()).as_deref() {
        Some("assistant") | Some("ai") | Some("model") => Role::Assistant,
        Some("system") => Role::System,
        Some("tool") => Role::Tool,
        _ => Role::User, // 缺省 user
    }
}

fn parse_event_type(s: &str) -> Option<EventType> {
    Some(match s {
        "tool_call_started" => EventType::ToolCallStarted,
        "tool_call_completed" => EventType::ToolCallCompleted,
        "command_started" => EventType::CommandStarted,
        "command_completed" => EventType::CommandCompleted,
        "file_read" => EventType::FileRead,
        "file_created" => EventType::FileCreated,
        "file_updated" => EventType::FileUpdated,
        "file_deleted" => EventType::FileDeleted,
        "diff_generated" => EventType::DiffGenerated,
        "approval_requested" => EventType::ApprovalRequested,
        "approval_granted" => EventType::ApprovalGranted,
        "approval_denied" => EventType::ApprovalDenied,
        "browser_action" => EventType::BrowserAction,
        "mcp_call" => EventType::McpCall,
        "subagent_started" => EventType::SubagentStarted,
        "subagent_completed" => EventType::SubagentCompleted,
        "plan_created" => EventType::PlanCreated,
        "status_changed" => EventType::StatusChanged,
        "error" => EventType::Error,
        "artifact_created" => EventType::ArtifactCreated,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_normalization::normalize;

    const SAMPLE: &str = r#"{"type":"meta","title":"JSONL 测试","model":"gpt-test"}
{"type":"message","role":"user","text":"你好"}
{"type":"message","role":"assistant","text":"你好啊"}
{"type":"event","event_type":"command_started","summary":"cargo build"}
{"type":"event","event_type":"diff_generated","summary":"main.rs"}
"#;

    #[test]
    fn parses_meta_and_messages() {
        let raw = parse_str(SAMPLE, "test.jsonl").unwrap();
        assert_eq!(raw.title.as_deref(), Some("JSONL 测试"));
        assert_eq!(raw.model.as_deref(), Some("gpt-test"));
        assert_eq!(raw.messages.len(), 2);
        assert_eq!(raw.messages[0].role, Role::User);
        assert_eq!(raw.messages[1].role, Role::Assistant);
    }

    #[test]
    fn parses_events() {
        let raw = parse_str(SAMPLE, "test.jsonl").unwrap();
        assert_eq!(raw.events.len(), 2);
        assert!(raw
            .events
            .iter()
            .any(|e| e.event_type == EventType::CommandStarted));
        assert!(raw
            .events
            .iter()
            .any(|e| e.event_type == EventType::DiffGenerated));
    }

    #[test]
    fn empty_lines_skipped() {
        let s = "{\"type\":\"message\",\"role\":\"user\",\"text\":\"hi\"}\n\n   \n";
        let raw = parse_str(s, "x").unwrap();
        assert_eq!(raw.messages.len(), 1);
    }

    #[test]
    fn no_meta_still_works() {
        let s = "{\"type\":\"message\",\"role\":\"user\",\"text\":\"hi\"}\n";
        let raw = parse_str(s, "x").unwrap();
        assert!(raw.title.is_none());
        assert_eq!(raw.messages.len(), 1);
    }

    #[test]
    fn default_role_is_user() {
        let s = "{\"type\":\"message\",\"text\":\"no role\"}\n";
        let raw = parse_str(s, "x").unwrap();
        assert_eq!(raw.messages[0].role, Role::User);
    }

    #[test]
    fn unknown_type_ignored() {
        let s = "{\"type\":\"weird\",\"data\":1}\n{\"type\":\"message\",\"role\":\"user\",\"text\":\"hi\"}\n";
        let raw = parse_str(s, "x").unwrap();
        assert_eq!(raw.messages.len(), 1);
    }

    #[test]
    fn unknown_event_type_errors() {
        let s = "{\"type\":\"event\",\"event_type\":\"not_a_real_type\",\"summary\":\"x\"}\n";
        assert!(parse_str(s, "x").is_err());
    }

    #[test]
    fn content_json_preserved() {
        let s = "{\"type\":\"message\",\"role\":\"tool\",\"text\":\"result\",\"content_json\":{\"tool\":\"bash\",\"code\":0}}\n";
        let raw = parse_str(s, "x").unwrap();
        assert!(raw.messages[0].content_json.is_some());
        let json = raw.messages[0].content_json.as_ref().unwrap();
        assert_eq!(json["tool"], "bash");
    }

    #[test]
    fn empty_file_errors() {
        assert!(parse_str("", "x").is_err());
        assert!(parse_str("   \n\n", "x").is_err());
    }

    #[test]
    fn only_events_ok() {
        let s = "{\"type\":\"event\",\"event_type\":\"error\",\"summary\":\"boom\"}\n";
        let raw = parse_str(s, "x").unwrap();
        assert_eq!(raw.events.len(), 1);
        assert!(raw.messages.is_empty());
    }

    #[test]
    fn file_roundtrip() {
        use tempfile::NamedTempFile;
        let f = NamedTempFile::new().unwrap();
        std::fs::write(f.path(), SAMPLE).unwrap();
        let raw = parse_file(f.path()).unwrap();
        assert_eq!(raw.messages.len(), 2);
    }

    #[test]
    fn integrates_with_normalization() {
        let raw = parse_str(SAMPLE, "test.jsonl").unwrap();
        let n = normalize(raw).unwrap();
        assert_eq!(n.messages.len(), 2);
        assert_eq!(n.events.len(), 2);
        // 有 command + diff → 至少 Partial
        assert!(matches!(n.completeness.label(), "部分" | "完整"));
    }

    #[test]
    fn invalid_json_reports_line_number() {
        let s = "{\"type\":\"message\",\"role\":\"user\",\"text\":\"ok\"}\n{bad json}\n";
        let err = parse_str(s, "x").unwrap_err();
        match err {
            JsonlAdapterError::Json { line, .. } => assert_eq!(line, 2),
            _ => panic!("expected Json error, got {err:?}"),
        }
    }
}
