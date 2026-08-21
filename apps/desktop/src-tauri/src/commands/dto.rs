//! 前端 DTO 与领域对象 → DTO 的转换（跨域共享）。

use ch_domain::Workspace;

#[derive(serde::Serialize)]
pub struct WorkspaceDto {
    pub id: String,
    pub display_name: String,
    pub user_title: Option<String>,
    pub status: String,
    /// Unix 毫秒
    pub created_at_ms: Option<i64>,
    /// Unix 毫秒
    pub updated_at_ms: Option<i64>,
}

#[derive(serde::Serialize)]
pub struct ConversationDto {
    pub favorite: bool,
    pub archived: bool,
    pub id: String,
    pub provider: String,
    pub source_conversation_id: String,
    pub title: Option<String>,
    pub user_title: Option<String>,
    pub status: Option<String>,
    pub model: Option<String>,
    pub completeness_score: Option<f64>,
    pub workspace_id: Option<String>,
    /// Unix 毫秒
    pub started_at_ms: Option<i64>,
    /// Unix 毫秒
    pub updated_at_ms: Option<i64>,
    /// 来源侧父会话 ID（None=顶层主任务）
    pub source_parent_id: Option<String>,
    /// 子任务数量
    pub child_count: i64,
}

#[derive(serde::Serialize)]
pub struct MessageDto {
    pub id: String,
    pub role: String,
    pub content_text: Option<String>,
    pub sequence_number: i64,
    /// Unix 毫秒
    pub created_at_ms: Option<i64>,
}

#[derive(serde::Serialize)]
pub struct EventDto {
    pub created_at_ms: Option<i64>,
    pub id: String,
    pub event_type: String,
    pub summary: Option<String>,
    pub sequence_number: i64,
    /// 事件状态（completed/failed 等，若有）。
    pub status: Option<String>,
    /// 完成时间（Unix 毫秒；命令/工具类事件有，可与 created_at 相减得耗时）。
    pub completed_at_ms: Option<i64>,
    /// payload JSON 字符串（事件详情：命令输出、diff 摘要等；超 8KB 截断）。
    pub payload_json: Option<String>,
}

/// 会话完整详情：消息 + 事件（plan §6.4 回溯修改过程）。
#[derive(serde::Serialize)]
pub struct ConversationDetailDto {
    pub tags: Vec<String>,
    pub conversation: ConversationDto,
    pub messages: Vec<MessageDto>,
    pub events: Vec<EventDto>,
    /// 完整度档位标签（plan §17.3：完整/部分/有限）。
    pub completeness_label: String,
}

#[derive(serde::Serialize)]
pub struct SearchResultDto {
    pub message_id: String,
    pub conversation_id: String,
    pub provider: String,
    pub role: String,
    pub title: Option<String>,
    /// 带 <b> 高亮标签的命中片段（前端 dangerouslySetInnerHTML 渲染）。
    pub snippet: String,
}

/// 搜索命中按「主对话」分组的聚合行（GUI 搜索模式左栏）。
/// 子任务命中折叠到所属主对话之下：root_* 是主对话信息，
/// conversation_id / title 是实际命中的会话（is_child=true 时为子任务）。
#[derive(serde::Serialize)]
pub struct SearchHitGroupDto {
    pub root_conversation_id: String,
    pub root_title: Option<String>,
    pub root_updated_at_ms: Option<i64>,
    pub provider: String,
    pub conversation_id: String,
    pub title: Option<String>,
    pub is_child: bool,
    pub hit_count: i64,
    /// 最佳命中（引擎相关序第一条）定位 + 片段。
    pub best_message_id: String,
    pub best_role: String,
    pub snippet: String,
}

#[derive(serde::Serialize)]
pub(crate) struct ImportResultDto {
    pub(crate) conversation_id: String,
    pub(crate) workspace_id: Option<String>,
    pub(crate) messages: usize,
    pub(crate) events: usize,
    pub(crate) completeness: String,
}

#[derive(serde::Serialize)]
pub(crate) struct SourceSessionDto {
    pub(crate) session_id: String,
    pub(crate) title: String,
    pub(crate) detail: String,
    pub(crate) message_count: Option<i64>,
    /// 已导入标记（一次 HashSet 查询批量判定，非逐条）。
    pub(crate) imported: bool,
}

/// 风险调用 DTO：ts 转毫秒数（此前直接序列化 OffsetDateTime → 前端 Invalid Date）。
#[derive(serde::Serialize)]
pub(crate) struct RiskyCallDto {
    pub(crate) id: String,
    pub(crate) provider: String,
    pub(crate) source_session_id: String,
    pub(crate) tool_name: String,
    /// Unix 毫秒
    pub(crate) ts_ms: i64,
    pub(crate) read_only: Option<bool>,
    pub(crate) destructive: Option<bool>,
    pub(crate) approval_status: Option<String>,
    pub(crate) exit_code: Option<i64>,
    pub(crate) duration_ms: Option<i64>,
    pub(crate) status: String,
    pub(crate) command_text: Option<String>,
}

/// 导出单条会话为 Markdown 或 JSON 字符串（plan §6.6）。
#[derive(serde::Serialize)]
pub(crate) struct ExportOutput {
    pub(crate) content: String,
    pub(crate) format: String,
    pub(crate) filename: String,
}

pub(crate) fn workspace_dto(ws: Workspace) -> WorkspaceDto {
    WorkspaceDto {
        id: ws.id,
        display_name: ws.display_name,
        user_title: ws.user_title,
        status: ws.status.as_str().to_string(),
        created_at_ms: ts_to_ms(Some(ws.created_at)),
        updated_at_ms: ts_to_ms(Some(ws.updated_at)),
    }
}

pub(crate) fn ts_to_ms(ts: Option<ch_domain::Timestamp>) -> Option<i64> {
    ts.map(|t| (t - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64)
}

pub(crate) fn conversation_dto(
    c: ch_domain::Conversation,
    child_count: i64,
    flags: (bool, bool),
) -> ConversationDto {
    ConversationDto {
        favorite: flags.0,
        archived: flags.1,
        id: c.id,
        provider: c.provider.to_string(),
        source_conversation_id: c.source_conversation_id,
        title: c.title,
        user_title: c.user_title,
        status: c.status.map(|s| s.as_str().to_string()),
        model: c.model,
        completeness_score: c.completeness_score,
        workspace_id: c.workspace_id,
        started_at_ms: ts_to_ms(c.started_at),
        updated_at_ms: ts_to_ms(c.updated_at),
        source_parent_id: c.source_parent_id,
        child_count,
    }
}

pub(crate) fn message_dto(m: ch_domain::Message) -> MessageDto {
    MessageDto {
        id: m.id,
        role: m.role.to_string(),
        content_text: m.content_text,
        sequence_number: m.sequence_number,
        created_at_ms: ts_to_ms(m.created_at),
    }
}

/// payload JSON 序列化上限：超出截断并标注（防大 diff/输出把详情弹窗撑爆）。
const PAYLOAD_MAX_CHARS: usize = 8_192;

pub(crate) fn event_dto(e: ch_domain::Event) -> EventDto {
    let payload_json = e.payload_json.map(|v| {
        let s = v.to_string();
        if s.chars().count() > PAYLOAD_MAX_CHARS {
            let cut: String = s.chars().take(PAYLOAD_MAX_CHARS).collect();
            format!("{cut}…（已截断）")
        } else {
            s
        }
    });
    EventDto {
        created_at_ms: ts_to_ms(e.created_at),
        id: e.id,
        event_type: e.event_type.to_string(),
        summary: e.summary,
        sequence_number: e.sequence_number,
        status: e.status.map(|s| format!("{s:?}").to_lowercase()),
        completed_at_ms: ts_to_ms(e.completed_at),
        payload_json,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_domain::{Event, EventType};

    #[test]
    fn event_dto_maps_detail_fields() {
        let mut e = Event::new("c1", EventType::CommandStarted, 5);
        e.summary = Some("cargo build".into());
        e.status = Some(ch_domain::Status::Completed);
        e.payload_json = Some(serde_json::json!({"exit_code": 0}));
        e.created_at = Some(time::OffsetDateTime::from_unix_timestamp(1_000).expect("ts"));
        e.completed_at = Some(time::OffsetDateTime::from_unix_timestamp(53_000).expect("ts"));
        let dto = event_dto(e);
        assert_eq!(dto.status.as_deref(), Some("completed"));
        assert_eq!(dto.completed_at_ms, Some(53_000_000));
        assert_eq!(dto.created_at_ms, Some(1_000_000));
        assert!(dto.payload_json.as_deref().unwrap().contains("exit_code"));
    }

    #[test]
    fn event_dto_truncates_huge_payload() {
        let mut e = Event::new("c1", EventType::DiffGenerated, 1);
        let big = "x".repeat(20_000);
        e.payload_json = Some(serde_json::json!({ "diff": big }));
        let dto = event_dto(e);
        let p = dto.payload_json.expect("payload");
        assert!(p.len() < 12_000, "超限 payload 必须截断：{} 字符", p.len());
        assert!(p.ends_with("（已截断）"));
    }

    #[test]
    fn event_dto_none_fields_stay_none() {
        let e = Event::new("c1", EventType::Error, 1);
        let dto = event_dto(e);
        assert!(dto.status.is_none());
        assert!(dto.completed_at_ms.is_none());
        assert!(dto.payload_json.is_none());
    }
}
