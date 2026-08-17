//! Golden Fixture Tests（plan §20.2）：对 `fixtures/markdown/` 脱敏样本断言领域事实。
//!
//! 断言解析的**结构**（消息数/角色/事件类型/标题），不做逐字快照——
//! 快照对无害改动（空白/措辞）过度敏感。

use ch_domain::{EventType, Role};

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/markdown")
}

#[test]
fn golden_tauri_background() {
    let raw =
        ch_adapter_markdown::parse_file(fixtures_dir().join("tauri-background.md")).expect("parse");
    assert_eq!(
        raw.title.as_deref(),
        Some("Tauri Android background task migration")
    );
    // 消息：User / Assistant / Assistant /（Error 段走兜底 System）
    // 事件：Command x2 + Diff + ToolCall（Tool 段归事件）
    assert_eq!(raw.messages.len(), 4);
    assert_eq!(raw.messages[0].role, Role::User);
    assert_eq!(raw.messages[1].role, Role::Assistant);
    assert_eq!(raw.messages[2].role, Role::Assistant);
    assert_eq!(raw.messages[3].role, Role::System);
    assert!(raw.messages[1]
        .text
        .as_deref()
        .unwrap_or_default()
        .contains("WorkManager"));
    assert_eq!(raw.events.len(), 3);
    let types: Vec<EventType> = raw.events.iter().map(|e| e.event_type).collect();
    assert!(types.contains(&EventType::CommandStarted));
    assert!(types.contains(&EventType::DiffGenerated));
    assert!(types.contains(&EventType::ToolCallStarted));
}

#[test]
fn golden_rust_error_handling_chinese() {
    let raw = ch_adapter_markdown::parse_file(fixtures_dir().join("rust-error-handling.md"))
        .expect("parse");
    assert_eq!(raw.title.as_deref(), Some("Rust 错误处理选型讨论"));
    assert_eq!(raw.messages.len(), 4);
    assert_eq!(raw.messages[0].role, Role::User);
    assert_eq!(raw.messages[1].role, Role::Assistant);
    assert!(raw.events.is_empty());
    // 中文正文不被破坏
    assert!(raw.messages[1]
        .text
        .as_deref()
        .unwrap_or_default()
        .contains("thiserror"));
}
