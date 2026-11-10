//! Sun Harness（`sun`）Adapter，对标 dsh adapter（plan §10.5「按来源 Adapter」）。
//!
//! ## 数据源
//!
//! sun 把会话存为追加式 JSONL 事件日志（无压缩），根目录 `SUN_HOME` 优先、
//! 缺省 `~/.sun`，两种文件形态：
//!
//! - `<root>/sessions/<id>.jsonl`（平铺，id 形如 `20261107-01b4eed9`）
//! - `<root>/sessions/<id>/session.jsonl`（目录形态，sun `publishImport` 导入发布）
//!
//! 每行一个 JSON 事件，关键字段 `type` / `seq` / `ts`（RFC3339 字符串，字段全在顶层）：
//!
//! - `session/meta`：首行会话头 `{id,title,mode,cwd,model,createdAt,approval?,
//!   parent?,importedThrough?}`；`parent` 表示 subagent 委派会话（映射
//!   `source_parent_id`）；`importedThrough` 非空表示外部导入镜像（整体排除）
//! - `user/message`：`{text, source}`；`source=="user"` 为正文，其余
//!   （`hook/system/schedule/job/verify/memory/trigger`）是 runtime 注入 →
//!   保留为 `SystemBlock` 事件（全保真），不计入正文
//! - `assistant/message`：`{text, toolCalls[{id,name,arguments}], usage, model}`；
//!   `text` 为正文（可与 toolCalls 并存），usage 保真进 `content_json`
//! - `tool/result`：`{callId, name, output, isError, durationMs}`，按 `callId`
//!   与调用配对（命令类升格 `CommandCompleted`；孤立 result 忽略，同 dsh）
//! - `session/title`：标题事件，后发覆盖先发
//! - `model/switch`：`{to}` 作模型名兜底
//! - `request/header`：`systemPrompt` → `SystemBlock` 事件（全保真保留）
//! - 其余（`run/status`/`checkpoint`/`todo/update`/`approval/*`/`job/*`/
//!   `verify/result`）：降级忽略——不崩溃、不猜字段（plan §11.6）
//!
//! ## 能力
//!
//! - [`discover_sessions`]：顶层会话（面板列表用）
//! - [`discover_all_sessions`]：全部会话含 subagent（auto_sync 用，parent 链）
//! - [`parse_session`]：单条会话 → `RawConversation`

use ch_domain::{EventType, Provider, Role};
use ch_normalization::{RawConversation, RawEvent, RawMessage};
use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub type AdapterResult<T> = std::result::Result<T, SunHarnessError>;

#[derive(Debug, Error)]
pub enum SunHarnessError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("session not found: {0}")]
    NotFound(String),

    #[error("session {0} has no messages")]
    Empty(String),

    /// sun「会话导入」镜像（`session/meta.importedThrough` 非空）：外部工具
    /// 会话在 sun 侧的副本，本体由各自 adapter 导入，sun adapter 不消费。
    #[error("session {0} is an import mirror of another tool's session")]
    ExternalImport(String),
}

pub const ADAPTER_ID: &str = "sun-harness";
pub const PROVIDER: Provider = Provider::SunHarness;

/// 发现的 sun 会话。
#[derive(Debug, Clone)]
pub struct DiscoveredSession {
    pub session_id: String,
    /// `session/title` 最后一条（源侧重命名后为新名）。
    pub title: String,
    pub message_count: i64,
    /// 会话头 `createdAt`（RFC3339 → Unix 毫秒）。
    pub created_at_ms: i64,
    pub file_path: String,
    pub size_bytes: u64,
    /// 日志文件修改时间（Unix 毫秒）——「已导入」新鲜度判定用。
    pub mtime_ms: Option<i64>,
    /// 会话头 `parent`（subagent 委派），None = 顶层。
    pub parent_id: Option<String>,
}

/// JSONL 单行上限：超过视为二进制负载跳过（防超长 base64 行内存尖峰）。
const MAX_LINE: usize = 2 * 1024 * 1024;

/// sun 根目录下的会话日志文件：平铺 `*.jsonl` 与目录形态 `<id>/session.jsonl`。
fn scan_log_files(sessions_root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(sessions_root) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            let inner = p.join("session.jsonl");
            if inner.is_file() {
                out.push(inner);
            }
        } else if p.extension().and_then(|x| x.to_str()) == Some("jsonl") {
            out.push(p);
        }
    }
}

/// 逐行读取（限行防超长负载），回调文本行；坏行容忍由调用方 JSON 解析兜底。
fn for_each_line(path: &Path, mut f: impl FnMut(&str)) -> AdapterResult<()> {
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
        if oversized {
            continue;
        }
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        f(t);
    }
    Ok(())
}

/// RFC3339 → 时间戳（sun 的 `ts`/`createdAt` 为 ISO 字符串）。
fn iso_to_ts(s: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
}

/// RFC3339 → Unix 毫秒。
fn iso_to_ms(s: &str) -> Option<i64> {
    iso_to_ts(s).map(|t| t.unix_timestamp_nanos() as i64 / 1_000_000)
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

/// 会话 id 兜底：平铺文件取文件名词干，目录形态取目录名。
fn session_id_from_path(path: &Path) -> String {
    if path.file_name().and_then(|n| n.to_str()) == Some("session.jsonl") {
        path.parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        path.file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// 列出 sun **顶层**会话（无 `parent`），按 mtime 降序。面板列表用。
pub fn discover_sessions(sun_root: impl AsRef<Path>) -> AdapterResult<Vec<DiscoveredSession>> {
    Ok(discover_all_sessions(sun_root)?
        .into_iter()
        .filter(|s| s.parent_id.is_none())
        .collect())
}

/// 列出 sun **所有**会话（含 subagent 委派），按 mtime 降序。auto_sync 用：
/// 主会话先导入（parent=null），子会话后导入（parent=父 ID）。
pub fn discover_all_sessions(sun_root: impl AsRef<Path>) -> AdapterResult<Vec<DiscoveredSession>> {
    let root = sun_root.as_ref().join("sessions");
    let mut files = Vec::new();
    scan_log_files(&root, &mut files);

    let mut sessions = Vec::new();
    for f in files {
        let Some(info) = quick_info(&f) else {
            continue; // 导入镜像 / 无 meta 头：不进入发现结果
        };
        let meta = std::fs::metadata(&f);
        let size = meta.as_ref().map_or(0, std::fs::Metadata::len);
        let mtime_ms = meta
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64);
        sessions.push(DiscoveredSession {
            session_id: info.session_id,
            title: info.title,
            message_count: info.message_count,
            created_at_ms: info.created_at_ms,
            file_path: f.to_string_lossy().into_owned(),
            size_bytes: size,
            mtime_ms,
            parent_id: info.parent_id,
        });
    }
    sessions.sort_by_key(|s| std::cmp::Reverse(s.mtime_ms.unwrap_or(0)));
    Ok(sessions)
}

/// 轻量扫描：只解析 `session/meta` 与 `session/title` 行，
/// 消息计数用子串匹配（面板展示用途，容许极小概率的文本误报）。
struct QuickInfo {
    session_id: String,
    title: String,
    message_count: i64,
    created_at_ms: i64,
    parent_id: Option<String>,
}

fn quick_info(path: &Path) -> Option<QuickInfo> {
    let mut session_id: Option<String> = None;
    let mut created_at_ms = 0i64;
    let mut parent_id: Option<String> = None;
    let mut title: Option<String> = None;
    let mut message_count = 0i64;
    let mut imported = false;

    for_each_line(path, |line| {
        if line.contains("\"session/meta\"") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                session_id = v.get("id").and_then(|x| x.as_str()).map(String::from);
                created_at_ms = v
                    .get("createdAt")
                    .and_then(|x| x.as_str())
                    .and_then(iso_to_ms)
                    .unwrap_or(0);
                parent_id = v.get("parent").and_then(|x| x.as_str()).map(String::from);
                if v.get("importedThrough").is_some_and(|x| !x.is_null()) {
                    imported = true;
                }
                title = v.get("title").and_then(|x| x.as_str()).map(String::from);
            }
            return;
        }
        // 标题：解析命中行（后发覆盖先发——含源侧重命名）
        if line.contains("\"session/title\"") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(t) = v.get("title").and_then(|x| x.as_str()) {
                    title = Some(t.to_string());
                }
            }
            return;
        }
        if line.contains("\"user/message\"") || line.contains("\"assistant/message\"") {
            message_count += 1;
        }
    })
    .ok()?;

    if imported {
        return None;
    }
    let session_id = session_id.unwrap_or_else(|| session_id_from_path(path));
    if session_id.is_empty() {
        return None;
    }
    Some(QuickInfo {
        session_id,
        title: title.unwrap_or_default(),
        message_count,
        created_at_ms,
        parent_id,
    })
}

/// 解析单条 sun 会话。
#[allow(clippy::too_many_lines)] // JSONL 解析主循环：事件分类与领域装配一体
pub fn parse_session(file_path: impl AsRef<Path>) -> AdapterResult<RawConversation> {
    let path = file_path.as_ref();
    if !path.exists() {
        return Err(SunHarnessError::NotFound(path.display().to_string()));
    }

    let mut session_id: Option<String> = None;
    let mut started_at: Option<time::OffsetDateTime> = None;
    let mut source_parent_id: Option<String> = None;
    let mut title: Option<String> = None;
    let mut model: Option<String> = None;
    let mut model_switch: Option<String> = None;
    let mut messages: Vec<RawMessage> = Vec::new();
    let mut events: Vec<RawEvent> = Vec::new();
    // callId → 该调用产生的事件下标（输出到达时合并 payload；调用 1:1 配对）
    let mut pending_outputs: HashMap<String, usize> = HashMap::new();
    // 高保真上下文事件（注入块 / systemPrompt）：单独收集、最后整体 append——
    // 事件序号按 vec 位置分配，追加保证存量事件 seq 稳定（重导 upsert 按序号命中旧行）
    let mut context_events: Vec<RawEvent> = Vec::new();
    // 外部导入镜像：发现层已按 importedThrough 排除；此处防御直连 parse 的调用方
    let mut external = false;

    for_each_line(path, |line| {
        let Ok(rec) = serde_json::from_str::<serde_json::Value>(line) else {
            return; // 坏行（crash torn line）：跳过
        };
        let rec_type = rec.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let seq = rec
            .get("seq")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let ts = rec.get("ts").and_then(|v| v.as_str()).and_then(iso_to_ts);

        match rec_type {
            "session/meta" => {
                if rec.get("importedThrough").is_some_and(|v| !v.is_null()) {
                    external = true;
                }
                session_id = rec.get("id").and_then(|v| v.as_str()).map(String::from);
                started_at = rec
                    .get("createdAt")
                    .and_then(|v| v.as_str())
                    .and_then(iso_to_ts);
                source_parent_id = rec.get("parent").and_then(|v| v.as_str()).map(String::from);
                if model.is_none() {
                    model = rec.get("model").and_then(|v| v.as_str()).map(String::from);
                }
                if title.is_none() {
                    title = rec.get("title").and_then(|v| v.as_str()).map(String::from);
                }
            }
            "session/title" => {
                if let Some(t) = rec.get("title").and_then(|v| v.as_str()) {
                    title = Some(t.to_string());
                }
            }
            "model/switch" => {
                if let Some(m) = rec.get("to").and_then(|v| v.as_str()) {
                    model_switch = Some(m.to_string());
                }
            }
            "user/message" => {
                let text = rec
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if text.trim().is_empty() {
                    return;
                }
                let source = rec.get("source").and_then(|v| v.as_str()).unwrap_or("user");
                if source != "user" {
                    // runtime 注入（hook/schedule/system/job/verify/memory/trigger）：
                    // 全保真保留为 SystemBlock 事件，不计入对话正文
                    context_events.push(RawEvent {
                        event_type: EventType::SystemBlock,
                        summary: Some(truncate_chars(text.trim(), 100)),
                        payload_json: Some(serde_json::json!({ "text": text })),
                        source_event_id: Some(format!("injected-{seq}")),
                        created_at: ts,
                    });
                    return;
                }
                messages.push(RawMessage {
                    role: Role::User,
                    text: Some(text),
                    content_json: None,
                    source_message_id: None,
                    created_at: ts,
                });
            }
            "assistant/message" => {
                // 工具调用挂在 assistant 消息的 toolCalls[]（sun 无独立 tool/call 事件）
                if let Some(calls) = rec.get("toolCalls").and_then(|v| v.as_array()) {
                    for call in calls {
                        let call_id = call
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = call
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("tool")
                            .to_string();
                        // arguments 是 JSON 字符串；容错：直接内嵌 JSON 对象
                        let args = call
                            .get("arguments")
                            .and_then(|v| v.as_str())
                            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                            .unwrap_or_else(|| call.get("arguments").cloned().unwrap_or_default());
                        let (event_type, summary, mut payload) = event_for_tool_call(&name, &args);
                        payload["tool"] = serde_json::json!(name);
                        let idx = events.len();
                        events.push(RawEvent {
                            event_type,
                            summary: Some(summary),
                            payload_json: Some(payload),
                            source_event_id: Some(format!("call-{seq}-{idx}")),
                            created_at: ts,
                        });
                        if !call_id.is_empty() {
                            pending_outputs.insert(call_id, idx);
                        }
                    }
                }
                let text = rec
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if text.is_empty() {
                    return; // 纯工具调用轮次：无正文消息
                }
                if model.is_none() {
                    model = rec.get("model").and_then(|v| v.as_str()).map(String::from);
                }
                // usage token 用量保真进 content_json（RawMessage 无专字段）
                let content_json = rec
                    .get("usage")
                    .filter(|u| u.is_object())
                    .map(|u| serde_json::json!({ "usage": u }));
                messages.push(RawMessage {
                    role: Role::Assistant,
                    text: Some(text),
                    content_json,
                    source_message_id: None,
                    created_at: ts,
                });
            }
            "tool/result" => {
                // 与调用按 callId 配对：输出并入对应事件 payload；
                // 命令类事件升格 CommandCompleted；孤立 result 忽略（同 dsh）
                let Some(call_id) = rec.get("callId").and_then(|v| v.as_str()).map(String::from)
                else {
                    return;
                };
                let Some(&idx) = pending_outputs.get(&call_id) else {
                    return;
                };
                let output = rec
                    .get("output")
                    .map(|v| match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default();
                if let Some(ev) = events.get_mut(idx) {
                    if let Some(p) = ev.payload_json.as_mut() {
                        if let Some(obj) = p.as_object_mut() {
                            // 输出全量保留（全保真导入：不截断）
                            obj.insert("output".into(), serde_json::json!(output));
                            if let Some(err) = rec.get("isError").and_then(serde_json::Value::as_bool) {
                                obj.insert("isError".into(), serde_json::json!(err));
                            }
                            if let Some(d) = rec.get("durationMs").and_then(serde_json::Value::as_i64) {
                                obj.insert("durationMs".into(), serde_json::json!(d));
                            }
                        }
                    }
                    if ev.event_type == EventType::CommandStarted {
                        ev.event_type = EventType::CommandCompleted;
                    }
                }
            }
            "request/header" => {
                let Some(sp) = rec.get("systemPrompt").and_then(|v| v.as_str()) else {
                    return;
                };
                if sp.trim().is_empty() {
                    return;
                }
                context_events.push(RawEvent {
                    event_type: EventType::SystemBlock,
                    summary: Some(truncate_chars(sp.trim(), 100)),
                    payload_json: Some(serde_json::json!({ "systemPrompt": sp })),
                    source_event_id: Some(format!("header-{seq}")),
                    created_at: ts,
                });
            }
            _ => {} // run/status、checkpoint、todo/update、approval/*、job/*、verify/result：降级忽略
        }
    })?;

    // 高保真上下文事件追加在末尾（保证存量事件 seq 稳定）
    events.append(&mut context_events);

    let session_id = session_id.unwrap_or_else(|| session_id_from_path(path));
    if external {
        return Err(SunHarnessError::ExternalImport(session_id));
    }
    if messages.is_empty() && events.is_empty() {
        return Err(SunHarnessError::Empty(session_id));
    }

    // 标题：session/meta + session/title（后发覆盖）优先，
    // 兜底首条真实用户消息前 60 字（注入块已转事件）
    let title = title.or_else(|| {
        messages
            .iter()
            .find(|m| m.role == Role::User)
            .or_else(|| messages.iter().find(|m| m.role == Role::Assistant))
            .and_then(|m| m.text.as_deref())
            .map(|t| truncate_chars(t.trim(), 60))
    });

    Ok(RawConversation {
        provider: PROVIDER,
        source_conversation_id: session_id,
        title,
        model: model.or(model_switch),
        started_at,
        messages,
        events,
        source_parent_id,
    })
}

/// 工具调用 →（领域事件类型，人类可读摘要，payload）。
/// 未知工具泛化为 `ToolCallStarted`，保留原始名与参数（plan §11.6 降级）。
fn event_for_tool_call(
    name: &str,
    args: &serde_json::Value,
) -> (EventType, String, serde_json::Value) {
    let arg_str = |key: &str| args.get(key).and_then(|v| v.as_str());
    match name {
        "bash" | "shell" | "exec" | "run_command" | "terminal" => {
            let cmd = arg_str("command").or_else(|| arg_str("cmd")).unwrap_or("");
            if cmd.is_empty() {
                return (
                    EventType::CommandStarted,
                    "（空命令）".into(),
                    serde_json::json!({}),
                );
            }
            let first_line = cmd.lines().next().unwrap_or("").trim();
            let summary = truncate_chars(first_line, 120);
            let mut payload = serde_json::json!({ "cmd": cmd });
            if let Some(wd) = args.get("workdir").and_then(|v| v.as_str()) {
                payload["workdir"] = serde_json::json!(wd);
            }
            (EventType::CommandStarted, summary, payload)
        }
        "read" | "read_file" | "view" | "view_image" => {
            let path = arg_str("path").or_else(|| arg_str("file")).unwrap_or("");
            let name_short = path.rsplit('/').next().unwrap_or(path);
            (
                EventType::FileRead,
                format!("读取 {}", truncate_chars(name_short, 80)),
                serde_json::json!({ "path": path }),
            )
        }
        "write" | "write_file" => {
            let path = arg_str("path").or_else(|| arg_str("file")).unwrap_or("");
            (
                EventType::FileCreated,
                format!("新建 {}", truncate_chars(path, 80)),
                serde_json::json!({ "path": path }),
            )
        }
        "edit" | "edit_file" | "apply_patch" | "applypatch" | "str_replace" => {
            let path = arg_str("path").or_else(|| arg_str("file")).unwrap_or("");
            (
                EventType::DiffGenerated,
                format!("修改 {}", truncate_chars(path, 80)),
                serde_json::json!({ "path": path, "args": args }),
            )
        }
        _ => {
            let summary = if arg_str("query").unwrap_or("").is_empty() {
                format!("调用 {name}")
            } else {
                format!(
                    "{name}: {}",
                    truncate_chars(arg_str("query").unwrap_or("").trim(), 80)
                )
            };
            (
                EventType::ToolCallStarted,
                summary,
                serde_json::json!({ "args": args }),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成贴近真实 schema 的 sun 会话（平铺落盘）。
    fn make_session(dir: &Path, session_id: &str) -> PathBuf {
        let lines = [
            r#"{"seq":0,"ts":"2026-11-07T06:54:02.180Z","type":"session/meta","id":"sun-t1","title":"[eval] js-http-endpoint","mode":"coding","cwd":"/tmp/proj","model":"qwen/qwen-flash","createdAt":"2026-11-07T06:54:02.180Z","approval":"full-auto"}"#,
            r#"{"seq":1,"ts":"2026-11-07T06:54:02.239Z","type":"user/message","text":"帮我看下 git 状态","source":"user"}"#,
            // runtime 注入（hook）：保留为 SystemBlock 事件，不计入正文
            r#"{"seq":2,"ts":"2026-11-07T06:54:02.300Z","type":"user/message","text":"<system-reminder>\n规则内容\n</system-reminder>","source":"hook"}"#,
            r#"{"seq":3,"ts":"2026-11-07T06:54:02.360Z","type":"request/header","model":"qwen/qwen-flash","systemPrompt":"You are sun-harness, an expert agent."}"#,
            // 纯工具调用轮次：text 空 + toolCalls
            r#"{"seq":4,"ts":"2026-11-07T06:54:03.597Z","type":"assistant/message","text":"","toolCalls":[{"id":"call_a1","name":"bash","arguments":"{\"command\":\"git status --short\"}"}],"usage":{"inputTokens":3760,"outputTokens":36},"model":"qwen/qwen-flash"}"#,
            r#"{"seq":5,"ts":"2026-11-07T06:54:03.600Z","type":"tool/result","callId":"call_a1","name":"bash","output":" M src/lib.rs","isError":false,"durationMs":12}"#,
            // 正文轮次
            r#"{"seq":6,"ts":"2026-11-07T06:54:04.000Z","type":"assistant/message","text":"我先跑 git status。","toolCalls":[],"usage":{"inputTokens":100,"outputTokens":10},"model":"qwen/qwen-flash"}"#,
            // 孤立 result（无配对调用）：忽略
            r#"{"seq":7,"ts":"2026-11-07T06:54:04.500Z","type":"tool/result","callId":"call_orphan","name":"read_file","output":"?","isError":true}"#,
            r#"{"seq":8,"ts":"2026-11-07T06:54:05.000Z","type":"model/switch","from":"qwen/qwen-flash","to":"openai/gpt-5","reason":"user"}"#,
            // 源侧重命名（最后一条 title 生效）
            r#"{"seq":9,"ts":"2026-11-07T06:55:00.000Z","type":"session/title","title":"我的重命名会话"}"#,
        ];
        let path = dir.join(format!("{session_id}.jsonl"));
        std::fs::write(&path, lines.join("\n")).expect("write failed");
        path
    }

    #[test]
    fn parse_extracts_messages_events_title() {
        let dir = tempfile::TempDir::new().expect("tempdir failed");
        let f = make_session(dir.path(), "sun-t1");
        let raw = parse_session(&f).expect("parse failed");

        assert_eq!(raw.provider, Provider::SunHarness);
        assert_eq!(raw.source_conversation_id, "sun-t1");
        assert_eq!(
            raw.title.as_deref(),
            Some("我的重命名会话"),
            "最后的 title 事件应生效"
        );
        assert_eq!(raw.model.as_deref(), Some("qwen/qwen-flash"), "meta 模型优先");
        assert_eq!(raw.source_parent_id, None);
        assert!(raw.started_at.is_some(), "createdAt 应解析为 started_at");

        // 注入块转事件后：1 条用户 + 1 条助手正文
        assert_eq!(raw.messages.len(), 2);
        assert_eq!(raw.messages[0].role, Role::User);
        assert_eq!(raw.messages[0].text.as_deref(), Some("帮我看下 git 状态"));
        assert_eq!(raw.messages[1].role, Role::Assistant);
        assert_eq!(raw.messages[1].text.as_deref(), Some("我先跑 git status。"));
        let cj = raw.messages[1].content_json.as_ref().expect("usage json");
        assert_eq!(
            cj.get("usage")
                .and_then(|u| u.get("inputTokens"))
                .and_then(|v| v.as_i64()),
            Some(100),
            "usage 应保真进 content_json"
        );

        // bash → CommandCompleted（配对升格）；孤立 result 忽略；
        // 注入块 + systemPrompt 两个 SystemBlock 追加在末尾
        assert_eq!(raw.events.len(), 3, "工具 1 + SystemBlock 2");
        assert_eq!(raw.events[0].event_type, EventType::CommandCompleted);
        assert_eq!(raw.events[0].summary.as_deref(), Some("git status --short"));
        let p = raw.events[0].payload_json.as_ref().expect("payload");
        assert_eq!(p.get("output").and_then(|o| o.as_str()), Some(" M src/lib.rs"));
        assert_eq!(p.get("isError").and_then(|e| e.as_bool()), Some(false));
        assert_eq!(p.get("durationMs").and_then(|d| d.as_i64()), Some(12));
        assert_eq!(raw.events[1].event_type, EventType::SystemBlock);
        assert!(raw.events[2].event_type == EventType::SystemBlock);
        assert!(raw.events[2]
            .payload_json
            .as_ref()
            .expect("payload")
            .get("systemPrompt")
            .is_some());
    }

    /// 全保真：工具输出不截断（长输出原样入库）。
    #[test]
    fn tool_result_output_not_truncated() {
        use std::fmt::Write as _;
        let dir = tempfile::TempDir::new().expect("tempdir failed");
        let long_output = "x".repeat(10_000);
        let mut content = concat!(
            r#"{"seq":0,"ts":"2026-11-07T06:54:02.180Z","type":"session/meta","id":"sun-long","title":"t","mode":"coding","cwd":"/p","model":"m","createdAt":"2026-11-07T06:54:02.180Z"}"#,
            "\n",
            r#"{"seq":1,"ts":"2026-11-07T06:54:02.239Z","type":"user/message","text":"run","source":"user"}"#,
            "\n",
            r#"{"seq":2,"ts":"2026-11-07T06:54:03.000Z","type":"assistant/message","text":"","toolCalls":[{"id":"call_l1","name":"bash","arguments":"{\"command\":\"big\"}"}],"model":"m"}"#,
            "\n",
        )
        .to_string();
        write!(
            content,
            r#"{{"seq":3,"ts":"2026-11-07T06:54:03.100Z","type":"tool/result","callId":"call_l1","name":"bash","output":"{long_output}","isError":false}}"#
        )
        .expect("写入 String 不会失败");
        let f = dir.path().join("sun-long.jsonl");
        std::fs::write(&f, content).expect("write failed");
        let raw = parse_session(&f).expect("parse failed");
        let ev = raw
            .events
            .iter()
            .find(|e| e.event_type == EventType::CommandCompleted)
            .expect("command event");
        let out = ev
            .payload_json
            .as_ref()
            .and_then(|p| p.get("output"))
            .and_then(|o| o.as_str())
            .expect("output");
        assert_eq!(out.len(), 10_000, "长输出应全量保留，不截断");
    }

    /// 目录形态 `<id>/session.jsonl`（publishImport 发布布局）。
    #[test]
    fn parse_directory_form_session() {
        let dir = tempfile::TempDir::new().expect("tempdir failed");
        let sdir = dir.path().join("20261107-01dirform");
        std::fs::create_dir_all(&sdir).expect("mkdir failed");
        let lines = [
            r#"{"seq":0,"ts":"2026-11-07T06:54:02.180Z","type":"session/meta","id":"20261107-01dirform","title":"目录形态","mode":"coding","cwd":"/p","model":"m","createdAt":"2026-11-07T06:54:02.180Z"}"#,
            r#"{"seq":1,"ts":"2026-11-07T06:54:02.239Z","type":"user/message","text":"你好","source":"user"}"#,
        ];
        let f = sdir.join("session.jsonl");
        std::fs::write(&f, lines.join("\n")).expect("write failed");
        let raw = parse_session(&f).expect("parse failed");
        assert_eq!(raw.source_conversation_id, "20261107-01dirform");
        assert_eq!(raw.messages.len(), 1);
    }

    /// 坏行（crash torn line）跳过，其余行照常解析。
    #[test]
    fn torn_final_line_tolerated() {
        let dir = tempfile::TempDir::new().expect("tempdir failed");
        let f = make_session(dir.path(), "sun-torn");
        let mut content = std::fs::read_to_string(&f).expect("read failed");
        content.push_str("\n{\"seq\":10,\"ts\":\"2026-11-07T06:5"); // 截断的坏行
        std::fs::write(&f, content).expect("write failed");
        let raw = parse_session(&f).expect("parse failed");
        assert_eq!(raw.messages.len(), 2, "坏行不应影响有效事件");
    }

    #[test]
    fn parse_subagent_session_has_parent() {
        let dir = tempfile::TempDir::new().expect("tempdir failed");
        let f = dir.path().join("20261107-01child.jsonl");
        let lines = [
            r#"{"seq":0,"ts":"2026-11-07T06:54:02.180Z","type":"session/meta","id":"20261107-01child","title":"子任务","mode":"coding","cwd":"/p","model":"m","createdAt":"2026-11-07T06:54:02.180Z","parent":"20261107-01parent"}"#,
            r#"{"seq":1,"ts":"2026-11-07T06:54:02.239Z","type":"user/message","text":"子任务指令","source":"user"}"#,
        ];
        std::fs::write(&f, lines.join("\n")).expect("write failed");
        let raw = parse_session(&f).expect("parse failed");
        assert_eq!(raw.source_parent_id.as_deref(), Some("20261107-01parent"));
    }

    #[test]
    fn discover_filters_children_and_mirrors_and_reads_title() {
        let dir = tempfile::TempDir::new().expect("tempdir failed");
        let sessions = dir.path().join(".sun/sessions");
        std::fs::create_dir_all(&sessions).expect("mkdir failed");
        // 顶层会话（平铺）
        std::fs::write(
            sessions.join("20261107-01top.jsonl"),
            concat!(
                r#"{"seq":0,"ts":"2026-11-07T06:00:00.000Z","type":"session/meta","id":"20261107-01top","title":"顶层标题","mode":"coding","cwd":"/p","model":"m","createdAt":"2026-11-07T06:00:00.000Z"}"#,
                "\n",
                r#"{"seq":1,"ts":"2026-11-07T06:00:01.000Z","type":"user/message","text":"hi","source":"user"}"#,
            ),
        )
        .expect("write failed");
        // subagent 子会话（平铺 + parent）
        std::fs::write(
            sessions.join("20261107-02child.jsonl"),
            concat!(
                r#"{"seq":0,"ts":"2026-11-07T06:00:00.000Z","type":"session/meta","id":"20261107-02child","title":"子","mode":"coding","cwd":"/p","model":"m","createdAt":"2026-11-07T06:00:00.000Z","parent":"20261107-01top"}"#,
                "\n",
                r#"{"seq":1,"ts":"2026-11-07T06:00:01.000Z","type":"user/message","text":"子任务","source":"user"}"#,
            ),
        )
        .expect("write failed");
        // 目录形态原生会话
        let dirform = sessions.join("20261107-03dirform");
        std::fs::create_dir_all(&dirform).expect("mkdir failed");
        std::fs::write(
            dirform.join("session.jsonl"),
            concat!(
                r#"{"seq":0,"ts":"2026-11-07T06:00:00.000Z","type":"session/meta","id":"20261107-03dirform","title":"目录形态","mode":"coding","cwd":"/p","model":"m","createdAt":"2026-11-07T06:00:00.000Z"}"#,
                "\n",
                r#"{"seq":1,"ts":"2026-11-07T06:00:01.000Z","type":"user/message","text":"目录","source":"user"}"#,
            ),
        )
        .expect("write failed");
        // 导入镜像（importedThrough 非空）：排除
        std::fs::write(
            sessions.join("20261107-04mirror.jsonl"),
            concat!(
                r#"{"seq":0,"ts":"2026-11-07T05:00:00.000Z","type":"session/meta","id":"20261107-04mirror","title":"镜像","mode":"coding","cwd":"/p","model":"m","createdAt":"2026-11-07T05:00:00.000Z","importedThrough":3}"#,
                "\n",
                r#"{"seq":1,"ts":"2026-11-07T05:00:01.000Z","type":"user/message","text":"hi","source":"user"}"#,
            ),
        )
        .expect("write failed");

        let home = dir.path().join(".sun");
        let top_only = discover_sessions(&home).expect("discover failed");
        assert_eq!(top_only.len(), 2, "面板只列顶层（top + dirform）");
        assert!(top_only.iter().all(|s| s.parent_id.is_none()));

        let all = discover_all_sessions(&home).expect("discover failed");
        assert_eq!(all.len(), 3, "auto_sync 含子会话、排除镜像");
        let child = all
            .iter()
            .find(|s| s.session_id == "20261107-02child")
            .expect("child session");
        assert_eq!(child.parent_id.as_deref(), Some("20261107-01top"));
        let top = all
            .iter()
            .find(|s| s.session_id == "20261107-01top")
            .expect("top session");
        assert_eq!(top.title, "顶层标题");
        assert_eq!(top.message_count, 1);
        assert!(top.mtime_ms.is_some());
        assert!(
            all.iter().all(|s| s.session_id != "20261107-04mirror"),
            "导入镜像不应进入发现结果"
        );

        // 解析层防御：直连 parse 镜像文件 → ExternalImport 错误
        match parse_session(sessions.join("20261107-04mirror.jsonl")) {
            Err(SunHarnessError::ExternalImport(id)) => assert_eq!(id, "20261107-04mirror"),
            other => panic!("expected ExternalImport, got {other:?}"),
        }
    }

    #[test]
    fn empty_session_errors() {
        let dir = tempfile::TempDir::new().expect("tempdir failed");
        let f = dir.path().join("20261107-01empty.jsonl");
        std::fs::write(
            &f,
            r#"{"seq":0,"ts":"2026-11-07T06:00:00.000Z","type":"session/meta","id":"20261107-01empty","title":"空","mode":"coding","cwd":"/p","model":"m","createdAt":"2026-11-07T06:00:00.000Z"}"#,
        )
        .expect("write failed");
        match parse_session(&f) {
            Err(SunHarnessError::Empty(id)) => assert_eq!(id, "20261107-01empty"),
            other => panic!("expected Empty, got {other:?}"),
        }
    }

    #[test]
    fn normalize_pipeline_accepts_raw() {
        let dir = tempfile::TempDir::new().expect("tempdir failed");
        let f = make_session(dir.path(), "sun-norm");
        let raw = parse_session(&f).expect("parse failed");
        let normalized = ch_normalization::normalize(raw).expect("normalize failed");
        assert_eq!(normalized.messages.len(), 2);
        assert!(normalized.conversation.title.is_some());
    }

    /// 真实环境回归（手动跑）：本机 ~/.sun 存在时 discover + parse 全量会话。
    /// cargo test -p ch-adapter-sun-harness --lib -- --ignored real_sun --show-output
    #[test]
    #[ignore = "依赖本机真实 ~/.sun 数据，CI 不跑"]
    fn real_sun_home_smoke() {
        let home = std::env::var("HOME").expect("HOME");
        let sun = std::path::Path::new(&home).join(".sun");
        assert!(sun.exists(), "本机无 ~/.sun，跳过语义不适用");
        let all = discover_all_sessions(&sun).expect("discover failed");
        assert!(!all.is_empty(), "本机应有至少 1 个 sun 会话");
        for s in &all {
            let raw = parse_session(&s.file_path).expect("parse failed");
            println!(
                "session={} title={:?} msgs={} events={} model={:?} parent={:?}",
                s.session_id,
                raw.title,
                raw.messages.len(),
                raw.events.len(),
                raw.model,
                raw.source_parent_id
            );
            assert!(!raw.title.unwrap_or_default().is_empty(), "标题不应为空");
        }
    }
}
