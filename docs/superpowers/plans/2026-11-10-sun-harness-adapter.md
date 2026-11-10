# sun-harness 接入实施计划（对标 dsh 全链路 + v1.8.0 发版）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** threadock 只读接入本地 sun-harness（`~/.sun/sessions` 的 JSONL 会话），能力对标 deepseek-harness 适配器，随后发 v1.8.0。

**Architecture:** 新 crate `ch-adapter-sun-harness` 提供 discover/parse 三入口产出 `RawConversation`；桌面端经 `source_table` 表驱动进入 auto_sync / 全保真回填 / 红点计数；前端补来源枚举与展示。Spec：`docs/superpowers/specs/2026-11-10-sun-harness-adapter-design.md`。

**Tech Stack:** Rust（ch-domain / ch-normalization / serde_json / time）、Tauri 2、React + TS。

## Global Constraints

- Provider id 一律 `"sun-harness"`；providers 表 id `"prov_sun-harness"`；stat_key `"sun"`（自动派生 `sun_imported`/`sun_skipped`）；workspace 显示名 `"Sun Harness"`；前端短标签 `"Sun"`；徽章色 `#e8a33d`
- 根目录解析：`SUN_HOME` 环境变量非空优先，否则 `~/.sun`（sun 自身 `paths.ts sunHome()` 同规则）
- 会话文件两种形态：平铺 `<root>/sessions/<id>.jsonl`；目录 `<root>/sessions/<id>/session.jsonl`
- `session/meta.importedThrough` 非空 = 外部导入镜像 → 发现层跳过、parse 返回 `ExternalImport`
- 降级原则：未知事件/工具/字段不崩溃不猜字段；坏行（torn line）跳过
- 范围外（禁改）：live-monitor、handoff、export、`crates/llm`；`crates/storage` 生产代码零改动（dsh 的 storage 命中均为测试与模型名启发式，sun 无需）
- spec 勘误（相对 spec §3.7）：`SettingsView.tsx` 无需改动（grep 命中是 LLM 供应商预设，非来源清单）；storage/ops_queries 无需改动（sun 模型串如 `deepseek/…` 已命中现有启发式，缺省回退 200K）
- 注释用中文、风格对齐 dsh adapter；提交遵循 conventional commits；pre-commit/pre-push 钩子存在（npm test 可能 flaky，重跑即过）
- 发版版本 **1.8.0**：四处（根 `Cargo.toml [workspace.package]`、`apps/desktop/package.json`、`apps/desktop/package-lock.json` ×2 处、`apps/desktop/src-tauri/tauri.conf.json`）+ 两份 Cargo.lock（根 + `apps/desktop/src-tauri`）用 `cargo metadata` 刷新（禁止 `cargo update -w` 全量升级）

---

### Task 1: domain 增加 `Provider::SunHarness`

**Files:**
- Modify: `crates/domain/src/lib.rs:39-98`（枚举/as_str/from_str）与 `:650-670`（roundtrip 测试）

**Interfaces:**
- Produces: `Provider::SunHarness`（`as_str()=="sun-harness"`，`"sun-harness".parse::<Provider>()` 可逆）——Task 2 crate 依赖

- [x] **Step 1: 枚举追加成员**（必须追加在末尾，序列化稳定）

```rust
    /// DeepSeek Harness（dsh）CLI。
    /// 按上方约定追加到枚举末尾，保证已有成员序列化稳定。
    #[serde(rename = "deepseek-harness")]
    DeepSeekHarness,
    /// Sun Harness（sun）CLI/Desktop。
    /// 按上方约定追加到枚举末尾，保证已有成员序列化稳定。
    #[serde(rename = "sun-harness")]
    SunHarness,
```

- [x] **Step 2: as_str / from_str 各加一行**

```rust
            Provider::DeepSeekHarness => "deepseek-harness",
            Provider::SunHarness => "sun-harness",
```

```rust
            "deepseek-harness" => Ok(Provider::DeepSeekHarness),
            "sun-harness" => Ok(Provider::SunHarness),
```

- [x] **Step 3: roundtrip 测试列表加 `Provider::SunHarness,`**（`provider_roundtrip`）

- [x] **Step 4: 跑测试** `cargo test -p ch-domain` → PASS

- [x] **Step 5: Commit** `git commit -m "feat(domain): Provider::SunHarness 来源枚举"`

### Task 2: `ch-adapter-sun-harness` crate（TDD）

**Files:**
- Create: `crates/adapter-sun-harness/Cargo.toml`
- Create: `crates/adapter-sun-harness/src/lib.rs`
- Modify: `Cargo.toml`（members 增 `"crates/adapter-sun-harness"`，加在 `"crates/adapter-deepseek-harness",` 之后）

**Interfaces:**
- Consumes: `Provider::SunHarness`（Task 1）；`ch_normalization::{RawConversation, RawEvent, RawMessage}`、`ch_domain::{EventType, Role}`（现有）
- Produces: `discover_sessions(root: impl AsRef<Path>) -> AdapterResult<Vec<DiscoveredSession>>`（顶层）、`discover_all_sessions(root)`（含 subagent）、`parse_session(file_path: impl AsRef<Path>) -> AdapterResult<RawConversation>`、`DiscoveredSession{session_id,title,message_count,created_at_ms,file_path,size_bytes,mtime_ms,parent_id}`、`ADAPTER_ID="sun-harness"`、`PROVIDER`——Task 3/4 依赖

- [x] **Step 1: Cargo.toml**

```toml
[package]
name = "ch-adapter-sun-harness"
description = "Sun Harness (sun) Adapter：读取 $SUN_HOME|~/.sun/sessions 下 <id>.jsonl（plan §10.5）"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
ch-domain = { path = "../domain" }
ch-normalization = { path = "../normalization" }
thiserror.workspace = true
serde.workspace = true
serde_json.workspace = true
time.workspace = true

[dev-dependencies]
tempfile = "3"
```

- [x] **Step 2: 先写失败测试 + 完整实现**（同文件一次落盘，随后跑测试验证）

`src/lib.rs` 完整内容（实现与测试一体；事件 schema 全部顶层字段，`ts`/`createdAt` 为 RFC3339 字符串）：

```rust
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
                            .unwrap_or_else(|| {
                                call.get("arguments").cloned().unwrap_or_default()
                            });
                        let (event_type, summary, mut payload) =
                            event_for_tool_call(&name, &args);
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
                            if let Some(err) =
                                rec.get("isError").and_then(serde_json::Value::as_bool)
                            {
                                obj.insert("isError".into(), serde_json::json!(err));
                            }
                            if let Some(d) =
                                rec.get("durationMs").and_then(serde_json::Value::as_i64)
                            {
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
        assert!(cj.to_string().contains("3760"), "usage 应保真进 content_json");

        // bash → CommandCompleted（配对升格）；孤立 result 忽略；
        // 注入块 + systemPrompt 两个 SystemBlock 追加在末尾
        assert_eq!(raw.events.len(), 3, "工具 1 + SystemBlock 2");
        assert_eq!(raw.events[0].event_type, EventType::CommandCompleted);
        assert_eq!(
            raw.events[0].summary.as_deref(),
            Some("git status --short")
        );
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
```

- [x] **Step 3: 跑测试**

Run: `cargo test -p ch-adapter-sun-harness`
Expected: 全部 PASS（`real_sun_home_smoke` 被 `#[ignore]` 跳过）

- [x] **Step 4: 真机回归** `cargo test -p ch-adapter-sun-harness --lib -- --ignored --show-output`
Expected: 435 个会话全量 discover+parse 无 panic、标题非空

- [x] **Step 5: Commit** `git add Cargo.toml crates/adapter-sun-harness && git commit -m "feat(adapter): sun-harness 会话发现与解析（对标 dsh）"`

### Task 3: 桌面端后端接线（import.rs / lib.rs / typegen）

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands/import.rs`（dsh 段之后，≈`:460` 处加命令；`source_table` dsh 段后加 sun 段；tests 模块加 sun 测试）
- Modify: `apps/desktop/src-tauri/src/lib.rs:288-289`（注册命令）与 `:429`（stat keys 测试数组）
- Modify: `apps/desktop/src-tauri/Cargo.toml`（依赖，`ch-adapter-deepseek-harness` 行后）
- Modify: `apps/desktop/src-tauri/examples/typegen.rs:194-203`（命令签名表）

**Interfaces:**
- Consumes: Task 2 的 `discover_sessions`/`discover_all_sessions`/`parse_session`
- Produces: tauri 命令 `list_sun_sessions(state) -> Vec<SourceSessionDto>`、`import_from_sun(state, session_id) -> ImportResultDto`；`source_table` 增 `provider_id:"prov_sun-harness"` / `stat_key:"sun"` / `workspace:"Sun Harness"` 段

- [x] **Step 1: Cargo.toml 依赖**

```toml
ch-adapter-sun-harness = { path = "../../../crates/adapter-sun-harness" }
```

- [x] **Step 2: import.rs 加 `sun_home` + 两条命令**（紧跟 `import_from_deepseek` 之后；`use` 块已有 `import_ctx`/`imported_flag`/`run_blocking`/`SourceSessionDto`/`ImportResultDto`）

```rust
/// Sun Harness (sun) home 路径：SUN_HOME 优先（sun 自身解析规则），缺省 ~/.sun。
pub(crate) fn sun_home() -> Result<String, String> {
    if let Ok(p) = std::env::var("SUN_HOME") {
        if !p.trim().is_empty() {
            return Ok(p);
        }
    }
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    Ok(format!("{home}/.sun"))
}

/// 列出 Sun Harness 会话（面板只列顶层会话）。
#[tauri::command]
pub(crate) async fn list_sun_sessions(
    state: tauri::State<'_, DaemonState>,
) -> Result<Vec<SourceSessionDto>, String> {
    let home = sun_home()?;
    let sessions = ch_adapter_sun_harness::discover_sessions(&home)
        .map_err(|e| format!("discover sun harness: {e}"))?;
    let ictx = import_ctx(&state);
    Ok(sessions
        .into_iter()
        .map(|s| SourceSessionDto {
            imported: imported_flag(&ictx, "prov_sun-harness", &s.session_id, s.mtime_ms),
            session_id: s.session_id,
            title: s.title,
            detail: format!("{} 条消息", s.message_count),
            message_count: Some(s.message_count),
        })
        .collect())
}

/// 从 Sun Harness 导入一条会话。
#[tauri::command]
pub(crate) async fn import_from_sun(
    state: tauri::State<'_, DaemonState>,
    session_id: String,
) -> Result<ImportResultDto, String> {
    run_blocking(|| {
        let home = sun_home()?;
        let sessions = ch_adapter_sun_harness::discover_all_sessions(&home)
            .map_err(|e| format!("discover sun harness: {e}"))?;
        let session = sessions
            .into_iter()
            .find(|s| s.session_id == session_id)
            .ok_or_else(|| format!("session not found: {session_id}"))?;
        let raw = ch_adapter_sun_harness::parse_session(&session.file_path)
            .map_err(|e| format!("parse sun harness: {e}"))?;
        import_raw_to_state(&state, raw, Some("Sun Harness"), session.mtime_ms)
    })
}
```

- [x] **Step 3: `source_table` 加 sun 段**（dsh 段之后、`out` 返回之前；注意 `discover` 闭包 move `sun_root`，无需 clone——存在性检查在 move 之前完成）

```rust
    // Sun Harness：$SUN_HOME|~/.sun/sessions 下平铺 <id>.jsonl（或 <id>/session.jsonl）
    let sun_root = std::env::var("SUN_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("{home}/.sun"));
    if mk(std::path::Path::new(&format!("{sun_root}/sessions")).exists()) {
        let root = sun_root;
        out.push(SourceSync {
            provider_id: "prov_sun-harness",
            stat_key: "sun",
            workspace: "Sun Harness",
            records_observed: true,
            discover: Box::new(move || {
                ch_adapter_sun_harness::discover_all_sessions(&root)
                    .map(|v| {
                        v.into_iter()
                            .map(|s| SourceItem {
                                session_id: s.session_id.clone(),
                                src_ms: s.mtime_ms.unwrap_or(0),
                                observed_ms: s.mtime_ms,
                                file_path: Some(s.file_path),
                                parent_id: s.parent_id,
                                title: listing_title(s.title),
                            })
                            .collect()
                    })
                    .map_err(|e| e.to_string())
            }),
            parse: Box::new(move |it| {
                ch_adapter_sun_harness::parse_session(
                    it.file_path.as_deref().unwrap_or_default(),
                )
                .map_err(|e| e.to_string())
            }),
        });
    }
```

- [x] **Step 4: lib.rs 注册命令**（`:288-289` 附近 `import_from_deepseek,` 后）

```rust
            list_sun_sessions,
            import_from_sun,
```

- [x] **Step 5: lib.rs 测试 stat keys 数组加 `"sun",`**（`"deepseek",` 后，≈`:429`）

- [x] **Step 6: typegen.rs 命令签名表加两行**（dsh 两行之后）

```rust
        ("import_from_sun", "sessionId", "ImportResultDto"),
        ("list_sun_sessions", "", "SourceSessionDto[]"),
```

- [x] **Step 7: import.rs tests 模块加 sun auto-sync 测试**（复用该模块既有 `DaemonState` 构造模式；env 改 HOME 与既有测试同款）

```rust
    #[test]
    fn auto_sync_sun_imports_and_skips_mirrors() {
        let data_dir = tempfile::TempDir::new().expect("tempdir");
        let state = DaemonState::open(DaemonStateConfig {
            data_dir: data_dir.path().to_path_buf(),
            ..Default::default()
        })
        .expect("state open");

        // 临时 HOME + ~/.sun/sessions：1 原生会话 + 1 importedThrough 镜像
        let home = tempfile::TempDir::new().expect("tempdir");
        std::env::set_var("HOME", home.path());
        std::env::remove_var("SUN_HOME");
        let sessions = home.path().join(".sun/sessions");
        std::fs::create_dir_all(&sessions).expect("mkdir");
        std::fs::write(
            sessions.join("20261107-01native.jsonl"),
            concat!(
                r#"{"seq":0,"ts":"2026-11-07T06:54:02.180Z","type":"session/meta","id":"20261107-01native","title":"原生会话","mode":"coding","cwd":"/tmp/p","model":"qwen/qwen-flash","createdAt":"2026-11-07T06:54:02.180Z"}"#,
                "\n",
                r#"{"seq":1,"ts":"2026-11-07T06:54:02.239Z","type":"user/message","text":"你好","source":"user"}"#,
            ),
        )
        .expect("write native");
        std::fs::write(
            sessions.join("20261107-02mirror.jsonl"),
            concat!(
                r#"{"seq":0,"ts":"2026-11-07T06:00:00.000Z","type":"session/meta","id":"20261107-02mirror","title":"外部镜像","mode":"coding","cwd":"/tmp/p","model":"m","createdAt":"2026-11-07T06:00:00.000Z","importedThrough":3}"#,
                "\n",
                r#"{"seq":1,"ts":"2026-11-07T06:00:01.000Z","type":"user/message","text":"hi","source":"user"}"#,
            ),
        )
        .expect("write mirror");

        let v = auto_sync_inner(&state, None).expect("sync");
        assert_eq!(
            v.get("sun_imported"),
            Some(&serde_json::json!(1)),
            "只导入原生会话"
        );
        // 幂等：重跑无新增
        let v2 = auto_sync_inner(&state, None).expect("sync2");
        assert_eq!(v2.get("sun_imported"), Some(&serde_json::json!(0)));
        // 镜像不入库
        let repo = state.repo.lock().expect("repo lock");
        let sun: Vec<String> = repo
            .list_conversation_sources()
            .expect("sources")
            .into_iter()
            .filter(|(pid, _)| pid == "prov_sun-harness")
            .map(|(_, sid)| sid)
            .collect();
        assert_eq!(sun, vec!["20261107-01native".to_string()]);
    }
```

- [x] **Step 8: 编译 + 测试**

Run: `cargo test -p ch-threadock-desktop --lib commands::import` 与 `cargo test -p ch-threadock-desktop --lib`（crate 名以 `apps/desktop/src-tauri/Cargo.toml [package] name` 为准）
Expected: PASS（含新 sun 测试与既有 lib.rs stat-keys 测试）

- [x] **Step 9: Commit** `git commit -m "feat(import): sun-harness 列表/导入/auto_sync 接线"`

### Task 4: journey 端到端测试

**Files:**
- Modify: `apps/desktop/src-tauri/src/adapter_journeys.rs`（`make_dsh_session`/dsh journey 之后追加）

**Interfaces:**
- Consumes: Task 2 `parse_session`；现有 `import_raw_to_state` / `get_conversation_detail`

- [x] **Step 1: 追加 fixture + journey 测试**

```rust
/// 合成一个 Sun Harness 会话文件（标题演进 + bash 输出配对 + systemPrompt 保留）。
fn make_sun_session(dir: &std::path::Path) -> std::path::PathBuf {
    let f = dir.join("20261107-01journey.jsonl");
    let lines = [
        r#"{"seq":0,"ts":"2026-11-07T06:54:02.180Z","type":"session/meta","id":"20261107-01journey","title":"初始标题","mode":"coding","cwd":"/tmp/proj","model":"qwen/qwen-flash","createdAt":"2026-11-07T06:54:02.180Z"}"#,
        r#"{"seq":1,"ts":"2026-11-07T06:54:02.239Z","type":"user/message","text":"跑一下测试","source":"user"}"#,
        r#"{"seq":2,"ts":"2026-11-07T06:54:02.360Z","type":"request/header","model":"qwen/qwen-flash","systemPrompt":"You are sun-harness."}"#,
        r#"{"seq":3,"ts":"2026-11-07T06:54:03.597Z","type":"assistant/message","text":"","toolCalls":[{"id":"call_j1","name":"bash","arguments":"{\"command\":\"cargo test -p app\"}"}],"usage":{"inputTokens":100,"outputTokens":5},"model":"qwen/qwen-flash"}"#,
        r#"{"seq":4,"ts":"2026-11-07T06:54:03.600Z","type":"tool/result","callId":"call_j1","name":"bash","output":"test result: ok. 3 passed","isError":false,"durationMs":1200}"#,
        r#"{"seq":5,"ts":"2026-11-07T06:54:04.000Z","type":"assistant/message","text":"测试全绿。","toolCalls":[],"usage":{"inputTokens":10,"outputTokens":2},"model":"qwen/qwen-flash"}"#,
        r#"{"seq":6,"ts":"2026-11-07T06:55:00.000Z","type":"session/title","title":"源侧重命名"}"#,
    ];
    std::fs::write(&f, lines.join("\n")).expect("file I/O failed");
    f
}

/// 旅程：sun 会话 → 入库 → 详情（标题取最后 title 事件、命令配对输出、系统块保留）。
#[test]
fn journey_sun_harness_import_to_detail() {
    let app = tauri::test::mock_app();
    let dir = tempfile::TempDir::new().expect("tempdir");
    use tauri::Manager as _;
    let state = DaemonState::open(DaemonStateConfig {
        data_dir: dir.path().to_path_buf(),
        ..Default::default()
    })
    .expect("open state");
    app.manage(state);
    let state = app.state::<DaemonState>();

    // 1. 解析：标题 / 消息 / 事件
    let session_file = make_sun_session(dir.path());
    let raw = ch_adapter_sun_harness::parse_session(&session_file).expect("parse sun session");
    assert_eq!(
        raw.title.as_deref(),
        Some("源侧重命名"),
        "最后的 title 事件生效"
    );
    assert_eq!(raw.messages.len(), 2, "user + assistant 正文各 1 条");
    assert_eq!(raw.events.len(), 2, "bash 1 条 + 末尾 systemPrompt 1 条");

    // 2. 入库
    let dto = crate::commands::import_raw_to_state(&state, raw, Some("Sun Harness"), None)
        .expect("import");
    let conv_id = dto.conversation_id.clone();

    // 3. 详情：标题 / provider / 命令完成 / 系统块全链路可见
    let detail = tauri::async_runtime::block_on(crate::commands::get_conversation_detail(
        state.clone(),
        conv_id.clone(),
    ))
    .expect("detail");
    assert_eq!(detail.conversation.title.as_deref(), Some("源侧重命名"));
    assert_eq!(detail.conversation.provider, "sun-harness");
    let cmd = detail
        .events
        .iter()
        .find(|e| e.event_type == "command_completed")
        .expect("bash 配对后应 Completed");
    assert!(
        cmd.payload_json.as_deref().unwrap().contains("3 passed"),
        "payload 应含配对输出"
    );
    assert!(
        detail
            .events
            .iter()
            .any(|e| e.event_type == "system_block"),
        "systemPrompt 系统块应进详情（上下文插件展示用）"
    );
}
```

- [x] **Step 2: 跑** `cargo test -p <desktop-crate> --lib journey_sun` → PASS

- [x] **Step 3: Commit** `git commit -m "test(journey): sun-harness 入库到详情端到端"`

### Task 5: 前端接线（6 文件）

**Files:**
- Modify: `apps/desktop/src/types.ts:170`
- Modify: `apps/desktop/src/ImportMenu.tsx:14`
- Modify: `apps/desktop/src/App.tsx:263` 与 `:1434`
- Modify: `apps/desktop/src/ConversationList.tsx:384` 与 `:610`
- Modify: `apps/desktop/src/ops-types.ts:63`
- Modify: `apps/desktop/src/OverviewCards.tsx:14`

**Interfaces:**
- Consumes: 后端 `sun_imported` 计数键、`sun-harness` provider 字符串
- Produces: 前端来源枚举/徽章/文案含 sun

- [x] **Step 1: types.ts 来源标签映射**（`"deepseek-harness": "dsh"` 后加）

```ts
"sun-harness": "Sun",
```

- [x] **Step 2: ImportMenu.tsx NewCount 接口**（`deepseek?: number;` 后加）

```ts
  sun?: number;
```

- [x] **Step 3: App.tsx 同步结果标签对**（`["deepseek","dsh"]` 后加）

```ts
,["sun","Sun"]
```

- [x] **Step 4: App.tsx:1434 与 ConversationList.tsx:610 引导文案**：`Codex / DeepSeek Harness` → `Codex / DeepSeek Harness / Sun Harness`

- [x] **Step 5: ConversationList.tsx:384 providerChips**（`"deepseek-harness"` 后加）

```ts
, "sun-harness"
```

- [x] **Step 6: ops-types.ts:63 与 OverviewCards.tsx:14 PROVIDER_META**（dsh 行后加）

```ts
  "sun-harness": { label: "Sun", color: "#e8a33d" },
```

- [x] **Step 7: 类型检查/构建** `cd apps/desktop && pnpm build`（或项目既有 tsc 命令）→ 无类型错误

- [x] **Step 8: Commit** `git commit -m "feat(ui): sun-harness 来源徽章/计数/文案"`

### Task 6: 全量验证 + 真机冒烟

**Files:** 无新改动（验证任务；发现问题回改后重跑）

- [x] **Step 1: workspace 全量测试** `cargo test --workspace` → PASS
- [x] **Step 2: clippy** `cargo clippy --workspace --all-targets` 无新告警（注意 CI stable clippy 新 pedantic lint 漂移：如有 `-A` 需求，ci.yml 与 scripts/precheck.sh 同步改）
- [x] **Step 3: 真机回归** `cargo test -p ch-adapter-sun-harness --lib -- --ignored --show-output` → 435 会话全通过
- [x] **Step 4: precheck** `bash scripts/precheck.sh` → 全绿
- [x] **Step 5: GUI 冒烟** `cd apps/desktop && pnpm tauri dev`：验证 ① 会话列表出现 Sun 徽章会话 ② 抽样点开 sun 会话详情（消息/命令/系统块）③ 搜索命中 sun 会话内容 ④ ops 卡片含 Sun ⑤ 截图留证。若 GUI 环境不可用，以 Step 3 的全量真实数据回归 + Step 4 为替代证据并在交付说明中明示

### Task 7: README 同步（EN 权威 + zh-CN）

**Files:**
- Modify: `README.md`（来源清单 ≈`:10`、adapter 数量与清单、目录树 adapters、致谢）
- Modify: `README.zh-CN.md`（`:10`、`:98`、`:206`、`:238`、`:369`）

- [x] **Step 1: EN**：来源列表加 `Sun Harness`；adapter 数量 8→9 并把 `sun-harness` 加入 adapter 清单；目录树加 `│   ├── adapter-sun-harness/ Sun Harness Adapter`（与 dsh 行并列）
- [x] **Step 2: zh-CN**：`:10` 来源列表加 `Sun Harness`；`:98` 「8 个只读 Adapter」→「9 个只读 Adapter」+ 清单加 `Sun Harness`；`:206` 八个→九个 + `sun-harness`；`:238` 目录树加 adapter-sun-harness 行；`:369` 致谢加 `Sun Harness`
- [x] **Step 3: 其余 6 语言翻译版允许滞后（既有惯例），不改**
- [x] **Step 4: Commit** `git commit -m "docs(readme): sun-harness 接入（EN + zh-CN）"`

### Task 8: v1.8.0 发版

**Files:**
- Modify: `Cargo.toml:34`（`version = "1.7.0"` → `"1.8.0"`）
- Modify: `apps/desktop/package.json`（version → 1.8.0）
- Modify: `apps/desktop/package-lock.json`（**两处**：顶层 `version` 与 `packages.""` 的 `version`）
- Modify: `apps/desktop/src-tauri/tauri.conf.json`（`"version": "1.7.0"` → `"1.8.0"`）
- Regenerate: `Cargo.lock` 与 `apps/desktop/src-tauri/Cargo.lock`

- [x] **Step 1: 改四处版本号为 1.8.0**（package-lock 两处都要，漏一处必挂）
- [x] **Step 2: 刷新两份 lock（禁止全量升级）**

```bash
cargo metadata --format-version 1 > /dev/null          # 根 lock：新 crate 条目 + workspace 版本
(cd apps/desktop/src-tauri && cargo metadata --format-version 1 > /dev/null)  # src-tauri lock
```

验证：`grep -c '"ch-adapter-sun-harness"' Cargo.lock` ≥1；两份 lock 中 `ch-domain` 等成员版本为 1.8.0：`grep -A1 'name = "ch-domain"' Cargo.lock apps/desktop/src-tauri/Cargo.lock`

- [x] **Step 3: 全量测试 + precheck 复跑**（版本改动会触发 round5 类 CORE_VERSION 测试）`cargo test --workspace && bash scripts/precheck.sh` → 全绿
- [x] **Step 4: 发版提交**

```bash
git add -A && git commit -m "release: v1.8.0——sun-harness 接入（对标 dsh 全链路）"
```

- [x] **Step 5: 推送 master（HTTPS + 代理兜底；钩子 flaky 则重推）**

```bash
git push https://github.com/SunArthurX/threadock.git master:master
# 失败时：https_proxy=http://127.0.0.1:7890 git push https://github.com/SunArthurX/threadock.git master:master
git ls-remote https://github.com/SunArthurX/threadock.git refs/heads/master  # SHA == 本地 rev-parse HEAD
```

- [x] **Step 6: 推 tag 触发 Release**

```bash
git tag v1.8.0 && git push https://github.com/SunArthurX/threadock.git refs/tags/v1.8.0
gh run list --limit 3   # release.yml 触发；CI npm test flaky 重推 tag 即过
```

- [x] **Step 7: 确认 GitHub Release 生成**（`gh release view v1.8.0`）

---

## Self-Review 记录

- **Spec 覆盖**：§1 发现层→Task 2；§2 事件映射→Task 2；§3 接线 1-6→Task 1-4，7→Task 5，8→Task 3/8；§4 降级→Task 2 实现+测试；§5 测试验收→Task 2/3/4/6；§6 发版→Task 8。spec §3.7 的 SettingsView/storage 两处经核实为误报（Global Constraints 已勘误），无对应任务
- **占位符**：无 TBD/TODO；所有代码步骤含完整代码
- **类型一致性**：`Discover`/`parse` 签名在 Task 2 定义、Task 3/4 消费一致；`stat_key:"sun"` → `sun_imported` 与 Task 3 测试断言一致；provider 字符串 `sun-harness`/`prov_sun-harness` 全计划统一
