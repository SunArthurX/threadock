//! 设置域：通用 app_settings 键值读写（前端设置面板的持久化后端）。
//!
//! 现有键：`last_conv_sync_ms` / `last_ops_sync_ms` / `last_assets_sync_ms` /
//! `last_automations_sync_ms`（同步时间戳，内部维护）；
//! 新增用户设置键：`sync_interval_min`（自动同步间隔，分钟；0 = 关闭）。
//! 预算/定价/策略等域设置走各自命令（budget_* / ops_pricing_* / policy_*）。

use super::*;
use ch_daemon::DaemonState;

/// 读取一个应用设置（不存在返回 None）。
#[tauri::command]
pub(crate) async fn app_setting_get(
    state: tauri::State<'_, DaemonState>,
    key: String,
) -> Result<Option<String>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.get_setting(&key).map_err(|e| storage_err(e))
}

/// 写入一个应用设置（upsert）。
#[tauri::command]
pub(crate) async fn app_setting_set(
    state: tauri::State<'_, DaemonState>,
    key: String,
    value: String,
) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    repo.set_setting(&key, &value).map_err(|e| storage_err(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_setting_roundtrip() {
        let state = DaemonState::open_in_memory().expect("state open");
        {
            let repo = state.repo.lock().expect("mutex poisoned");
            assert_eq!(
                repo.get_setting("sync_interval_min")
                    .expect("SQL execution failed"),
                None
            );
            repo.set_setting("sync_interval_min", "10")
                .expect("SQL execution failed");
            assert_eq!(
                repo.get_setting("sync_interval_min")
                    .expect("SQL execution failed"),
                Some("10".to_string())
            );
            // upsert 覆盖
            repo.set_setting("sync_interval_min", "0")
                .expect("SQL execution failed");
            assert_eq!(
                repo.get_setting("sync_interval_min")
                    .expect("SQL execution failed"),
                Some("0".to_string())
            );
        }
    }
}
