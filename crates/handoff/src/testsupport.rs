//! 测试共用的输入构造器（仅测试编译）。
//!
//! 样本数据通过 `Box::leak` 固定为 `'static` 借用——仅测试内使用，量级固定，
//! 泄漏可忽略。

use crate::HandoffInput;
use ch_domain::{Conversation, Event, EventType, Message, Role, Workspace};

/// 构造一条带 workspace / 消息 / 事件的样本会话借用视图。
pub(crate) fn make_input() -> HandoffInput<'static> {
    let mut conversation = Conversation::new(ch_domain::Provider::ZCode, "src-test-1");
    conversation.title = Some("修登录 bug".into());
    conversation.model = Some("GLM-5.3".into());

    let mut workspace = Workspace::new("demo-proj");
    workspace.canonical_path = Some("/tmp/demo-proj/06.code".into());

    let mut m1 = Message::new("conv_test", Role::User, 1);
    m1.content_text = Some("帮我在 GLM-5.3 上继续开发这个功能".into());
    let mut m2 = Message::new("conv_test", Role::Assistant, 2);
    m2.content_text = Some("好的，上下文已就绪，下一步写测试。".into());

    let mut ev = Event::new("conv_test", EventType::CommandCompleted, 1);
    ev.summary = Some("cargo test".into());

    let sample = Box::leak(Box::new((
        conversation,
        Some(workspace),
        vec![m1, m2],
        vec![ev],
    )));
    HandoffInput {
        conversation: &sample.0,
        workspace: sample.1.as_ref(),
        messages: &sample.2,
        events: &sample.3,
    }
}
