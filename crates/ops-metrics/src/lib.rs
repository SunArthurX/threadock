//! CodeAgentOps 采集层（plan codeagent-ops M1）。
//!
//! 从各 Agent 的本地数据源**只读**提取 ops 指标，统一为
//! [`UsageRecord`]（模型用量）与 [`ToolCallRecord`]（工具调用）。
//!
//! 与对话采集完全解耦：独立的表、独立的同步命令，不影响现有管线。
//!
//! | 来源 | 用量数据 | 工具调用数据 |
//! |------|---------|-------------|
//! | ZCode | `turn_usage` / `model_usage` | `tool_usage`（含 destructive/approval） |
//! | MiniMax | `local_runtime_token_usage`（含 cost_usd） | — |
//! | Claude Code | JSONL `usage` 字段 | tool_use 事件 |
//! | Codex | JSONL `token_count.info.total_token_usage`（累计快照） | function_call |

use ch_domain::Timestamp;
use std::path::Path;
use thiserror::Error;

pub mod claude_code;
pub mod codex;
pub mod minimax;
pub mod zcode;

pub use claude_code::collect_claude_code;
pub use codex::collect_codex;
pub use minimax::collect_minimax;
pub use zcode::collect_zcode;

pub type OpsResult<T> = std::result::Result<T, OpsError>;

#[derive(Debug, Error)]
pub enum OpsError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Unix 毫秒 → Timestamp（非法值回退当前时间）。
fn ms_to_ts(ms: i64) -> Timestamp {
    time::OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000)
        .unwrap_or_else(|_| ch_domain::now_utc())
}

/// 只读打开 SQLite。
fn open_ro(db_path: impl AsRef<Path>) -> OpsResult<rusqlite::Connection> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    Ok(conn)
}

/// Bash 类命令的破坏性推断规则（非 ZCode 来源用）。
pub fn infer_destructive(command: &str) -> bool {
    let c = command.trim();
    let rules = [
        "rm -rf",
        "rm -fr",
        "git push --force",
        "git push -f",
        "git reset --hard",
        "git checkout --",
        "git clean -fd",
        "chmod 777",
        "mkfs",
        "dd of=",
        ":(){ :|:& };:",
        "curl",
        "wget",
        "sudo ",
        "> /dev/sd",
    ];
    rules.iter().any(|r| c.starts_with(r) || c.contains(&format!(" {r}")) || c.contains(&format!("; {r}")) || c.contains(&format!("&& {r}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_domain::{Provider, UsageRecord, UsageStatus};

    #[test]
    fn destructive_rules() {
        assert!(infer_destructive("rm -rf /tmp/x"));
        assert!(infer_destructive("cd a && git push --force origin main"));
        assert!(infer_destructive("sudo apt install"));
        assert!(!infer_destructive("ls -la"));
        assert!(!infer_destructive("cargo build"));
    }

    #[test]
    fn billable_tokens() {
        let u = UsageRecord {
            id: "u1".into(),
            provider: Provider::ZCode,
            source_session_id: "s".into(),
            turn_id: None,
            model: None,
            ts: ch_domain::now_utc(),
            input_tokens: 100,
            output_tokens: 50,
            reasoning_tokens: 10,
            cache_read_tokens: 999,
            cache_write_tokens: 0,
            cost_usd: None,
            status: UsageStatus::Completed,
            duration_ms: None,
            retry_count: None,
        };
        assert_eq!(u.billable_tokens(), 160, "cache 不计费");
    }
}
