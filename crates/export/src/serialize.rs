//! JSON 序列化导出，对应 plan §6.6「原始 JSON/JSONL 导出」。
//!
//! 导出格式自描述、可重新导入（plan §6.6：数据可移植性验证）。

use crate::redact::{redact, RedactionStats};
use ch_domain::{Conversation, Event, Message, Workspace};
use serde::{Deserialize, Serialize};

/// 导出选项。
#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// 是否包含命令事件。
    pub include_commands: bool,
    /// 是否包含 Diff 事件。
    pub include_diffs: bool,
    /// 是否包含其它事件（tool call / approval / artifact）。
    pub include_events: bool,
    /// 是否启用脱敏。
    pub redact_secrets: bool,
}

impl ExportOptions {
    pub fn everything() -> Self {
        Self {
            include_commands: true,
            include_diffs: true,
            include_events: true,
            redact_secrets: true,
        }
    }
    pub fn messages_only() -> Self {
        Self::default()
    }
}

/// 导出数据结构（一个文件含 workspace + conversation + messages + events）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExportData {
    /// 导出器版本，便于未来导入兼容。
    pub format_version: u32,
    pub workspace: Option<Workspace>,
    pub conversation: Conversation,
    pub messages: Vec<Message>,
    pub events: Vec<Event>,
    /// 脱敏统计（若启用）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redaction: Option<RedactionStats>,
}

/// 组装导出数据：按选项过滤事件，并可选脱敏。
pub fn build_export_data(
    workspace: Option<&Workspace>,
    conversation: &Conversation,
    messages: &[Message],
    events: &[Event],
    options: &ExportOptions,
) -> ExportData {
    use ch_domain::EventType;
    // 过滤事件
    let filtered: Vec<Event> = events
        .iter()
        .filter(|e| match e.event_type {
            EventType::CommandStarted | EventType::CommandCompleted => options.include_commands,
            EventType::DiffGenerated => options.include_diffs,
            _ => options.include_events,
        })
        .cloned()
        .collect();

    let mut conv = conversation.clone();
    let mut msgs: Vec<Message> = messages.to_vec();
    let mut evs = filtered;

    let redaction = if options.redact_secrets {
        let mut total = RedactionStats::default();
        // 对每个文本字段脱敏
        if let Some(t) = conv.title.take() {
            let (r, s) = redact(&t);
            conv.title = Some(r);
            total = merge_stats(total, s);
        }
        for m in &mut msgs {
            if let Some(t) = m.content_text.take() {
                let (r, s) = redact(&t);
                m.content_text = Some(r);
                total = merge_stats(total, s);
            }
        }
        for e in &mut evs {
            if let Some(s) = e.summary.take() {
                let (r, st) = redact(&s);
                e.summary = Some(r);
                total = merge_stats(total, st);
            }
        }
        if total.total() > 0 {
            Some(total)
        } else {
            None
        }
    } else {
        None
    };

    ExportData {
        format_version: 1,
        workspace: workspace.cloned(),
        conversation: conv,
        messages: msgs,
        events: evs,
        redaction,
    }
}

/// 序列化为 JSON 字符串。
pub fn to_json(
    workspace: Option<&Workspace>,
    conversation: &Conversation,
    messages: &[Message],
    events: &[Event],
    options: &ExportOptions,
) -> Result<String, serde_json::Error> {
    let data = build_export_data(workspace, conversation, messages, events, options);
    serde_json::to_string_pretty(&data)
}

fn merge_stats(mut a: RedactionStats, b: RedactionStats) -> RedactionStats {
    a.aws_access_key += b.aws_access_key;
    a.aws_secret_key += b.aws_secret_key;
    a.github_token += b.github_token;
    a.bearer_token += b.bearer_token;
    a.api_key += b.api_key;
    a.email += b.email;
    a.private_ip += b.private_ip;
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_domain::{Conversation, Event, EventType, Message, Provider, Role};

    fn sample() -> (Conversation, Vec<Message>, Vec<Event>) {
        let mut c = Conversation::new(Provider::Codex, "src-1");
        c.title = Some("token=ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ1234567890 讨论".into());
        let m1 = Message {
            content_text: Some("邮箱 alice@corp.com 和密钥 AKIAIOSFODNN7EXAMPLE".into()),
            ..Message::new("c1", Role::User, 1)
        };
        let m2 = Message {
            content_text: Some("回复".into()),
            ..Message::new("c1", Role::Assistant, 2)
        };
        let e1 = Event {
            event_type: EventType::CommandStarted,
            summary: Some("cargo build".into()),
            ..Event::new("c1", EventType::CommandStarted, 1)
        };
        let e2 = Event {
            event_type: EventType::DiffGenerated,
            summary: Some("main.rs 改动".into()),
            ..Event::new("c1", EventType::DiffGenerated, 2)
        };
        (c, vec![m1, m2], vec![e1, e2])
    }

    #[test]
    fn export_messages_only_excludes_events() {
        let (c, msgs, evs) = sample();
        let data = build_export_data(None, &c, &msgs, &evs, &ExportOptions::messages_only());
        assert_eq!(data.messages.len(), 2);
        assert_eq!(data.events.len(), 0);
    }

    #[test]
    fn export_everything_includes_all_events() {
        let (c, msgs, evs) = sample();
        let data = build_export_data(None, &c, &msgs, &evs, &ExportOptions::everything());
        assert_eq!(data.events.len(), 2);
    }

    #[test]
    fn export_includes_commands_only() {
        let (c, msgs, evs) = sample();
        let opts = ExportOptions {
            include_commands: true,
            ..Default::default()
        };
        let data = build_export_data(None, &c, &msgs, &evs, &opts);
        assert_eq!(data.events.len(), 1);
        assert_eq!(data.events[0].event_type, EventType::CommandStarted);
    }

    #[test]
    fn export_redacts_secrets_when_enabled() {
        let (c, msgs, evs) = sample();
        let data = build_export_data(None, &c, &msgs, &evs, &ExportOptions::everything());
        // 标题里的 ghp_ 应被脱敏
        assert!(data
            .conversation
            .title
            .as_deref()
            .unwrap()
            .contains("[REDACTED:github_token]"));
        // 消息里的邮箱和 AWS key 应被脱敏
        let m0 = &data.messages[0];
        assert!(m0
            .content_text
            .as_deref()
            .unwrap()
            .contains("[REDACTED:email]"));
        assert!(m0
            .content_text
            .as_deref()
            .unwrap()
            .contains("[REDACTED:aws_access_key]"));
        // 应有脱敏统计
        let stats = data.redaction.expect("should have redaction stats");
        assert!(stats.github_token >= 1);
        assert!(stats.email >= 1);
        assert!(stats.aws_access_key >= 1);
    }

    #[test]
    fn export_keeps_secrets_when_disabled() {
        let (c, msgs, evs) = sample();
        let opts = ExportOptions {
            include_events: true,
            include_commands: true,
            include_diffs: true,
            redact_secrets: false,
        };
        let data = build_export_data(None, &c, &msgs, &evs, &opts);
        assert!(data.conversation.title.as_deref().unwrap().contains("ghp_"));
        assert!(data.redaction.is_none());
    }

    #[test]
    fn to_json_produces_valid_json() {
        let (c, msgs, evs) = sample();
        let json = to_json(None, &c, &msgs, &evs, &ExportOptions::everything()).unwrap();
        // 可重新反序列化（数据可移植性，plan §6.6）
        let back: ExportData = serde_json::from_str(&json).unwrap();
        assert_eq!(back.format_version, 1);
        assert_eq!(back.messages.len(), 2);
    }

    #[test]
    fn export_roundtrips_through_json() {
        let (c, msgs, evs) = sample();
        let json = to_json(None, &c, &msgs, &evs, &ExportOptions::everything()).unwrap();
        let back: ExportData = serde_json::from_str(&json).unwrap();
        assert_eq!(back.conversation.provider, Provider::Codex);
        assert_eq!(back.conversation.source_conversation_id, "src-1");
    }
}
