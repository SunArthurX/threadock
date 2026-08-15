//! Markdown 导出，对应 plan §6.6「单条 Conversation 导出 Markdown」。
//!
//! 格式与 adapter-markdown 的导入约定对称（便于 round-trip）：
//!
//! ```markdown
//! # <标题>
//!
//! ## User
//! 用户消息……
//!
//! ## Assistant
//! 助手回复……
//!
//! ## Command
//! cargo build
//! ```

use crate::redact::redact;
use crate::serialize::ExportOptions;
use ch_domain::{Conversation, Event, EventType, Message, Role};

/// 把会话导出为 Markdown 字符串。
pub fn to_markdown(
    conversation: &Conversation,
    messages: &[Message],
    events: &[Event],
    options: &ExportOptions,
) -> String {
    let mut out = String::new();

    // 标题
    let title = conversation.effective_title();
    let title = if options.redact_secrets {
        redact(title).0
    } else {
        title.to_string()
    };
    out.push_str(&format!("# {title}\n\n"));

    // 元信息
    out.push_str("<!-- meta\n");
    out.push_str(&format!("provider: {}\n", conversation.provider));
    out.push_str(&format!(
        "source_conversation_id: {}\n",
        conversation.source_conversation_id
    ));
    if let Some(m) = &conversation.model {
        out.push_str(&format!("model: {m}\n"));
    }
    if let Some(score) = conversation.completeness_score {
        out.push_str(&format!("completeness: {score:.2}\n"));
    }
    out.push_str("-->\n\n");

    // 消息（按 sequence_number 顺序）
    let mut msgs = messages.to_vec();
    msgs.sort_by_key(|m| m.sequence_number);
    for m in msgs {
        let role_label = match m.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::System => "System",
            Role::Tool => "Tool",
        };
        out.push_str(&format!("## {role_label}\n"));
        let body = m.content_text.clone().unwrap_or_default();
        let body = if options.redact_secrets {
            redact(&body).0
        } else {
            body
        };
        out.push_str(&body);
        out.push_str("\n\n");
    }

    // 事件（按选项过滤）
    let mut evs: Vec<&Event> = events.iter().collect();
    evs.sort_by_key(|e| e.sequence_number);
    for e in evs {
        let (label, include) = match e.event_type {
            EventType::CommandStarted | EventType::CommandCompleted => {
                ("Command", options.include_commands)
            }
            EventType::DiffGenerated => ("Diff", options.include_diffs),
            EventType::ToolCallStarted | EventType::ToolCallCompleted => {
                ("Tool", options.include_events)
            }
            EventType::ApprovalRequested
            | EventType::ApprovalGranted
            | EventType::ApprovalDenied => ("Approval", options.include_events),
            EventType::ArtifactCreated => ("Artifact", options.include_events),
            _ => continue,
        };
        if !include {
            continue;
        }
        out.push_str(&format!("## {label}\n"));
        let summary = e.summary.clone().unwrap_or_default();
        let summary = if options.redact_secrets {
            redact(&summary).0
        } else {
            summary
        };
        out.push_str(&summary);
        out.push_str("\n\n");
    }

    out.trim_end().to_string() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_domain::{Conversation, Event, EventType, Message, Provider, Role};

    fn sample() -> (Conversation, Vec<Message>, Vec<Event>) {
        let mut c = Conversation::new(Provider::Codex, "src-md-1");
        c.title = Some("测试导出".into());
        c.model = Some("gpt-test".into());
        let m1 = Message {
            content_text: Some("你好".into()),
            ..Message::new("c", Role::User, 1)
        };
        let m2 = Message {
            content_text: Some("你好啊".into()),
            ..Message::new("c", Role::Assistant, 2)
        };
        let e1 = Event {
            event_type: EventType::CommandStarted,
            summary: Some("cargo build".into()),
            ..Event::new("c", EventType::CommandStarted, 1)
        };
        let e2 = Event {
            event_type: EventType::DiffGenerated,
            summary: Some("foo.rs".into()),
            ..Event::new("c", EventType::DiffGenerated, 2)
        };
        (c, vec![m1, m2], vec![e1, e2])
    }

    #[test]
    fn markdown_has_title_and_messages() {
        let (c, msgs, evs) = sample();
        let md = to_markdown(&c, &msgs, &evs, &ExportOptions::everything());
        assert!(md.contains("# 测试导出"));
        assert!(md.contains("## User"));
        assert!(md.contains("## Assistant"));
        assert!(md.contains("你好"));
    }

    #[test]
    fn markdown_has_meta_block() {
        let (c, msgs, evs) = sample();
        let md = to_markdown(&c, &msgs, &evs, &ExportOptions::everything());
        assert!(md.contains("<!-- meta"));
        assert!(md.contains("provider: codex"));
        assert!(md.contains("model: gpt-test"));
    }

    #[test]
    fn markdown_includes_commands_and_diffs_when_enabled() {
        let (c, msgs, evs) = sample();
        let md = to_markdown(&c, &msgs, &evs, &ExportOptions::everything());
        assert!(md.contains("## Command"));
        assert!(md.contains("## Diff"));
        assert!(md.contains("cargo build"));
    }

    #[test]
    fn markdown_excludes_commands_when_disabled() {
        let (c, msgs, evs) = sample();
        let opts = ExportOptions {
            include_diffs: true,
            include_events: true,
            ..Default::default()
        };
        let md = to_markdown(&c, &msgs, &evs, &opts);
        assert!(!md.contains("## Command"));
        assert!(md.contains("## Diff"));
    }

    #[test]
    fn markdown_redacts_secrets() {
        let mut c = Conversation::new(Provider::Codex, "s");
        c.title = Some("含 ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ1234567890 的标题".into());
        let m = Message {
            content_text: Some("联系 alice@corp.com".into()),
            ..Message::new("c", Role::User, 1)
        };
        let md = to_markdown(&c, &[m], &[], &ExportOptions::everything());
        assert!(md.contains("[REDACTED:github_token]"));
        assert!(md.contains("[REDACTED:email]"));
        assert!(!md.contains("ghp_aBcDeF"));
    }

    #[test]
    fn markdown_roundtrips_with_import() {
        // 导出后再用 adapter-markdown 解析，标题和消息应能还原
        use ch_adapter_markdown::parse_str;
        let (c, msgs, evs) = sample();
        let md = to_markdown(&c, &msgs, &evs, &ExportOptions::everything());
        let raw = parse_str(&md, "exported.md").unwrap();
        assert_eq!(raw.title.as_deref(), Some("测试导出"));
        assert_eq!(raw.messages.len(), 2);
        assert!(raw.messages[0].text.as_deref().unwrap().contains("你好"));
    }
}
