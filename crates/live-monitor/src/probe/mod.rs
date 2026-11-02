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

/// 按源分发 transcript（commands/live.rs 的 live_transcript 同款路径；测试辅助）。
#[must_use]
pub fn transcript_for(task: &LiveTask, limit: usize) -> Vec<crate::types::LiveActivity> {
    let Ok(home) = crate::home_dir() else {
        return vec![];
    };
    match task.source {
        LiveSource::ClaudeCode => {
            let projects = home.join(".claude/projects");
            let mut found = None;
            if let Ok(dirs) = std::fs::read_dir(&projects) {
                for d in dirs.filter_map(std::result::Result::ok) {
                    let c = d.path().join(format!("{}.jsonl", task.session_id));
                    if c.is_file() {
                        found = Some(c);
                        break;
                    }
                }
            }
            found
                .map(|p| claude::transcript(&p, limit))
                .unwrap_or_default()
        }
        LiveSource::Codex => {
            let sessions = home.join(".codex/sessions");
            let mut found = None;
            let mut stack = vec![sessions];
            while let Some(dir) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for e in entries.filter_map(std::result::Result::ok) {
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else if p.extension().and_then(|e| e.to_str()) == Some("jsonl")
                        && p.file_stem()
                            .and_then(|s| s.to_str())
                            .is_some_and(|s| s.ends_with(&task.session_id))
                    {
                        found = Some(p);
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            found
                .map(|p| codex::transcript(&p, limit))
                .unwrap_or_default()
        }
        LiveSource::ZCode => zcode::transcript(
            &home.join(".zcode/cli/db/db.sqlite"),
            &task.session_id,
            limit,
        ),
        LiveSource::MinimaxCode => minimax::transcript(
            &home.join(".minimax/v2/sqlite/runtime-state.sqlite"),
            &task.session_id,
            limit,
        ),
    }
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

        // transcript 冒烟：对每个 live task 取最近 5 条活动（live_transcript 命令的同一代码路径）
        for t in &out.tasks {
            let items = transcript_for(t, 5);
            println!(
                "── transcript [{:?}] {} → {} 条",
                t.source,
                t.session_id,
                items.len()
            );
            for a in &items {
                println!(
                    "   {:>8} | {}",
                    a.kind,
                    a.snippet.chars().take(60).collect::<String>()
                );
            }
        }
    }
}
