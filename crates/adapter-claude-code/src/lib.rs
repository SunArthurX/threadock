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
//! - `parse_session`：把单个 JSONL 文件解析为 `RawConversation`（含消息 + `tool_use` 事件）。

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
        let project_name = entry.file_name().to_string_lossy().into_owned();
        for file in std::fs::read_dir(entry.path())? {
            let file = file?;
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                let session_id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                let meta = file.metadata();
                let size = meta.as_ref().map_or(0, std::fs::Metadata::len);
                let mtime_ms = meta
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64);
                sessions.push(DiscoveredSession {
                    session_id,
                    file_path: path,
                    project_dir: project_name.clone(),
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
#[allow(clippy::too_many_lines)] // JSONL 解析主循环：按消息类型分支装配，拆分反而难读
pub fn parse_str(content: &str, session_id: &str) -> AdapterResult<RawConversation> {
    let mut messages = Vec::new();
    let mut events = Vec::new();
    // 高保真上下文事件（thinking / 系统注入 / 钩子附件）：单独收集、最后整体
    // append——事件序号按 vec 位置分配（normalize），追加可让存量事件 seq 稳定；
    // 但纯注入 user 消息会从消息流移出（消息 seq 变化），重导须走 replace 路径
    let mut context_events: Vec<RawEvent> = Vec::new();
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
                        .is_some_and(|arr| {
                            arr.iter().all(|item| {
                                item.get("type").and_then(|t| t.as_str())
                                    == Some("tool_result")
                            })
                        });

                    if !is_pure_tool_result {
                        if let Some(text) = extract_text(content_val) {
                            if !text.trim().is_empty() {
                                // 纯注入块（system-reminder / IDE 选区 / 命令回显等）
                                // 是 runtime 上下文而非用户输入：移出消息流，转为
                                // SystemBlock 事件全量保留（不进全文索引、不污染
                                // 「仅用户消息」筛选与 FTS）
                                if role == Role::User && is_injected(&text) {
                                    context_events.push(RawEvent {
                                        event_type: EventType::SystemBlock,
                                        summary: Some(truncate_chars(text.trim(), 100)),
                                        payload_json: Some(serde_json::json!({ "block": text })),
                                        source_event_id: obj
                                            .get("uuid")
                                            .and_then(|u| u.as_str())
                                            .map(String::from),
                                        created_at: msg_ts,
                                    });
                                } else {
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
                    }

                    // 提取 tool_use 作为事件（摘要带实际命令/路径，而非只有工具名）
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
                                    let (event_type, summary) =
                                        summarize_tool_use(tool_name, item.get("input"));
                                    let mut payload = item.clone();
                                    strip_image_base64(&mut payload);
                                    events.push(RawEvent {
                                        event_type,
                                        summary,
                                        payload_json: Some(payload),
                                        source_event_id: item
                                            .get("id")
                                            .and_then(|i| i.as_str())
                                            .map(String::from),
                                        created_at: msg_ts,
                                    });
                                }
                                "tool_result" => {
                                    let mut payload = item.clone();
                                    strip_image_base64(&mut payload);
                                    events.push(RawEvent {
                                        event_type: EventType::ToolCallCompleted,
                                        summary: Some(summarize_tool_result(item)),
                                        payload_json: Some(payload),
                                        source_event_id: item
                                            .get("tool_use_id")
                                            .and_then(|i| i.as_str())
                                            .map(String::from),
                                        created_at: msg_ts,
                                    });
                                }
                                "thinking" => {
                                    // 思考过程全量保留为 Thinking 事件（不进全文索引；
                                    // signature 是签名回传，剥离）
                                    if let Some(t) =
                                        item.get("thinking").and_then(|t| t.as_str())
                                    {
                                        if !t.trim().is_empty() {
                                            context_events.push(RawEvent {
                                                event_type: EventType::Thinking,
                                                summary: Some(truncate_chars(t.trim(), 100)),
                                                payload_json: Some(serde_json::json!({
                                                    "thinking": t,
                                                })),
                                                source_event_id: item
                                                    .get("uuid")
                                                    .and_then(|u| u.as_str())
                                                    .map(String::from),
                                                created_at: msg_ts,
                                            });
                                        }
                                    }
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
            "attachment" => {
                // 钩子/系统附件（SessionStart hook 注入、agent 清单等）：每个会话
                // 重复注入同一段 boilerplate（实测 2.1 万条 114.7 MB），内容不落库，
                // 只记一条轻量 SystemBlock 事件（类型 + 来源 + 体量），实现去重
                let a = obj.get("attachment").cloned().unwrap_or_default();
                let a_type = a.get("type").and_then(|t| t.as_str()).unwrap_or("attachment");
                let hook_event = a.get("hookEvent").and_then(|t| t.as_str());
                let hook_name = a.get("hookName").and_then(|t| t.as_str());
                let summary = match (hook_event, hook_name) {
                    (Some(he), Some(hn)) => format!("钩子注入 {he}:{hn}"),
                    (Some(he), None) => format!("钩子注入 {he}"),
                    _ => format!("系统附件 {a_type}"),
                };
                context_events.push(RawEvent {
                    event_type: EventType::SystemBlock,
                    summary: Some(summary),
                    payload_json: Some(serde_json::json!({
                        "attachment_type": a_type,
                        "hook_event": hook_event,
                        "hook_name": hook_name,
                        "bytes": a.to_string().len(),
                        "note": "附件内容为重复 boilerplate，不落库（去重）",
                    })),
                    source_event_id: None,
                    created_at: obj
                        .get("timestamp")
                        .and_then(|t| t.as_str())
                        .and_then(parse_iso_timestamp),
                });
            }
            _ => {}
        }
    }

    // 高保真上下文事件追加在末尾（保证存量事件 seq 稳定）
    events.append(&mut context_events);

    if messages.is_empty() && events.is_empty() {
        return Err(ClaudeCodeError::Empty);
    }

    // 标题兜底：绝大多数会话没有 ai-title/summary 记录（实测 1386 个会话
    // 仅 32 个有），无标题时用首条真实用户消息生成（对齐 Codex/MiniMax 适配器，
    // GUI 不再显示「(无标题)」）。
    let title = title.or_else(|| fallback_title(&messages));

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

/// Claude Code 注入块标签（2026-10 扫描真实会话库归纳）：IDE 选区、
/// 命令回显、后台任务通知等以 user 角色入档但非用户真实输入，不作标题。
const INJECTED_TAGS: &[&str] = &[
    "<task-notification>",
    "<bash-input>",
    "<bash-stdout>",
    "<local-command-caveat>",
    "<local-command-stdout>",
    "<command-name>",
    "<ide_selection>",
    "<system-reminder>",
];

fn is_injected(text: &str) -> bool {
    let t = text.trim_start();
    INJECTED_TAGS.iter().any(|tag| t.starts_with(tag))
}

/// 无 aiTitle/summary 时的标题兜底：首条非注入 user 消息，回退首条
/// assistant 消息，截 60 字符（超出加省略号，与 Codex 适配器一致）。
fn fallback_title(messages: &[RawMessage]) -> Option<String> {
    messages
        .iter()
        .find(|m| m.role == Role::User && !m.text.as_deref().is_some_and(is_injected))
        .or_else(|| messages.iter().find(|m| m.role == Role::Assistant))
        .and_then(|m| m.text.as_deref())
        .map(|t| {
            let t = t.trim();
            let mut chars = t.chars();
            let head: String = chars.by_ref().take(60).collect();
            if chars.next().is_some() {
                format!("{head}…")
            } else {
                head
            }
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
/// tool_use →（事件类型，带内容的摘要）：Bash 显示命令、Read/Write/Edit 显示
/// 路径、TodoWrite 标注清单更新，其余显示工具名。
fn summarize_tool_use(
    tool_name: &str,
    input: Option<&serde_json::Value>,
) -> (EventType, Option<String>) {
    let input = input.unwrap_or(&serde_json::Value::Null);
    let arg = |k: &str| input.get(k).and_then(|v| v.as_str());
    match tool_name {
        "Bash" => {
            let cmd = arg("command").unwrap_or("");
            let summary = if cmd.is_empty() {
                "Bash".to_string()
            } else {
                truncate_chars(cmd.lines().next().unwrap_or("").trim(), 120)
            };
            (EventType::CommandStarted, Some(summary))
        }
        "Read" => (
            EventType::FileRead,
            Some(file_summary("读取", arg("file_path"))),
        ),
        "Write" => (
            EventType::FileCreated,
            Some(file_summary("写入", arg("file_path"))),
        ),
        "Edit" | "MultiEdit" | "NotebookEdit" => (
            EventType::FileUpdated,
            Some(file_summary("修改", arg("file_path"))),
        ),
        "Grep" | "Glob" => (
            EventType::ToolCallStarted,
            Some(format!("搜索 {}", arg("pattern").unwrap_or(""))),
        ),
        "TodoWrite" => (EventType::ToolCallStarted, Some("更新 TODO".into())),
        _ => (
            EventType::ToolCallStarted,
            Some(format!("工具 {tool_name}")),
        ),
    }
}

/// `读取 src/main.rs`（路径取 basename，防超长）。
fn file_summary(verb: &str, path: Option<&str>) -> String {
    match path {
        Some(p) if !p.is_empty() => {
            let name = p.rsplit('/').next().unwrap_or(p);
            format!("{verb} {}", truncate_chars(name, 80))
        }
        _ => verb.to_string(),
    }
}

/// tool_result → 内容预览（content 可能是字符串或 [{type:text,text}] 数组）。
fn summarize_tool_result(item: &serde_json::Value) -> String {
    let content = item.get("content").unwrap_or(&serde_json::Value::Null);
    let text = match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    };
    let t = text.trim();
    if t.is_empty() {
        "工具结果（无文本）".to_string()
    } else {
        let one_line: String = t
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        truncate_chars(&one_line, 100)
    }
}

/// 按字符截断（中文安全），超长加省略号。
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

/// content 可能是 string，也可能是 [{type:text, text:...}, {`type:tool_use`, ...}]。
/// 过滤 thinking 类型（内部思考，不是正文；思考内容由 thinking 事件单独承载）。
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

/// 递归剥离 JSON 里的 base64 图片数据（只留媒体类型引用）。
/// 截图/图片 base64 是全保真导入的体积黑洞（Codex 源 13 GB 里 99.9% 是图片
/// base64），payload 只存「这里有一张图」的引用，不存内容。
fn strip_image_base64(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            if map.get("type").and_then(|t| t.as_str()) == Some("image") {
                let media_type = map
                    .get("source")
                    .and_then(|s| s.get("media_type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown");
                map.insert(
                    "source".into(),
                    serde_json::json!({
                        "type": "stripped",
                        "media_type": media_type,
                        "note": "base64 图片不落库，只存引用",
                    }),
                );
                return;
            }
            for (_, child) in map.iter_mut() {
                strip_image_base64(child);
            }
        }
        serde_json::Value::Array(arr) => {
            for child in arr.iter_mut() {
                strip_image_base64(child);
            }
        }
        _ => {}
    }
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
        let raw = parse_str(SAMPLE, "test-session").expect("parse failed");
        assert_eq!(raw.title.as_deref(), Some("PDF 添加书签"));
        // user + assistant(文字) + assistant(完成) = 3 条
        // 纯 tool_result 的 user 不计入消息
        assert_eq!(raw.messages.len(), 3);
        assert_eq!(raw.messages[0].role, Role::User);
        assert!(raw.messages[0]
            .text
            .as_deref()
            .expect("unexpected None")
            .contains("书签"));
    }

    #[test]
    fn thinking_filtered_out() {
        let raw = parse_str(SAMPLE, "test-session").expect("parse failed");
        // thinking 内容不应出现在任何消息中
        assert!(raw.messages.iter().all(|m| !m
            .text
            .as_deref()
            .unwrap_or("")
            .contains("我应该先查看")));
    }

    /// 全保真上下文：thinking 块 → Thinking 事件（全量保留，追加在事件末尾）。
    #[test]
    fn thinking_becomes_context_event() {
        let raw = parse_str(SAMPLE, "test-session").expect("parse failed");
        let th = raw
            .events
            .iter()
            .find(|e| e.event_type == EventType::Thinking)
            .expect("thinking 事件");
        assert!(th
            .payload_json
            .as_ref()
            .and_then(|p| p.get("thinking"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| s.contains("我应该先查看")));
        // 追加在末尾：thinking 事件在所有工具事件之后（存量 seq 稳定）
        let th_idx = raw
            .events
            .iter()
            .position(|e| e.event_type == EventType::Thinking)
            .expect("idx");
        assert!(th_idx >= raw.events.len() - 1);
    }

    /// 纯注入 user 消息（system-reminder 等）→ SystemBlock 事件，不进消息流、
    /// 不进 FTS（全保真导入的「系统块」部分）。
    #[test]
    fn injected_user_message_becomes_system_block() {
        let content = r#"{"type":"user","message":{"role":"user","content":"<system-reminder>\nSessionStart hook 注入的长文本\n</system-reminder>"}}
{"type":"user","message":{"role":"user","content":"真实提问"}}"#;
        let raw = parse_str(content, "s").expect("parse failed");
        assert_eq!(raw.messages.len(), 1, "注入块移出消息流");
        assert_eq!(raw.messages[0].text.as_deref(), Some("真实提问"));
        let sb = raw
            .events
            .iter()
            .find(|e| e.event_type == EventType::SystemBlock)
            .expect("system block 事件");
        assert!(sb
            .payload_json
            .as_ref()
            .and_then(|p| p.get("block"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| s.contains("hook 注入")));
        // 标题兜底取真实提问，不取注入块
        assert_eq!(raw.title.as_deref(), Some("真实提问"));
    }

    /// 钩子附件（SessionStart hook boilerplate，实测 2.1 万条 114.7 MB）：
    /// 内容不落库，只记轻量 SystemBlock 事件（类型 + 体量），实现去重。
    #[test]
    fn attachment_record_becomes_light_system_block() {
        let content = r#"{"type":"user","message":{"role":"user","content":"hi"}}
{"parentUuid":"p1","type":"attachment","timestamp":"2026-10-01T10:00:00.000Z","attachment":{"type":"hook_success","hookName":"SessionStart:startup","hookEvent":"SessionStart","content":"","stdout":"<EXTREMELY_IMPORTANT> superpowers skill 全文……"}}"#;
        let raw = parse_str(content, "s").expect("parse failed");
        let sb = raw
            .events
            .iter()
            .find(|e| e.event_type == EventType::SystemBlock)
            .expect("附件轻量事件");
        let p = sb.payload_json.as_ref().expect("payload");
        assert_eq!(
            p.get("attachment_type").and_then(|v| v.as_str()),
            Some("hook_success")
        );
        assert_eq!(
            p.get("hook_name").and_then(|v| v.as_str()),
            Some("SessionStart:startup")
        );
        assert!(p.get("bytes").and_then(serde_json::Value::as_i64).is_some());
        // 内容与 stdout 都不落库（去重的关键）
        assert!(p.get("stdout").is_none());
        assert!(p.get("content").is_none());
        assert!(!sb
            .summary
            .as_deref()
            .unwrap_or("")
            .contains("superpowers skill 全文"));
    }

    /// 全保真体积黑洞防护：tool_result 里的 base64 图片只留引用不落内容。
    #[test]
    fn tool_result_image_base64_stripped() {
        let content = r#"{"type":"user","message":{"role":"user","content":"看图"}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/a.png"}}]}}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgoAAAANSUhEUg是超长base64图片数据"}}]}]}}"#;
        let raw = parse_str(content, "s").expect("parse failed");
        let result = raw
            .events
            .iter()
            .find(|e| e.event_type == EventType::ToolCallCompleted)
            .expect("tool_result 事件");
        let serialized = result.payload_json.as_ref().expect("payload").to_string();
        assert!(
            !serialized.contains("iVBORw0KGgo"),
            "base64 图片数据必须剥离"
        );
        assert!(serialized.contains("image/png"), "媒体类型引用保留");
    }

    #[test]
    fn tool_result_user_filtered() {
        let raw = parse_str(SAMPLE, "test-session").expect("parse failed");
        // 纯 tool_result 的 user 消息不应出现
        assert!(raw
            .messages
            .iter()
            .all(|m| !m.text.as_deref().unwrap_or("").contains("file.pdf")));
    }

    #[test]
    fn extracts_tool_use_and_result_as_events() {
        let raw = parse_str(SAMPLE, "test-session").expect("parse failed");
        assert!(!raw.events.is_empty());
        // Bash 的摘要应是命令本身（而非 "Tool: Bash"）
        let bash = raw
            .events
            .iter()
            .find(|e| e.event_type == EventType::CommandStarted)
            .expect("Bash → CommandStarted");
        assert_eq!(bash.summary.as_deref(), Some("ls"));
        assert!(raw
            .events
            .iter()
            .any(|e| e.event_type == EventType::ToolCallCompleted));
    }

    #[test]
    fn tool_summaries_carry_content() {
        let assistant_tools = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/a/b/Config.kt"}},{"type":"tool_use","id":"t2","name":"Edit","input":{"file_path":"/a/b/Main.rs","old_string":"x"}},{"type":"tool_use","id":"t3","name":"TodoWrite","input":{"todos":[]}},{"type":"tool_use","id":"t4","name":"mcp_server__tool","input":{"k":"v"}},{"type":"tool_use","id":"t5","name":"Bash","input":{"command":"cargo build\n&& cargo test"}}]}}"#;
        let result_line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t5","content":"   Compiling x\n Finished dev"}]}}"#;
        let user_line = r#"{"type":"user","message":{"role":"user","content":"go"}}"#;
        let raw = parse_str(&[user_line, assistant_tools, result_line].join("\n"), "s")
            .expect("parse failed");
        let find = |id: &str| {
            raw.events
                .iter()
                .find(|e| e.source_event_id.as_deref() == Some(id))
                .unwrap_or_else(|| panic!("event {id} missing"))
        };
        assert_eq!(find("t1").event_type, EventType::FileRead);
        assert_eq!(find("t1").summary.as_deref(), Some("读取 Config.kt"));
        assert_eq!(find("t2").summary.as_deref(), Some("修改 Main.rs"));
        assert_eq!(find("t3").summary.as_deref(), Some("更新 TODO"));
        assert!(find("t4")
            .summary
            .as_deref()
            .is_some_and(|s| s.contains("mcp_server__tool")));
        // 多行命令取首行
        assert_eq!(find("t5").summary.as_deref(), Some("cargo build"));
        // 结果预览：压缩空白（同一 t5 键，按事件类型区分调用与结果）
        let result = raw
            .events
            .iter()
            .find(|e| {
                e.source_event_id.as_deref() == Some("t5")
                    && e.event_type == EventType::ToolCallCompleted
            })
            .expect("result event");
        assert!(result
            .summary
            .as_deref()
            .expect("unexpected None")
            .contains("Compiling x"));
    }

    #[test]
    fn extracts_model() {
        let with_model = r#"{"type":"assistant","message":{"role":"assistant","model":"claude-sonnet-4","content":"hi"}}"#;
        let raw = parse_str(with_model, "s").expect("parse failed");
        assert_eq!(raw.model.as_deref(), Some("claude-sonnet-4"));
    }

    #[test]
    fn handles_string_content() {
        let raw = parse_str(
            r#"{"type":"user","message":{"role":"user","content":"hello"}}"#,
            "s",
        )
        .expect("unexpected None");
        assert_eq!(raw.messages[0].text.as_deref(), Some("hello"));
    }

    #[test]
    fn handles_array_content() {
        let raw = parse_str(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"line1"},{"type":"text","text":"line2"}]}}"#,
            "s",
        )
        .expect("unexpected None");
        assert!(raw.messages[0]
            .text
            .as_deref()
            .expect("unexpected None")
            .contains("line1"));
        assert!(raw.messages[0]
            .text
            .as_deref()
            .expect("unexpected None")
            .contains("line2"));
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
        .expect("unexpected None");
        assert_eq!(raw.messages.len(), 1);
    }

    #[test]
    fn provider_is_claude_code() {
        let raw = parse_str(SAMPLE, "s").expect("parse failed");
        assert_eq!(raw.provider, Provider::ClaudeCode);
    }

    /// 回归（2026-10 真实库发现）：绝大多数会话没有 ai-title/summary 记录，
    /// 标题需回退到首条用户消息，GUI 不再显示「(无标题)」。
    #[test]
    fn title_falls_back_to_first_user_message() {
        let no_title = r#"{"type":"user","message":{"role":"user","content":"执行 claude-obsidian 的 wiki-ingest skill 方法论"}}
{"type":"assistant","message":{"role":"assistant","model":"claude-sonnet-4","content":"开始执行。"}}"#;
        let raw = parse_str(no_title, "s").expect("parse failed");
        assert_eq!(
            raw.title.as_deref(),
            Some("执行 claude-obsidian 的 wiki-ingest skill 方法论")
        );
    }

    /// aiTitle 存在时优先于首条消息兜底。
    #[test]
    fn ai_title_takes_precedence_over_fallback() {
        let with_title = r#"{"type":"ai-title","aiTitle":"知识库摄取"}
{"type":"user","message":{"role":"user","content":"执行 wiki-ingest"}}"#;
        let raw = parse_str(with_title, "s").expect("parse failed");
        assert_eq!(raw.title.as_deref(), Some("知识库摄取"));
    }

    /// 注入块（任务通知 / IDE 选区 / 命令回显）不作标题，跳到下一条真实输入。
    #[test]
    fn fallback_skips_injected_first_messages() {
        let injected_first = r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>bf7os1swy</task-id>\n</task-notification>"}}
{"type":"user","message":{"role":"user","content":"<ide_selection>The user selected the lines 88 to 103</ide_selection>"}}
{"type":"user","message":{"role":"user","content":"<bash-input>ls</bash-input>"}}
{"type":"user","message":{"role":"user","content":"帮我看看选中的这段代码为什么慢"}}"#;
        let raw = parse_str(injected_first, "s").expect("parse failed");
        assert_eq!(raw.title.as_deref(), Some("帮我看看选中的这段代码为什么慢"));
    }

    /// 超长首条消息截 60 字符加省略号。
    #[test]
    fn fallback_title_truncates_to_60_chars() {
        let long = "一".repeat(80);
        let raw = parse_str(
            &format!(r#"{{"type":"user","message":{{"role":"user","content":"{long}"}}}}"#),
            "s",
        )
        .expect("parse failed");
        let title = raw.title.expect("unexpected None");
        assert_eq!(title.chars().count(), 61, "60 字 + 省略号");
        assert!(title.ends_with('…'));
    }
}
