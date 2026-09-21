//! DeepSeek Harness（`dsh`）Adapter，对应 plan §10.5「按来源 Adapter」。
//!
//! ## 数据源
//!
//! dsh 把会话存为追加式 JSONL 事件日志（可 zstd 压缩）：
//!
//! - `~/.dsh/sessions/<projectKey>/<sessionId>/session.jsonl.zstd`
//! - `~/.dsh/sessions/<projectKey>/<sessionId>/session.jsonl`（未压缩配置）
//!
//! 每行一个 JSON 事件，关键字段 `type` / `seq` / `time`（Unix 毫秒）：
//!
//! - `session`：首行会话头 `{id, createdAt, cwd?, parentSession?, delegationDepth}`；
//!   `parentSession` 存在表示 subagent 委派会话（映射 `source_parent_id` 主子链路）
//! - `session/title`：标题事件，`data.title`，后发覆盖先发
//!   （`source.kind`：`fallback` 首条消息截断 / `provider` LLM 生成 / `user` 手动重命名）
//! - `user/message`：`data.content[].type=text` 文本块；runtime 注入块
//!   （`<system-reminder>` 等）过滤
//! - `assistant/message`：最终装配的助手消息（流式 chunk 事件不消费），
//!   `data.message.content[]` 取 `text` 块（`reasoning` 块不计入正文）；
//!   `data.message.source.model` 为模型名
//! - `tool/call`：`data.{callId, name, arguments(JSON 字符串)}` → 领域事件
//!   （bash→命令、read→读文件、edit/apply_patch→变更、其余泛化为工具调用）
//! - `tool/result`：按 `callId` 与调用配对，输出并入事件 payload，
//!   命令类事件升格 `CommandCompleted`
//! - `model/selection`：会话中途切换模型（`data.model`，作模型名兜底）
//!
//! ## 降级原则（plan §11.6）
//!
//! 未知事件类型 / 未知工具名 / 未知 content 块类型：不崩溃、不猜字段——
//! 未知工具统一映射 `ToolCallStarted` 并保留原始名与参数。
//!
//! ## 外部导入镜像（不消费）
//!
//! dsh 的 session-import 功能会把 ZCode / Codex / Claude Code 等外部工具的
//! 历史会话镜像进 `~/.dsh/sessions/`：会话 id 恒为 `ext-<provider>-<原id>`
//! （dsh 源码 `targetIdFor` 的确定性约定），且日志内带
//! `session-import/source` 来源事件。这些镜像是其它来源会话的副本，本体由
//! 各自 adapter 导入——本 adapter 在发现层整体排除（目录前缀 + 事件双判定），
//! `parse_session` 对直连调用防御性返回 [`DeepSeekHarnessError::ExternalImport`]。
//!
//! ## 能力
//!
//! - [`discover_sessions`]：顶层会话（面板列表用）
//! - [`discover_all_sessions`]：全部会话含 subagent（auto_sync 用，parent 链）
//! - [`parse_session`]：单条会话 → `RawConversation`

use ch_domain::{EventType, Provider, Role};
use ch_normalization::{RawConversation, RawEvent, RawMessage};
use std::collections::{HashMap, HashSet};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub type AdapterResult<T> = std::result::Result<T, DeepSeekHarnessError>;

#[derive(Debug, Error)]
pub enum DeepSeekHarnessError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("zstd decode error: {0}")]
    Zstd(String),

    #[error("session not found: {0}")]
    NotFound(String),

    #[error("session {0} has no messages")]
    Empty(String),

    /// dsh「外部会话导入」镜像（`ext-<provider>-<原id>`，含
    /// `session-import/source` 事件）：是 ZCode / Codex / Claude Code 等
    /// 来源会话在 dsh 侧的副本，本体由各自 adapter 导入，dsh adapter 不消费。
    #[error("session {0} is an external-import mirror of another tool's session")]
    ExternalImport(String),
}

pub const ADAPTER_ID: &str = "deepseek-harness";
pub const PROVIDER: Provider = Provider::DeepSeekHarness;

/// 发现的 dsh 会话。
#[derive(Debug, Clone)]
pub struct DiscoveredSession {
    pub session_id: String,
    /// 最后一条 `session/title` 事件的标题（源侧重命名后为新名）。
    pub title: String,
    pub message_count: i64,
    /// 会话头 `createdAt`（Unix 毫秒）。
    pub created_at_ms: i64,
    pub file_path: String,
    pub size_bytes: u64,
    /// 日志文件修改时间（Unix 毫秒）——「已导入」新鲜度判定用。
    pub mtime_ms: Option<i64>,
    /// 会话头 `parentSession`（subagent 委派），None = 顶层。
    pub parent_id: Option<String>,
}

/// dsh 注入块标签（runtime 注入而非用户输入）。
const INJECTED_TAGS: &[&str] = &[
    "<system-reminder>",
    "<environment_context>",
    "<user_instructions>",
    "<ENVIRONMENT",
];

/// JSONL 单行上限：超过视为二进制负载跳过（防超长 base64 行内存尖峰）。
const MAX_LINE: usize = 2 * 1024 * 1024;

/// 会话日志是否属于 dsh「外部会话导入」镜像：会话目录名（= 会话 id）
/// 以 `ext-` 开头即为镜像，无需读文件即可判定。
fn session_dir_is_external(path: &Path) -> bool {
    path.parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("ext-"))
}

/// dsh 根目录（`~/.dsh`）下的会话日志文件（按 mtime 降序）。
fn scan_log_files(sessions_root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(sessions_root) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            scan_log_files(&p, out);
        } else {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "session.jsonl" || name == "session.jsonl.zstd" {
                out.push(p);
            }
        }
    }
}

/// 打开日志读取器：`.zstd` 后缀走 zstd 流式解压，其余按纯文本。
fn open_log_reader(path: &Path) -> AdapterResult<Box<dyn BufRead>> {
    let file = std::fs::File::open(path)?;
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zstd"))
    {
        let decoder = zstd::stream::read::Decoder::new(file)
            .map_err(|e| DeepSeekHarnessError::Zstd(e.to_string()))?;
        Ok(Box::new(std::io::BufReader::new(decoder)))
    } else {
        Ok(Box::new(std::io::BufReader::new(file)))
    }
}

/// 逐行读取（限行防超长负载），回调文本行。
fn for_each_line(path: &Path, mut f: impl FnMut(&str)) -> AdapterResult<()> {
    let mut reader = open_log_reader(path)?;
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

/// Unix 毫秒 → 时间戳。
fn ms_to_ts(ms: i64) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000).ok()
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

/// runtime 注入块判定（AGENTS.md / 环境上下文 / 系统提醒）。
fn is_injected_context(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("# AGENTS.md") || INJECTED_TAGS.iter().any(|tag| t.starts_with(tag))
}

/// content 块数组 → 拼接的 text 块文本（`\n` 分隔）。
fn content_texts(content: &serde_json::Value, block_type: &str) -> String {
    content
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|b| {
                    b.get("type").and_then(|t| t.as_str()) == Some(block_type)
                        || (block_type == "text"
                            && b.get("type").and_then(|t| t.as_str()) == Some("input_text"))
                })
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// 列出 dsh **顶层**会话（无 `parentSession`），按 mtime 降序。面板列表用。
pub fn discover_sessions(dsh_home: impl AsRef<Path>) -> AdapterResult<Vec<DiscoveredSession>> {
    Ok(discover_all_sessions(dsh_home)?
        .into_iter()
        .filter(|s| s.parent_id.is_none())
        .collect())
}

/// 列出 dsh **所有**会话（含 subagent 委派），按 mtime 降序。auto_sync 用：
/// 主会话先导入（parent=null），子会话后导入（parent=父 ID）。
pub fn discover_all_sessions(dsh_home: impl AsRef<Path>) -> AdapterResult<Vec<DiscoveredSession>> {
    let root = dsh_home.as_ref().join("sessions");
    let mut files = Vec::new();
    scan_log_files(&root, &mut files);

    let mut sessions = Vec::new();
    for f in files {
        // dsh「外部会话导入」镜像（session-import）：目录名即会话 id，
        // 恒为 `ext-<provider>-<原id>`（dsh 源码 targetIdFor 的确定性约定）。
        // 这些是 ZCode / Codex / Claude Code 会话在 dsh 侧的副本，本体由
        // 各自 adapter 导入——直接跳过，避免重复导入 + 错误归属为 dsh。
        // 事件级兜底（非 ext- 前缀但带 session-import/source 的异常文件）
        // 在 quick_info 内处理。
        if session_dir_is_external(&f) {
            continue;
        }
        let Some(info) = quick_info(&f) else {
            continue;
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

/// 轻量扫描：只解析首行会话头与 `session/title` 行，
/// 消息计数用子串匹配（面板展示用途，容许极小概率的文本误报）。
struct QuickInfo {
    session_id: String,
    title: String,
    message_count: i64,
    created_at_ms: i64,
    parent_id: Option<String>,
}

fn quick_info(path: &Path) -> Option<QuickInfo> {
    let mut header_id: Option<String> = None;
    let mut created_at_ms = 0i64;
    let mut parent_id: Option<String> = None;
    let mut title: Option<String> = None;
    let mut message_count = 0i64;
    let mut header_seen = false;
    let mut external = false;

    for_each_line(path, |line| {
        if !header_seen {
            // 规范上首行即会话头；容错：非 JSON 首行跳过继续找
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                if v.get("type").and_then(|t| t.as_str()) == Some("session") {
                    header_id = v.get("id").and_then(|x| x.as_str()).map(String::from);
                    created_at_ms = v
                        .get("createdAt")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0);
                    parent_id = v
                        .get("parentSession")
                        .and_then(|x| x.as_str())
                        .map(String::from);
                    header_seen = true;
                    return;
                }
            }
        }
        // 外部导入镜像兜底：非 ext- 目录但带 session-import/source 事件
        // （dsh session-import 的规范来源标记）同样视为镜像，不进入发现结果
        if line.contains("\"session-import/source\"") {
            external = true;
            return;
        }
        // 标题：解析命中行（后发覆盖先发——含源侧手动重命名）
        if line.contains("\"session/title\"") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(t) = v.pointer("/data/title").and_then(|x| x.as_str()) {
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

    if external {
        return None;
    }

    let session_id = header_id.unwrap_or_else(|| {
        path.parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
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

/// 解析单条 dsh 会话。
#[allow(clippy::too_many_lines)] // JSONL 解析主循环：事件分类与领域装配一体
pub fn parse_session(file_path: impl AsRef<Path>) -> AdapterResult<RawConversation> {
    let path = file_path.as_ref();
    if !path.exists() {
        return Err(DeepSeekHarnessError::NotFound(path.display().to_string()));
    }

    let mut session_id: Option<String> = None;
    let mut started_at: Option<time::OffsetDateTime> = None;
    let mut source_parent_id: Option<String> = None;
    let mut title: Option<String> = None;
    let mut model: Option<String> = None;
    let mut model_selection: Option<String> = None;
    let mut messages: Vec<RawMessage> = Vec::new();
    let mut events: Vec<RawEvent> = Vec::new();
    // 消息 id 去重（同一消息的修正/重放只取首次）
    let mut seen_msg_ids: HashSet<String> = HashSet::new();
    // callId → 该调用产生的事件下标（输出到达时合并 payload）
    let mut pending_outputs: HashMap<String, Vec<usize>> = HashMap::new();
    // 外部导入镜像：发现层已按 ext- 前缀排除；此处防御直连 parse 的调用方
    // （如手动按文件路径导入）。镜像首条事件即 session-import/source，命中即中止。
    let mut external = false;

    for_each_line(path, |line| {
        if line.contains("\"session-import/source\"") {
            external = true;
            return;
        }
        let Ok(rec) = serde_json::from_str::<serde_json::Value>(line) else {
            return;
        };
        let rec_type = rec.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let seq = rec
            .get("seq")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let ts = rec
            .get("time")
            .and_then(serde_json::Value::as_i64)
            .and_then(ms_to_ts);
        let data = rec.get("data").cloned().unwrap_or_default();

        match rec_type {
            "session" => {
                session_id = data
                    .pointer("/id")
                    .and_then(|v| v.as_str())
                    .or_else(|| rec.pointer("/id").and_then(|v| v.as_str()))
                    .map(String::from);
                started_at = rec
                    .get("createdAt")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(ms_to_ts);
                source_parent_id = rec
                    .get("parentSession")
                    .and_then(|v| v.as_str())
                    .map(String::from);
            }
            "session/title" => {
                if let Some(t) = data.get("title").and_then(|v| v.as_str()) {
                    title = Some(t.to_string());
                }
            }
            "model/selection" => {
                if let Some(m) = data.get("model").and_then(|v| v.as_str()) {
                    model_selection = Some(m.to_string());
                }
            }
            "user/message" => {
                let text = content_texts(
                    data.get("content").unwrap_or(&serde_json::Value::Null),
                    "text",
                );
                if text.is_empty() || is_injected_context(&text) {
                    return;
                }
                let msg_id = data.get("id").and_then(|v| v.as_str()).map(String::from);
                if let Some(id) = &msg_id {
                    if !seen_msg_ids.insert(id.clone()) {
                        return;
                    }
                }
                messages.push(RawMessage {
                    role: Role::User,
                    text: Some(text),
                    content_json: None,
                    source_message_id: msg_id,
                    created_at: ts,
                });
            }
            "assistant/message" => {
                let msg = data.get("message").cloned().unwrap_or_default();
                // 正文只取 text 块；reasoning 块是思考过程，不计入正文
                let text = content_texts(
                    msg.get("content").unwrap_or(&serde_json::Value::Null),
                    "text",
                );
                if text.is_empty() {
                    return;
                }
                if model.is_none() {
                    model = msg
                        .pointer("/source/model")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
                let msg_id = msg.get("id").and_then(|v| v.as_str()).map(String::from);
                if let Some(id) = &msg_id {
                    if !seen_msg_ids.insert(id.clone()) {
                        return;
                    }
                }
                messages.push(RawMessage {
                    role: Role::Assistant,
                    text: Some(text),
                    content_json: None,
                    source_message_id: msg_id,
                    created_at: ts,
                });
            }
            "tool/call" => {
                let name = data
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("tool")
                    .to_string();
                let args = data
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                    .unwrap_or(serde_json::Value::Null);
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
                if let Some(id) = data.get("callId").and_then(|v| v.as_str()) {
                    pending_outputs.entry(id.to_string()).or_default().push(idx);
                }
            }
            "tool/result" => {
                // 与调用按 callId 配对：输出并入对应事件 payload；
                // 命令类事件升格 CommandCompleted
                let call_id = data
                    .pointer("/message/source/callId")
                    .or_else(|| data.pointer("/message/content/0/toolCallId"))
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let Some(call_id) = call_id else { return };
                let output = tool_result_text(&data);
                if let Some(indices) = pending_outputs.get(&call_id) {
                    let merged = serde_json::json!({ "output": truncate_chars(&output, 4096) });
                    for &idx in indices {
                        if let Some(ev) = events.get_mut(idx) {
                            if let Some(p) = ev.payload_json.as_mut() {
                                if let Some(obj) = p.as_object_mut() {
                                    obj.extend(merged.as_object().cloned().unwrap_or_default());
                                }
                            }
                            if ev.event_type == EventType::CommandStarted {
                                ev.event_type = EventType::CommandCompleted;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    })?;

    let session_id = session_id.unwrap_or_else(|| {
        path.parent()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    });

    if external {
        return Err(DeepSeekHarnessError::ExternalImport(session_id));
    }

    if messages.is_empty() && events.is_empty() {
        return Err(DeepSeekHarnessError::Empty(session_id));
    }

    // 标题：session/title 事件（含源侧手动重命名）优先，
    // 兜底首条真实用户消息前 60 字（注入块已过滤）
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
        model: model.or(model_selection),
        started_at,
        messages,
        events,
        source_parent_id,
    })
}

/// `tool/call` →（领域事件类型，人类可读摘要，payload）。
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
        "read" | "view" | "view_image" => {
            let path = arg_str("path").or_else(|| arg_str("file")).unwrap_or("");
            let name_short = path.rsplit('/').next().unwrap_or(path);
            (
                EventType::FileRead,
                format!("读取 {}", truncate_chars(name_short, 80)),
                serde_json::json!({ "path": path }),
            )
        }
        "write" => {
            let path = arg_str("path").or_else(|| arg_str("file")).unwrap_or("");
            (
                EventType::FileCreated,
                format!("新建 {}", truncate_chars(path, 80)),
                serde_json::json!({ "path": path }),
            )
        }
        "edit" | "edit_file" | "apply_patch" | "applypatch" => {
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

/// `tool/result` → 输出文本（嵌套 content 的 text 块拼接）。
fn tool_result_text(data: &serde_json::Value) -> String {
    let mut out = String::new();
    let Some(blocks) = data.pointer("/message/content").and_then(|v| v.as_array()) else {
        return out;
    };
    for b in blocks {
        if let Some(inner) = b.get("content").and_then(|v| v.as_array()) {
            for part in inner {
                if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(t);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成一个贴近真实 schema 的 dsh 会话（未压缩落盘）。
    fn make_session(dir: &Path, session_id: &str) -> PathBuf {
        let lines = [
            r#"{"type":"session","version":0,"id":"session-t1","createdAt":1788601901335,"cwd":"/tmp/proj","delegationDepth":0,"agentPreset":"standard"}"#,
            r#"{"type":"permission/preset","seq":0,"time":1788601901520,"data":{"preset":"workspace-write"}}"#,
            r#"{"type":"session/title","seq":11,"time":1788601902997,"data":{"title":"hi","messageSeqs":[7],"source":{"kind":"fallback"}}}"#,
            r#"{"type":"user/message","seq":7,"time":1788601902996,"data":{"content":[{"type":"text","text":"帮我看下 git 状态"}],"source":{"kind":"user","rpcId":"r1"},"role":"user","id":"m-user-1"},"surfaceOp":"append"}"#,
            // runtime 注入的 AGENTS.md 块（应被过滤）
            r#"{"type":"user/message","seq":8,"time":1788601902996,"data":{"content":[{"type":"text","text":"<system-reminder>\nAGENTS.md 内容\n</system-reminder>"}],"source":{"kind":"user","rpcId":"r2"},"role":"user","id":"m-injected"},"surfaceOp":"append"}"#,
            r#"{"type":"assistant/message","seq":55,"time":1788601903709,"data":{"turn":1,"step":1,"message":{"role":"assistant","content":[{"type":"reasoning","text":"思考过程"},{"type":"text","text":"我先跑 git status。"}],"source":{"kind":"model","provider":"deepseek-official","model":"deepseek-v4-flash"},"id":"m-asst-1"},"usage":{"inputTokens":12880,"outputTokens":34}},"sourceEventSeqs":[15,16,[18,54]],"surfaceOp":"append"}"#,
            r#"{"type":"tool/call","seq":60,"time":1788602305674,"data":{"turn":2,"step":1,"callId":"call_a1","name":"bash","arguments":"{\"command\":\"git status --short | head -50\",\"workdir\":\"/tmp/proj\"}"}}"#,
            r#"{"type":"tool/result","seq":61,"time":1788602305833,"data":{"turn":2,"step":1,"message":{"source":{"kind":"tool","callId":"call_a1"},"content":[{"type":"tool-result","toolCallId":"call_a1","content":[{"type":"text","text":" M src/lib.rs"}]}]}}}"#,
            // 未知工具（泛化 ToolCallStarted）
            r#"{"type":"tool/call","seq":62,"time":1788602400000,"data":{"turn":3,"step":1,"callId":"call_a2","name":"lsp_hover","arguments":"{\"file\":\"a.rs\",\"line\":3}"}}"#,
            // 源侧手动重命名（最后一条 title 事件生效）
            r#"{"type":"session/title","seq":70,"time":1788602500000,"data":{"title":"我的重命名会话","messageSeqs":[],"source":{"kind":"user"}}}"#,
        ];
        let session_dir = dir.join(session_id);
        std::fs::create_dir_all(&session_dir).expect("mkdir failed");
        let path = session_dir.join("session.jsonl");
        std::fs::write(&path, lines.join("\n")).expect("write failed");
        path
    }

    #[test]
    fn parse_extracts_messages_events_title() {
        let dir = tempfile::TempDir::new().expect("tempdir failed");
        let f = make_session(dir.path(), "session-t1");
        let raw = parse_session(&f).expect("parse failed");

        assert_eq!(raw.provider, Provider::DeepSeekHarness);
        assert_eq!(raw.source_conversation_id, "session-t1");
        assert_eq!(
            raw.title.as_deref(),
            Some("我的重命名会话"),
            "最后的 user 重命名 title 应生效"
        );
        assert_eq!(raw.model.as_deref(), Some("deepseek-v4-flash"));
        assert_eq!(raw.source_parent_id, None);
        assert!(raw.started_at.is_some());

        // 注入块过滤后：1 条用户 + 1 条助手
        assert_eq!(raw.messages.len(), 2);
        assert_eq!(raw.messages[0].role, Role::User);
        assert_eq!(raw.messages[0].text.as_deref(), Some("帮我看下 git 状态"));
        assert_eq!(
            raw.messages[0].source_message_id.as_deref(),
            Some("m-user-1")
        );
        assert_eq!(raw.messages[1].role, Role::Assistant);
        assert_eq!(
            raw.messages[1].text.as_deref(),
            Some("我先跑 git status。"),
            "reasoning 块不计入正文"
        );

        // bash → CommandCompleted（输出配对升格）；未知工具 → ToolCallStarted
        assert_eq!(raw.events.len(), 2);
        assert_eq!(raw.events[0].event_type, EventType::CommandCompleted);
        assert!(raw.events[0]
            .summary
            .as_deref()
            .expect("summary")
            .starts_with("git status"));
        assert!(raw.events[0]
            .payload_json
            .as_ref()
            .expect("payload")
            .to_string()
            .contains(" M src/lib.rs"));
        assert_eq!(raw.events[1].event_type, EventType::ToolCallStarted);
        assert!(raw.events[1]
            .payload_json
            .as_ref()
            .expect("payload")
            .to_string()
            .contains("lsp_hover"));
    }

    #[test]
    fn parse_zstd_session() {
        let dir = tempfile::TempDir::new().expect("tempdir failed");
        let f = write_zstd_session(dir.path(), "session-z1");
        let raw = parse_session(&f).expect("parse failed");
        assert_eq!(raw.source_conversation_id, "session-z1");
        assert_eq!(raw.messages.len(), 2);
    }

    fn write_zstd_session(dir: &Path, session_id: &str) -> PathBuf {
        let lines = [
            r#"{"type":"session","version":0,"id":"session-z1","createdAt":1788601901335,"delegationDepth":0}"#,
            r#"{"type":"user/message","seq":1,"time":1788601902996,"data":{"content":[{"type":"text","text":"你好"}],"role":"user","id":"mz-1"}}"#,
            r#"{"type":"assistant/message","seq":2,"time":1788601903709,"data":{"message":{"role":"assistant","content":[{"type":"text","text":"你好！"}],"source":{"kind":"model","provider":"p","model":"glm-5.3"},"id":"mz-2"}}}"#,
        ];
        let session_dir = dir.join(session_id);
        std::fs::create_dir_all(&session_dir).expect("mkdir failed");
        let path = session_dir.join("session.jsonl.zstd");
        let bytes = zstd::stream::encode_all(lines.join("\n").as_bytes(), 3).expect("zstd encode");
        std::fs::write(&path, bytes).expect("write failed");
        path
    }

    #[test]
    fn parse_subagent_session_has_parent() {
        let dir = tempfile::TempDir::new().expect("tempdir failed");
        let session_dir = dir.path().join("session-child");
        std::fs::create_dir_all(&session_dir).expect("mkdir failed");
        let lines = [
            r#"{"type":"session","version":0,"id":"session-child","createdAt":1788601901335,"parentSession":"session-parent","origin":"subagent","delegationDepth":1}"#,
            r#"{"type":"user/message","seq":1,"time":1788601902996,"data":{"content":[{"type":"text","text":"子任务指令"}],"role":"user","id":"mc-1"}}"#,
        ];
        let f = session_dir.join("session.jsonl");
        std::fs::write(&f, lines.join("\n")).expect("write failed");

        let raw = parse_session(&f).expect("parse failed");
        assert_eq!(raw.source_parent_id.as_deref(), Some("session-parent"));
    }

    #[test]
    fn discover_sessions_filters_children_and_reads_title() {
        let dir = tempfile::TempDir::new().expect("tempdir failed");
        let home = dir.path().join(".dsh");
        let proj = home.join("sessions/-tmp-proj");
        // 顶层会话
        let top = proj.join("session-top");
        std::fs::create_dir_all(&top).expect("mkdir failed");
        std::fs::write(
            top.join("session.jsonl"),
            concat!(
                r#"{"type":"session","version":0,"id":"session-top","createdAt":1788601901335,"delegationDepth":0}"#,
                "\n",
                r#"{"type":"session/title","seq":3,"time":1788601903000,"data":{"title":"顶层标题","messageSeqs":[1],"source":{"kind":"fallback"}}}"#,
                "\n",
                r#"{"type":"user/message","seq":1,"time":1788601902996,"data":{"content":[{"type":"text","text":"hi"}],"role":"user","id":"mt-1"}}"#,
            ),
        )
        .expect("write failed");
        // subagent 子会话
        let child = proj.join("session-child");
        std::fs::create_dir_all(&child).expect("mkdir failed");
        std::fs::write(
            child.join("session.jsonl"),
            concat!(
                r#"{"type":"session","version":0,"id":"session-child","createdAt":1788601901335,"parentSession":"session-top","delegationDepth":1}"#,
                "\n",
                r#"{"type":"user/message","seq":1,"time":1788601902996,"data":{"content":[{"type":"text","text":"子任务"}],"role":"user","id":"mtc-1"}}"#,
            ),
        )
        .expect("write failed");

        let top_only = discover_sessions(&home).expect("discover failed");
        assert_eq!(top_only.len(), 1, "面板只列顶层会话");
        assert_eq!(top_only[0].session_id, "session-top");
        assert_eq!(top_only[0].title, "顶层标题");
        assert_eq!(top_only[0].message_count, 1);
        assert!(top_only[0].mtime_ms.is_some());

        let all = discover_all_sessions(&home).expect("discover failed");
        assert_eq!(all.len(), 2, "auto_sync 含子会话");
        let child_item = all
            .iter()
            .find(|s| s.session_id == "session-child")
            .expect("child session");
        assert_eq!(child_item.parent_id.as_deref(), Some("session-top"));
    }

    #[test]
    fn empty_session_errors() {
        let dir = tempfile::TempDir::new().expect("tempdir failed");
        let session_dir = dir.path().join("session-empty");
        std::fs::create_dir_all(&session_dir).expect("mkdir failed");
        let f = session_dir.join("session.jsonl");
        std::fs::write(
            &f,
            r#"{"type":"session","version":0,"id":"session-empty","createdAt":1,"delegationDepth":0}"#,
        )
        .expect("write failed");
        match parse_session(&f) {
            Err(DeepSeekHarnessError::Empty(id)) => assert_eq!(id, "session-empty"),
            other => panic!("expected Empty, got {other:?}"),
        }
    }

    #[test]
    fn normalize_pipeline_accepts_raw() {
        let dir = tempfile::TempDir::new().expect("tempdir failed");
        let f = write_zstd_session(dir.path(), "session-norm");
        let raw = parse_session(&f).expect("parse failed");
        let normalized = ch_normalization::normalize(raw).expect("normalize failed");
        assert_eq!(normalized.messages.len(), 2);
        assert!(normalized.conversation.title.is_some());
    }

    /// dsh「外部导入镜像」（session-import）必须被整体排除：
    /// 镜像 id 恒为 ext-<provider>-<原id>（目录前缀判定），
    /// 且日志带 session-import/source 来源事件（事件级兜底 + parse 防御）。
    #[test]
    fn external_import_mirrors_are_excluded() {
        let dir = tempfile::TempDir::new().expect("tempdir failed");
        let home = dir.path().join(".dsh");
        let proj = home.join("sessions/-tmp-game");

        // 原生 dsh 会话
        let native = proj.join("session-native");
        std::fs::create_dir_all(&native).expect("mkdir failed");
        std::fs::write(
            native.join("session.jsonl"),
            concat!(
                r#"{"type":"session","version":0,"id":"session-native","createdAt":1788601901335,"delegationDepth":0}"#,
                "\n",
                r#"{"type":"user/message","seq":1,"time":1788601902996,"data":{"content":[{"type":"text","text":"原生会话"}],"role":"user","id":"mn-1"}}"#,
            ),
        )
        .expect("write failed");

        // 外部镜像：ZCode 会话被 dsh session-import 复制（真实布局还原）
        let mirror = proj.join("ext-zcode-sess_0ad213ea-06bc-4bfb-82d4-25588f9f9878");
        std::fs::create_dir_all(&mirror).expect("mkdir failed");
        std::fs::write(
            mirror.join("session.jsonl"),
            concat!(
                r#"{"type":"session","version":0,"id":"ext-zcode-sess_0ad213ea-06bc-4bfb-82d4-25588f9f9878","createdAt":1788450960519,"cwd":"/tmp/game","seedLength":3113,"delegationDepth":0}"#,
                "\n",
                r#"{"type":"request/header","seq":0,"time":1788450960519,"data":{"header":{"config":{"provider":"zcode","model":"GLM-5.3-Flash"}},"reason":"initial"}}"#,
                "\n",
                r#"{"type":"session-import/source","seq":1,"time":1788607545294,"data":{"provider":"zcode","sourceId":"sess_0ad213ea-06bc-4bfb-82d4-25588f9f9878","sourcePath":"/Users/x/.zcode/cli/db/db.sqlite"}}"#,
                "\n",
                r#"{"type":"user/message","seq":2,"time":1788450961000,"data":{"content":[{"type":"text","text":"给我继续优化这个项目，参考炉石传说"}],"role":"user","id":"me-1"}}"#,
            ),
        )
        .expect("write failed");

        // 发现层：只返回原生会话（镜像按 ext- 目录前缀排除）
        let all = discover_all_sessions(&home).expect("discover failed");
        assert_eq!(all.len(), 1, "外部镜像不应进入发现结果");
        assert_eq!(all[0].session_id, "session-native");
        let top = discover_sessions(&home).expect("discover failed");
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].session_id, "session-native");

        // 解析层防御：直连 parse 镜像文件 → ExternalImport 错误
        match parse_session(mirror.join("session.jsonl")) {
            Err(DeepSeekHarnessError::ExternalImport(id)) => {
                assert!(id.starts_with("ext-zcode-"));
            }
            other => panic!("expected ExternalImport, got {other:?}"),
        }

        // 事件级兜底：目录名不带 ext- 前缀但日志带 session-import/source 的
        // 异常文件同样被 quick_info 排除
        let odd = proj.join("session-weird");
        std::fs::create_dir_all(&odd).expect("mkdir failed");
        std::fs::write(
            odd.join("session.jsonl"),
            concat!(
                r#"{"type":"session","version":0,"id":"session-weird","createdAt":1,"delegationDepth":0}"#,
                "\n",
                r#"{"type":"session-import/source","seq":1,"time":2,"data":{"provider":"codex"}}"#,
                "\n",
                r#"{"type":"user/message","seq":2,"time":3,"data":{"content":[{"type":"text","text":"hi"}],"role":"user","id":"mw-1"}}"#,
            ),
        )
        .expect("write failed");
        let all2 = discover_all_sessions(&home).expect("discover failed");
        assert!(
            all2.iter().all(|s| s.session_id != "session-weird"),
            "事件级兜底应排除带导入来源事件的会话"
        );
    }

    /// 真实环境回归（手动跑）：本机 ~/.dsh 存在时 discover + parse 全量会话。
    /// cargo test -p ch-adapter-deepseek-harness --lib -- real_dsh --show-output
    #[test]
    #[ignore = "依赖本机真实 ~/.dsh 数据，CI 不跑"]
    fn real_dsh_home_smoke() {
        let home = std::env::var("HOME").expect("HOME");
        let dsh = std::path::Path::new(&home).join(".dsh");
        assert!(dsh.exists(), "本机无 ~/.dsh，跳过语义不适用");
        let all = discover_all_sessions(&dsh).expect("discover failed");
        assert!(!all.is_empty(), "本机应有至少 1 个 dsh 会话");
        assert!(
            all.iter().all(|s| !s.session_id.starts_with("ext-")),
            "外部导入镜像（ext-*）不应出现在发现结果中"
        );
        for s in &all {
            println!(
                "session={} title={:?} msgs={} parent={:?} mtime={:?}",
                s.session_id, s.title, s.message_count, s.parent_id, s.mtime_ms
            );
            let raw = parse_session(&s.file_path).expect("parse failed");
            println!(
                "  → parse: title={:?} model={:?} msgs={} events={}",
                raw.title,
                raw.model,
                raw.messages.len(),
                raw.events.len()
            );
            assert!(!raw.title.unwrap_or_default().is_empty(), "标题不应为空");
            assert!(!raw.messages.is_empty(), "真实会话应有消息");
        }
    }
}
