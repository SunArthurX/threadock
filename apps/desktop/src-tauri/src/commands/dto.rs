//! 前端 DTO 与领域对象 → DTO 的转换（跨域共享）。

use ch_domain::Workspace;
use ch_storage::SearchResult as DbSearchResult;

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
    pub id: String,
    pub event_type: String,
    pub summary: Option<String>,
    pub sequence_number: i64,
}

/// 会话完整详情：消息 + 事件（plan §6.4 回溯修改过程）。
#[derive(serde::Serialize)]
pub struct ConversationDetailDto {
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

pub(crate) fn conversation_dto(c: ch_domain::Conversation, child_count: i64) -> ConversationDto {
    ConversationDto {
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

pub(crate) fn event_dto(e: ch_domain::Event) -> EventDto {
    EventDto {
        id: e.id,
        event_type: e.event_type.to_string(),
        summary: e.summary,
        sequence_number: e.sequence_number,
    }
}

pub(crate) fn search_result_dto(r: DbSearchResult) -> SearchResultDto {
    SearchResultDto {
        message_id: r.message_id,
        conversation_id: r.conversation_id,
        provider: r.provider.to_string(),
        role: r.role.to_string(),
        title: r.title,
        snippet: r.snippet,
    }
}
