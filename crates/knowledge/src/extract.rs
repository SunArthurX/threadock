//! 规则提取器：基于确定性规则从消息 + 事件提取知识结构（plan §13.5 MVP）。
//!
//! 这是纯算法、可充分测试的基线。未来可新增 `LlmExtractor` 走模型 API，
//! `ExtractionResult` 契约不变。

use crate::model::{
    Decision, ErrorItem, ExtractionInput, ExtractionResult, FileRef, TodoItem, TodoStatus,
};
use ch_domain::{EventType, Role};
use regex::Regex;
use std::collections::HashMap;

/// 规则提取器版本标识（落库 `extractor` 字段；升级即触发全量重提取）。
pub const RULE_EXTRACTOR: &str = "rule-v2";

/// Agent 框架注入到 system 消息里的模板句（"TodoWrite tool hasn't been used" 等）。
/// 真实语料中 54% 的 TODO 命中来自这类噪音，逐句排除。
const HARNESS_BOILERPLATE: &[&str] = &[
    "todowrite tool hasn't been used",
    "consider cleaning up the todo list",
    "existing contents of your todo list",
    "if you're working on tasks that would benefit",
    "<system-reminder>",
    "<command-name>",
];

/// 句内完成措辞（自身即判定 Done）。
const DONE_MARKS: &[&str] = &[
    "已完成",
    "已搞定",
    "已修复",
    "已实现",
    "已添加",
    "已处理",
    "已解决",
    "已通过",
    "已重编",
    "✅",
    "[x]",
];

/// 会话末尾窗口：TODO 出现在最后 5 条消息或后 15% 内视为「仍未解决」。
const PENDING_TAIL_MSGS: usize = 5;
const PENDING_TAIL_RATIO: f64 = 0.85;

/// 后文完成证据与 TODO 的最小公共子串长度（字符）。
const EVIDENCE_OVERLAP_CHARS: usize = 4;

/// 规则提取器（无状态，线程安全）。
pub struct RuleExtractor;

#[allow(clippy::unused_self)] // 提取器方法形态保持 API 一致（未来可携带配置）
impl RuleExtractor {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// 执行提取。
    #[must_use]
    pub fn extract(&self, input: &ExtractionInput) -> ExtractionResult {
        let timing = std::env::var_os("CH_EXTRACT_TIMING").is_some();
        let tp = |name: &str, t: std::time::Instant| {
            if timing {
                eprintln!("[extract] {name} {:?}", t.elapsed());
            }
        };
        let t0 = std::time::Instant::now();
        let summary = self.summarize(input);
        tp("summary", t0);
        let t1 = std::time::Instant::now();
        let todos = self.extract_todos(input);
        tp("todos", t1);
        let t2 = std::time::Instant::now();
        let commands = self.extract_commands(input);
        tp("commands", t2);
        let t3 = std::time::Instant::now();
        let errors = self.extract_errors(input);
        tp("errors", t3);
        let t4 = std::time::Instant::now();
        let decisions = self.extract_decisions(input);
        tp("decisions", t4);
        let t5 = std::time::Instant::now();
        let files = self.extract_files(input);
        tp("files", t5);

        ExtractionResult {
            summary,
            decisions,
            todos,
            errors,
            commands,
            files,
            extractor: RULE_EXTRACTOR.to_string(),
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
        // 摘要全文输出（2026-08-15 用户反馈：弹窗中 200 字符截断的「…」丢失信息）
        let mut out = String::new();
        if !title.is_empty() {
            out.push_str("【主题】");
            out.push_str(title);
            out.push(' ');
        }
        if !first_user.is_empty() {
            out.push_str("【问题】");
            out.push_str(first_user);
            out.push(' ');
        }
        if !longest_assistant.is_empty() {
            out.push_str("【要点】");
            out.push_str(longest_assistant);
        }
        out.trim().to_string()
    }

    // ── TODO：匹配关键词的句子 + 完成态判定 ────────────────────────────────

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
        let total = input.messages.len();

        // 完成证据池：user/assistant 消息里带完成措辞的句子，**文本去重**只存最晚下标
        // ——大会话重复率极高（「已完成 ✅」等反复出现），去重让 O(候选×证据) 匹配
        // 从 12s 级降到亚秒（1954 条消息会话实测）
        let mut evidence: HashMap<String, usize> = HashMap::new();
        for (i, m) in input.messages.iter().enumerate() {
            if !matches!(m.role, Role::User | Role::Assistant) {
                continue;
            }
            let Some(text) = m.content_text.as_deref() else {
                continue;
            };
            for s in split_sentences(text) {
                if DONE_MARKS.iter().any(|k| s.contains(k)) {
                    evidence
                        .entry(s)
                        .and_modify(|idx| *idx = (*idx).max(i))
                        .or_insert(i);
                }
            }
        }

        // 候选句（顺序即对话序）：文本去勾选框前缀、排除 system/工具输出与框架模板句
        // 候选文本去重**提前**（首现顺序 + 最晚下标/状态——与「后文陈述覆盖早期」语义一致），
        // 状态判定在去重后的集合上进行，大语料下候选数砍掉重复项
        let mut cands = collect_todo_candidates(input);

        // 完成态：句内措辞 > 后文证据 > 会话位置（末尾=未解决，早期=过期）
        // 后文证据匹配用**单调指针 + gram 集合**（语义与逐对 contains 等价）：
        // 候选按最晚下标降序处理，指针把下标更大的证据句 gram 增量并入集合——
        // O(候选 gram + 证据 gram) 取代 O(候选×证据×gram) 交叉匹配
        // （1954 条消息会话实测 12s 级 → 亚秒）
        let mut order: Vec<usize> = (0..cands.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(cands[i].msg_idx));
        let mut ev_list: Vec<(usize, &str)> =
            evidence.iter().map(|(s, &i)| (i, s.as_str())).collect();
        ev_list.sort_by_key(|(i, _)| std::cmp::Reverse(*i));
        let mut gram_set: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut p = 0usize;
        for &ci in &order {
            // 并入所有下标大于当前候选最晚下标的证据句 gram
            while p < ev_list.len() && ev_list[p].0 > cands[ci].msg_idx {
                push_grams(ev_list[p].1, &mut gram_set);
                p += 1;
            }
            let c = &mut cands[ci];
            if c.status == TodoStatus::Done {
                continue;
            }
            if DONE_MARKS.iter().any(|k| c.text.contains(k)) {
                c.status = TodoStatus::Done;
                continue;
            }
            if !gram_set.is_empty() && grams_of(&c.text).any(|g| gram_set.contains(&g)) {
                c.status = TodoStatus::Done;
                continue;
            }
            let tail = total.saturating_sub(PENDING_TAIL_MSGS);
            let ratio = if total == 0 {
                1.0
            } else {
                c.msg_idx as f64 / total as f64
            };
            c.status = if c.msg_idx >= tail || ratio >= PENDING_TAIL_RATIO {
                TodoStatus::Pending
            } else {
                TodoStatus::Stale
            };
        }

        cands
            .into_iter()
            .map(|c| TodoItem {
                text: c.text,
                status: c.status,
                source_message_ids: vec![c.msg_id],
            })
            .collect()
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
        // 字符类不含空格：含空格的"路径"是误报，且超宽字符类在百万字符级
        // 长文本上的扫描使提取耗时 11.5s/会话（实测）；(?:) 非捕获组更快
        let path_re =
            Regex::new(r"[\w\-./]+/[\w\-./]+\.\w+|[\w\-]+\.(?:rs|ts|js|py|go|md|toml|json)")
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

/// 勾选框行前缀（`- [x] 文本`）——进程级缓存（正则编译昂贵，见 extract_path 教训）。
static CHECKBOX_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"^[-*+]\s*\[([ xX])\]\s*").expect("invalid regex"));

/// 关键词净化表：否定形态与「待办工具自述」不构成待办。
const KEYWORD_SCRUB: &[&str] = &[
    "不需要",
    "无需",
    "todowrite",
    "todo-write",
    "todo list",
    "todolist",
    "todo tool",
    "待办清单",
    "待办列表",
];

/// TODO 候选（聚合去重后）：最晚出现位置与最晚状态决定最终判定。
struct Cand {
    text: String,
    msg_idx: usize,
    msg_id: String,
    status: TodoStatus,
}

/// 收集 TODO 候选（勾选框剥离、关键词净化、文本去重提前——最晚下标/状态覆盖）。
fn collect_todo_candidates(input: &ExtractionInput) -> Vec<Cand> {
    let keywords = RuleExtractor::todo_keywords();
    let checkbox = &*CHECKBOX_RE;
    let mut cands: Vec<Cand> = Vec::new();
    let mut slot: HashMap<String, usize> = HashMap::new();
    for (i, m) in input.messages.iter().enumerate() {
        if !matches!(m.role, Role::User | Role::Assistant) {
            continue; // system 消息是框架注入（TodoWrite 提醒等），不是任何人的 TODO
        }
        let Some(text) = m.content_text.as_deref() else {
            continue;
        };
        for sentence in split_sentences(text) {
            let lower = sentence.to_lowercase();
            if HARNESS_BOILERPLATE.iter().any(|b| lower.contains(b)) {
                continue;
            }
            // 勾选框行：`- [x] 文本` 本身就是 TODO，x 即已完成；剥离前缀再走关键词
            let mut status = TodoStatus::Pending;
            let mut is_checkbox = false;
            let body = match checkbox.captures(&sentence) {
                Some(cap) => {
                    is_checkbox = true;
                    if cap
                        .get(1)
                        .is_some_and(|c| c.as_str().trim().eq_ignore_ascii_case("x"))
                    {
                        status = TodoStatus::Done;
                    }
                    sentence[cap.get(0).map_or(0, |c| c.end())..]
                        .trim()
                        .to_string()
                }
                None => sentence,
            };
            // 措辞净化后再匹配关键词：否定形态（不需要/无需）与
            // 「待办工具自述」（先更新 TodoList / 建立待办清单…）都不构成待办
            let kw_target = KEYWORD_SCRUB
                .iter()
                .fold(body.to_lowercase(), |acc, pat| acc.replace(pat, " "));
            let hit_keyword = keywords
                .iter()
                .any(|k| kw_target.contains(&k.to_lowercase()));
            // 勾选框行无需关键词即入候选；其余句子必须命中关键词
            if !hit_keyword && !is_checkbox {
                continue;
            }
            let trimmed = body.trim();
            if trimmed.chars().count() <= 3 {
                continue;
            }
            if let Some(&si) = slot.get(trimmed) {
                let c = &mut cands[si];
                c.msg_idx = i;
                c.msg_id.clone_from(&m.id);
                c.status = status;
            } else {
                slot.insert(trimmed.to_string(), cands.len());
                cands.push(Cand {
                    text: trimmed.to_string(),
                    msg_idx: i,
                    msg_id: m.id.clone(),
                    status,
                });
            }
        }
    }
    cands
}

/// 文本的 4-gram 迭代器（[`EVIDENCE_OVERLAP_CHARS`] 字符滑动窗）。
/// 「接下来修分页」的 gram 命中后文「分页已修复」的证据集合 → 判 Done。
fn grams_of(text: &str) -> impl Iterator<Item = String> {
    let chars: Vec<char> = text.chars().collect();
    // 短于窗长的句子（如单字符「✅」证据句）没有 gram，返回空迭代
    let n = chars
        .len()
        .checked_sub(EVIDENCE_OVERLAP_CHARS)
        .map_or(0, |d| d + 1);
    (0..n).map(move |i| chars[i..i + EVIDENCE_OVERLAP_CHARS].iter().collect())
}

/// 把句子 gram 并入证据集合（单调指针法增量使用）。
fn push_grams(text: &str, set: &mut std::collections::HashSet<String>) {
    for g in grams_of(text) {
        set.insert(g);
    }
}

/// 事件 summary 的路径 token 正则（**进程级缓存**：每个事件调一次 extract_path，
/// 函数体内编译正则曾使万级事件会话的提取耗时 12s——实测热点）。
static PATH_TOKEN_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"[\w\-./]+/[\w\-./]+|[\w\-]+\.\w+").expect("invalid regex")
});

/// 从文本提取第一个看起来像路径的片段。
fn extract_path(s: &str) -> Option<String> {
    PATH_TOKEN_RE
        .captures(s)
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
        assert_eq!(r.extractor, "rule-v2");
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
        // 单条消息的会话：一切都在「末尾窗口」内，判 Pending
        assert!(r.todos.iter().all(|t| t.status == TodoStatus::Pending));
    }

    #[test]
    fn todos_skip_system_role_messages() {
        // 回归：TodoWrite 提醒等框架注入存为 system 角色，占真实语料 TODO 命中的 54%
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            None,
            vec![
                msg("s1", Role::System, "TODO 添加测试"),
                msg(
                    "s2",
                    Role::System,
                    "Here are the existing contents of your todo list:\nTODO fix all",
                ),
            ],
            vec![],
        ));
        assert!(r.todos.is_empty(), "system 消息不产生 TODO");
    }

    #[test]
    fn todos_skip_harness_boilerplate_in_user_messages() {
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            None,
            vec![msg(
                "m1",
                Role::User,
                "The TodoWrite tool hasn't been used recently. Also consider cleaning up the todo list",
            )],
            vec![],
        ));
        assert!(r.todos.is_empty(), "框架模板句不产生 TODO");
    }

    #[test]
    fn todo_negation_not_matched() {
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            None,
            vec![msg(
                "m1",
                Role::Assistant,
                "应用不需要额外注册，无需修改配置",
            )],
            vec![],
        ));
        assert!(r.todos.is_empty(), "「不需要/无需」不是待办");
    }

    #[test]
    fn todo_checkboxes_parsed_with_status() {
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            None,
            vec![msg(
                "m1",
                Role::Assistant,
                "- [x] 修复登录页\n- [ ] 补充测试",
            )],
            vec![],
        ));
        let done = r.todos.iter().find(|t| t.text.contains("登录页"));
        let pending = r.todos.iter().find(|t| t.text.contains("测试"));
        assert_eq!(
            done.map(|t| (t.status, t.text.contains("[x]"))),
            Some((TodoStatus::Done, false)),
            "勾选框剥离且判 Done"
        );
        assert_eq!(pending.map(|t| t.status), Some(TodoStatus::Pending));
    }

    #[test]
    fn todo_stale_when_stated_early_and_passed() {
        let ext = RuleExtractor::new();
        let filler: Vec<Message> = (0..8)
            .map(|i| msg(&format!("f{i}"), Role::Assistant, "普通讨论内容"))
            .collect();
        let mut msgs = vec![msg("m1", Role::Assistant, "接下来实现分页功能")];
        msgs.extend(filler);
        msgs.push(msg("m9", Role::Assistant, "还需要处理空状态"));
        let r = ext.extract(&input(None, msgs, vec![]));
        assert_eq!(
            r.todos
                .iter()
                .find(|t| t.text.contains("分页"))
                .map(|t| t.status),
            Some(TodoStatus::Stale),
            "会话早期的叙事计划判 Stale"
        );
        assert_eq!(
            r.todos
                .iter()
                .find(|t| t.text.contains("空状态"))
                .map(|t| t.status),
            Some(TodoStatus::Pending),
            "会话末尾的待办判 Pending"
        );
    }

    #[test]
    fn grams_handle_short_sentences() {
        // 短于窗长的句子（「✅」）不得越界
        let mut set = std::collections::HashSet::new();
        push_grams("✅", &mut set);
        push_grams("已完成", &mut set);
        assert!(set.is_empty(), "无完整 gram 可提取");
        push_grams("分页溢出已修复完成", &mut set);
        assert!(
            grams_of("接下来修复分页溢出").any(|g| set.contains(&g)),
            "正常长度句仍可命中"
        );
    }

    #[test]
    fn todo_done_by_later_evidence() {
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            None,
            vec![
                msg("m1", Role::Assistant, "需要修复分页边界的溢出问题"),
                msg("m2", Role::Assistant, "分页边界的溢出问题已修复，测试通过"),
            ],
            vec![],
        ));
        assert_eq!(
            r.todos.first().map(|t| t.status),
            Some(TodoStatus::Done),
            "后文出现同主题完成措辞 → Done"
        );
    }

    #[test]
    fn todo_own_done_marker_wins() {
        let ext = RuleExtractor::new();
        let r = ext.extract(&input(
            None,
            vec![msg("m1", Role::Assistant, "脱敏小节需要重编号，已完成")],
            vec![],
        ));
        assert_eq!(r.todos.first().map(|t| t.status), Some(TodoStatus::Done));
    }

    #[test]
    fn todo_old_json_without_status_deserializes_as_pending() {
        // rule-v1 落库的 JSON 无 status 字段，读取回退 Pending
        let old = r#"{"text":"TODO 修复 bug","source_message_ids":["m1"]}"#;
        let item: TodoItem = serde_json::from_str(old).expect("parse failed");
        assert_eq!(item.status, TodoStatus::Pending);
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

    #[test]
    fn summarize_long_text_not_truncated() {
        // 回归：摘要全文输出（旧实现 200 字符截断产生「…」）
        let long: String = "长".repeat(500);
        let input = ExtractionInput {
            title: Some("主题".into()),
            messages: vec![ch_domain::Message {
                content_text: Some(long.clone()),
                ..ch_domain::Message::new("c1", ch_domain::Role::User, 1)
            }],
            events: vec![],
        };
        let r = RuleExtractor::new().extract(&input);
        assert!(r.summary.contains(&long), "用户消息必须全文进入摘要");
        assert!(!r.summary.contains('…'), "不得再出现截断省略号");
        let tail = "结尾标记".to_string();
        let input2 = ExtractionInput {
            title: None,
            messages: vec![ch_domain::Message {
                content_text: Some(format!("{long}{tail}")),
                ..ch_domain::Message::new("c1", ch_domain::Role::User, 1)
            }],
            events: vec![],
        };
        let r2 = RuleExtractor::new().extract(&input2);
        assert!(
            r2.summary.contains("结尾标记"),
            "500 字后的尾部内容必须保留"
        );
    }
}
