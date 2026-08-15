//! 导出域：会话导出（含自定义脱敏规则）与文件落盘。

use super::*;
use ch_daemon::DaemonState;

#[tauri::command]
pub(crate) async fn export_conversation(
    state: tauri::State<'_, DaemonState>,
    conversation_id: String,
    format: String,
) -> Result<ExportOutput, String> {
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    let conv = repo
        .get_conversation(&conversation_id)
        .map_err(|e| storage_err(e))?
        .ok_or_else(|| format!("conversation not found: {conversation_id}"))?;
    let messages = repo
        .list_messages(&conversation_id)
        .map_err(|e| storage_err(e))?;
    let events = repo
        .list_events(&conversation_id)
        .map_err(|e| storage_err(e))?;
    let opts = ch_export::ExportOptions::everything();
    // 用户自定义脱敏规则与 CLI 导出保持一致生效（plan §14.6）
    let custom_rules: Vec<ch_export::CustomRule> = repo
        .list_redaction_rules()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.enabled)
        .map(|r| ch_export::CustomRule::new(r.name, r.pattern))
        .collect();
    let safe_title: String = conv
        .effective_title()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(40)
        .collect();
    let (content, ext) = match format.as_str() {
        "json" => (
            ch_export::to_json(None, &conv, &messages, &events, &opts)
                .map_err(|e| storage_err(e))?,
            "json",
        ),
        _ => (
            ch_export::to_markdown(&conv, &messages, &events, &opts),
            "md",
        ),
    };
    let content = if custom_rules.is_empty() {
        content
    } else {
        ch_export::redact_with(&content, &custom_rules).0
    };
    let filename = format!(
        "{}.{}",
        if safe_title.is_empty() {
            "conversation".into()
        } else {
            safe_title
        },
        ext
    );
    Ok(ExportOutput {
        content,
        format: ext.to_string(),
        filename,
    })
}

/// 写文本文件到指定路径（前端 save 对话框返回的路径）。
/// 避免引入 tauri-plugin-fs，用最简方式把导出内容落盘。
#[tauri::command]
pub(crate) fn save_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content.as_bytes()).map_err(|e| io_err(e))
}
