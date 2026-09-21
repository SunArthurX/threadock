//! Golden 测试：fixtures/deepseek-harness 样本 → 领域事实断言（plan §11.6）。
//! 覆盖：标题三段演进（fallback→provider→user 手动重命名，最后生效）、
//! 注入块过滤、reasoning 不入正文、bash 输出配对升格、未知工具降级。

use ch_domain::{EventType, Provider, Role};

fn fixture_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/deepseek-harness")
}

#[test]
fn golden_session_parses_domain_facts() {
    let file = fixture_dir().join("session-golden/session.jsonl");
    let raw = ch_adapter_deepseek_harness::parse_session(&file).expect("parse golden failed");

    assert_eq!(raw.provider, Provider::DeepSeekHarness);
    assert_eq!(raw.source_conversation_id, "session-golden");
    // 标题：最后一条 session/title（user 手动重命名）生效
    assert_eq!(raw.title.as_deref(), Some("我的手动重命名"));
    // 模型：首条助手消息的 source.model（非 model/selection 的 glm-5.3）
    assert_eq!(raw.model.as_deref(), Some("deepseek-v4-flash"));
    assert!(raw.started_at.is_some());
    assert_eq!(raw.source_parent_id, None);

    // 注入块过滤：1 条用户 + 1 条助手
    assert_eq!(raw.messages.len(), 2);
    assert_eq!(raw.messages[0].role, Role::User);
    assert_eq!(raw.messages[0].text.as_deref(), Some("构建挂了，帮我看看"));
    assert_eq!(raw.messages[1].role, Role::Assistant);
    assert_eq!(
        raw.messages[1].text.as_deref(),
        Some("我先复现一下构建错误。")
    );

    // 事件：bash（配对输出升格 Completed）/ read（FileRead）/ 未知工具（降级 ToolCallStarted）
    assert_eq!(raw.events.len(), 3);
    assert_eq!(raw.events[0].event_type, EventType::CommandCompleted);
    assert!(raw.events[0]
        .summary
        .as_deref()
        .expect("summary")
        .starts_with("cargo build"));
    assert!(raw.events[0]
        .payload_json
        .as_ref()
        .expect("payload")
        .to_string()
        .contains("error[E0308]"));
    assert_eq!(raw.events[1].event_type, EventType::FileRead);
    assert_eq!(raw.events[2].event_type, EventType::ToolCallStarted);
    assert!(raw.events[2]
        .payload_json
        .as_ref()
        .expect("payload")
        .to_string()
        .contains("future_tool_x"));
}

#[test]
fn golden_discover_reads_title_and_counts() {
    // discover_* 以 <home>/sessions 为根：把 fixture 按目录布局搭进临时结构
    let tmp = tempfile::TempDir::new().expect("tempdir failed");
    let inner = tmp.path().join("sessions/-demo-proj/session-golden");
    std::fs::create_dir_all(&inner).expect("mkdir failed");
    std::fs::copy(
        fixture_dir().join("session-golden/session.jsonl"),
        inner.join("session.jsonl"),
    )
    .expect("copy failed");

    let sessions =
        ch_adapter_deepseek_harness::discover_sessions(tmp.path()).expect("discover failed");
    assert_eq!(sessions.len(), 1, "应发现 1 个会话");
    let s = &sessions[0];
    assert_eq!(s.session_id, "session-golden");
    assert_eq!(s.title, "我的手动重命名");
    assert_eq!(s.parent_id, None);
    assert!(s.message_count >= 2);
}
