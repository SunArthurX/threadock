//! 提取输入与输出结构，严格对齐 plan §13.5 的 JSON schema。

use ch_domain::{Event, EventType, Message};
use serde::{Deserialize, Serialize};

/// 提取输入：从一条会话的消息 + 事件组装（无需完整 Conversation，减少耦合）。
#[derive(Debug, Clone)]
pub struct ExtractionInput {
    pub title: Option<String>,
    pub messages: Vec<Message>,
    pub events: Vec<Event>,
}

/// 提取结果，对应 plan §13.5 输出结构。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub summary: String,
    pub decisions: Vec<Decision>,
    pub todos: Vec<TodoItem>,
    pub errors: Vec<ErrorItem>,
    pub commands: Vec<String>,
    pub files: Vec<FileRef>,
    /// 提取引擎标识（plan §13.5：记录使用的模型/Prompt 版本）。
    pub extractor: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub decision: String,
    pub reason: Option<String>,
    pub source_message_ids: Vec<String>,
}

/// TODO 完成态（rule-v2 起提取时判定）。
///
/// - `Pending`：待办——出现在会话末尾且无完成证据；
/// - `Done`：已完成——自带完成措辞、勾选框 `[x]`，或后文有完成证据；
/// - `Stale`：过期——会话早期的「接下来/需要」叙事计划，会话推进后已被覆盖。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TodoStatus {
    #[default]
    Pending,
    Done,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TodoItem {
    pub text: String,
    /// 旧记录（rule-v1）无此字段，反序列化回退为 [`TodoStatus::Pending`]。
    #[serde(default)]
    pub status: TodoStatus,
    pub source_message_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorItem {
    pub error: String,
    pub solution: Option<String>,
    pub source_message_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileRef {
    pub path: String,
    pub source_message_ids: Vec<String>,
}

/// 用于按 `event_type` 过滤的便利方法。
#[must_use]
pub fn events_of_type(events: &[Event], t: EventType) -> Vec<&Event> {
    events.iter().filter(|e| e.event_type == t).collect()
}
