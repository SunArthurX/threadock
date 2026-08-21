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

/// 读取文本文件（导入配置 / 读取报告用）。
/// 限制最大 1 MB 防止误读大文件把内存吃满。
#[tauri::command]
pub(crate) fn read_text_file(path: String) -> Result<String, String> {
    const MAX_BYTES: u64 = 1_048_576; // 1 MB
    let meta = std::fs::metadata(&path).map_err(|e| io_err(e))?;
    if meta.len() > MAX_BYTES {
        return Err(format!("文件过大 ({} 字节 > 1 MB 上限)", meta.len()));
    }
    std::fs::read_to_string(&path).map_err(|e| io_err(e))
}

/// 内联图片数据（base64 data URL，前端直接喂给 `<img src>`）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImageData {
    pub mime: String,
    pub data_url: String,
}

/// 读取本机图片为 data URL（消息内联图片展示：消息引用的截图等仍在本机时直接展示）。
///
/// - 扩展名白名单 → MIME；上限 20 MB；
/// - 文件不存在返回 `Ok(None)`（前端显示「已不在原位置」占位，不算错误）。
#[tauri::command]
pub(crate) fn read_image_file(path: String) -> Result<Option<ImageData>, String> {
    const MAX_BYTES: u64 = 20 * 1024 * 1024;
    let ext = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        _ => {
            return Err(format!(
                "不支持的图片格式：.{ext}（仅 png/jpg/jpeg/gif/webp/bmp/svg/ico）"
            ))
        }
    };
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_err(e)),
    };
    if meta.len() > MAX_BYTES {
        return Err(format!(
            "图片过大（{} MB > 20 MB 上限）",
            meta.len() / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| io_err(e))?;
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(Some(ImageData {
        mime: mime.to_string(),
        data_url: format!("data:{mime};base64,{b64}"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小合法 PNG（1×1 透明像素）。
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn read_image_file_roundtrip() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let p = dir.path().join("shot.png");
        std::fs::write(&p, TINY_PNG).expect("write png");
        let r = read_image_file(p.to_string_lossy().into_owned())
            .expect("read image")
            .expect("should exist");
        assert_eq!(r.mime, "image/png");
        assert!(r.data_url.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn read_image_file_missing_is_none() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let missing = dir.path().join("gone.png");
        let r = read_image_file(missing.to_string_lossy().into_owned())
            .expect("missing file is not an error");
        assert!(r.is_none(), "不存在 → None（前端占位），不报错");
    }

    #[test]
    fn read_image_file_rejects_non_image() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let p = dir.path().join("notes.txt");
        std::fs::write(&p, "hi").expect("write txt");
        let err =
            read_image_file(p.to_string_lossy().into_owned()).expect_err("txt must be rejected");
        assert!(err.contains("不支持"), "{err}");
    }

    #[test]
    fn read_image_file_rejects_oversize() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let p = dir.path().join("huge.png");
        // 写 20MB + 1 字节（稀疏写：set_len 直接扩展文件长度，不实际占内存）
        let f = std::fs::File::create(&p).expect("create");
        f.set_len(20 * 1024 * 1024 + 1).expect("set_len");
        drop(f);
        let err = read_image_file(p.to_string_lossy().into_owned())
            .expect_err("oversize must be rejected");
        assert!(err.contains("过大"), "{err}");
    }
}
