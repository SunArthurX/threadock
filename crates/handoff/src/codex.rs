//! Codex 写回器：`~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`。
//!
//! 记录形态对齐真实 rollout（session_meta + response_item message，ordinal 递增）。
//! `codex resume <session_id>` 即可继续。

use crate::transcript::build_transcript;
use crate::{
    effective_cwd, HandoffError, HandoffInput, HandoffOptions, HandoffOutcome, HandoffResult,
};
use ch_domain::{Role, Timestamp};
use std::path::PathBuf;
use time::macros::format_description;
use uuid::Uuid;

/// 声明来源（真实文件此处是 codex_cli / codex_work_desktop）。
const ORIGINATOR: &str = "threadock";

/// ISO 时间戳（毫秒 + Z，对齐真实文件）。
fn iso_millis(t: Timestamp) -> String {
    let fmt =
        format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");
    t.format(&fmt).unwrap_or_default()
}

/// 文件名时间戳段：`2026-10-18T18-22-15`（冒号换连字符，对齐真实命名）。
fn filename_ts(t: Timestamp) -> String {
    let fmt = format_description!("[year]-[month]-[day]T[hour]-[minute]-[second]");
    t.format(&fmt).unwrap_or_default()
}

/// 写入一条 Codex rollout 会话。
pub fn write(input: &HandoffInput, opts: &HandoffOptions) -> HandoffResult<HandoffOutcome> {
    let items = build_transcript(input, opts);
    if items.is_empty() {
        return Err(HandoffError::Empty);
    }

    let session_id = Uuid::new_v4().to_string();
    let cwd = effective_cwd(input, opts);
    let started = input.started_at();
    let codex_root = crate::resolve_dest(crate::HandoffTarget::Codex, opts);
    // 目录按会话起始日分桶（YYYY/MM/DD，UTC）。
    let day = format_description!("[year]/[month]/[day]");
    let day_dir = started
        .format(&day)
        .unwrap_or_else(|_| "1970/01/01".to_string());
    let file_path: PathBuf = codex_root.join("sessions").join(&day_dir).join(format!(
        "rollout-{}-{session_id}.jsonl",
        filename_ts(started)
    ));

    if opts.dry_run {
        return Ok(HandoffOutcome {
            target: "codex".into(),
            conversation_id: input.conversation.id.clone(),
            title: input.title().to_string(),
            session_id,
            destination: file_path.to_string_lossy().into_owned(),
            items_written: items.len(),
            dry_run: true,
        });
    }

    let meta_ts = iso_millis(started);
    let mut lines: Vec<String> = Vec::with_capacity(items.len() + 1);
    lines.push(
        serde_json::json!({
            "timestamp": meta_ts,
            "ordinal": 0,
            "type": "session_meta",
            "payload": {
                "id": session_id,
                "timestamp": meta_ts,
                "cwd": cwd,
                "originator": ORIGINATOR,
                "cli_version": env!("CARGO_PKG_VERSION"),
                "source": ORIGINATOR,
            },
        })
        .to_string(),
    );

    for (idx, item) in items.iter().enumerate() {
        let text_type = if item.role == Role::Assistant {
            "output_text"
        } else {
            "input_text"
        };
        let role = if item.role == Role::Assistant {
            "assistant"
        } else {
            "user"
        };
        lines.push(
            serde_json::json!({
                "timestamp": iso_millis(item.at),
                "ordinal": idx + 1,
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": role,
                    "content": [{ "type": text_type, "text": item.text }],
                },
            })
            .to_string(),
        );
    }

    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if file_path.exists() {
        return Err(HandoffError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "refuse to overwrite existing session file: {}",
                file_path.display()
            ),
        )));
    }
    std::fs::write(&file_path, lines.join("\n") + "\n")?;

    Ok(HandoffOutcome {
        target: "codex".into(),
        conversation_id: input.conversation.id.clone(),
        title: input.title().to_string(),
        session_id,
        destination: file_path.to_string_lossy().into_owned(),
        items_written: items.len(),
        dry_run: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::make_input;

    #[test]
    fn writes_rollout_and_roundtrips() {
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let input = make_input();
        let opts = HandoffOptions {
            dest_override: Some(dir.path().to_path_buf()),
            ..HandoffOptions::new("/nonexistent-home")
        };
        let out = write(&input, &opts).expect("write failed");
        // 平台无关断言：sessions 日分桶目录下（Windows 分隔符为 \）
        let in_sessions = std::path::Path::new(&out.destination)
            .components()
            .any(|c| c.as_os_str() == "sessions");
        assert!(in_sessions);
        assert!(out.destination.contains("rollout-"));

        let raw =
            ch_adapter_codex::parse_session(&out.destination).expect("roundtrip parse failed");
        assert_eq!(raw.provider, ch_domain::Provider::Codex);
        assert_eq!(raw.source_conversation_id, out.session_id);
        assert!(raw.messages.len() >= 3, "preamble + dialog");
        assert!(raw.messages.iter().any(|m| m
            .text
            .as_deref()
            .is_some_and(|t| t.contains("[threadock 交接存档]"))));
        assert!(raw
            .messages
            .iter()
            .any(|m| m.text.as_deref().is_some_and(|t| t.contains("继续开发"))));
    }

    #[test]
    fn dry_run_computes_path_without_write() {
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let input = make_input();
        let opts = HandoffOptions {
            dest_override: Some(dir.path().to_path_buf()),
            dry_run: true,
            ..HandoffOptions::new("/nonexistent-home")
        };
        let out = write(&input, &opts).expect("dry-run failed");
        assert!(out.dry_run);
        assert!(!std::path::Path::new(&out.destination).exists());
    }
}
