//! Handoff：把 threadock 归档会话写回目标 Agent 的原生会话存储。
//!
//! 与 adapter 层的「只读」相反，本 crate 是唯一的写出口：按各目标工具的
//! 原生格式（JSONL / SQLite）落一条**全新**会话（新 ID、纯增量写入，不触碰
//! 目标库已有数据），让用户在 Claude Code / Codex / ZCode / MiniMax Code /
//! Cursor 里 resume 后基于完整上下文继续开发。
//!
//! 保真策略见 [`transcript`]：工具调用无法跨工具原生重放，统一折叠为
//! 上下文前言 + 纯文本对话记录。

pub mod claude;
pub mod codex;
pub mod cursor;
pub mod loader;
pub mod minimax;
pub mod transcript;
pub mod zcode;

#[cfg(test)]
mod testsupport;

use ch_domain::Provider;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

pub use loader::OwnedHandoffInput;
pub use transcript::HandoffInput;

/// 写回目标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HandoffTarget {
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "zcode")]
    ZCode,
    #[serde(rename = "minimax-code")]
    MinimaxCode,
    #[serde(rename = "cursor")]
    Cursor,
}

impl HandoffTarget {
    /// 稳定字符串标识（CLI 参数 / RPC 参数用）。
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            HandoffTarget::ClaudeCode => "claude-code",
            HandoffTarget::Codex => "codex",
            HandoffTarget::ZCode => "zcode",
            HandoffTarget::MinimaxCode => "minimax-code",
            HandoffTarget::Cursor => "cursor",
        }
    }

    /// 全部目标（按默认展示顺序）。
    #[must_use]
    pub fn all() -> Vec<HandoffTarget> {
        vec![
            HandoffTarget::ClaudeCode,
            HandoffTarget::Codex,
            HandoffTarget::ZCode,
            HandoffTarget::MinimaxCode,
            HandoffTarget::Cursor,
        ]
    }
}

impl std::fmt::Display for HandoffTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for HandoffTarget {
    type Err = HandoffError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude-code" | "claude" => Ok(HandoffTarget::ClaudeCode),
            "codex" => Ok(HandoffTarget::Codex),
            "zcode" => Ok(HandoffTarget::ZCode),
            "minimax-code" | "minimax" => Ok(HandoffTarget::MinimaxCode),
            "cursor" => Ok(HandoffTarget::Cursor),
            other => Err(HandoffError::InvalidTarget(other.to_string())),
        }
    }
}

/// 单次 handoff 的执行选项。
#[derive(Debug, Clone)]
pub struct HandoffOptions {
    /// 用户主目录（默认目标路径的根：`~/.claude` 等）。
    pub home_dir: PathBuf,
    /// 工作目录兜底（会话无 workspace 时用；Claude/Codex/ZCode/MiniMax 均需要 cwd）。
    pub fallback_cwd: PathBuf,
    /// 目标存储路径覆盖（测试与「导出到别处再拷贝」场景用）：
    /// claude-code 传 `.claude` 目录；codex 传 `.codex` 目录；
    /// zcode / minimax / cursor 传 sqlite 文件路径。
    pub dest_override: Option<PathBuf>,
    /// 只计算不写入（列出将要创建的会话）。
    pub dry_run: bool,
    /// 是否附带上下文前言（执行时间线）。
    pub include_context: bool,
    /// 用户自定义脱敏规则（与导出同源，见 plan §14.6）：交接写入目标库前
    /// 逐条应用到消息文本与前言。空 = 不脱敏。
    pub redact_rules: Vec<ch_export::CustomRule>,
}

impl HandoffOptions {
    /// 以给定 home 目录构造默认选项。
    #[must_use]
    pub fn new(home_dir: impl Into<PathBuf>) -> Self {
        let home_dir = home_dir.into();
        let fallback_cwd = home_dir.clone();
        Self {
            home_dir,
            fallback_cwd,
            dest_override: None,
            dry_run: false,
            include_context: true,
            redact_rules: Vec::new(),
        }
    }
}

/// 单条会话的 handoff 结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoffOutcome {
    /// 目标标识（`HandoffTarget::as_str`）。
    pub target: String,
    /// 归档侧会话内部 ID（`conv_...`）。
    pub conversation_id: String,
    /// 会话标题。
    pub title: String,
    /// 目标侧新会话 ID（resume 时用）。
    pub session_id: String,
    /// 落点（文件路径或 sqlite 路径）。
    pub destination: String,
    /// 写入的记录数（含前言）。
    pub items_written: usize,
    /// 是否 dry-run（未落盘）。
    pub dry_run: bool,
}

/// 批量结果：单条失败不中断整批。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoffBatchResult {
    pub ok: Vec<HandoffOutcome>,
    pub failed: Vec<HandoffFailure>,
}

/// 单条失败记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoffFailure {
    pub conversation_id: String,
    pub title: String,
    pub error: String,
}

#[derive(Debug, Error)]
pub enum HandoffError {
    #[error("unknown handoff target: {0} (claude-code | codex | zcode | minimax-code | cursor)")]
    InvalidTarget(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("database error: {0}")]
    Db(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("conversation has no renderable messages")]
    Empty,
    #[error("目标库缺少表 {0}（Agent 版本可能已升级变更存储格式），已中止且未写入任何数据")]
    SchemaMismatch(String),
}

impl From<rusqlite::Error> for HandoffError {
    fn from(e: rusqlite::Error) -> Self {
        HandoffError::Db(e.to_string())
    }
}

pub type HandoffResult<T> = std::result::Result<T, HandoffError>;

/// 把一条会话交接给目标工具（写一条全新原生会话）。
pub fn handoff(
    input: &HandoffInput,
    target: HandoffTarget,
    opts: &HandoffOptions,
) -> HandoffResult<HandoffOutcome> {
    match target {
        HandoffTarget::ClaudeCode => claude::write(input, opts),
        HandoffTarget::Codex => codex::write(input, opts),
        HandoffTarget::ZCode => zcode::write(input, opts),
        HandoffTarget::MinimaxCode => minimax::write(input, opts),
        HandoffTarget::Cursor => cursor::write(input, opts),
    }
}

/// 批量交接：逐条执行，失败记入 [`HandoffBatchResult::failed`]。
#[must_use]
pub fn handoff_batch(
    inputs: &[HandoffInput<'_>],
    target: HandoffTarget,
    opts: &HandoffOptions,
) -> HandoffBatchResult {
    let mut result = HandoffBatchResult {
        ok: Vec::new(),
        failed: Vec::new(),
    };
    for input in inputs {
        match handoff(input, target, opts) {
            Ok(o) => result.ok.push(o),
            Err(e) => result.failed.push(HandoffFailure {
                conversation_id: input.conversation.id.clone(),
                title: input.title().to_string(),
                error: e.to_string(),
            }),
        }
    }
    result
}

/// 会话的来源 Provider 是否与目标同源（同源交接时用于提示）。
#[must_use]
pub fn same_provider(conversation_provider: Provider, target: HandoffTarget) -> bool {
    matches!(
        (conversation_provider, target),
        (Provider::ClaudeCode, HandoffTarget::ClaudeCode)
            | (Provider::Codex, HandoffTarget::Codex)
            | (Provider::ZCode, HandoffTarget::ZCode)
            | (Provider::MinimaxCode, HandoffTarget::MinimaxCode)
            | (Provider::Cursor, HandoffTarget::Cursor)
    )
}

// ── 目标存储路径 ──────────────────────────────────────────────────────────

/// 用户主目录：优先 `HOME`，Windows 无 `HOME` 时回退 `USERPROFILE`。
#[must_use]
pub fn user_home() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_or_else(|_| PathBuf::from("."), PathBuf::from)
}

/// Cursor 的 `state.vscdb` 路径（按 OS 解析；与读取侧 `ch_domain::config::cursor_db` 同源）。
#[must_use]
pub fn cursor_db_path(home: &std::path::Path) -> PathBuf {
    ch_domain::config::cursor_db_path(home)
}

/// 目标的默认落点：claude-code / codex 为其 home 根目录，其余为 sqlite 库文件。
#[must_use]
pub fn default_destination(target: HandoffTarget, home: &std::path::Path) -> PathBuf {
    match target {
        HandoffTarget::ClaudeCode => home.join(".claude"),
        HandoffTarget::Codex => home.join(".codex"),
        HandoffTarget::ZCode => home.join(".zcode/cli/db/db.sqlite"),
        HandoffTarget::MinimaxCode => home.join(".minimax/v2/sqlite/runtime-state.sqlite"),
        HandoffTarget::Cursor => cursor_db_path(home),
    }
}

/// 目标存储是否已在本机存在（判断该 Agent 是否安装并初始化过）。
#[must_use]
pub fn store_exists(target: HandoffTarget, home: &std::path::Path) -> bool {
    default_destination(target, home).exists()
}

/// 解析实际落点：`dest_override` 优先，否则默认路径。
pub(crate) fn resolve_dest(target: HandoffTarget, opts: &HandoffOptions) -> PathBuf {
    opts.dest_override
        .clone()
        .unwrap_or_else(|| default_destination(target, &opts.home_dir))
}

/// 写入前的 schema 守卫：目标库缺少任一必需表时立即中止。
///
/// 各 Agent 升级可能变更存储格式（如 Cursor 的 TEXT/BLOB 漂移先例）；
/// 先查 `sqlite_master` 再开事务，避免把行写进结构不符的库。
pub(crate) fn ensure_tables(conn: &rusqlite::Connection, tables: &[&str]) -> HandoffResult<()> {
    for table in tables {
        let found: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get(0),
            )
            .map_err(HandoffError::from)?;
        if found == 0 {
            return Err(HandoffError::SchemaMismatch((*table).to_string()));
        }
    }
    Ok(())
}

// ── 共用小工具 ────────────────────────────────────────────────────────────

/// 时间戳 → Unix 毫秒。
pub(crate) fn ts_ms(t: ch_domain::Timestamp) -> i64 {
    t.unix_timestamp() * 1000 + i64::from(t.nanosecond() / 1_000_000)
}

/// 目标 cwd：workspace 路径优先，其次兜底目录。
pub(crate) fn effective_cwd(input: &HandoffInput, opts: &HandoffOptions) -> String {
    input.workspace_path().map_or_else(
        || opts.fallback_cwd.to_string_lossy().into_owned(),
        ToOwned::to_owned,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_roundtrip_and_aliases() {
        for t in HandoffTarget::all() {
            let back: HandoffTarget = t.as_str().parse().expect("parse failed");
            assert_eq!(t, back);
        }
        assert_eq!(
            "claude".parse::<HandoffTarget>().expect("alias"),
            HandoffTarget::ClaudeCode
        );
        assert_eq!(
            "minimax".parse::<HandoffTarget>().expect("alias"),
            HandoffTarget::MinimaxCode
        );
        assert!("nope".parse::<HandoffTarget>().is_err());
    }

    #[test]
    fn serde_uses_kebab_ids() {
        let json = serde_json::to_string(&HandoffTarget::MinimaxCode).expect("serialize");
        assert_eq!(json, "\"minimax-code\"");
        let json = serde_json::to_string(&HandoffTarget::ZCode).expect("serialize");
        assert_eq!(json, "\"zcode\"");
    }

    #[test]
    fn default_destinations_follow_reader_paths() {
        let home = std::path::Path::new("/home/u");
        assert_eq!(
            default_destination(HandoffTarget::ClaudeCode, home),
            PathBuf::from("/home/u/.claude")
        );
        assert!(default_destination(HandoffTarget::ZCode, home)
            .to_string_lossy()
            .ends_with(".zcode/cli/db/db.sqlite"));
        assert!(!store_exists(HandoffTarget::Cursor, home));
    }

    #[test]
    fn same_provider_detects_origin() {
        assert!(same_provider(Provider::Codex, HandoffTarget::Codex));
        assert!(!same_provider(Provider::Codex, HandoffTarget::ClaudeCode));
    }
}
