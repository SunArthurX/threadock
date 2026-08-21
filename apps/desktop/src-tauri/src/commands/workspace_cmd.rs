//! v1.0.0 Workspace 治理 + 保存搜索 + 原始视图 + 来源应用命令（plan §4.3/§13.2/P2-3）。

use super::*;
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
    repo.rename_workspace(&id, &new_name)
        .map_err(|e| storage_err(e))
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

/// 会话 → 恢复命令（仅 claude-code / codex 有官方 resume）。
fn resume_cmd_for(conv: &ch_domain::Conversation) -> Option<String> {
    match conv.provider.as_str() {
        "claude-code" => Some(format!("claude --resume {}", conv.source_conversation_id)),
        "codex" => Some(format!("codex resume {}", conv.source_conversation_id)),
        _ => None,
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
    Ok(resume_cmd_for(&conv))
}

/// 直接在系统终端里执行「恢复原会话」命令（新开一个终端窗口）。
/// 命令在后端按会话来源构造，前端不传自由文本（避免任意命令执行面）。
#[tauri::command]
pub(crate) async fn resume_in_terminal(
    state: tauri::State<'_, DaemonState>,
    conversation_id: String,
) -> Result<Option<String>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let conv = repo
        .get_conversation(&conversation_id)
        .map_err(|e| storage_err(e))?
        .ok_or_else(|| AppError::not_found("会话不存在").to_string())?;
    let Some(cmd) = resume_cmd_for(&conv) else {
        return Ok(None); // 来源不支持 resume（与 resume_command 同口径）
    };
    open_terminal(&cmd).map(|()| Some(cmd))
}

/// 在系统终端新窗口执行命令（macOS Terminal / Windows cmd / Linux 常见终端）。
fn open_terminal(cmd: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "tell application \"Terminal\" to do script \"{}\"",
            escape_applescript(cmd)
        );
        let out = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .map_err(|e| format!("无法启动 osascript：{e}"))?;
        if !out.status.success() {
            return Err(format!(
                "Terminal 打开失败：{}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "Threadock", "cmd", "/K", cmd])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("无法打开终端：{e}"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let run = format!("{cmd}; exec bash");
        // 常见终端逐个尝试（不存在则 NotFound，换下一个）
        let candidates: Vec<(&str, Vec<String>)> = vec![
            (
                "gnome-terminal",
                vec!["--".into(), "bash".into(), "-c".into(), run.clone()],
            ),
            (
                "konsole",
                vec!["-e".into(), "bash".into(), "-c".into(), run.clone()],
            ),
            (
                "xterm",
                vec!["-e".into(), "bash".into(), "-c".into(), run.clone()],
            ),
        ];
        for (bin, args) in candidates {
            match std::process::Command::new(bin).args(&args).spawn() {
                Ok(_) => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(format!("打开 {bin} 失败：{e}")),
            }
        }
        Err("未找到可用终端（尝试过 gnome-terminal / konsole / xterm）".into())
    }
}

/// AppleScript 字符串转义：反斜杠与双引号（命令本身由后端构造，双保险）。
fn escape_applescript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applescript_escaping() {
        assert_eq!(
            escape_applescript("claude --resume abc"),
            "claude --resume abc"
        );
        assert_eq!(
            escape_applescript("echo \"hi\" && cd \\tmp"),
            "echo \\\"hi\\\" && cd \\\\tmp"
        );
    }

    #[test]
    fn resume_cmd_provider_matrix() {
        let conv = ch_domain::Conversation::new(ch_domain::Provider::ClaudeCode, "s-1");
        assert_eq!(
            resume_cmd_for(&conv).as_deref(),
            Some("claude --resume s-1")
        );
        let codex = ch_domain::Conversation::new(ch_domain::Provider::Codex, "s-2");
        assert_eq!(resume_cmd_for(&codex).as_deref(), Some("codex resume s-2"));
        let generic = ch_domain::Conversation::new(ch_domain::Provider::Generic, "s-3");
        assert!(resume_cmd_for(&generic).is_none());
    }
}
