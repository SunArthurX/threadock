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

/// JSONL 单行大小上限（2MB）：超过视为二进制/图片负载，跳过不解析。
/// 背景：Codex 会话里存在单行数百 MB 的 base64 记录，整行读入 + JSON 解析
/// 会造成 GB 级内存尖峰，把整机和 UI 拖死（2026-08-14 治理页卡死事故）。
pub const MAX_JSONL_LINE: usize = 2 * 1024 * 1024;

/// 一次限行读取的结果。
pub struct CappedLine {
    /// 是否读到行结束符（EOF 且无内容时为 false）。
    pub complete: bool,
    /// 是否因超过上限被截断（调用方应跳过该行）。
    pub oversized: bool,
}

/// 流式读一行到 buf（复用容量，超限即停止存储、剩余直接丢弃）。
/// 相比 `BufRead::lines()`：永不因单行超大而膨胀内存，也避免对超大行做 JSON 解析。
pub fn read_line_capped<R: std::io::BufRead>(
    r: &mut R,
    buf: &mut Vec<u8>,
    cap: usize,
) -> std::io::Result<CappedLine> {
    buf.clear();
    let mut oversized = false;
    loop {
        let (done, consumed) = {
            let available = match r.fill_buf() {
                Ok(a) => a,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if available.is_empty() {
                return Ok(CappedLine {
                    complete: false,
                    oversized,
                });
            }
            match available.iter().position(|&b| b == b'\n') {
                Some(pos) => {
                    if !oversized {
                        let room = cap - buf.len();
                        let want = (pos + 1).min(room);
                        buf.extend_from_slice(&available[..want]);
                        if want <= pos {
                            oversized = true;
                        }
                    }
                    (true, pos + 1)
                }
                None => {
                    if !oversized {
                        let room = cap - buf.len();
                        if room > 0 {
                            let want = available.len().min(room);
                            buf.extend_from_slice(&available[..want]);
                            if want < available.len() {
                                oversized = true;
                            }
                        } else {
                            oversized = true;
                        }
                    }
                    (false, available.len())
                }
            }
        };
        r.consume(consumed);
        if done {
            return Ok(CappedLine {
                complete: true,
                oversized,
            });
        }
    }
}

pub mod assets;
pub mod automations;
pub mod claude_code;
pub mod codex;
pub mod minimax;
pub mod zcode;

pub use assets::collect_assets;
pub use automations::collect_automations;
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
    rules.iter().any(|r| {
        c.starts_with(r)
            || c.contains(&format!(" {r}"))
            || c.contains(&format!("; {r}"))
            || c.contains(&format!("&& {r}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_domain::{Provider, UsageRecord, UsageStatus};

    #[test]
    fn read_line_capped_skips_oversized_and_handles_last_line() {
        use std::io::BufReader;
        let giant = "x".repeat(64);
        let data = format!("a\nb\n{}\nc", giant); // 最后行无换行
        let mut r = BufReader::new(data.as_bytes());
        let mut buf = Vec::new();
        let cap = 32;

        let l1 = read_line_capped(&mut r, &mut buf, cap).expect("unexpected None");
        assert!(l1.complete && !l1.oversized);
        assert_eq!(buf.as_slice().trim_ascii_end(), b"a");

        let _ = read_line_capped(&mut r, &mut buf, cap).expect("unexpected None");
        assert_eq!(buf.as_slice().trim_ascii_end(), b"b");

        let l3 = read_line_capped(&mut r, &mut buf, cap).expect("unexpected None");
        assert!(l3.complete && l3.oversized, "超限行应标记 oversized");

        let l4 = read_line_capped(&mut r, &mut buf, cap).expect("unexpected None");
        assert!(!l4.complete, "EOF 且无更多内容");
        assert_eq!(
            buf.as_slice().trim_ascii_end(),
            b"c",
            "末行无换行符也应读出"
        );
    }

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
            source_dir: None,
            context_exceeded: 0,
        };
        assert_eq!(u.billable_tokens(), 160, "cache 不计费");
    }
}
