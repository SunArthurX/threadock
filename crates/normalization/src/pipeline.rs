//! 标准化流水线，对应 plan §8.3「数据处理流水线」的后半段：
//! Normalize → Resolve → (由 storage 负责事务写入) → Index → Cursor。
//!
//! 本 crate 不直接写数据库，只产出可入库的领域对象 + 元数据。

use crate::completeness::{completeness_score, grade, Completeness};
use crate::hash;
use ch_domain::{Conversation, Event, EventType, Message, Provider, Role, Timestamp};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;

pub type NormalizationResult<T> = std::result::Result<T, NormalizationError>;

#[derive(Debug, Error)]
pub enum NormalizationError {
    #[error("raw conversation has no messages")]
    NoMessages,
    #[error("invalid data: {0}")]
    Invalid(String),
}

/// Adapter 解析出的「原始」会话——已经接近领域模型，但尚未计算 hash / 完整度。
///
/// 这是 Adapter 与 Normalization 之间的契约：Adapter 只负责把来源格式
/// 解析成这个结构，不计算 hash、不评估完整度。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawConversation {
    pub provider: Provider,
    pub source_conversation_id: String,
    pub title: Option<String>,
    pub model: Option<String>,
    pub started_at: Option<Timestamp>,
    pub messages: Vec<RawMessage>,
    pub events: Vec<RawEvent>,
    /// 来源侧父会话 ID（主子任务，None=顶层主任务）。
    pub source_parent_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawMessage {
    pub role: Role,
    pub text: Option<String>,
    pub content_json: Option<serde_json::Value>,
    pub source_message_id: Option<String>,
    pub created_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawEvent {
    pub event_type: EventType,
    pub summary: Option<String>,
    pub payload_json: Option<serde_json::Value>,
    pub source_event_id: Option<String>,
    pub created_at: Option<Timestamp>,
}

/// 标准化结果：可直接送入 Repository 的对象集合 + 元信息。
#[derive(Debug, Clone)]
pub struct Normalized {
    pub conversation: Conversation,
    pub messages: Vec<Message>,
    pub events: Vec<Event>,
    pub conversation_hash: String,
    pub completeness: Completeness,
    pub completeness_score: f64,
}

/// 把 `RawConversation` 标准化为 Normalized。
///
/// 步骤（对齐 plan §11.2 流水线后半段）：
/// 1. 校验至少有一条消息。
/// 2. 为每条消息分配序号并计算 hash（plan §11.3）。
/// 3. 为 conversation 计算 hash（汇总所有消息 hash）。
/// 4. 评估完整度（plan §17.3）。
/// 5. 分配事件序号并归类。
#[allow(clippy::needless_pass_by_value)] // 管线语义：raw 为一次性输入，按值交接所有权
pub fn normalize(raw: RawConversation) -> NormalizationResult<Normalized> {
    if raw.messages.is_empty() {
        return Err(NormalizationError::NoMessages);
    }

    let now = OffsetDateTime::now_utc();

    // 1. 构造 conversation 骨架
    let mut conversation = Conversation::new(raw.provider, raw.source_conversation_id.clone());
    conversation.title.clone_from(&raw.title);
    conversation.model.clone_from(&raw.model);
    conversation.started_at = raw.started_at;
    conversation
        .source_parent_id
        .clone_from(&raw.source_parent_id);
    // updated_at 取最后一条消息的时间（更准确反映对话更新时刻）
    let last_msg_time = raw.messages.iter().rev().find_map(|m| m.created_at);
    conversation.updated_at = Some(last_msg_time.or(raw.started_at).unwrap_or(now));

    // 2. 消息：分配序号 + hash
    let mut messages = Vec::with_capacity(raw.messages.len());
    let mut message_hashes: Vec<String> = Vec::with_capacity(raw.messages.len());
    for (idx, rm) in raw.messages.iter().enumerate() {
        let mut m = Message::new(&conversation.id, rm.role, (idx as i64) + 1);
        m.content_text.clone_from(&rm.text);
        m.content_json.clone_from(&rm.content_json);
        m.source_message_id.clone_from(&rm.source_message_id);
        m.created_at = rm.created_at.or(Some(now));
        let h = hash::content_hash_for_message(
            raw.provider,
            &raw.source_conversation_id,
            rm.role,
            rm.text.as_deref().unwrap_or(""),
            rm.content_json.as_ref(),
        );
        m.content_hash = Some(h.clone());
        message_hashes.push(h);
        messages.push(m);
    }

    // 3. conversation hash
    let hash_refs: Vec<&str> = message_hashes
        .iter()
        .map(std::string::String::as_str)
        .collect();
    let conversation_hash =
        hash::content_hash_for_conversation(raw.provider, &raw.source_conversation_id, &hash_refs);
    conversation.content_hash = Some(conversation_hash.clone());

    // 4. 事件
    let mut events = Vec::with_capacity(raw.events.len());
    for (idx, re) in raw.events.iter().enumerate() {
        let mut e = Event::new(&conversation.id, re.event_type, (idx as i64) + 1);
        e.summary.clone_from(&re.summary);
        e.payload_json.clone_from(&re.payload_json);
        e.source_event_id.clone_from(&re.source_event_id);
        e.created_at = re.created_at.or(Some(now));
        events.push(e);
    }

    // 5. 完整度
    let has_tools = events.iter().any(|e| {
        matches!(
            e.event_type,
            EventType::ToolCallStarted | EventType::ToolCallCompleted
        )
    });
    let has_diffs = events
        .iter()
        .any(|e| matches!(e.event_type, EventType::DiffGenerated));
    let has_commands = events.iter().any(|e| {
        matches!(
            e.event_type,
            EventType::CommandStarted | EventType::CommandCompleted
        )
    });
    let has_approvals = events.iter().any(|e| {
        matches!(
            e.event_type,
            EventType::ApprovalRequested | EventType::ApprovalGranted | EventType::ApprovalDenied
        )
    });
    let score = completeness_score(true, has_tools, has_diffs, has_commands, has_approvals);
    conversation.completeness_score = Some(score);
    let completeness = grade(score);

    Ok(Normalized {
        conversation,
        messages,
        events,
        conversation_hash,
        completeness,
        completeness_score: score,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_domain::Provider;

    fn sample_raw() -> RawConversation {
        RawConversation {
            provider: Provider::Codex,
            source_conversation_id: "src-1".into(),
            title: Some("test".into()),
            model: Some("gpt-test".into()),
            started_at: None,
            messages: vec![
                RawMessage {
                    role: Role::User,
                    text: Some("hello".into()),
                    content_json: None,
                    source_message_id: Some("m1".into()),
                    created_at: None,
                },
                RawMessage {
                    role: Role::Assistant,
                    text: Some("hi there".into()),
                    content_json: None,
                    source_message_id: Some("m2".into()),
                    created_at: None,
                },
            ],
            events: vec![],
            source_parent_id: None,
        }
    }

    #[test]
    fn normalize_assigns_sequence_numbers() {
        let n = normalize(sample_raw()).expect("unexpected None");
        assert_eq!(n.messages.len(), 2);
        assert_eq!(n.messages[0].sequence_number, 1);
        assert_eq!(n.messages[1].sequence_number, 2);
    }

    #[test]
    fn normalize_computes_message_hashes() {
        let n = normalize(sample_raw()).expect("unexpected None");
        for m in &n.messages {
            assert!(m.content_hash.is_some(), "every message should have a hash");
        }
    }

    #[test]
    fn normalize_computes_conversation_hash() {
        let n = normalize(sample_raw()).expect("unexpected None");
        assert!(!n.conversation_hash.is_empty());
        assert_eq!(
            n.conversation.content_hash.as_deref(),
            Some(n.conversation_hash.as_str())
        );
    }

    #[test]
    fn normalize_deterministic_hash() {
        // 相同输入两次标准化，hash 必须相同
        let n1 = normalize(sample_raw()).expect("unexpected None");
        let n2 = normalize(sample_raw()).expect("unexpected None");
        assert_eq!(n1.conversation_hash, n2.conversation_hash);
        for (a, b) in n1.messages.iter().zip(n2.messages.iter()) {
            assert_eq!(a.content_hash, b.content_hash);
        }
    }

    #[test]
    fn normalize_rejects_empty() {
        let raw = RawConversation {
            provider: Provider::Codex,
            source_conversation_id: "src-x".into(),
            title: None,
            model: None,
            started_at: None,
            messages: vec![],
            events: vec![],
            source_parent_id: None,
        };
        assert!(matches!(
            normalize(raw),
            Err(NormalizationError::NoMessages)
        ));
    }

    #[test]
    fn completeness_reflects_events() {
        // 无事件 → 有限
        let n = normalize(sample_raw()).expect("unexpected None");
        assert_eq!(n.completeness, Completeness::Limited);

        // 加一个 command → 部分
        let mut raw = sample_raw();
        raw.events.push(RawEvent {
            event_type: EventType::CommandCompleted,
            summary: Some("cargo build".into()),
            payload_json: None,
            source_event_id: None,
            created_at: None,
        });
        let n2 = normalize(raw).expect("unexpected None");
        assert_eq!(n2.completeness, Completeness::Partial);

        // tool + diff + command + approval → 完整
        let mut raw = sample_raw();
        raw.events = vec![
            RawEvent {
                event_type: EventType::ToolCallStarted,
                summary: None,
                payload_json: None,
                source_event_id: None,
                created_at: None,
            },
            RawEvent {
                event_type: EventType::DiffGenerated,
                summary: None,
                payload_json: None,
                source_event_id: None,
                created_at: None,
            },
            RawEvent {
                event_type: EventType::CommandCompleted,
                summary: None,
                payload_json: None,
                source_event_id: None,
                created_at: None,
            },
            RawEvent {
                event_type: EventType::ApprovalGranted,
                summary: None,
                payload_json: None,
                source_event_id: None,
                created_at: None,
            },
        ];
        let n3 = normalize(raw).expect("unexpected None");
        assert_eq!(n3.completeness, Completeness::Full);
        assert!((n3.completeness_score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn events_get_sequence_numbers_starting_at_1() {
        let mut raw = sample_raw();
        raw.events = vec![
            RawEvent {
                event_type: EventType::CommandStarted,
                summary: None,
                payload_json: None,
                source_event_id: None,
                created_at: None,
            },
            RawEvent {
                event_type: EventType::CommandCompleted,
                summary: None,
                payload_json: None,
                source_event_id: None,
                created_at: None,
            },
        ];
        let n = normalize(raw).expect("unexpected None");
        assert_eq!(n.events.len(), 2);
        assert_eq!(n.events[0].sequence_number, 1);
        assert_eq!(n.events[1].sequence_number, 2);
    }

    #[test]
    fn conversation_carries_provider_and_source_id() {
        let n = normalize(sample_raw()).expect("unexpected None");
        assert_eq!(n.conversation.provider, Provider::Codex);
        assert_eq!(n.conversation.source_conversation_id, "src-1");
    }
}
