//! 备份 GUI：创建加密备份 / 恢复到指定目录（不覆盖当前数据）。
//!
//! 复用 ch-backup（Argon2id + XChaCha20-Poly1305 + zstd）。

use super::*;
use ch_daemon::DaemonState;

/// 创建加密备份（前端 save 对话框给出目标路径，密码经参数传入——本地进程内使用）。
#[tauri::command]
pub(crate) async fn backup_create(
    state: tauri::State<'_, DaemonState>,
    path: String,
    password: String,
) -> Result<serde_json::Value, String> {
    if password.len() < 8 {
        return Err("密码至少 8 位".into());
    }
    let meta = run_blocking(|| {
        ch_backup::create_backup(
            &ch_backup::BackupSource {
                db_path: state.data_dir.join("threadock.db"),
                raw_root: Some(state.data_dir.join("raw")),
            },
            &password,
            std::path::Path::new(&path),
        )
        .map_err(|e| storage_err(e))
    })?;
    let _ = {
        let repo = state.repo.lock().map_err(|e| storage_err(e))?;
        repo.log_governance_action("backup_create", Some("file"), Some(&path), "ok", None)
    };
    Ok(serde_json::json!({
        "db_size": meta.db_size,
        "raw_count": meta.raw_count,
        "raw_bytes": meta.raw_bytes,
    }))
}

/// 从备份恢复到用户选择的目录（恢复的是副本，不动当前运行数据）。
#[tauri::command]
pub(crate) async fn backup_restore(
    state: tauri::State<'_, DaemonState>,
    path: String,
    password: String,
    target_dir: String,
) -> Result<serde_json::Value, String> {
    let _ = state; // 当前数据不受影响；命令需要 State 仅为权限一致性
    let meta = run_blocking(|| {
        ch_backup::restore_backup(
            std::path::Path::new(&path),
            &password,
            std::path::Path::new(&target_dir),
        )
        .map_err(|e| storage_err(e))
    })?;
    Ok(serde_json::json!({
        "db_size": meta.db_size,
        "raw_count": meta.raw_count,
    }))
}
