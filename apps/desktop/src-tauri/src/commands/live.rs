//! 实时任务监控域：`live_tasks`（全源扫描 + 深链回填）/ `live_transcript`（源侧最近活动）。
//!
//! 数据一律由 ch-live-monitor 直接读源侧存储（JSONL / 只读 SQLite），
//! 不经导入流水线，与主同步（BusyGuard）互不干扰。
//! 前端 3s 轮询 live_tasks；此处再做 1s 结果缓存，防多入口叠加请求打爆源库。

use super::*;
use ch_daemon::DaemonState;
use ch_live_monitor as live;

/// live_tasks 结果缓存（1s TTL）。
static CACHE: std::sync::Mutex<Option<(std::time::Instant, LiveTasksDto)>> =
    std::sync::Mutex::new(None);
const CACHE_TTL_MS: u128 = 1_000;

/// 单个实时任务（叠加已导入会话 ID 供前端深链）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTaskDto {
    #[serde(flatten)]
    pub task: live::LiveTask,
    /// 命中 threadock 已导入会话时的主键（「在 Threadock 打开」深链）。
    pub conversation_id: Option<String>,
}

/// live_tasks 返回。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTasksDto {
    pub tasks: Vec<LiveTaskDto>,
    /// probe 级错误（单源失败不影响其他源，前端可提示）。
    pub errors: Vec<live::SourceError>,
    pub scanned_at_ms: i64,
}

/// 扫描全部源的实时任务。`nowMs` 为扫描时刻（Unix 毫秒）。
#[tauri::command]
pub(crate) async fn live_tasks(
    state: tauri::State<'_, DaemonState>,
) -> Result<LiveTasksDto, String> {
    if let Some((at, dto)) = CACHE
        .lock()
        .map_err(|_| "live cache poisoned".to_string())
        .ok()
        .and_then(|guard| guard.as_ref().map(|(at, dto)| (*at, dto.clone())))
    {
        if at.elapsed().as_millis() < CACHE_TTL_MS {
            return Ok(dto);
        }
    }
    let scanned_at_ms = live::system_time_ms(std::time::SystemTime::now()).unwrap_or_default();
    let outcome = run_blocking(move || live::scan_all(scanned_at_ms));
    // 深链回填：源侧扫出的任务通常个位数，逐条查已导入会话
    let mut tasks: Vec<LiveTaskDto> = Vec::with_capacity(outcome.tasks.len());
    if !outcome.tasks.is_empty() {
        let repo = state.read_repo.lock().map_err(storage_err)?;
        for t in outcome.tasks {
            let conversation_id = repo
                .find_conversation_by_source(t.source.provider_str(), &t.session_id)
                .map_err(storage_err)?
                .map(|c| c.id);
            tasks.push(LiveTaskDto {
                task: t,
                conversation_id,
            });
        }
    }
    let dto = LiveTasksDto {
        tasks,
        errors: outcome.errors,
        scanned_at_ms,
    };
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some((std::time::Instant::now(), dto.clone()));
    }
    Ok(dto)
}

/// 单个实时任务的最近活动（直接读源，供抽屉实时滚动）。
/// `source`：`claude-code` / `codex` / `zcode` / `minimax-code`。
#[tauri::command]
pub(crate) async fn live_transcript(
    source: String,
    session_id: String,
    limit: Option<u32>,
) -> Result<Vec<live::LiveActivity>, String> {
    let limit = usize::try_from(limit.unwrap_or(60)).unwrap_or(60).min(200);
    let home = live::home_dir().map_err(io_err)?;
    let session_id = session_id.clone();
    run_blocking(move || {
        match source.as_str() {
            "claude-code" => {
                // session_id 即文件名：projects/<encoded>/<session_id>.jsonl
                let projects = home.join(".claude/projects");
                let mut found = None;
                if let Ok(dirs) = std::fs::read_dir(&projects) {
                    for d in dirs.filter_map(std::result::Result::ok) {
                        let candidate = d.path().join(format!("{session_id}.jsonl"));
                        if candidate.is_file() {
                            found = Some(candidate);
                            break;
                        }
                    }
                }
                Ok(found
                    .map(|p| live::probe::claude::transcript(&p, limit))
                    .unwrap_or_default())
            }
            "codex" => {
                // rollout-<ts>-<uuid>.jsonl 的 stem 以 uuid 结尾
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
                                .is_some_and(|s| s.ends_with(&session_id))
                        {
                            found = Some(p);
                            break;
                        }
                    }
                    if found.is_some() {
                        break;
                    }
                }
                Ok(found
                    .map(|p| live::probe::codex::transcript(&p, limit))
                    .unwrap_or_default())
            }
            "zcode" => Ok(live::probe::zcode::transcript(
                &home.join(".zcode/cli/db/db.sqlite"),
                &session_id,
                limit,
            )),
            "minimax-code" => Ok(live::probe::minimax::transcript(
                &home.join(".minimax/v2/sqlite/runtime-state.sqlite"),
                &session_id,
                limit,
            )),
            other => Err(import_err(format!("未知实时源: {other}"))),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_task_dto_flattens_task_fields() {
        let dto = LiveTaskDto {
            task: live::LiveTask {
                source: live::LiveSource::ZCode,
                session_id: "s1".into(),
                title: Some("t".into()),
                workspace: None,
                originator: None,
                started_at_ms: None,
                last_activity_ms: 1,
                state: live::LiveState::Running,
                activity: None,
                progress: None,
            },
            conversation_id: Some("conv-1".into()),
        };
        let json = serde_json::to_string(&dto).expect("serialize");
        assert!(json.contains("sessionId"), "flatten 应展开源侧字段: {json}");
        assert!(json.contains("conversationId"), "{json}");
        assert!(json.contains("\"source\":\"zcode\""), "{json}");
    }
}
