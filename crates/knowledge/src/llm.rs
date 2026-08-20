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

use crate::model::{Decision, ErrorItem, ExtractionInput, ExtractionResult, FileRef, TodoItem};

/// Prompt 版本（随 `extractor` 字段落库，plan §13.5）。
pub const PROMPT_VERSION: &str = "prompt-v1";
/// 每类条目上限（防模型超长输出拖垮 UI）。
const MAX_ITEMS: usize = 50;
/// 摘要字符上限。
const MAX_SUMMARY_CHARS: usize = 2000;
/// 单次请求生成 token 上限。
const MAX_OUTPUT_TOKENS: u32 = 2048;

/// system prompt：严格 JSON 输出契约（source 引用转录编号）。
const SYSTEM_PROMPT: &str = r#"你是严格的会话纪要提取器。从「用户 ↔ AI 助手」的编号对话转录中提取结构化知识。
只输出一个 JSON 对象，不要输出任何其他文字、解释或代码围栏。schema：
{"summary": string,
 "decisions": [{"decision": string, "reason": string|null, "source": [int]}],
 "todos": [{"text": string, "source": [int]}],
 "errors": [{"error": string, "solution": string|null, "source": [int]}],
 "commands": [string],
 "files": [{"path": string, "source": [int]}]}
规则：
- source 是转录中的消息编号数组（如 [1,3]），只引用真实依据，没有依据给 []。
- 不编造：没有的内容对应字段给空数组或空串。
- summary 概括会话主题与结论，不超过 500 字。
- decisions/todos/errors 各选最重要的，不超过 20 条；commands/files 去重。"#;

/// LLM 提取器（传输注入，无状态）。
pub struct LlmExtractor {
    chat: Arc<dyn Chat>,
    model_label: String,
    max_input_chars: usize,
}

impl LlmExtractor {
    /// `model_label` 会记入 `extractor` 字段（`llm:{label}@prompt-v1`）。
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

/// 编号转录：`[n] role: text`，返回 (转录文本, 编号→消息 id)。
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
        let Some(text) = m.content_text.as_deref() else {
            continue;
        };
        let t = text.trim();
        if t.is_empty() {
            continue;
        }
        ids.push(m.id.clone());
        let role = match m.role {
            ch_domain::Role::User => "user",
            ch_domain::Role::Assistant => "assistant",
            _ => "other",
        };
        let _ = writeln!(out, "[{}] {}: {}", ids.len(), role, t);
    }
    if out.chars().count() > max_chars {
        let truncated: String = out.chars().take(max_chars).collect();
        out = truncated;
        out.push_str("\n…（转录过长，已截断）");
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
        assert_eq!(r.extractor, "llm:mock-model@prompt-v1");
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
        assert_eq!(r.extractor, "llm:mock-model@prompt-v1");
        assert!(chat.last_user_prompt().is_empty(), "空转录不应发起模型调用");
    }

    #[test]
    fn transcript_numbers_and_truncates() {
        let long: String = "字".repeat(3000);
        let input = ExtractionInput {
            title: Some("主题".into()),
            messages: vec![msg("m1", Role::User, &long)],
            events: vec![],
        };
        let chat = MockChat::ok(FULL_JSON);
        let _ = LlmExtractor::new(chat.clone(), "m".into(), 2_000)
            .extract(&input)
            .expect("extract ok");
        let prompt = chat.last_user_prompt();
        assert!(prompt.contains("标题：主题"), "{prompt}");
        assert!(prompt.contains("[1] user: "), "{prompt}");
        assert!(prompt.contains("已截断"), "{prompt}");
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
