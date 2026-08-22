//! LLM 提取器：调用 OpenAI 兼容模型，输出与规则引擎同构的 [`ExtractionResult`]
//! （plan §13.5：可选本地或云模型、记录模型与 Prompt 版本、结果有来源引用）。
//!
//! 流程：编号转录（截断上限）→ system prompt（严格 JSON schema + source 编号）
//! → [`ch_llm::Chat`] 调用 → 宽松解析（剥围栏/取最外层 JSON/缺字段补默认/
//! 条目裁剪）→ source 编号映射回真实 `message.id`。
//!
//! 传输通过 [`ch_llm::Chat`] trait 注入：生产为 `HttpChat`，测试为 mock，
//! 本模块自身不做网络 IO。

use std::fmt::Write as _;
use std::sync::Arc;

use ch_llm::{Chat, ChatRequest, LlmError};

use crate::model::{
    Decision, ErrorItem, ExtractionInput, ExtractionResult, FileRef, TodoItem, TodoStatus,
};

/// Prompt 版本（随 `extractor` 字段落库，plan §13.5）。
/// v3（2026-08 用户决策）：只提炼**可复用经验**（prompt 心得、踩坑与解法、关键决策），
/// 每类 ≤5 条宁缺毋滥——v2 的全量六类提取条目多、耗时 90s+、费用高。
pub const PROMPT_VERSION: &str = "prompt-v3";
/// 每类条目上限（经验导向：少而精）。
const MAX_ITEMS: usize = 10;
/// 摘要字符上限。
const MAX_SUMMARY_CHARS: usize = 2000;
/// 单次请求生成 token 上限（经验提炼输出量小；思考型模型的思考计入，
/// 留思考余量）。计费只按实际生成算。
const MAX_OUTPUT_TOKENS: u32 = 8192;

/// system prompt：严格 JSON 输出契约（source 引用转录编号）。
const SYSTEM_PROMPT: &str = r#"你是会话经验提炼器。从「用户 ↔ AI 助手」的编号对话转录中，只提炼**可复用的经验**——这次对话教会了我们什么。
只输出一个 JSON 对象，不要输出任何其他文字、解释或代码围栏。schema：
{"summary": string,
 "decisions": [{"decision": string, "reason": string|null, "source": [int]}],
 "todos": [{"text": string, "status": "pending"|"done", "source": [int]}],
 "errors": [{"error": string, "solution": string|null, "source": [int]}],
 "commands": [string],
 "files": [{"path": string, "source": [int]}]}
提炼原则（宁缺毋滥，每类最多 5 条）：
- summary：本次对话的经验总结（≤300 字）——有效的 prompt 写法、验证过的方案选型结论、值得复用的教训；写干货，不要流水账。
- decisions：跨会话仍有参考价值的关键决策（选型/架构/取舍），不复述临时操作。
- todos：对话结束时**仍未解决**的事项；已完成的不要。
- errors：踩过的坑 + 对应解法（solution 尽量给可操作的步骤）；没有解法的普通报错不要。
- commands/files：只收对复现/修复该问题真正关键的，最多各 3 条，通常给空数组。
- 排除框架注入的提醒文字与对 todo 工具的自述；不编造，没有就给空数组/空串。"#;

/// LLM 提取器（传输注入，无状态）。
pub struct LlmExtractor {
    chat: Arc<dyn Chat>,
    model_label: String,
    max_input_chars: usize,
}

impl LlmExtractor {
    /// `model_label` 会记入 `extractor` 字段（`llm:{label}@{PROMPT_VERSION}`）。
    #[must_use]
    pub fn new(chat: Arc<dyn Chat>, model_label: String, max_input_chars: usize) -> Self {
        Self {
            chat,
            model_label,
            max_input_chars: max_input_chars.max(1_000),
        }
    }

    /// 执行提取。空转录（无文本消息）直接返回空结果，不发起调用。
    ///
    /// # Errors
    /// 网络/HTTP/解析错误透传 [`LlmError`]（Display 不含凭据）。
    pub fn extract(&self, input: &ExtractionInput) -> Result<ExtractionResult, LlmError> {
        let extractor = format!("llm:{}@{PROMPT_VERSION}", self.model_label);
        let (transcript, ids) = build_transcript(input, self.max_input_chars);
        if ids.is_empty() {
            return Ok(ExtractionResult {
                summary: String::new(),
                decisions: vec![],
                todos: vec![],
                errors: vec![],
                commands: vec![],
                files: vec![],
                extractor,
            });
        }
        let reply = self.chat.chat(&ChatRequest {
            system: SYSTEM_PROMPT.to_string(),
            user: transcript,
            max_tokens: MAX_OUTPUT_TOKENS,
            json_mode: true,
        })?;
        parse_reply(&reply.content, &ids, &extractor)
    }
}

/// 单条消息转录上限（超出掐中间保头尾）。
const PER_MSG_LIMIT: usize = 2_000;

/// 编号转录：`[n] role: text`，返回 (转录文本, 编号→消息 id)。
/// 超长截断为**保头保尾**（头部 70% + 尾部 30%）：会话的结论/收尾（TODO 清单、
/// 最终方案、错误解决）集中在尾部，纯掐头会把最有价值的部分丢掉。
fn build_transcript(input: &ExtractionInput, max_chars: usize) -> (String, Vec<String>) {
    let mut ids: Vec<String> = Vec::new();
    let mut out = String::new();
    if let Some(title) = input.title.as_deref() {
        let t = title.trim();
        if !t.is_empty() {
            out.push_str("标题：");
            out.push_str(t);
            out.push('\n');
        }
    }
    for m in &input.messages {
        // 只转录 user/assistant：system 是框架注入提醒，tool 是原始输出，
        // 对提取无价值且白占 token（大会话中占比可观）
        let role = match m.role {
            ch_domain::Role::User => "user",
            ch_domain::Role::Assistant => "assistant",
            _ => continue,
        };
        let Some(text) = m.content_text.as_deref() else {
            continue;
        };
        let t = text.trim();
        if t.is_empty() {
            continue;
        }
        // 单条消息截断（头 1500 + 尾 500）：超长转储（代码块/日志）中段无提取价值，
        // 头部是意图、尾部是结论
        let body = if t.chars().count() > PER_MSG_LIMIT {
            let total = t.chars().count();
            let head: String = t.chars().take(1_500).collect();
            let tail: String = t.chars().skip(total - 500).collect();
            format!(
                "{head}\n…（本条消息超长，中间 {} 字符已省略）…\n{tail}",
                total - 2_000
            )
        } else {
            t.to_string()
        };
        ids.push(m.id.clone());
        let _ = writeln!(out, "[{}] {}: {}", ids.len(), role, body);
    }
    if out.chars().count() > max_chars {
        let total = out.chars().count();
        let head_len = max_chars * 7 / 10;
        let tail_len = max_chars - head_len;
        let skipped = total - head_len - tail_len;
        let head: String = out.chars().take(head_len).collect();
        let tail: String = out.chars().skip(total - tail_len).collect();
        out = format!("{head}\n…（中间 {skipped} 字符已省略，以下为会话尾部）…\n{tail}");
    }
    (out, ids)
}

/// 模型输出 → ExtractionResult：剥围栏/杂文 → JSON → 宽松映射 + 裁剪。
fn parse_reply(
    content: &str,
    ids: &[String],
    extractor: &str,
) -> Result<ExtractionResult, LlmError> {
    let json_str = extract_json_object(content)
        .ok_or_else(|| LlmError::Parse("模型响应中未找到 JSON 对象".into()))?;
    let v: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| LlmError::Parse(format!("JSON 解析失败：{e}")))?;

    Ok(ExtractionResult {
        summary: parse_summary(&v),
        decisions: parse_decisions(&v, ids),
        todos: parse_todos(&v, ids),
        errors: parse_errors(&v, ids),
        commands: parse_commands(&v),
        files: parse_files(&v, ids),
        extractor: extractor.to_string(),
    })
}

/// source 编号数组 → 真实消息 id（越界/非正数忽略）。
fn map_ids(val: Option<&serde_json::Value>, ids: &[String]) -> Vec<String> {
    val.and_then(serde_json::Value::as_array)
        .map_or_else(Vec::new, |arr| {
            arr.iter()
                .filter_map(serde_json::Value::as_i64)
                .filter(|&i| i >= 1 && usize::try_from(i).is_ok_and(|u| u <= ids.len()))
                .map(|i| ids[(i - 1) as usize].clone())
                .collect()
        })
}

fn parse_summary(v: &serde_json::Value) -> String {
    v.get("summary")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .chars()
        .take(MAX_SUMMARY_CHARS)
        .collect()
}

/// 取对象数组中非空字符串字段，附带可选说明字段与 source 映射。
fn str_field<'a>(item: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    item.get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn parse_decisions(v: &serde_json::Value, ids: &[String]) -> Vec<Decision> {
    items_of(v, "decisions").map_or_else(Vec::new, |arr| {
        arr.iter()
            .take(MAX_ITEMS)
            .filter_map(|item| {
                Some(Decision {
                    decision: str_field(item, "decision")?.to_string(),
                    reason: str_field(item, "reason").map(String::from),
                    source_message_ids: map_ids(item.get("source"), ids),
                })
            })
            .collect()
    })
}

fn parse_todos(v: &serde_json::Value, ids: &[String]) -> Vec<TodoItem> {
    items_of(v, "todos").map_or_else(Vec::new, |arr| {
        arr.iter()
            .take(MAX_ITEMS)
            .filter_map(|item| {
                Some(TodoItem {
                    text: str_field(item, "text")?.to_string(),
                    status: match item.get("status").and_then(serde_json::Value::as_str) {
                        Some("done") => TodoStatus::Done,
                        _ => TodoStatus::Pending,
                    },
                    source_message_ids: map_ids(item.get("source"), ids),
                })
            })
            .collect()
    })
}

fn parse_errors(v: &serde_json::Value, ids: &[String]) -> Vec<ErrorItem> {
    items_of(v, "errors").map_or_else(Vec::new, |arr| {
        arr.iter()
            .take(MAX_ITEMS)
            .filter_map(|item| {
                Some(ErrorItem {
                    error: str_field(item, "error")?.to_string(),
                    solution: str_field(item, "solution").map(String::from),
                    source_message_ids: map_ids(item.get("source"), ids),
                })
            })
            .collect()
    })
}

fn parse_files(v: &serde_json::Value, ids: &[String]) -> Vec<FileRef> {
    items_of(v, "files").map_or_else(Vec::new, |arr| {
        arr.iter()
            .take(MAX_ITEMS)
            .filter_map(|item| {
                Some(FileRef {
                    path: str_field(item, "path")?.to_string(),
                    source_message_ids: map_ids(item.get("source"), ids),
                })
            })
            .collect()
    })
}

/// commands：数组形态为主，容错单字符串。
fn parse_commands(v: &serde_json::Value) -> Vec<String> {
    match v.get("commands") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .take(MAX_ITEMS)
            .filter_map(|c| c.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect(),
        Some(serde_json::Value::String(s)) => {
            let s = s.trim();
            if s.is_empty() {
                Vec::new()
            } else {
                vec![s.to_string()]
            }
        }
        _ => Vec::new(),
    }
}

fn items_of<'a>(v: &'a serde_json::Value, key: &str) -> Option<&'a [serde_json::Value]> {
    v.get(key)
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
}

/// 从模型输出中取 JSON 对象子串：首个 `{` 到最后一个 `}`（兼容围栏与前后杂文）。
fn extract_json_object(content: &str) -> Option<&str> {
    let start = content.find('{')?;
    let end = content.rfind('}')?;
    if end > start {
        Some(&content[start..=end])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_domain::{Message, Role};
    use std::sync::Mutex;

    fn msg(id: &str, role: Role, text: &str) -> Message {
        let mut m = Message::new("conv", role, id.parse().unwrap_or(1));
        m.id = id.into();
        m.content_text = Some(text.into());
        m
    }

    fn input_2msgs() -> ExtractionInput {
        ExtractionInput {
            title: Some("Tauri 会话".into()),
            messages: vec![
                msg("m1", Role::User, "怎么在 Tauri 里做后台任务？"),
                msg("m2", Role::Assistant, "决定使用 WorkManager，需要写测试。"),
            ],
            events: vec![],
        }
    }

    /// 记录请求的 mock：返回预设内容。
    struct MockChat {
        reply: Mutex<Result<String, LlmError>>,
        seen_request: Mutex<Vec<String>>,
    }

    impl MockChat {
        fn ok(content: &str) -> Arc<Self> {
            Arc::new(Self {
                reply: Mutex::new(Ok(content.to_string())),
                seen_request: Mutex::new(Vec::new()),
            })
        }
        fn err(e: LlmError) -> Arc<Self> {
            Arc::new(Self {
                reply: Mutex::new(Err(e)),
                seen_request: Mutex::new(Vec::new()),
            })
        }
        fn last_user_prompt(&self) -> String {
            self.seen_request
                .lock()
                .expect("mutex poisoned")
                .last()
                .cloned()
                .unwrap_or_default()
        }
    }

    impl Chat for MockChat {
        fn chat(&self, req: &ChatRequest) -> Result<ch_llm::ChatReply, LlmError> {
            self.seen_request
                .lock()
                .expect("mutex poisoned")
                .push(req.user.clone());
            let content = self.reply.lock().expect("mutex poisoned").clone()?;
            Ok(ch_llm::ChatReply {
                content,
                model: "mock-model".into(),
            })
        }
    }

    fn extractor(chat: Arc<MockChat>) -> LlmExtractor {
        LlmExtractor::new(chat, "mock-model".into(), 48_000)
    }

    const FULL_JSON: &str = r#"{"summary":"讨论 Tauri 后台任务方案",
"decisions":[{"decision":"使用 WorkManager","reason":"官方推荐","source":[2]}],
"todos":[{"text":"写测试","source":[2]}],
"errors":[{"error":"编译失败","solution":"补依赖","source":[1,2]}],
"commands":["cargo test"],
"files":[{"path":"src/main.rs","source":[1]}]}"#;

    #[test]
    fn parses_full_json_and_maps_sources() {
        let chat = MockChat::ok(FULL_JSON);
        let r = extractor(chat).extract(&input_2msgs()).expect("extract ok");
        assert_eq!(r.summary, "讨论 Tauri 后台任务方案");
        assert_eq!(r.extractor, "llm:mock-model@prompt-v3");
        assert_eq!(
            r.decisions[0].source_message_ids,
            vec!["m2".to_string()],
            "source [2] → 真实消息 id"
        );
        assert_eq!(r.errors[0].source_message_ids, vec!["m1", "m2"]);
        assert_eq!(r.files[0].source_message_ids, vec!["m1"]);
        assert_eq!(r.commands, vec!["cargo test".to_string()]);
        assert_eq!(r.decisions[0].reason.as_deref(), Some("官方推荐"));
        assert_eq!(r.errors[0].solution.as_deref(), Some("补依赖"));
    }

    #[test]
    fn strips_code_fences_and_prose() {
        let chat = MockChat::ok(&format!(
            "好的，以下是提取结果：\n```json\n{FULL_JSON}\n```\n以上。"
        ));
        let r = extractor(chat).extract(&input_2msgs()).expect("extract ok");
        assert!(!r.summary.is_empty());
        assert_eq!(r.todos.len(), 1);
    }

    #[test]
    fn garbage_reply_is_parse_error() {
        let chat = MockChat::ok("抱歉，我无法完成该任务。");
        let err = extractor(chat)
            .extract(&input_2msgs())
            .expect_err("should fail");
        assert!(matches!(err, LlmError::Parse(_)), "{err:?}");
    }

    #[test]
    fn transport_error_propagates() {
        let chat = MockChat::err(LlmError::HttpStatus {
            code: 401,
            detail: "bad key".into(),
        });
        let err = extractor(chat)
            .extract(&input_2msgs())
            .expect_err("should fail");
        assert!(matches!(err, LlmError::HttpStatus { code: 401, .. }));
    }

    #[test]
    fn out_of_range_sources_ignored() {
        let chat = MockChat::ok(r#"{"summary":"s","todos":[{"text":"t","source":[99,0,-1,1]}]}"#);
        let r = extractor(chat).extract(&input_2msgs()).expect("extract ok");
        assert_eq!(r.todos[0].source_message_ids, vec!["m1".to_string()]);
    }

    #[test]
    fn todo_status_parsed_and_defaulted() {
        let chat = MockChat::ok(
            r#"{"summary":"s","todos":[{"text":"写测试","status":"done","source":[2]},{"text":"重构","status":"pending"},{"text":"缺省态"}]}"#,
        );
        let r = extractor(chat).extract(&input_2msgs()).expect("extract ok");
        assert_eq!(r.todos[0].status, TodoStatus::Done);
        assert_eq!(r.todos[1].status, TodoStatus::Pending);
        assert_eq!(
            r.todos[2].status,
            TodoStatus::Pending,
            "无 status 默认 pending"
        );
    }

    #[test]
    fn missing_fields_defaults() {
        let chat = MockChat::ok(r#"{"summary":"只有摘要"}"#);
        let r = extractor(chat).extract(&input_2msgs()).expect("extract ok");
        assert_eq!(r.summary, "只有摘要");
        assert!(r.decisions.is_empty());
        assert!(r.commands.is_empty());
    }

    #[test]
    fn oversize_lists_truncated() {
        let todos: Vec<String> = (0..80).map(|i| format!(r#"{{"text":"t{i}"}}"#)).collect();
        let body = format!(r#"{{"summary":"s","todos":[{}]}}"#, todos.join(","));
        let chat = MockChat::ok(&body);
        let r = extractor(chat).extract(&input_2msgs()).expect("extract ok");
        assert_eq!(r.todos.len(), MAX_ITEMS);
    }

    #[test]
    fn commands_tolerates_single_string() {
        let chat = MockChat::ok(r#"{"summary":"","commands":"npm install"}"#);
        let r = extractor(chat).extract(&input_2msgs()).expect("extract ok");
        assert_eq!(r.commands, vec!["npm install".to_string()]);
    }

    #[test]
    fn empty_transcript_returns_empty_without_call() {
        let chat = MockChat::ok(FULL_JSON);
        let r = extractor(chat.clone())
            .extract(&ExtractionInput {
                title: None,
                messages: vec![],
                events: vec![],
            })
            .expect("extract ok");
        assert!(r.summary.is_empty());
        assert!(r.todos.is_empty());
        assert_eq!(r.extractor, "llm:mock-model@prompt-v3");
        assert!(chat.last_user_prompt().is_empty(), "空转录不应发起模型调用");
    }

    #[test]
    fn transcript_skips_non_user_roles_and_truncates_long_message() {
        // 输入瘦身（2026-08）：system/tool 不进转录；单条超长消息保头尾掐中间
        let long_body = format!("{}{}", "开".repeat(1800), "尾".repeat(1800)); // 3600 字符
        let mut sys = msg("s1", Role::System, "TODO 系统提醒");
        sys.role = Role::System;
        let input = ExtractionInput {
            title: None,
            messages: vec![
                sys,
                msg("m1", Role::User, &long_body),
                msg("t1", ch_domain::Role::Tool, "tool output"),
            ],
            events: vec![],
        };
        let chat = MockChat::ok(FULL_JSON);
        let _ = LlmExtractor::new(chat.clone(), "m".into(), 48_000)
            .extract(&input)
            .expect("extract ok");
        let prompt = chat.last_user_prompt();
        assert!(!prompt.contains("系统提醒"), "system 不进转录：{prompt}");
        assert!(!prompt.contains("tool output"), "tool 不进转录：{prompt}");
        assert!(
            prompt.contains("本条消息超长，中间 1600 字符已省略"),
            "{prompt}"
        );
        let head_probe: String = "开".repeat(30);
        let tail_probe: String = "尾".repeat(30);
        assert!(prompt.contains(&head_probe), "头部保留");
        assert!(prompt.contains(&tail_probe), "尾部保留");
        // 编号连续（跳过的角色不占号）
        assert!(prompt.contains("[1] user: "), "{prompt}");
        assert!(!prompt.contains("[2]"), "只应有 1 条转录消息：{prompt}");
    }

    #[test]
    fn transcript_numbers_and_truncates() {
        let head = "开".repeat(2000);
        let tail = "尾".repeat(2000);
        let input = ExtractionInput {
            title: Some("主题".into()),
            messages: vec![msg("m1", Role::User, &format!("{head}{tail}"))],
            events: vec![],
        };
        let chat = MockChat::ok(FULL_JSON);
        let _ = LlmExtractor::new(chat.clone(), "m".into(), 2_000)
            .extract(&input)
            .expect("extract ok");
        let prompt = chat.last_user_prompt();
        assert!(prompt.contains("标题：主题"), "{prompt}");
        assert!(prompt.contains("[1] user: "), "{prompt}");
        assert!(prompt.contains("已省略"), "{prompt}");
        // 保头保尾：掐中间而不是掐尾——结论集中在尾部
        let head_probe: String = head.chars().take(20).collect();
        let tail_probe: String = tail.chars().take(20).collect();
        assert!(prompt.contains(&head_probe), "头部必须保留");
        assert!(prompt.contains(&tail_probe), "尾部必须保留：{prompt}");
        let tail_full: String = tail
            .chars()
            .rev()
            .take(20)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert!(
            prompt.contains(&tail_full),
            "尾部最后内容必须在（非掐尾截断）"
        );
    }

    #[test]
    fn system_prompt_declares_schema() {
        let chat = MockChat::ok(FULL_JSON);
        let _ = extractor(chat).extract(&input_2msgs()).expect("extract ok");
        // （MockChat 只记录 user；system 为常量，这里静态断言其存在关键契约词）
        assert!(SYSTEM_PROMPT.contains("\"source\": [int]"));
        assert!(SYSTEM_PROMPT.contains("不要输出任何其他文字"));
    }
}
