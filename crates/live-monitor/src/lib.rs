//! `ch-live-monitor`：多 coding agent 实时任务被动监控（只读，零侵入）。
//!
//! 覆盖 Claude Code / Codex（CLI+Desktop）/ ZCode / MiniMax Code 四个源，
//! 回答「现在有哪些会话在跑、跑到哪了」。数据一律直接读源侧存储
//! （JSONL 追加文件 / 只读 SQLite），不经导入流水线，与主同步互不干扰。
//!
//! 状态判定（v1 启发式，详见 docs/superpowers/specs/2026-11-02-live-agent-monitor-design.md）：
//! - Running：近 [`RUNNING_WINDOW_MS`] 有写入，或源侧显式 running（ZCode turn_usage / MiniMax status）
//! - Waiting：尾部出现审批类事件（Codex approval）
//! - Idle / Recent：写入新鲜度递降
//! - 不猜 Finished——源侧没有能证明「结束」的信号。

pub mod probe;
pub mod tail;
pub mod types;

pub use probe::{scan_all, ScanOutcome};
pub use types::{LiveActivity, LiveProgress, LiveSource, LiveState, LiveTask, SourceError};

/// 45 秒内有写入视为活跃运行。
pub const RUNNING_WINDOW_MS: i64 = 45_000;
/// 45 秒 ~ 10 分钟无写入视为 Idle。
pub const IDLE_WINDOW_MS: i64 = 10 * 60_000;
/// 30 分钟内有过活动的会话才上板（Live Board 只关心「现在」）。
pub const BOARD_WINDOW_MS: i64 = 30 * 60_000;

/// 按写入新鲜度分级（源侧无显式状态字段时的兜底判定）。
#[must_use]
pub fn classify(now_ms: i64, last_activity_ms: i64) -> LiveState {
    let dt = (now_ms - last_activity_ms).max(0);
    if dt <= RUNNING_WINDOW_MS {
        LiveState::Running
    } else if dt <= IDLE_WINDOW_MS {
        LiveState::Idle
    } else {
        LiveState::Recent
    }
}

/// 解析 HOME（约定同其他 adapter：`std::env::var("HOME")`）。
pub fn home_dir() -> Result<std::path::PathBuf, String> {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .map_err(|_| "no HOME".to_string())
}

/// 按字符截断（中文安全），超长加省略号；换行压成空格（摘要单行展示）。
#[must_use]
pub fn snippet(s: &str, max_chars: usize) -> String {
    let flattened: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let trimmed = flattened.trim();
    let mut out: String = trimmed.chars().take(max_chars).collect();
    if trimmed.chars().count() > max_chars {
        out.push('…');
    }
    out
}

/// Unix 毫秒 → 毫秒（`SystemTime` 辅助）。
#[must_use]
pub fn system_time_ms(t: std::time::SystemTime) -> Option<i64> {
    t.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(0))
}

/// ISO 8601（`2026-11-02T10:00:00.123Z`）→ Unix 毫秒（极简解析，失败返回 None）。
#[must_use]
pub fn iso_to_ms(s: &str) -> Option<i64> {
    let (date, rest) = s.split_once('T')?;
    let rest = rest.trim_end_matches('Z');
    let (time, _frac) = rest.split_once('.').unwrap_or((rest, ""));
    let mut dp = date.split('-');
    let y: i64 = dp.next()?.parse().ok()?;
    let mo: i64 = dp.next()?.parse().ok()?;
    let d: i64 = dp.next()?.parse().ok()?;
    let mut tp = time.split(':');
    let h: i64 = tp.next()?.parse().ok()?;
    let mi: i64 = tp.next()?.parse().ok()?;
    let se: i64 = tp.next().unwrap_or("0").parse().ok()?;
    // days_from_civil（Howard Hinnant）公历日期 → Unix 天数
    let y_adj = if mo <= 2 { y - 1 } else { y };
    let era = y_adj.div_euclid(400);
    let yoe = y_adj - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((((days * 24 + h) * 60 + mi) * 60 + se) * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_windows() {
        let now = 10_000_000_000i64; // 远大于任何窗口，避免负数被 max(0) 吞掉
        assert_eq!(classify(now, now), LiveState::Running);
        assert_eq!(
            classify(now, now - 45_000),
            LiveState::Running,
            "45s 边界含"
        );
        assert_eq!(classify(now, now - 45_001), LiveState::Idle, "刚过 45s");
        assert_eq!(classify(now, now - 10 * 60_000), LiveState::Idle);
        assert_eq!(classify(now, now - 10 * 60_000 - 1), LiveState::Recent);
        // 未来时间戳（时钟偏差）不 panic，按 Running 处理
        assert_eq!(classify(now, now + 100), LiveState::Running);
    }

    #[test]
    fn snippet_flattens_and_truncates() {
        assert_eq!(snippet("a\nb", 10), "a b");
        assert_eq!(snippet("你好世界", 3), "你好世…");
        assert_eq!(snippet("  x  ", 10), "x");
    }
}
