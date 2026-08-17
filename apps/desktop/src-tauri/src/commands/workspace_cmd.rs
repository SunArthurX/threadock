//! v1.0.0 Workspace 治理 + 保存搜索 + 原始视图 + 来源应用命令（plan §4.3/§13.2/P2-3）。

use super::{io_err, storage_err};
use ch_daemon::DaemonState;

// ── 保存搜索（plan §13.2，V14）────────────────────────────────────────

#[tauri::command]
pub(crate) async fn saved_search_list(
    state: tauri::State<'_, DaemonState>,
) -> Result<Vec<ch_storage::SavedSearchRecord>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.list_saved_searches().map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn saved_search_upsert(
    state: tauri::State<'_, DaemonState>,
    name: String,
    query: String,
) -> Result<String, String> {
    if name.trim().is_empty() || query.trim().is_empty() {
        return Err("名称与查询不能为空".into());
    }
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    repo.upsert_saved_search(name.trim(), query.trim())
        .map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn saved_search_delete(
    state: tauri::State<'_, DaemonState>,
    id: String,
) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    repo.delete_saved_search(&id).map_err(|e| storage_err(e))
}

// ── Workspace 手动合并/拆分/重命名 + 置信度（plan §4.3 / P2-2）─────────

#[tauri::command]
pub(crate) async fn workspace_merge(
    state: tauri::State<'_, DaemonState>,
    source_id: String,
    target_id: String,
) -> Result<usize, String> {
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    repo.merge_workspaces(&source_id, &target_id)
        .map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn workspace_split(
    state: tauri::State<'_, DaemonState>,
    conversation_ids: Vec<String>,
    new_name: String,
) -> Result<String, String> {
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    repo.split_workspace(&conversation_ids, &new_name)
        .map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn workspace_rename(
    state: tauri::State<'_, DaemonState>,
    id: String,
    new_name: String,
) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    repo.rename_workspace(&id, &new_name).map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn workspace_source_links(
    state: tauri::State<'_, DaemonState>,
) -> Result<Vec<ch_storage::SourceWorkspaceLink>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.list_source_workspace_links()
        .map_err(|e| storage_err(e))
}

// ── 原始视图 ↔ 统一视图（plan P2-3）───────────────────────────────────

/// 读取会话的原始归档（Raw Store 里的未标准化数据）。
/// 无 raw_payload_id（旧数据/直读导入）返回 None。
#[tauri::command]
pub(crate) async fn conversation_raw(
    state: tauri::State<'_, DaemonState>,
    conversation_id: String,
) -> Result<Option<String>, String> {
    let raw_hash = {
        let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
        repo.get_conversation(&conversation_id)
            .map_err(|e| storage_err(e))?
            .and_then(|c| c.raw_payload_id)
    };
    let Some(hash) = raw_hash else {
        return Ok(None);
    };
    let raw_store = state.raw_store.lock().map_err(|e| io_err(e))?;
    let bytes = raw_store.get(&hash).map_err(|e| io_err(e))?;
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

// ── 一键打开来源应用 / 恢复命令（plan P2-3）────────────────────────────

/// 各 provider 对应的桌面应用名（GUI 来源可拉起；CLI 来源走恢复命令）。
fn app_name_for_provider(provider: &str) -> Option<&'static str> {
    match provider {
        "cursor" => Some("Cursor"),
        "zcode" => Some("ZCode"),
        "minimax-code" => Some("MiniMax Code"),
        // claude-code / codex 是 CLI 工具，没有可拉起的桌面应用
        _ => None,
    }
}

/// 打开来源应用（plan P2-3「一键打开来源应用」）。
/// 返回给用户的说明文字；CLI 来源返回提示改用恢复命令。
#[tauri::command]
pub(crate) async fn open_source_app(provider: String) -> Result<String, String> {
    let Some(app) = app_name_for_provider(&provider) else {
        return Ok(format!(
            "{provider} 是命令行工具，请使用「复制恢复命令」在终端继续会话"
        ));
    };
    #[cfg(target_os = "macos")]
    let spawned = std::process::Command::new("open")
        .args(["-a", app])
        .spawn()
        .map_err(|e| io_err(e));
    #[cfg(target_os = "windows")]
    let spawned = std::process::Command::new("cmd")
        .args(["/C", "start", "", app])
        .spawn()
        .map_err(|e| io_err(e));
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let spawned = std::process::Command::new("sh")
        .arg("-c")
        .arg(app)
        .spawn()
        .map_err(|e| io_err(e));
    match spawned {
        Ok(_) => Ok(format!("已尝试打开 {app}")),
        Err(e) => Err(e),
    }
}

/// 生成「恢复原会话」命令（plan P2-3：来源支持时恢复原会话）。
/// 仅 CLI 来源（claude-code / codex）有官方 resume；其余返回 None。
#[tauri::command]
pub(crate) async fn resume_command(
    state: tauri::State<'_, DaemonState>,
    conversation_id: String,
) -> Result<Option<String>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let Some(conv) = repo
        .get_conversation(&conversation_id)
        .map_err(|e| storage_err(e))?
    else {
        return Ok(None);
    };
    let provider = conv.provider.as_str();
    let cmd = match provider {
        "claude-code" => format!("claude --resume {}", conv.source_conversation_id),
        "codex" => format!("codex resume {}", conv.source_conversation_id),
        _ => return Ok(None),
    };
    Ok(Some(cmd))
}
