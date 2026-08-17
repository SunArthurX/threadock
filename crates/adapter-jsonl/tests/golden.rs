//! Golden Fixture Tests（plan §20.2）：对 `fixtures/jsonl/` 脱敏样本断言领域事实。

use ch_domain::{EventType, Role};

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/jsonl")
}

#[test]
fn golden_opencode_style() {
    let raw =
        ch_adapter_jsonl::parse_file(fixtures_dir().join("opencode-style.jsonl")).expect("parse");
    assert_eq!(raw.title.as_deref(), Some("Refactor auth middleware"));
    assert_eq!(raw.model.as_deref(), Some("gpt-5.3-mini"));
    assert_eq!(raw.messages.len(), 3);
    assert_eq!(raw.messages[0].role, Role::User);
    assert_eq!(raw.messages[1].role, Role::Assistant);
    assert_eq!(raw.messages[2].role, Role::System);
    assert_eq!(raw.events.len(), 2);
    assert!(raw
        .events
        .iter()
        .any(|e| e.event_type == EventType::CommandStarted));
    assert!(raw
        .events
        .iter()
        .any(|e| e.event_type == EventType::DiffGenerated));
}

#[test]
fn golden_minimal() {
    let raw = ch_adapter_jsonl::parse_file(fixtures_dir().join("minimal.jsonl")).expect("parse");
    assert_eq!(raw.messages.len(), 2);
    assert_eq!(raw.messages[0].text.as_deref(), Some("ping"));
    assert_eq!(raw.messages[1].role, Role::Assistant);
    assert!(raw.title.is_none());
    assert!(raw.events.is_empty());
}
