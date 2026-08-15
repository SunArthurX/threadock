//! 规则提取器：基于确定性规则从消息 + 事件提取知识结构（plan §13.5 MVP）。
//!
//! 这是纯算法、可充分测试的基线。未来可新增 `LlmExtractor` 走模型 API，
//! `ExtractionResult` 契约不变。

use crate::model::{Decision, ErrorItem, ExtractionInput, ExtractionResult, FileRef, TodoItem};
use ch_domain::{EventType, Role};
use regex::Regex;

/// 规则提取器（无状态，线程安全）。
pub struct RuleExtractor;

impl RuleExtractor {
    #[must_use] 
    pub fn new() -> Self {
        Self
    }

    /// 执行提取。
    #[must_use] 
    pub fn extract(&self, input: &ExtractionInput) -> ExtractionResult {
        let summary = self.summarize(input);
        let todos = self.extract_todos(input);
        let commands = self.extract_commands(input);
        let errors = self.extract_errors(input);
        let decisions = self.extract_decisions(input);
        let files = self.extract_files(input);

        ExtractionResult {
            summary,
            decisions,
            todos,
            errors,
            commands,
            files,
            extractor: "rule-v1".to_string(),
        }
    }

    // ── 摘要：首条 user 消息 + assistant 最长回复 ──────────────────────────

    fn summarize(&self, input: &ExtractionInput) -> String {
        let first_user = input
            .messages
            .iter()
            .find(|m| m.role == Role::User)
            .and_then(|m| m.content_text.as_deref())
            .unwrap_or("");
        let longest_assistant = input
            .messages
            .iter()
            .filter(|m| m.role == Role::Assistant)
            .max_by_key(|m| m.content_text.as_ref().map_or(0, std::string::String::len))
            .and_then(|m| m.content_text.as_deref())
            .unwrap_or("");

        let title = input.title.as_deref().unwrap_or("");
        let mut parts: Vec<&str> = Vec::new();
        if !title.is_empty() {
            parts.push(title);
        }
        // 摘要里每段截断到 200 字符，避免过长
        let truncate = |s: &str| -> String {
            if s.chars().count() <= 200 {
                s.to_string()
            } else {
                let cut: String = s.chars().take(200).collect();
                format!("{cut}…")
            }
        };
        if !first_user.is_empty() {
            parts.push(""); // 分隔
            let _ = &mut parts; // 借用检查
        }
        let mut out = String::new();
        if !title.is_empty() {
            out.push_str("【主题】");
            out.push_str(title);
            out.push(' ');
        }
        if !first_user.is_empty() {
            out.push_str("【问题】");
            out.push_str(&truncate(first_user));
            out.push(' ');
        }
        if !longest_assistant.is_empty() {
            out.push_str("【要点】");
            out.push_str(&truncate(longest_assistant));
        }
        out.trim().to_string()
    }

    // ── TODO：匹配关键词的句子 ─────────────────────────────────────────────

    /// TODO 关键词（plan §13.5：TODO 提取）。
    #[must_use] 
    pub fn todo_keywords() -> &'static [&'static str] {
        &[
            "TODO",
            "FIXME",
            "待办",
            "需要",
            "应该",
            "接下来",
            "还要",
            "尚未",
        ]
    }

    fn extract_todos(&self, input: &ExtractionInput) -> Vec<TodoItem> {
        let keywords = Self::todo_keywords();
        let mut todos = Vec::new();
        for m in &input.messages {
            if let Some(text) = &m.content_text {
                for sentence in split_sentences(text) {
                    let lower = sentence.to_lowercase();
                    if keywords.iter().any(|k| lower.contains(&k.to_lowercase())) {
                        let trimmed = sentence.trim();
                        if trimmed.len() > 3 {
                            todos.push(TodoItem {
                                text: trimmed.to_string(),
                                source_message_ids: vec![m.id.clone()],
                            });
                        }
                    }
                }
            }
        }
        dedup_by_text(&mut todos, |t| &t.text);
        todos
    }

    // ── 命令：Command 事件 + 消息中的反引号代码 ────────────────────────────

    fn extract_commands(&self, input: &ExtractionInput) -> Vec<String> {
        let mut cmds: Vec<String> = Vec::new();

        // 来自 Command 事件
        for e in &input.events {
            if matches!(
                e.event_type,
                EventType::CommandStarted | EventType::CommandCompleted
            ) {
                if let Some(s) = &e.summary {
                    cmds.push(s.clone());
                }
            }
        }

        // 来自消息中的反引号代码块（`xxx`）
        let backtick = Regex::new(r"`([^`]{3,})`").expect("invalid regex");
        for m in &input.messages {
            if let Some(text) = &m.content_text {
                for cap in backtick.captures_iter(text) {
                    if let Some(c) = cap.get(1) {
                        let cmd = c.as_str().trim();
                        // 只保留看起来像命令的（含空格或已知命令前缀）
                        if looks_like_command(cmd) {
                            cmds.push(cmd.to_string());
                        }
                    }
                }
            }
        }

        dedup_strings(&mut cmds);
        cmds
    }

    // ── 错误：关键词句子 + Error 事件 ──────────────────────────────────────

    fn extract_errors(&self, input: &ExtractionInput) -> Vec<ErrorItem> {
        let keywords = [
            "error",
            "错误",
            "failed",
            "failure",
            "panic",
            "exception",
            "报错",
        ];
        let mut errors = Vec::new();

        // Error 事件
        for e in &input.events {
            if e.event_type == EventType::Error {
                if let Some(s) = &e.summary {
                    errors.push(ErrorItem {
                        error: s.clone(),
                        solution: None,
                        source_message_ids: vec![],
                    });
                }
            }
        }

        // 消息中的错误句子
        for m in &input.messages {
            if let Some(text) = &m.content_text {
                let lower = text.to_lowercase();
                if keywords.iter().any(|k| lower.contains(k)) {
                    for sentence in split_sentences(text) {
                        let sl = sentence.to_lowercase();
                        if keywords.iter().any(|k| sl.contains(k)) {
                            let trimmed = sentence.trim();
                            if trimmed.len() > 5 {
                                errors.push(ErrorItem {
                                    error: trimmed.to_string(),
                                    solution: None,
                                    source_message_ids: vec![m.id.clone()],
                                });
                            }
                        }
                    }
                }
            }
        }

        dedup_by_text(&mut errors, |e| &e.error);
        errors
    }

    // ── 决策：决策性表述 ───────────────────────────────────────────────────

    fn extract_decisions(&self, input: &ExtractionInput) -> Vec<Decision> {
        let keywords = [
            "决定",
            "选用",
            "结论",
            "应该",
            "采用",
            "选择",
            "最终",
            "recommend",
            "decide",
        ];
        let mut decisions = Vec::new();
        for m in &input.messages {
            if let Some(text) = &m.content_text {
                for sentence in split_sentences(text) {
                    let lower = sentence.to_lowercase();
                    if keywords.iter().any(|k| lower.contains(&k.to_lowercase())) {
                        let trimmed = sentence.trim();
                        if trimmed.len() > 5 {
                            decisions.push(Decision {
                                decision: trimmed.to_string(),
                                reason: None,
                                source_message_ids: vec![m.id.clone()],
                            });
                        }
                    }
                }
            }
        }
        dedup_by_text(&mut decisions, |d| &d.decision);
        decisions
    }

    // ── 涉及文件：Diff/File 事件 + 路径模式 ────────────────────────────────

    fn extract_files(&self, input: &ExtractionInput) -> Vec<FileRef> {
        let mut files = Vec::new();

        // File/Diff 事件
        for e in &input.events {
            if matches!(
                e.event_type,
                EventType::FileRead
                    | EventType::FileCreated
                    | EventType::FileUpdated
                    | EventType::FileDeleted
                    | EventType::DiffGenerated
            ) {
                if let Some(s) = &e.summary {
                    if let Some(path) = extract_path(s) {
                        files.push(FileRef {
                            path,
                            source_message_ids: vec![],
                        });
                    }
                }
            }
        }

        // 消息中的路径模式（src/xxx 或 *.ext）
        let path_re =
            Regex::new(r"[\w\-./]+/[ \w\-./]+\.\w+|[\w\-]+\.(rs|ts|js|py|go|md|toml|json)")
                .expect("unexpected None");
        for m in &input.messages {
            if let Some(text) = &m.content_text {
                for cap in path_re.captures_iter(text) {
                    if let Some(c) = cap.get(0) {
                        files.push(FileRef {
                            path: c.as_str().to_string(),
                            source_message_ids: vec![m.id.clone()],
                        });
                    }
                }
            }
        }

        dedup_by_text(&mut files, |f| &f.path);
        files
    }
}

impl Default for RuleExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// 提取的 TODO 关键词常量（便于外部引用）。
pub const TODOS: &[&str] = &["TODO", "FIXME", "待办", "需要"];

// ── 辅助 ──────────────────────────────────────────────────────────────────

/// 按中文句号/问号/感叹号/换行/英文句点切句。
fn split_sentences(text: &str) -> Vec<String> {
    text.split(['。', '？', '！', '\n', '.'])
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .collect()
}

/// 判断反引号内容是否像命令（含空格，或是已知单字命令）。
fn looks_like_command(s: &str) -> bool {
    if s.contains(' ') {
        return true;
    }
    // 单字但属于常见命令
    matches!(
        s,
        "ls" | "pwd" | "git" | "cargo" | "npm" | "node" | "python" | "make"
    )
}

/// 从文本提取第一个看起来像路径的片段。
fn extract_path(s: &str) -> Option<String> {
    // 取第一个含 / 或已知扩展名的 token
    let re = Regex::new(r"[\w\-./]+/[ \w\-./]+|[\w\-]+\.\w+").expect("invalid regex");
    re.captures(s)
        .and_then(|c| c.get(0))
        .map(|m| m.as_str().to_string())
}

fn dedup_strings(v: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    v.retain(|s| seen.insert(s.clone()));
}

fn dedup_by_text<T, F>(v: &mut Vec<T>, key: F)
where
    F: Fn(&T) -> &str,
{
    let mut seen = std::collections::HashSet::new();
    v.retain(|item| seen.insert(key(item).to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ExtractionInput;
    use ch_domain::{Event, EventType, Message, Role};

    fn msg(id: &str, role: Role, text: &str) -> Message {
        let mut m = Message::new("conv", role, id.parse().unwrap_or(1));
        m.id = id.into();
        m.content_text = Some(text.into());
        m
    }

    fn event(id: &str, et: EventType, seq: i64, summary: &str) -> Event {
        let mut e = Event::new("conv", et, seq);
        e.id = id.into();
        e.summary = Some(summary.into());
        e
    }

    fn input(title: Option<&str>, messages: Vec<Message>, events: Vec<Event>) -> ExtractionInput {
        ExtractionInput {
            title: title.map(String::from),
            messages,
            events,
        }
    }

    #[test]
    fn summary_includes_title_and_question() {
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            Some("Tauri 讨论"),
            vec![
                msg("m1", Role::User, "怎么用 Tauri 做 Android 后台任务？"),
                msg("m2", Role::Assistant, "用 WorkManager。"),
            ],
            vec![],
        ));
        assert!(r.summary.contains("Tauri 讨论"));
        assert!(r.summary.contains("Android"));
        assert_eq!(r.extractor, "rule-v1");
    }

    #[test]
    fn extract_todos_from_keywords() {
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            None,
            vec![msg(
                "m1",
                Role::Assistant,
                "TODO 添加测试\n需要处理边界情况\nFIXME 内存泄漏\n这是普通句子",
            )],
            vec![],
        ));
        assert!(r.todos.iter().any(|t| t.text.contains("添加测试")));
        assert!(r.todos.iter().any(|t| t.text.contains("边界情况")));
        assert!(r.todos.iter().any(|t| t.text.contains("内存泄漏")));
        assert!(!r.todos.iter().any(|t| t.text.contains("普通句子")));
    }

    #[test]
    fn extract_commands_from_events() {
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            None,
            vec![],
            vec![
                event("e1", EventType::CommandStarted, 1, "cargo build"),
                event("e2", EventType::CommandCompleted, 2, "cargo test"),
            ],
        ));
        assert!(r.commands.contains(&"cargo build".to_string()));
        assert!(r.commands.contains(&"cargo test".to_string()));
    }

    #[test]
    fn extract_commands_from_backticks() {
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            None,
            vec![msg(
                "m1",
                Role::Assistant,
                "运行 `cargo build --release` 然后 `npm test`",
            )],
            vec![],
        ));
        assert!(r.commands.iter().any(|c| c.contains("cargo build")));
        assert!(r.commands.iter().any(|c| c.contains("npm test")));
    }

    #[test]
    fn extract_errors_from_messages() {
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            None,
            vec![msg(
                "m1",
                Role::Assistant,
                "编译时遇到 error: cannot find value\n检查后发现拼写错误",
            )],
            vec![],
        ));
        assert!(r.errors.iter().any(|e| e.error.contains("cannot find")));
    }

    #[test]
    fn extract_errors_from_error_events() {
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            None,
            vec![],
            vec![event(
                "e1",
                EventType::Error,
                1,
                "panic: index out of bounds",
            )],
        ));
        assert!(r.errors.iter().any(|e| e.error.contains("panic")));
    }

    #[test]
    fn extract_decisions() {
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            None,
            vec![msg(
                "m1",
                Role::Assistant,
                "决定使用 SQLite 作为主数据存储\n因为它是单文件嵌入式数据库",
            )],
            vec![],
        ));
        assert!(r.decisions.iter().any(|d| d.decision.contains("SQLite")));
    }

    #[test]
    fn extract_files_from_diff_events() {
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            None,
            vec![],
            vec![event(
                "e1",
                EventType::DiffGenerated,
                1,
                "src/main.rs 修改了入口",
            )],
        ));
        assert!(r.files.iter().any(|f| f.path.contains("main.rs")));
    }

    #[test]
    fn extract_files_from_messages() {
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            None,
            vec![msg(
                "m1",
                Role::User,
                "请看 src-tauri/Cargo.toml 和 lib.rs 的实现",
            )],
            vec![],
        ));
        assert!(r.files.iter().any(|f| f.path.contains("Cargo.toml")));
    }

    #[test]
    fn dedup_repeated_items() {
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            None,
            vec![
                msg("m1", Role::Assistant, "TODO 修复 bug"),
                msg("m2", Role::Assistant, "TODO 修复 bug"),
            ],
            vec![],
        ));
        // 两条相同 TODO 应去重为 1 条
        let count = r
            .todos
            .iter()
            .filter(|t| t.text.contains("修复 bug"))
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn empty_input_produces_empty_result() {
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(None, vec![], vec![]));
        assert!(r.summary.is_empty());
        assert!(r.todos.is_empty());
        assert!(r.commands.is_empty());
        assert!(r.errors.is_empty());
        assert!(r.decisions.is_empty());
        assert!(r.files.is_empty());
    }

    #[test]
    fn result_is_serializable() {
        // plan §13.5：输出结构可序列化（便于存储/展示）
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            Some("test"),
            vec![msg("m1", Role::User, "TODO something")],
            vec![],
        ));
        let json = serde_json::to_string(&r).expect("unexpected None");
        let back: ExtractionResult = serde_json::from_str(&json).expect("parse failed");
        assert_eq!(r, back);
    }
}
