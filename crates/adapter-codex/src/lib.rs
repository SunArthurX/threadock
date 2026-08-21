//! Codex (`ChatGPT` CLI / Codex Desktop) Adapter，对应 plan §10.5「Codex」。
//!
//! ## 数据源
//!
//! Codex 把会话存在 `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`
//! 与 `~/.codex/archived_sessions/`，每行一个 JSON 记录：
//!
//! - `session_meta`：`payload.{id, timestamp, cwd, source}` 会话元信息
//! - `response_item`：`payload.type`：
//!   - `message`（`role=user` → `content[].input_text`；
//!     `role=assistant` → `content[].output_text`）
//!   - `function_call`（旧式：`name`/`arguments` JSON）→ 命令/工具事件
//!   - `custom_tool_call`（新式 JS 工具桥：命令内容在 `input` 字段的
//!     `tools.xxx({...})` JS 代码里，`arguments` 恒空）→ 见 [`js_bridge`]
//!   - `function_call_output` / `custom_tool_call_output`：按 `call_id`
//!     与调用配对，把执行输出合并进对应事件的 payload
//! - `event_msg`：`payload.type=token_count` 的 `info.total_token_usage`
//!   为累计用量（ops 数据，由 ch-ops-metrics 消费）
//!
//! ## 降噪
//!
//! - `name=wait` 的轮询调用（等生图/长任务的空转）不产生事件；
//! - `get_goal` 等纯读取跳过。
//!
//! ## 能力
//!
//! - `discover_sessions`：列出 sessions/ 与 `archived_sessions`/ 下所有 .jsonl
//! - `parse_session`：读取单条会话 → RawConversation（消息 + 工具事件 + 时间戳）

pub mod js_bridge;

use ch_domain::{EventType, Provider, Role};
use ch_normalization::{RawConversation, RawEvent, RawMessage};
use js_bridge::{extract_js_tool_calls, output_to_text, JsToolCall};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

pub type AdapterResult<T> = std::result::Result<T, CodexError>;

#[derive(Debug, Error)]
pub enum CodexError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("session not found: {0}")]
    NotFound(String),

    #[error("session {0} has no messages")]
    Empty(String),
}

pub const ADAPTER_ID: &str = "codex";
pub const PROVIDER: Provider = Provider::Codex;

/// 发现的 Codex 会话。
#[derive(Debug, Clone)]
pub struct DiscoveredSession {
    pub session_id: String,
    /// 标题：取首条用户消息前 60 字（Codex 无显式标题）。
    pub title: String,
    pub message_count: i64,
    /// 首条记录时间（ISO 字符串）。
    pub created_at: Option<String>,
    pub file_path: String,
    pub size_bytes: u64,
    /// 文件修改时间（Unix 毫秒）——「已导入」新鲜度判定用。
    pub mtime_ms: Option<i64>,
}

/// 扫描目录下所有 .jsonl（按修改时间降序）。
fn scan_dir(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            scan_dir(&p, out);
        } else if p.extension().and_then(|x| x.to_str()) == Some("jsonl") {
            out.push(p);
        }
    }
}

/// 列出 Codex 所有会话（sessions/ + `archived_sessions/，按文件大小降序`）。
pub fn discover_sessions(codex_home: impl AsRef<Path>) -> AdapterResult<Vec<DiscoveredSession>> {
    let home = codex_home.as_ref();
    let mut files = Vec::new();
    scan_dir(&home.join("sessions"), &mut files);
    scan_dir(&home.join("archived_sessions"), &mut files);

    let mut sessions = Vec::new();
    for f in files {
        // 快速读第一行拿 session_meta
        let Ok(meta_line) = read_first_json_line(&f) else {
            continue;
        };
        let session_id = meta_line
            .pointer("/payload/id")
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string);
        let Some(session_id) = session_id else {
            continue;
        };
        let created_at = meta_line
            .get("timestamp")
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string);
        let meta = std::fs::metadata(&f);
        let size = meta.as_ref().map_or(0, std::fs::Metadata::len);
        let mtime_ms = meta
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64);
        sessions.push(DiscoveredSession {
            session_id,
            title: format!("Codex 会话 ({} KB)", size / 1024),
            message_count: 0,
            created_at,
            file_path: f.to_string_lossy().into_owned(),
            size_bytes: size,
            mtime_ms,
        });
    }
    sessions.sort_by_key(|s| std::cmp::Reverse(s.size_bytes));
    Ok(sessions)
}

/// JSONL 单行上限：超过视为二进制负载跳过（防超长行内存尖峰）。
const MAX_LINE: usize = 2 * 1024 * 1024;

/// 读文件第一行并解析为 JSON（跳过空行；限行防超长 base64 行）。
fn read_first_json_line(path: &Path) -> AdapterResult<serde_json::Value> {
    use std::io::BufRead;
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    loop {
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        let oversized = buf.len() > MAX_LINE;
        let line = String::from_utf8_lossy(&buf).into_owned();
        buf.clear();
        let t = line.trim();
        if oversized || t.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str(t) {
            return Ok(v);
        }
    }
    Err(CodexError::NotFound(path.display().to_string()))
}

/// 解析单条 Codex 会话。
#[allow(clippy::too_many_lines)] // JSONL 解析主循环：事件分类与领域装配一体
pub fn parse_session(file_path: impl AsRef<Path>) -> AdapterResult<RawConversation> {
    use std::io::BufRead;
    let path = file_path.as_ref();
    let file =
        std::fs::File::open(path).map_err(|_| CodexError::NotFound(path.display().to_string()))?;

    let mut session_id: Option<String> = None;
    let mut started_at: Option<time::OffsetDateTime> = None;
    let mut first_ts: Option<time::OffsetDateTime> = None;
    let mut messages = Vec::new();
    let mut events = Vec::new();
    // call_id → 该调用产生的事件下标（输出到达时合并 payload）
    let mut pending_outputs: HashMap<String, Vec<usize>> = HashMap::new();
    let mut item_seq: i64 = 0;

    // 限行流式读取：跳过单行 >2MB 的二进制/图片负载（防内存尖峰卡死）
    let mut reader = std::io::BufReader::new(file);
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    loop {
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        let oversized = buf.len() > MAX_LINE;
        let line = String::from_utf8_lossy(&buf).into_owned();
        buf.clear();
        if oversized {
            continue;
        }
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let Ok(rec) = serde_json::from_str::<serde_json::Value>(t) else {
            continue;
        };
        let ts = rec
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(parse_iso);
        if first_ts.is_none() {
            first_ts = ts;
        }
        let rec_type = rec.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let payload = rec.get("payload").cloned().unwrap_or_default();

        match rec_type {
            "session_meta" => {
                session_id = payload
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(std::string::ToString::to_string);
                started_at = payload
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .and_then(parse_iso)
                    .or(ts);
            }
            "response_item" => {
                item_seq += 1;
                let p_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match p_type {
                    "message" => {
                        let role = match payload.get("role").and_then(|v| v.as_str()) {
                            Some("user") => Role::User,
                            Some("assistant") => Role::Assistant,
                            Some("system") => Role::System,
                            _ => continue,
                        };
                        // content: [{type: input_text/output_text, text}]
                        let text = payload
                            .get("content")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            })
                            .unwrap_or_default();
                        if text.is_empty() {
                            continue;
                        }
                        // 过滤 Codex 注入的环境/系统块（非真实用户输入）
                        if role == Role::User && is_injected_context(&text) {
                            continue;
                        }
                        messages.push(RawMessage {
                            role,
                            text: Some(text),
                            content_json: None,
                            source_message_id: Some(format!("item-{item_seq}")),
                            created_at: ts,
                        });
                    }
                    "function_call" | "custom_tool_call" => {
                        let name = payload
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("tool");
                        // 降噪：wait 轮询（等生图/长任务的空转）不产生事件
                        if name == "wait" {
                            continue;
                        }
                        // 新式 JS 工具桥：命令在 input 的 tools.xxx({...}) 里；
                        // 旧式 function_call：arguments 为 JSON 字符串
                        let input_text = payload
                            .get("input")
                            .and_then(|v| v.as_str())
                            .or_else(|| payload.get("arguments").and_then(|v| v.as_str()))
                            .unwrap_or("");
                        let calls: Vec<JsToolCall> = if input_text.contains("tools.") {
                            extract_js_tool_calls(input_text)
                        } else {
                            // 旧式：包一层当单调用，参数按 JSON 尝试解析
                            let args = serde_json::from_str::<serde_json::Value>(input_text)
                                .unwrap_or(serde_json::Value::Null);
                            vec![JsToolCall {
                                tool: name.to_string(),
                                args,
                            }]
                        };
                        let call_id = payload
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .map(std::string::ToString::to_string);
                        for call in calls {
                            if let Some((event_type, summary, mut payload_json)) =
                                event_for_js_call(&call)
                            {
                                payload_json["tool"] = serde_json::json!(call.tool);
                                let idx = events.len();
                                events.push(RawEvent {
                                    event_type,
                                    summary: Some(summary),
                                    payload_json: Some(payload_json),
                                    source_event_id: Some(format!("call-{item_seq}-{idx}")),
                                    created_at: ts,
                                });
                                // call_id → 事件下标：输出到达时合并 payload 并完结
                                if let Some(id) = &call_id {
                                    pending_outputs.entry(id.clone()).or_default().push(idx);
                                }
                            }
                        }
                    }
                    "function_call_output" | "custom_tool_call_output" => {
                        // 与调用按 call_id 配对：输出并入对应事件 payload；
                        // 命令类事件升格为 CommandCompleted
                        if let Some(id) = payload.get("call_id").and_then(|v| v.as_str()) {
                            let output = output_to_text(payload.get("output").unwrap_or(
                                payload.get("result").unwrap_or(&serde_json::Value::Null),
                            ));
                            if let Some(indices) = pending_outputs.get(id) {
                                let mut merged = serde_json::Map::new();
                                merged.insert(
                                    "output".to_string(),
                                    serde_json::Value::String(truncate_chars(&output, 4096)),
                                );
                                for &idx in indices {
                                    if let Some(ev) = events.get_mut(idx) {
                                        if let Some(p) = ev.payload_json.as_mut() {
                                            if let Some(obj) = p.as_object_mut() {
                                                obj.extend(merged.clone());
                                            }
                                        }
                                        if ev.event_type == EventType::CommandStarted {
                                            ev.event_type = EventType::CommandCompleted;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    let session_id = session_id.unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    });

    if messages.is_empty() && events.is_empty() {
        return Err(CodexError::Empty(session_id));
    }

    // 标题：首条真实用户消息（注入块已过滤），回退到首条助手消息
    let title = messages
        .iter()
        .find(|m| m.role == Role::User)
        .or_else(|| messages.iter().find(|m| m.role == Role::Assistant))
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
        source_conversation_id: session_id,
        title,
        model: Some("codex".into()),
        started_at: started_at.or(first_ts),
        messages,
        events,
        source_parent_id: None,
    })
}

/// `tools.xxx({...})` 调用 →（领域事件类型，人类可读摘要，payload）。
/// 返回 None 表示跳过（纯读取类调用，无展示价值）。
fn event_for_js_call(call: &JsToolCall) -> Option<(EventType, String, serde_json::Value)> {
    let arg_str = |key: &str| call.args.get(key).and_then(|v| v.as_str());
    match call.tool.as_str() {
        "exec_command" | "shell" | "exec" => {
            let cmd = arg_str("cmd").or_else(|| arg_str("command")).unwrap_or("");
            if cmd.is_empty() {
                return Some((
                    EventType::CommandStarted,
                    "（空命令）".into(),
                    serde_json::json!({}),
                ));
            }
            let first_line = cmd.lines().next().unwrap_or("").trim();
            let summary = truncate_chars(first_line, 120);
            let mut payload = serde_json::json!({ "cmd": cmd });
            if let Some(wd) = arg_str("workdir") {
                payload["workdir"] = serde_json::json!(wd);
            }
            Some((EventType::CommandStarted, summary, payload))
        }
        "image_gen__imagegen" | "image_gen" => {
            let prompt = arg_str("prompt").unwrap_or("");
            let summary = if prompt.is_empty() {
                "生成图片".to_string()
            } else {
                format!("生成图片：{}", truncate_chars(prompt.trim(), 60))
            };
            Some((
                EventType::ArtifactCreated,
                summary,
                serde_json::json!({ "prompt": prompt }),
            ))
        }
        "view_image" => {
            let path = arg_str("path").unwrap_or("");
            let name = path.rsplit('/').next().unwrap_or(path);
            Some((
                EventType::FileRead,
                format!("查看图片 {}", truncate_chars(name, 80)),
                serde_json::json!({ "path": path }),
            ))
        }
        "apply_patch" => Some((
            EventType::DiffGenerated,
            "应用补丁".to_string(),
            call.args.clone(),
        )),
        "update_plan" => Some((
            EventType::ToolCallStarted,
            "更新计划".to_string(),
            call.args.clone(),
        )),
        "create_goal" | "update_goal" => {
            let obj = arg_str("objective").unwrap_or("");
            let summary = if obj.is_empty() {
                format!(
                    "{}目标",
                    if call.tool == "create_goal" {
                        "设定"
                    } else {
                        "更新"
                    }
                )
            } else {
                format!(
                    "{}目标：{}",
                    if call.tool == "create_goal" {
                        "设定"
                    } else {
                        "更新"
                    },
                    truncate_chars(obj.trim(), 60)
                )
            };
            Some((EventType::ToolCallStarted, summary, call.args.clone()))
        }
        // 纯读取类：跳过（无展示价值）
        "get_goal" | "get_plan" => None,
        _ => Some((
            EventType::ToolCallStarted,
            format!("工具 {}", call.tool),
            call.args.clone(),
        )),
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

/// Codex 注入块标签：环境上下文 / 系统指令 / 插件推荐等（runtime 注入而非用户输入）。
const INJECTED_TAGS: &[&str] = &[
    "<environment_context>",
    "<user_instructions>",
    "<recommended_plugins>",
    "<turn_context>",
    "<turn_aborted>",
    "<runtime_credentials>",
    "<IDE_INFORMATION>",
    "<system-reminder>",
    "<ENVIRONMENT",
];

/// Codex 注入块判定：上述 XML 标签开头，或 AGENTS.md 头。
/// 这些是 runtime 注入而非用户真实输入，不应入库为消息或用作标题。
fn is_injected_context(text: &str) -> bool {
    let t = text.trim_start();
    if t.starts_with("# AGENTS.md") || t.starts_with("# Systems") {
        return true;
    }
    INJECTED_TAGS.iter().any(|tag| t.starts_with(tag))
}

/// 解析 ISO 8601 时间戳。
fn parse_iso(s: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_session(dir: &Path) -> std::path::PathBuf {
        let f = dir.join("rollout-test.jsonl");
        let lines = [
            r#"{"timestamp":"2026-08-01T10:00:00.000Z","type":"session_meta","payload":{"id":"sess-codex-1","timestamp":"2026-08-01T10:00:00.000Z","cwd":"/tmp/proj"}}"#,
            r#"{"timestamp":"2026-08-01T10:00:01.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"帮我写个排序函数"}]}}"#,
            r#"{"timestamp":"2026-08-01T10:00:05.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"好的，这是快速排序：def qs(a): pass"}]}}"#,
            r#"{"timestamp":"2026-08-01T10:00:06.000Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"ls\"}","call_id":"c1"}}"#,
            r##"{"timestamp":"2026-08-01T10:00:02.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions\n环境注入"}]}}"##,
        ];
        std::fs::write(&f, lines.join("\n")).expect("file I/O failed");
        f
    }

    #[test]
    fn parse_extracts_messages_and_events() {
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let f = make_test_session(dir.path());
        let raw = parse_session(&f).expect("parse failed");
        assert_eq!(raw.provider, Provider::Codex);
        assert_eq!(raw.source_conversation_id, "sess-codex-1");
        assert_eq!(raw.messages.len(), 2, "AGENTS.md 注入应被过滤");
        assert_eq!(raw.messages[0].role, Role::User);
        assert!(raw.messages[0]
            .text
            .as_deref()
            .expect("unexpected None")
            .contains("排序函数"));
        assert_eq!(raw.messages[1].role, Role::Assistant);
        assert!(raw.messages[1]
            .text
            .as_deref()
            .expect("unexpected None")
            .contains("快速排序"));
        assert_eq!(raw.events.len(), 1);
        assert_eq!(raw.events[0].event_type, EventType::CommandStarted);
        assert!(
            raw.events[0]
                .summary
                .as_deref()
                .expect("unexpected None")
                .contains("ls"),
            "旧式 function_call 应提取出 cmd 而非工具名"
        );
        assert_eq!(raw.title.as_deref(), Some("帮我写个排序函数"));
        assert!(raw.started_at.is_some());
        assert!(raw.messages[0].created_at.is_some());
    }

    /// 新式 JS 工具桥会话：custom_tool_call.input 含 tools.exec_command /
    /// view_image / update_plan；wait 轮询与 get_goal 应被降噪。
    #[test]
    fn parse_js_bridge_session() {
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let f = dir.path().join("rollout-js.jsonl");
        let lines = [
            r#"{"timestamp":"2026-08-15T21:00:00.000Z","type":"session_meta","payload":{"id":"js-1"}}"#,
            r#"{"timestamp":"2026-08-15T21:00:01.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"审计素材"}]}}"#,
            // JS 工具桥 exec（用户实例的真实形态）
            r#"{"timestamp":"2026-08-15T21:00:05.000Z","type":"response_item","payload":{"type":"custom_tool_call","name":"exec","call_id":"cA","input":"const r=await tools.exec_command({cmd:\"printf 'Prompt entries: '; rg -c '^([0-9]+)\\\\.' prompts.md\nfind png -name '*.png' | wc -l\",\"workdir\":\"/tmp/proj\"});text(r.output);"}}"#,
            // 输出配对（数组形态）
            r#"{"timestamp":"2026-08-15T21:00:09.000Z","type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"cA","output":[{"type":"input_text","text":"Prompt entries: 520\nPNG count: 520"}]}}"#,
            // wait 轮询：降噪跳过
            r#"{"timestamp":"2026-08-15T21:00:10.000Z","type":"response_item","payload":{"type":"function_call","name":"wait","arguments":"{\"ms\":5000}","call_id":"cW"}}"#,
            // 查看图片
            r#"{"timestamp":"2026-08-15T21:00:12.000Z","type":"response_item","payload":{"type":"custom_tool_call","name":"exec","call_id":"cB","input":"await tools.view_image({path:\"/tmp/png/498_袁隆平 Modern Field.png\"});"}}"#,
            // 计划 + 目标（同一条 input 内两个调用）
            r#"{"timestamp":"2026-08-15T21:00:15.000Z","type":"response_item","payload":{"type":"custom_tool_call","name":"exec","call_id":"cC","input":"const a=await tools.create_goal({objective:\"完成 520 张立绘\"});const b=await tools.update_plan({p:\"step\"});"}}"#,
            // 纯读取：跳过
            r#"{"timestamp":"2026-08-15T21:00:16.000Z","type":"response_item","payload":{"type":"custom_tool_call","name":"exec","call_id":"cD","input":"await tools.get_goal({});"}}"#,
            // 旧式 wait 的输出（call_id 无对应事件）：安全忽略
            r#"{"timestamp":"2026-08-15T21:00:17.000Z","type":"response_item","payload":{"type":"function_call_output","call_id":"cW","output":"waited"}}"#,
        ];
        std::fs::write(&f, lines.join("\n")).expect("file I/O failed");

        let raw = parse_session(&f).expect("parse failed");
        let summaries: Vec<&str> = raw
            .events
            .iter()
            .map(|e| e.summary.as_deref().unwrap_or(""))
            .collect();
        assert_eq!(
            raw.events.len(),
            4,
            "wait/get_goal 降噪后应剩 4 个事件：{summaries:?}"
        );
        // exec → CommandStarted，输出配对后升格 CommandCompleted，payload 含输出
        let exec = &raw.events[0];
        assert_eq!(exec.event_type, EventType::CommandCompleted);
        assert!(exec
            .summary
            .as_deref()
            .is_some_and(|s| s.starts_with("printf 'Prompt entries")));
        let payload = exec.payload_json.as_ref().expect("unexpected None");
        assert!(payload
            .get("output")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| s.contains("520")));
        assert!(payload
            .get("cmd")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| s.contains("rg -c")));
        // view_image → FileRead 带文件名
        assert_eq!(raw.events[1].event_type, EventType::FileRead);
        assert!(summaries[1].contains("498_袁隆平"));
        // 一条 input 双调用：create_goal + update_plan 各一个事件
        assert!(summaries[2].contains("设定目标：完成 520 张立绘"));
        assert_eq!(summaries[3], "更新计划");
    }

    #[test]
    fn legacy_function_call_without_cmd_falls_back_to_tool_name() {
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let f = dir.path().join("rollout-legacy.jsonl");
        let lines = [
            r#"{"timestamp":"2026-08-01T10:00:00.000Z","type":"session_meta","payload":{"id":"lg-1"}}"#,
            r#"{"timestamp":"2026-08-01T10:00:01.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}}"#,
            // 非命令类旧式调用（无 cmd/command 键）→ 工具事件 + 工具名
            r#"{"timestamp":"2026-08-01T10:00:02.000Z","type":"response_item","payload":{"type":"function_call","name":"read_file","arguments":"{\"path\":\"/a.txt\"}","call_id":"c1"}}"#,
        ];
        std::fs::write(&f, lines.join("\n")).expect("file I/O failed");
        let raw = parse_session(&f).expect("parse failed");
        assert_eq!(raw.events.len(), 1);
        assert_eq!(raw.events[0].event_type, EventType::ToolCallStarted);
        assert!(raw.events[0]
            .summary
            .as_deref()
            .is_some_and(|s| s.contains("read_file")));
    }

    #[test]
    fn empty_session_errors() {
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let f = dir.path().join("empty.jsonl");
        std::fs::write(&f, r#"{"timestamp":"2026-08-01T10:00:00.000Z","type":"session_meta","payload":{"id":"e1"}}"#).expect("file I/O failed");
        assert!(matches!(parse_session(&f), Err(CodexError::Empty(_))));
    }

    #[test]
    fn missing_file_errors() {
        assert!(matches!(
            parse_session("/nonexistent/x.jsonl"),
            Err(CodexError::NotFound(_))
        ));
    }

    #[test]
    fn discover_finds_sessions() {
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let sub = dir.path().join("sessions/2026/08/01");
        std::fs::create_dir_all(&sub).expect("file I/O failed");
        std::fs::write(sub.join("rollout-a.jsonl"), "{\"timestamp\":\"2026-08-01T10:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"a1\"}}").expect("file I/O failed");
        let sessions = discover_sessions(dir.path()).expect("unexpected None");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "a1");
    }

    #[test]
    fn filters_xml_injection_blocks_and_title_fallback() {
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let f = dir.path().join("rollout-inject.jsonl");
        let lines = [
            r#"{"timestamp":"2026-08-01T10:00:00.000Z","type":"session_meta","payload":{"id":"inj1"}}"#,
            // 各种注入块：全应被过滤
            r#"{"timestamp":"2026-08-01T10:00:01.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>\n  <cwd>/Users/x/proj</cwd>\n</environment_context>"}]}}"#,
            r#"{"timestamp":"2026-08-01T10:00:02.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<recommended_plugins>\nHere is a list of plugins"}]}}"#,
            r#"{"timestamp":"2026-08-01T10:00:03.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<user_instructions>\nsome rules"}]}}"#,
            // 真实对话
            r#"{"timestamp":"2026-08-01T10:00:10.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"好的，我先分析项目结构。"}]}}"#,
        ];
        std::fs::write(&f, lines.join("\n")).expect("file I/O failed");

        let raw = parse_session(&f).expect("parse failed");
        assert_eq!(raw.messages.len(), 1, "三种注入块全被过滤");
        assert_eq!(raw.messages[0].role, Role::Assistant);
        // 无真实用户消息 → 标题回退到助手消息
        assert_eq!(raw.title.as_deref(), Some("好的，我先分析项目结构。"));
    }
}
