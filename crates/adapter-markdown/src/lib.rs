//! 通用 Markdown Adapter，对应 plan §10.5「Markdown/JSON Import」与 §22「Markdown Adapter」。
//!
//! 它是 plan §29「首个端到端链路」用的最简来源：用一个最朴素的 Markdown 约定
//! 验证整条 Source → Raw → Normalize → SQLite 流水线。
//!
//! ## 支持的 Markdown 约定（v0.1）
//!
//! ```markdown
//! # 标题（可选，第一行）
//!
//! ## User
//! 用户消息内容……
//!
//! ## Assistant
//! 助手回复……
//!
//! ## Command
//! cargo build
//! ```
//!
//! - 一级标题（`# `）→ conversation.title
//! - 二级标题（`## `）按关键词判定角色/事件：
//!   - `User` / `用户` → Role::User
//!   - `Assistant` / `助手` / `AI` → Role::Assistant
//!   - `System` / `系统` → Role::System
//!   - `Command` / `命令` → Command 事件
//!   - `Diff` / `变更` → Diff 事件
//!   - `Tool` / `工具` → ToolCall 事件
//! - 其他二级标题视为 System 消息并保留原标题。
//!
//! 这套约定故意简单：plan §29 的目标是「跑通流水线 + 落 Fixture」，
//! 不是构建生产级 Markdown 解析器。生产级 Parser 在 Phase 2 完善。

use ch_domain::{EventType, Provider, Role};
use ch_normalization::{RawConversation, RawEvent, RawMessage};
use std::path::Path;
use thiserror::Error;

pub type AdapterResult<T> = std::result::Result<T, MarkdownAdapterError>;

#[derive(Debug, Error)]
pub enum MarkdownAdapterError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("markdown has no message sections (## ...)")]
    NoSections,

    #[error("invalid utf-8 in markdown file")]
    InvalidUtf8,
}

/// Adapter 元信息。
pub const ADAPTER_ID: &str = "markdown";
pub const ADAPTER_VERSION: &str = "0.1.0";
pub const PROVIDER: Provider = Provider::Generic;

/// 一个段落属于哪种领域对象。
#[derive(Debug, Clone)]
enum SectionKind {
    Message(Role),
    Event(EventType),
}

/// 从文件路径解析。
pub fn parse_file(path: impl AsRef<Path>) -> AdapterResult<RawConversation> {
    let path_ref = path.as_ref();
    let bytes = std::fs::read(path_ref)?;
    let content = std::str::from_utf8(&bytes).map_err(|_| MarkdownAdapterError::InvalidUtf8)?;
    parse_str(content, &path_ref.to_string_lossy())
}

/// 从字符串解析。`source_id` 用作 source_conversation_id（通常为文件路径或哈希）。
pub fn parse_str(content: &str, source_id: &str) -> AdapterResult<RawConversation> {
    let mut title: Option<String> = None;
    let mut messages: Vec<RawMessage> = Vec::new();
    let mut events: Vec<RawEvent> = Vec::new();

    let mut current: Option<SectionKind> = None;
    let mut buf: String = String::new();

    for line in content.lines() {
        if let Some(h) = line.strip_prefix("# ") {
            // 一级标题：conversation 标题（仅取第一个）
            if title.is_none() {
                title = Some(h.trim().to_string());
            }
            continue;
        }
        if let Some(h2) = line.strip_prefix("## ") {
            // 二级标题：归档上一段，再开新段
            flush(&current, &buf, &mut messages, &mut events);
            buf.clear();
            current = Some(classify_heading(h2.trim()));
            continue;
        }
        // 普通行：累积到当前段落
        if current.is_some() {
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(line);
        }
    }
    // 文件末尾归档最后一段
    flush(&current, &buf, &mut messages, &mut events);

    if messages.is_empty() && events.is_empty() {
        return Err(MarkdownAdapterError::NoSections);
    }

    Ok(RawConversation {
        provider: PROVIDER,
        source_conversation_id: source_id.to_string(),
        title,
        model: None,
        started_at: None,
        messages,
        events,
        source_parent_id: None,
    })
}

/// 把当前累积段落写入对应容器。
fn flush(
    kind: &Option<SectionKind>,
    body: &str,
    messages: &mut Vec<RawMessage>,
    events: &mut Vec<RawEvent>,
) {
    let text = body.trim();
    if text.is_empty() {
        return;
    }
    let text = text.to_string();
    match kind {
        Some(SectionKind::Message(role)) => messages.push(RawMessage {
            role: *role,
            text: Some(text),
            content_json: None,
            source_message_id: None,
            created_at: None,
        }),
        Some(SectionKind::Event(et)) => events.push(RawEvent {
            event_type: *et,
            summary: Some(text),
            payload_json: None,
            source_event_id: None,
            created_at: None,
        }),
        None => {}
    }
}

/// 把二级标题文字归类为段落类型。
fn classify_heading(raw: &str) -> SectionKind {
    let lower = raw.to_lowercase();
    if matches_str(&lower, &["user", "用户", "我"]) {
        return SectionKind::Message(Role::User);
    }
    if matches_str(&lower, &["assistant", "助手", "ai", "模型"]) {
        return SectionKind::Message(Role::Assistant);
    }
    if matches_str(&lower, &["system", "系统"]) {
        return SectionKind::Message(Role::System);
    }
    if matches_str(&lower, &["command", "命令"]) {
        return SectionKind::Event(EventType::CommandStarted);
    }
    if matches_str(&lower, &["diff", "变更"]) {
        return SectionKind::Event(EventType::DiffGenerated);
    }
    if matches_str(&lower, &["tool", "工具", "tool call", "工具调用"]) {
        return SectionKind::Event(EventType::ToolCallStarted);
    }
    // 兜底：当作系统消息
    SectionKind::Message(Role::System)
}

fn matches_str(lower: &str, candidates: &[&str]) -> bool {
    candidates.iter().any(|c| lower == *c || lower.contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_normalization::normalize;

    const SAMPLE: &str = "# 关于 Tauri Android 后台任务\n\n## User\n之前在哪个 Agent 里讨论过 Tauri Android 后台任务？\n\n## Assistant\n我在 Codex 里讨论过，关键点是 WorkManager。\n\n## Command\ncargo tauri android init\n\n## Diff\nsrc-tauri/src/lib.rs 新增了 run_background_task 函数。\n";

    #[test]
    fn parses_title_and_messages() {
        let raw = parse_str(SAMPLE, "sample.md").unwrap();
        assert_eq!(raw.title.as_deref(), Some("关于 Tauri Android 后台任务"));
        assert_eq!(raw.messages.len(), 2);
        assert_eq!(raw.messages[0].role, Role::User);
        assert!(raw.messages[0]
            .text
            .as_deref()
            .unwrap()
            .contains("Tauri Android"));
        assert_eq!(raw.messages[1].role, Role::Assistant);
        assert!(raw.messages[1]
            .text
            .as_deref()
            .unwrap()
            .contains("WorkManager"));
    }

    #[test]
    fn parses_events() {
        let raw = parse_str(SAMPLE, "sample.md").unwrap();
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
    fn chinese_headings_recognized() {
        let md = "## 用户\n你好\n## 助手\n你好啊\n## 命令\ncargo build\n";
        let raw = parse_str(md, "zh.md").unwrap();
        assert_eq!(raw.messages[0].role, Role::User);
        assert_eq!(raw.messages[1].role, Role::Assistant);
        assert!(raw
            .events
            .iter()
            .any(|e| e.event_type == EventType::CommandStarted));
    }

    #[test]
    fn empty_document_errors() {
        assert!(parse_str("", "empty.md").is_err());
        assert!(parse_str("只有一段普通文字\n没有标题", "nosec.md").is_err());
    }

    #[test]
    fn no_title_still_works() {
        let md = "## User\nhi\n## Assistant\nhello\n";
        let raw = parse_str(md, "notitle.md").unwrap();
        assert!(raw.title.is_none());
        assert_eq!(raw.messages.len(), 2);
    }

    #[test]
    fn unknown_heading_becomes_system_message() {
        let md = "## Notes\n一些笔记\n## User\nhi\n";
        let raw = parse_str(md, "u.md").unwrap();
        assert_eq!(raw.messages[0].role, Role::System);
    }

    #[test]
    fn source_id_is_filename() {
        let raw = parse_str("## User\nhi\n", "/tmp/conv.md").unwrap();
        assert_eq!(raw.source_conversation_id, "/tmp/conv.md");
    }

    #[test]
    fn file_roundtrip() {
        use tempfile::NamedTempFile;
        let f = NamedTempFile::new().unwrap();
        std::fs::write(f.path(), "## User\nhi from file\n## Assistant\nyo\n").unwrap();
        let raw = parse_file(f.path()).unwrap();
        assert_eq!(raw.messages.len(), 2);
        assert!(raw.messages[0]
            .text
            .as_deref()
            .unwrap()
            .contains("hi from file"));
    }

    #[test]
    fn integrates_with_normalization() {
        // Adapter → Normalization 端到端
        let raw = parse_str(SAMPLE, "sample.md").unwrap();
        let n = normalize(raw).unwrap();
        assert_eq!(n.messages.len(), 2);
        assert_eq!(n.events.len(), 2);
        assert!(!n.conversation_hash.is_empty());
        // 有 command + diff → 至少 Partial
        let label = n.completeness.label();
        assert!(matches!(label, "部分" | "完整"));
    }

    #[test]
    fn multiline_body_preserved() {
        let md = "## User\n第一行\n第二行\n第三行\n## Assistant\nok\n";
        let raw = parse_str(md, "ml.md").unwrap();
        let body = raw.messages[0].text.as_deref().unwrap();
        assert!(body.contains("第一行"));
        assert!(body.contains("第二行"));
        assert!(body.contains("第三行"));
    }
}
