//! 交接域：把归档会话写回目标 Agent 的原生会话存储，供其 resume 后继续开发。
//!
//! 这是桌面端唯一会写第三方目录的命令域（其余 adapter 一律只读）：
//! 纯增量写入全新会话 ID，不触碰目标库已有数据。重活经 `run_blocking` 移出 runtime。

use super::*;
use ch_daemon::DaemonState;
use ch_handoff::{loader, HandoffFailure, HandoffOptions, HandoffOutcome, HandoffTarget};

/// 目标可用性（前端目标选择器据此灰掉未安装的 Agent）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct HandoffTargetInfo {
    pub target: String,
    pub available: bool,
    /// 默认落点（目录或 sqlite 文件）。
    pub destination: String,
}

/// 单目标批量结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct HandoffTargetResult {
    pub target: String,
    pub ok: Vec<HandoffOutcome>,
    pub failed: Vec<HandoffFailure>,
}

/// 一次交接的汇总。
#[derive(Debug, Clone, serde::Serialize)]
pub struct HandoffSummary {
    pub matched: usize,
    pub missing: Vec<String>,
    pub dry_run: bool,
    pub results: Vec<HandoffTargetResult>,
}

fn home_dir() -> std::path::PathBuf {
    ch_handoff::user_home()
}

/// 列出五个目标及其本机可用性。
#[tauri::command]
pub(crate) fn handoff_targets() -> Vec<HandoffTargetInfo> {
    let home = home_dir();
    HandoffTarget::all()
        .into_iter()
        .map(|t| HandoffTargetInfo {
            target: t.as_str().to_string(),
            available: ch_handoff::store_exists(t, &home),
            destination: ch_handoff::default_destination(t, &home)
                .to_string_lossy()
                .into_owned(),
        })
        .collect()
}

/// 交接会话：`conversation_ids` 点名，或 `all=true` 交接全部（排除软删除与子任务）。
/// `target` 为单个目标或 `all`。
#[tauri::command]
pub(crate) async fn handoff_conversations(
    state: tauri::State<'_, DaemonState>,
    conversation_ids: Vec<String>,
    target: String,
    all: Option<bool>,
    dry_run: Option<bool>,
    include_context: Option<bool>,
    cwd: Option<String>,
) -> Result<HandoffSummary, String> {
    let targets = if target == "all" {
        HandoffTarget::all()
    } else {
        vec![target.parse::<HandoffTarget>().map_err(|e| import_err(e))?]
    };
    let dry_run = dry_run.unwrap_or(false);
    let include_context = include_context.unwrap_or(true);
    let want_all = all.unwrap_or(false);
    if conversation_ids.is_empty() && !want_all {
        return Err(import_err("未选择任何会话"));
    }

    run_blocking(move || {
        let (owned, missing, redact_rules) = {
            let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
            let (inputs, missing) = if want_all {
                (
                    loader::load_filtered(&repo, &ch_storage::ConversationFilter::new())
                        .map_err(|e| storage_err(e))?,
                    Vec::new(),
                )
            } else {
                loader::load_by_ids(&repo, &conversation_ids).map_err(|e| storage_err(e))?
            };
            (inputs, missing, loader::load_redaction_rules(&repo))
        };

        let mut opts = HandoffOptions::new(home_dir());
        if let Some(c) = cwd.filter(|c| !c.trim().is_empty()) {
            opts.fallback_cwd = std::path::PathBuf::from(c);
        }
        opts.dry_run = dry_run;
        opts.include_context = include_context;
        // 与导出同策略：用户配置了自定义脱敏规则时，交接内容先脱敏再写入目标库。
        opts.redact_rules = redact_rules;

        let inputs = loader::as_inputs(&owned);
        let results = targets
            .into_iter()
            .map(|t| {
                let batch = ch_handoff::handoff_batch(&inputs, t, &opts);
                HandoffTargetResult {
                    target: t.as_str().to_string(),
                    ok: batch.ok,
                    failed: batch.failed,
                }
            })
            .collect();
        Ok(HandoffSummary {
            matched: owned.len(),
            missing,
            dry_run,
            results,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_cover_all_five_with_destinations() {
        let infos = handoff_targets();
        assert_eq!(infos.len(), 5);
        let ids: Vec<&str> = infos.iter().map(|i| i.target.as_str()).collect();
        assert_eq!(
            ids,
            vec!["claude-code", "codex", "zcode", "minimax-code", "cursor"]
        );
        assert!(infos.iter().all(|i| !i.destination.is_empty()));
    }
}
