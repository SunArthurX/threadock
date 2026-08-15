//! 会话浏览域：工作区/会话/消息/事件/详情/知识提取/搜索/收藏标签。

use super::*;
use ch_daemon::DaemonState;

#[tauri::command]
pub(crate) async fn list_workspaces(
    state: tauri::State<'_, DaemonState>,
) -> Result<Vec<WorkspaceDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    // 时间取自对话真实时间，按对话最新更新时间倒序
    let ws = repo
        .list_workspaces_by_conv_time()
        .map_err(|e| storage_err(e))?;
    Ok(ws.into_iter().map(workspace_dto).collect())
}

#[tauri::command]
pub(crate) async fn list_conversations(
    state: tauri::State<'_, DaemonState>,
    workspace_id: Option<String>,
    provider: Option<String>,
) -> Result<Vec<ConversationDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let convs = repo
        .list_conversations(workspace_id.as_deref())
        .map_err(|e| storage_err(e))?;
    // 子任务数一次 GROUP BY 全取（旧实现每会话一次 count_children = 1+N 查询）
    let child_counts = repo.child_counts_bulk().map_err(|e| storage_err(e))?;
    // 只返回顶层主任务（source_parent_id 为空），并统计各自子任务数
    let dtos = convs
        .into_iter()
        .filter(|c| c.source_parent_id.is_none())
        .filter(|c| {
            // 按 provider 筛选（标签筛选栏）
            match &provider {
                Some(p) => c.provider.as_str() == p,
                None => true,
            }
        })
        .map(|c| {
            let provider_id = format!("prov_{}", c.provider.as_str());
            let child_count = child_counts
                .get(&(c.source_conversation_id.clone(), provider_id))
                .copied()
                .unwrap_or(0);
            conversation_dto(c, child_count)
        })
        .collect();
    Ok(dtos)
}

/// 列出指定父会话的子任务（中栏展开时调用）。
#[tauri::command]
pub(crate) async fn list_child_conversations(
    state: tauri::State<'_, DaemonState>,
    parent_source_id: String,
    provider: String,
) -> Result<Vec<ConversationDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let provider_id = format!("prov_{}", provider);
    let convs = repo
        .list_child_conversations(&parent_source_id, &provider_id)
        .map_err(|e| storage_err(e))?;
    Ok(convs.into_iter().map(|c| conversation_dto(c, 0)).collect())
}

/// 请求取消当前正在进行的同步（对话/指标），无进行中的同步也无副作用。
#[tauri::command]
pub(crate) fn cancel_sync() -> Result<(), String> {
    CANCEL_SYNC.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// 重置所有数据（清空 conversations/workspaces + 索引 + raw blobs）。
/// 保留 schema 和用户自定义脱敏规则。
/// 若已有重置/同步在进行中，返回错误提示前端。
#[tauri::command]
pub(crate) async fn reset_all_data(state: tauri::State<'_, DaemonState>) -> Result<(), String> {
    let _guard = BusyGuard::acquire()?;
    let result = state.wipe_all().map_err(|e| storage_err(e));
    // 重置后 ops 节流标记必须清零，否则治理页 5 分钟内拿不到新数据
    LAST_OPS_SYNC_MS.store(0, std::sync::atomic::Ordering::SeqCst);
    result
}

#[tauri::command]
pub(crate) async fn list_messages(
    state: tauri::State<'_, DaemonState>,
    conversation_id: String,
) -> Result<Vec<MessageDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let msgs = repo
        .list_messages(&conversation_id)
        .map_err(|e| storage_err(e))?;
    Ok(msgs.into_iter().map(message_dto).collect())
}

#[tauri::command]
pub(crate) async fn list_events(
    state: tauri::State<'_, DaemonState>,
    conversation_id: String,
) -> Result<Vec<EventDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let events = repo
        .list_events(&conversation_id)
        .map_err(|e| storage_err(e))?;
    Ok(events.into_iter().map(event_dto).collect())
}

/// 获取会话完整详情（消息 + 事件 + 完整度，plan §6.4）。
#[tauri::command]
#[tracing::instrument(skip_all, level = "debug")]
pub(crate) async fn get_conversation_detail(
    state: tauri::State<'_, DaemonState>,
    conversation_id: String,
) -> Result<ConversationDetailDto, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let conv = repo
        .get_conversation(&conversation_id)
        .map_err(|e| storage_err(e))?
        .ok_or_else(|| format!("conversation not found: {conversation_id}"))?;
    let messages = repo
        .list_messages(&conversation_id)
        .map_err(|e| storage_err(e))?;
    let events = repo
        .list_events(&conversation_id)
        .map_err(|e| storage_err(e))?;
    let score = conv.completeness_score.unwrap_or(0.0);
    let label = if score >= 0.9 {
        "完整"
    } else if score >= 0.5 {
        "部分"
    } else {
        "有限"
    };
    let provider_id = format!("prov_{}", conv.provider.as_str());
    let child_count = repo
        .count_children(&conv.source_conversation_id, &provider_id)
        .unwrap_or(0);
    Ok(ConversationDetailDto {
        conversation: conversation_dto(conv, child_count),
        messages: messages.into_iter().map(message_dto).collect(),
        events: events.into_iter().map(event_dto).collect(),
        completeness_label: label.to_string(),
    })
}

/// 知识提取（plan §13.5）：返回 ExtractionResult 结构给前端。
#[tauri::command]
pub(crate) async fn extract_knowledge(
    state: tauri::State<'_, DaemonState>,
    conversation_id: String,
) -> Result<ch_knowledge::ExtractionResult, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let conv = repo
        .get_conversation(&conversation_id)
        .map_err(|e| storage_err(e))?
        .ok_or_else(|| format!("conversation not found: {conversation_id}"))?;
    let messages = repo
        .list_messages(&conversation_id)
        .map_err(|e| storage_err(e))?;
    let events = repo
        .list_events(&conversation_id)
        .map_err(|e| storage_err(e))?;
    let input = ch_knowledge::ExtractionInput {
        title: Some(conv.effective_title().to_string()),
        messages,
        events,
    };
    Ok(ch_knowledge::RuleExtractor::new().extract(&input))
}

#[tauri::command]
#[tracing::instrument(skip_all, level = "debug")]
pub(crate) async fn search(
    state: tauri::State<'_, DaemonState>,
    query: String,
) -> Result<Vec<SearchResultDto>, String> {
    // 优先走 Tantivy（plan §9.5 主检索），降级 FTS5
    let idx = state.search_index.lock().map_err(|e| search_err(e))?;
    let q = ch_search::SearchQuery::new(&query);
    match idx.search(&q) {
        Ok(hits) if !hits.is_empty() => Ok(hits
            .into_iter()
            .map(|h| SearchResultDto {
                message_id: h.message_id,
                conversation_id: h.conversation_id,
                provider: h.provider.to_string(),
                role: h.role.to_string(),
                title: h.title,
                snippet: h.snippet,
            })
            .collect()),
        _ => {
            // 降级 FTS5
            drop(idx);
            let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
            let q = ch_storage::SearchQuery::new(&query);
            let results = repo.search(&q).map_err(|e| storage_err(e))?;
            Ok(results.into_iter().map(search_result_dto).collect())
        }
    }
}

/// 按 provider + source_conversation_id 精确查会话（审计命中跳转用，含子任务）。
#[tauri::command]
pub(crate) async fn get_conversation_by_source(
    state: tauri::State<'_, DaemonState>,
    provider: String,
    source_conversation_id: String,
) -> Result<Option<ConversationDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let provider_id = format!("prov_{provider}");
    let conn_row = repo
        .find_conversation_by_source(&provider_id, &source_conversation_id)
        .map_err(|e| storage_err(e))?;
    Ok(conn_row.map(|c| conversation_dto(c, 0)))
}

#[tauri::command]
pub(crate) async fn set_favorite(
    state: tauri::State<'_, DaemonState>,
    id: String,
    favorite: bool,
) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    repo.set_favorite(&id, favorite).map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn add_tag(
    state: tauri::State<'_, DaemonState>,
    id: String,
    tag: String,
) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    repo.add_tag(&id, &tag).map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn list_tags(
    state: tauri::State<'_, DaemonState>,
    id: String,
) -> Result<Vec<String>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.list_tags(&id).map_err(|e| storage_err(e))
}
