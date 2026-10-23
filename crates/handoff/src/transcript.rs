//! 把归档会话渲染成「可继续开发」的对话脚本。
//!
//! 各目标 Agent 的原生会话格式互不相通，工具调用无法跨工具原生重放；
//! handoff 的策略是把存档事件折叠为一份上下文前言（工作目录、执行时间线），
//! 再拼接原始对话消息。目标 Agent resume 后即可基于完整上下文继续开发。

use ch_domain::{Conversation, Event, EventType, Message, Role, Timestamp, Workspace};
use std::time::Duration;

use crate::HandoffOptions;

/// 时间线最多收录的条目数（防超长会话把前言撑爆）。
const MAX_TIMELINE_ENTRIES: usize = 300;

/// 单条时间线摘要上限。
const MAX_SUMMARY_CHARS: usize = 200;

/// 一条待写入目标会话的记录（角色 + 文本 + 时间）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TranscriptItem {
    pub role: Role,
    pub text: String,
    pub at: Timestamp,
}

/// handoff 的输入：一条归档会话及其消息、事件、（可选）工作区。
#[derive(Debug, Clone)]
pub struct HandoffInput<'a> {
    pub conversation: &'a Conversation,
    pub workspace: Option<&'a Workspace>,
    pub messages: &'a [Message],
    pub events: &'a [Event],
}

impl HandoffInput<'_> {
    /// 会话标题（与 GUI 展示口径一致）。
    #[must_use]
    pub fn title(&self) -> &str {
        self.conversation.effective_title()
    }

    /// 工作目录：优先工作区 canonical_path，其次 cwd 兜底由调用方注入。
    #[must_use]
    pub fn workspace_path(&self) -> Option<&str> {
        self.workspace
            .and_then(|w| w.canonical_path.as_deref())
            .filter(|p| !p.is_empty())
    }

    /// 会话起始时间（缺省用当前时间）。
    #[must_use]
    pub fn started_at(&self) -> Timestamp {
        self.conversation
            .started_at
            .or_else(|| self.messages.first().and_then(|m| m.created_at))
            .unwrap_or_else(ch_domain::now_utc)
    }
}

/// 渲染对话脚本：上下文前言 + 原始消息（user/assistant）。
///
/// System/Tool 角色是运行时注入或工具回执，不属于对话主线，跳过。
/// 用户配置了自定义脱敏规则时逐条应用（与导出同策略：无自定义规则
/// 则完全不脱敏，保证保真；有则内置 + 自定义一并生效）。
pub(crate) fn build_transcript(input: &HandoffInput, opts: &HandoffOptions) -> Vec<TranscriptItem> {
    let now = ch_domain::now_utc();
    let started = input.started_at();
    let redact = |text: String| -> String {
        if opts.redact_rules.is_empty() {
            text
        } else {
            ch_export::redact_with(&text, &opts.redact_rules).0
        }
    };
    let mut items = Vec::new();

    if opts.include_context {
        items.push(TranscriptItem {
            role: Role::User,
            text: redact(context_preamble(input, now)),
            at: started,
        });
    }

    for m in input.messages {
        if !matches!(m.role, Role::User | Role::Assistant) {
            continue;
        }
        let Some(text) = m.content_text.as_deref() else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        items.push(TranscriptItem {
            role: m.role,
            text: redact(text.to_string()),
            at: m.created_at.unwrap_or(now),
        });
    }

    // 时间戳单调化：目标工具按时间排序时避免同毫秒乱序。
    let mut last = Timestamp::UNIX_EPOCH;
    for item in &mut items {
        if item.at < last {
            item.at = last + Duration::from_millis(1);
        }
        last = item.at;
    }
    items
}

/// 上下文前言：来源元信息 + 执行时间线。
fn context_preamble(input: &HandoffInput, now: Timestamp) -> String {
    let conv = input.conversation;
    let mut lines = Vec::new();
    lines.push("[threadock 交接存档]".to_string());
    lines.push(format!(
        "- 来源：{} 会话「{}」（{} 条消息）",
        conv.provider,
        input.title(),
        input.messages.len()
    ));
    if let Some(ws) = input.workspace_path() {
        lines.push(format!("- 工作目录：{ws}"));
    }
    if let Some(model) = &conv.model {
        lines.push(format!("- 模型：{model}"));
    }
    lines.push(format!(
        "- 会话时间：{} ~ {}",
        format_dt(
            conv.started_at
                .or_else(|| input.messages.first().and_then(|m| m.created_at))
                .unwrap_or(now)
        ),
        format_dt(conv.updated_at.unwrap_or(now))
    ));
    lines.push(String::new());
    lines.push(
        "以下先是该会话的工具执行时间线（命令、文件改动等），随后是完整对话记录。\
请把这一切当作已发生的历史上下文，在此基础上继续开发。"
            .to_string(),
    );

    if !input.events.is_empty() {
        lines.push(String::new());
        lines.push("## 执行时间线".to_string());
        let shown = input.events.len().min(MAX_TIMELINE_ENTRIES);
        for ev in &input.events[..shown] {
            lines.push(format!("- {}", timeline_entry(ev)));
        }
        if input.events.len() > shown {
            lines.push(format!(
                "……其余 {} 条事件见 threadock 存档。",
                input.events.len() - shown
            ));
        }
    }

    lines.push(String::new());
    lines.push("## 对话记录".to_string());
    lines.join("\n")
}

/// 单条事件的 时间线条目（`[命令] cargo build`）。
fn timeline_entry(ev: &Event) -> String {
    let label = match ev.event_type {
        EventType::CommandStarted | EventType::CommandCompleted => "命令",
        EventType::FileRead => "读文件",
        EventType::FileCreated => "建文件",
        EventType::FileUpdated => "改文件",
        EventType::FileDeleted => "删文件",
        EventType::DiffGenerated => "补丁",
        EventType::ToolCallStarted | EventType::ToolCallCompleted | EventType::McpCall => "工具",
        EventType::ApprovalRequested | EventType::ApprovalGranted | EventType::ApprovalDenied => {
            "审批"
        }
        EventType::SubagentStarted | EventType::SubagentCompleted => "子代理",
        EventType::BrowserAction => "浏览器",
        EventType::Error => "错误",
        EventType::ArtifactCreated | EventType::PlanCreated | EventType::StatusChanged => "事件",
    };
    let summary = ev.summary.as_deref().unwrap_or("");
    let summary = truncate_chars(summary, MAX_SUMMARY_CHARS);
    if summary.is_empty() {
        format!("[{label}]")
    } else {
        format!("[{label}] {summary}")
    }
}

/// 按字符截断（中文安全），超长加省略号（与 adapter 层口径一致）。
pub(crate) fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

/// 时间戳 → 人类可读字符串（前言用，无需精确格式）。
fn format_dt(t: Timestamp) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_domain::{Conversation, Message};

    fn sample() -> (Conversation, Vec<Message>, Vec<Event>) {
        let mut conv = Conversation::new(ch_domain::Provider::ZCode, "src-1");
        conv.title = Some("修登录 bug".into());
        conv.model = Some("GLM-5.3".into());
        let mut m1 = Message::new("c1", Role::User, 1);
        m1.content_text = Some("帮我修登录 bug".into());
        let mut m2 = Message::new("c1", Role::Assistant, 2);
        m2.content_text = Some("已修复，见 commit abc".into());
        let mut m3 = Message::new("c1", Role::System, 3);
        m3.content_text = Some("<system-reminder>注入</system-reminder>".into());
        let mut ev = Event::new("c1", EventType::CommandCompleted, 1);
        ev.summary = Some("cargo test".into());
        (conv, vec![m1, m2, m3], vec![ev])
    }

    #[test]
    fn transcript_contains_preamble_and_dialog() {
        let (conv, msgs, evs) = sample();
        let input = HandoffInput {
            conversation: &conv,
            workspace: None,
            messages: &msgs,
            events: &evs,
        };
        let ts = build_transcript(&input, &HandoffOptions::new("/tmp"));
        // 前言 + user + assistant（system 跳过）
        assert_eq!(ts.len(), 3);
        assert_eq!(ts[0].role, Role::User);
        assert!(ts[0].text.contains("[threadock 交接存档]"));
        assert!(ts[0].text.contains("修登录 bug"));
        assert!(ts[0].text.contains("[命令] cargo test"));
        assert_eq!(ts[1].role, Role::User);
        assert_eq!(ts[2].role, Role::Assistant);
    }

    #[test]
    fn transcript_without_context_skips_preamble() {
        let (conv, msgs, evs) = sample();
        let input = HandoffInput {
            conversation: &conv,
            workspace: None,
            messages: &msgs,
            events: &evs,
        };
        let ts = build_transcript(
            &input,
            &HandoffOptions {
                include_context: false,
                ..HandoffOptions::new("/tmp")
            },
        );
        assert_eq!(ts.len(), 2);
        assert!(ts[0].text.contains("登录"));
    }

    #[test]
    fn timestamps_monotonic() {
        let (conv, msgs, evs) = sample();
        let input = HandoffInput {
            conversation: &conv,
            workspace: None,
            messages: &msgs,
            events: &evs,
        };
        let ts = build_transcript(&input, &HandoffOptions::new("/tmp"));
        for pair in ts.windows(2) {
            assert!(pair[0].at <= pair[1].at, "时间戳必须单调不减");
        }
    }

    #[test]
    fn empty_messages_skipped() {
        let (conv, _, evs) = sample();
        let mut blank = Message::new("c1", Role::User, 1);
        blank.content_text = Some("   ".into());
        let input = HandoffInput {
            conversation: &conv,
            workspace: None,
            messages: &[blank],
            events: &evs,
        };
        let ts = build_transcript(&input, &HandoffOptions::new("/tmp"));
        assert_eq!(ts.len(), 1, "只有前言");
    }
    #[test]
    fn redaction_rules_applied_to_all_items() {
        let mut conv2 = Conversation::new(ch_domain::Provider::ZCode, "src-r");
        conv2.title = Some("secret".into());
        let mut m = Message::new("c", Role::User, 1);
        m.content_text = Some("token=ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ1234567890 done".into());
        let input2 = HandoffInput {
            conversation: &conv2,
            workspace: None,
            messages: &[m],
            events: &[],
        };
        let opts = HandoffOptions {
            redact_rules: vec![ch_export::CustomRule::new("ghp", r"ghp_[A-Za-z0-9]+")],
            ..HandoffOptions::new("/tmp")
        };
        let ts = build_transcript(&input2, &opts);
        assert!(ts.iter().all(|i| !i.text.contains("ghp_aBcDeF")));
        // token 只出现在消息项里 → 消息项必须被替换为 [REDACTED:...]
        assert!(ts.last().is_some_and(|i| i.text.contains("[REDACTED")));
        // 无规则 → 保真不脱敏
        let plain = build_transcript(&input2, &HandoffOptions::new("/tmp"));
        assert!(plain.last().is_some_and(|i| i.text.contains("ghp_aBcDeF")));
    }
}
