//! 审计域：全库扫描、HTML 报告、策略规则与预算设置。

use super::*;
use ch_daemon::DaemonState;

/// 全库审计扫描：敏感信息 + 危险命令（plan codeagent-ops M4）。
/// catch_unwind 兜底：扫描内部任何 panic 转为错误返回，绝不带崩整个应用。
#[tauri::command]
#[tracing::instrument(skip_all, level = "info")]
pub(crate) async fn audit_scan(
    state: tauri::State<'_, DaemonState>,
) -> Result<ch_audit::AuditReport, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    // 全库正则扫描是秒级同步重活，移出 runtime worker 线程；
    // catch_unwind 兜底：扫描内部任何 panic 转为错误返回，绝不带崩整个应用。
    run_blocking(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ch_audit::run_audit(&repo)))
            .map_err(|_| "扫描内部错误，请查看日志".to_string())?
            .map_err(|e| storage_err(e))
    })
}

/// 渲染 HTML 审计报告（前端保存对话框落盘）。同样带 panic 兜底。
#[tauri::command]
pub(crate) async fn audit_export_html(
    state: tauri::State<'_, DaemonState>,
) -> Result<String, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let report = run_blocking(|| -> Result<ch_audit::AuditReport, String> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ch_audit::run_audit(&repo)))
            .map_err(|_| "扫描内部错误，请查看日志".to_string())?
            .map_err(|e| storage_err(e))
    })?;
    Ok(ch_audit::render_html(&report))
}

/// 策略规则 CRUD（M4/M5：命令黑名单 + 自定义敏感规则）。
#[tauri::command]
pub(crate) async fn policy_list(
    state: tauri::State<'_, DaemonState>,
) -> Result<Vec<ch_storage::PolicyRuleRecord>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.list_policy_rules().map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn policy_upsert(
    state: tauri::State<'_, DaemonState>,
    rule: ch_storage::PolicyRuleRecord,
) -> Result<(), String> {
    // 校验正则合法
    regex::Regex::new(&rule.pattern).map_err(|e| format!("正则无效: {e}"))?;
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    repo.upsert_policy_rule(&rule).map_err(|e| storage_err(e))?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn policy_delete(
    state: tauri::State<'_, DaemonState>,
    name: String,
) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    repo.delete_policy_rule(&name).map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn budget_get(
    state: tauri::State<'_, DaemonState>,
) -> Result<ch_storage::BudgetSettings, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.get_budget_settings().map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn budget_set(
    state: tauri::State<'_, DaemonState>,
    settings: ch_storage::BudgetSettings,
) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    repo.set_budget_settings(&settings)
        .map_err(|e| storage_err(e))
}
