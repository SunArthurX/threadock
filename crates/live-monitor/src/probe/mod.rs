//! 实时探测主入口：`scan_all` 聚合四源，probe 级失败互不影响。

pub mod claude;
pub mod codex;
pub mod minimax;
pub mod zcode;

use crate::types::{LiveSource, LiveState, LiveTask, SourceError};

/// 扫描结果：任务按状态（Running 最前）+ 活跃时间排序。
#[derive(Debug, Default)]
pub struct ScanOutcome {
    pub tasks: Vec<LiveTask>,
    pub errors: Vec<SourceError>,
}

/// 扫描全部源。`now_ms` 为 Unix 毫秒（调用方注入时钟，便于测试）。
#[must_use]
pub fn scan_all(now_ms: i64) -> ScanOutcome {
    let mut out = ScanOutcome::default();
    for (source, result) in [
        (LiveSource::ClaudeCode, claude::scan(now_ms)),
        (LiveSource::Codex, codex::scan(now_ms)),
        (LiveSource::ZCode, zcode::scan(now_ms)),
        (LiveSource::MinimaxCode, minimax::scan(now_ms)),
    ] {
        match result {
            Ok(mut tasks) => out.tasks.append(&mut tasks),
            Err(message) => out.errors.push(SourceError { source, message }),
        }
    }
    out.tasks.sort_by(|a, b| {
        state_rank(a.state)
            .cmp(&state_rank(b.state))
            .then(b.last_activity_ms.cmp(&a.last_activity_ms))
    });
    out
}

fn state_rank(state: LiveState) -> u8 {
    match state {
        LiveState::Running => 0,
        LiveState::Waiting => 1,
        LiveState::Idle => 2,
        LiveState::Recent => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_all_sorts_running_first() {
        let mut out = ScanOutcome::default();
        let base = crate::types::LiveTask {
            source: LiveSource::ZCode,
            session_id: String::new(),
            title: None,
            workspace: None,
            originator: None,
            started_at_ms: None,
            last_activity_ms: 0,
            state: LiveState::Recent,
            activity: None,
            progress: None,
        };
        let mut old = base.clone();
        old.last_activity_ms = 100;
        let mut fresh = base;
        fresh.state = LiveState::Running;
        fresh.last_activity_ms = 200;
        out.tasks = vec![old, fresh];
        out.tasks.sort_by(|a, b| {
            state_rank(a.state)
                .cmp(&state_rank(b.state))
                .then(b.last_activity_ms.cmp(&a.last_activity_ms))
        });
        assert_eq!(out.tasks[0].state, LiveState::Running);
    }

    /// 真机冒烟：`cargo test -p ch-live-monitor -- --ignored --nocapture`。
    /// CI 无各 agent 数据，默认跳过。
    #[test]
    #[ignore = "需要本机真实 agent 数据"]
    fn scan_real_machine() {
        let now = crate::system_time_ms(std::time::SystemTime::now()).expect("clock");
        let out = scan_all(now);
        println!("── tasks: {} ──", out.tasks.len());
        for t in &out.tasks {
            println!(
                "[{:>12}] {:8?} {} @ {:?} · {:?}",
                t.source.label(),
                t.state,
                t.title.as_deref().unwrap_or("-"),
                t.workspace.as_deref().unwrap_or("-"),
                t.activity
                    .as_ref()
                    .map(|a| format!("{}: {}", a.kind, a.snippet)),
            );
        }
        for e in &out.errors {
            println!("[ERR {:>12}] {}", e.source.label(), e.message);
        }
        assert!(out.errors.is_empty(), "真机扫描不应有 probe 级错误");
    }
}
