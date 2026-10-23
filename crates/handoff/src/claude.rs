//! Claude Code 写回器：`~/.claude/projects/<encoded-cwd>/<session-uuid>.jsonl`。
//!
//! 字段集对齐 2026-10 真实会话文件（parentUuid 链 + sessionId + cwd + timestamp）。
//! 内容只写 text（user 纯字符串 / assistant text 块），不携带 tool_use，
//! 保证 `claude --resume <sessionId>` 重放时 API 请求合法。

use crate::transcript::{build_transcript, TranscriptItem};
use crate::{
    effective_cwd, ts_ms, HandoffError, HandoffInput, HandoffOptions, HandoffOutcome, HandoffResult,
};
use ch_domain::{Role, Timestamp};
use std::path::PathBuf;
use time::macros::format_description;
use uuid::Uuid;

/// Claude Code 版本号字段（元信息，不影响解析）。
const CLI_VERSION: &str = "2.1.267";

/// 项目目录编码：非字母数字一律替换为 `-`（`/Users/x/06.code` → `-Users-x-06-code`）。
#[must_use]
pub fn encode_project_dir(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// ISO 时间戳（毫秒 + Z，与真实文件一致：`2026-09-30T14:28:56.985Z`）。
fn iso_millis(t: Timestamp) -> String {
    let fmt =
        format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");
    t.format(&fmt).unwrap_or_default()
}

/// 写入一条 Claude Code 会话 JSONL。
pub fn write(input: &HandoffInput, opts: &HandoffOptions) -> HandoffResult<HandoffOutcome> {
    let items = build_transcript(input, opts);
    if items.is_empty() {
        return Err(HandoffError::Empty);
    }

    let session_id = Uuid::new_v4().to_string();
    let cwd = effective_cwd(input, opts);
    let claude_root = crate::resolve_dest(crate::HandoffTarget::ClaudeCode, opts);
    let file_path: PathBuf = claude_root
        .join("projects")
        .join(encode_project_dir(&cwd))
        .join(format!("{session_id}.jsonl"));

    if opts.dry_run {
        return Ok(HandoffOutcome {
            target: "claude-code".into(),
            conversation_id: input.conversation.id.clone(),
            title: input.title().to_string(),
            session_id,
            destination: file_path.to_string_lossy().into_owned(),
            items_written: items.len(),
            dry_run: true,
        });
    }

    let mut lines: Vec<String> = Vec::with_capacity(items.len() + 1);
    // 标题行：threadock 与 Claude Code 均识别 aiTitle；不识别时无害。
    lines.push(
        serde_json::json!({
            "type": "ai-title",
            "aiTitle": input.title(),
            "sessionId": session_id,
        })
        .to_string(),
    );

    let mut prev_uuid: Option<String> = None;
    for item in &items {
        let uuid = Uuid::new_v4().to_string();
        let ts = iso_millis(item.at);
        let mut record = serde_json::json!({
            "parentUuid": prev_uuid,
            "isSidechain": false,
            "type": if item.role == Role::Assistant { "assistant" } else { "user" },
            "message": message_block(item),
            "uuid": uuid,
            "timestamp": ts,
            "cwd": cwd,
            "sessionId": session_id,
            "version": CLI_VERSION,
            "userType": "external",
        });
        if item.role == Role::Assistant {
            record["message"]["model"] = serde_json::json!(input
                .conversation
                .model
                .as_deref()
                .unwrap_or("claude-sonnet-4"));
        }
        lines.push(record.to_string());
        prev_uuid = Some(uuid);
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
        target: "claude-code".into(),
        conversation_id: input.conversation.id.clone(),
        title: input.title().to_string(),
        session_id,
        destination: file_path.to_string_lossy().into_owned(),
        items_written: items.len(),
        dry_run: false,
    })
}

/// 单条消息的 `message` 块：user 纯字符串、assistant text 块数组（对齐真实文件形态）。
fn message_block(item: &TranscriptItem) -> serde_json::Value {
    if item.role == Role::Assistant {
        serde_json::json!({
            "id": format!("msg_{}_{}", ts_ms(item.at), Uuid::new_v4().simple()),
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "text", "text": item.text }],
        })
    } else {
        serde_json::json!({
            "role": "user",
            "content": item.text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::make_input;
    use ch_domain::{Conversation, Message, Role as DomainRole};

    #[test]
    fn encode_dir_matches_claude_convention() {
        assert_eq!(encode_project_dir("/Users/x/06.code"), "-Users-x-06-code");
    }

    #[test]
    fn writes_resumable_jsonl_and_roundtrips() {
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let input = make_input();
        let opts = HandoffOptions {
            dest_override: Some(dir.path().to_path_buf()),
            ..HandoffOptions::new("/nonexistent-home")
        };
        let out = write(&input, &opts).expect("write failed");
        assert!(!out.dry_run);
        assert_eq!(
            std::path::Path::new(&out.destination)
                .extension()
                .and_then(|e| e.to_str()),
            Some("jsonl")
        );
        // 平台无关断言：位于 projects/<编码目录>/ 下（Windows 分隔符为 \）
        let path = std::path::Path::new(&out.destination);
        let encoded_dir = path.parent().and_then(|p| p.file_name());
        assert_eq!(
            encoded_dir.and_then(|d| d.to_str()),
            Some("-tmp-demo-proj-06-code")
        );
        assert_eq!(
            path.parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.file_name())
                .and_then(|d| d.to_str()),
            Some("projects")
        );

        let file = std::path::Path::new(&out.destination);
        assert!(file.exists());
        // 往返：threadock 自己的 claude-code adapter 能读回。
        let raw = ch_adapter_claude_code::parse_session(file).expect("roundtrip parse failed");
        assert_eq!(raw.provider, ch_domain::Provider::ClaudeCode);
        assert_eq!(raw.source_conversation_id, out.session_id);
        // 前言 + user + assistant
        assert!(
            raw.messages.len() >= 3,
            "preamble + dialog, got {}",
            raw.messages.len()
        );
        assert!(raw.messages.iter().any(|m| m
            .text
            .as_deref()
            .is_some_and(|t| t.contains("[threadock 交接存档]"))));
        assert!(raw
            .messages
            .iter()
            .any(|m| m.text.as_deref().is_some_and(|t| t.contains("GLM-5.3"))));
    }

    #[test]
    fn dry_run_does_not_touch_disk() {
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

    #[test]
    fn fresh_ids_per_invocation() {
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let input = make_input();
        let opts = HandoffOptions {
            dest_override: Some(dir.path().to_path_buf()),
            ..HandoffOptions::new("/nonexistent-home")
        };
        let out = write(&input, &opts).expect("write failed");
        let path = std::path::PathBuf::from(&out.destination);
        assert!(path.exists());
        // 两次交接必须生成不同会话（uuid v4），永不覆盖已有文件。
        let mut conv = Conversation::new(ch_domain::Provider::ZCode, "src-x");
        conv.title = Some("t".into());
        let mut m = Message::new("c", DomainRole::User, 1);
        m.content_text = Some("hi".into());
        let input2 = HandoffInput {
            conversation: &conv,
            workspace: None,
            messages: &[m],
            events: &[],
        };
        let out2 = write(&input2, &opts).expect("write failed");
        assert_ne!(out.session_id, out2.session_id, "两次交接必须生成不同会话");
    }
}
