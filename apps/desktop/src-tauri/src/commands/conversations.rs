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
    favorite: Option<bool>,
    archived: Option<bool>,
    include_deleted: Option<bool>,
) -> Result<Vec<ConversationDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    // 过滤维度走 ConversationFilter（收藏/归档/删除）；provider 仍按内存过滤（标签栏）
    let filter = ch_storage::ConversationFilter {
        workspace_id: workspace_id.clone(),
        favorite,
        archived,
        deleted: match include_deleted {
            Some(true) => None,                // 含已删除
            Some(false) | None => Some(false), // 默认排除已删除
        },
        provider: None,
    };
    let convs = repo
        .list_conversations_filtered(&filter)
        .map_err(|e| storage_err(e))?;
    // 子任务数一次 GROUP BY 全取（旧实现每会话一次 count_children = 1+N 查询）
    let child_counts = repo.child_counts_bulk().map_err(|e| storage_err(e))?;
    let flags = repo.conversation_flags_bulk().map_err(|e| storage_err(e))?;
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
            let f = flags.get(&c.id).copied().unwrap_or((false, false));
            conversation_dto(c, child_count, f)
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
    let flags = repo.conversation_flags_bulk().map_err(|e| storage_err(e))?;
    Ok(convs
        .into_iter()
        .map(|c| {
            let f = flags.get(&c.id).copied().unwrap_or((false, false));
            conversation_dto(c, 0, f)
        })
        .collect())
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
    let tags = repo.list_tags(&conversation_id).unwrap_or_default();
    let flags = repo
        .get_conversation_flags(&conversation_id)
        .unwrap_or((false, false));
    Ok(ConversationDetailDto {
        tags,
        conversation: conversation_dto(conv, child_count, flags),
        messages: messages.into_iter().map(message_dto).collect(),
        events: events.into_iter().map(event_dto).collect(),
        completeness_label: label.to_string(),
    })
}

/// 归档/取消归档。
#[tauri::command]
pub(crate) async fn set_archived(
    state: tauri::State<'_, DaemonState>,
    id: String,
    archived: bool,
) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    repo.set_archived(&id, archived)
        .map_err(|e| storage_err(e))?;
    let _ = repo.log_governance_action(
        if archived {
            "archive_conversation"
        } else {
            "unarchive_conversation"
        },
        Some("conversation"),
        Some(&id),
        "ok",
        None,
    );
    Ok(())
}

/// 软删除（可恢复）。
#[tauri::command]
pub(crate) async fn delete_conversation(
    state: tauri::State<'_, DaemonState>,
    id: String,
) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    repo.soft_delete_conversation(&id)
        .map_err(|e| storage_err(e))?;
    let _ = repo.log_governance_action(
        "soft_delete_conversation",
        Some("conversation"),
        Some(&id),
        "ok",
        None,
    );
    Ok(())
}

/// 恢复软删除。
#[tauri::command]
pub(crate) async fn restore_conversation(
    state: tauri::State<'_, DaemonState>,
    id: String,
) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    repo.restore_conversation(&id).map_err(|e| storage_err(e))?;
    Ok(())
}

/// 彻底删除（不可恢复）：级联清理 DB + raw blob + 搜索索引，并记治理流水。
#[tauri::command]
pub(crate) async fn hard_delete_conversation(
    state: tauri::State<'_, DaemonState>,
    id: String,
) -> Result<(), String> {
    // 硬删是重活（索引提交），移出 runtime 线程
    run_blocking(move || {
        let (raw_hash, message_ids) = {
            let repo = state.repo.lock().map_err(|e| storage_err(e))?;
            let conv = repo
                .get_conversation(&id)
                .map_err(|e| storage_err(e))?
                .ok_or_else(|| format!("conversation not found: {id}"))?;
            let ids: Vec<String> = repo
                .list_messages(&id)
                .map_err(|e| storage_err(e))?
                .into_iter()
                .map(|m| m.id)
                .collect();
            (conv.raw_payload_id, ids)
        };
        // 1) DB 级联（messages/events/tags/knowledge 随 FK CASCADE）
        {
            let repo = state.repo.lock().map_err(|e| storage_err(e))?;
            repo.hard_delete_conversation(&id)
                .map_err(|e| storage_err(e))?;
            let _ = repo.log_governance_action(
                "hard_delete_conversation",
                Some("conversation"),
                Some(&id),
                "ok",
                Some(&format!(r#"{{"messages": {}}}"#, message_ids.len())),
            );
        }
        // 2) 搜索索引：删该会话全部消息文档后提交
        {
            let idx = state.search_index.lock().map_err(|e| storage_err(e))?;
            let mut writer = idx
                .writer(ch_search::index::DEFAULT_WRITER_HEAP)
                .map_err(|e| search_err(e))?;
            for mid in &message_ids {
                let _ = idx.delete_message(&mut writer, mid);
            }
            idx.commit(writer).map_err(|e| search_err(e))?;
        }
        // 3) raw blob
        if let Some(hash) = raw_hash {
            let raw_store = state.raw_store.lock().map_err(|e| storage_err(e))?;
            if let Err(e) = raw_store.delete(&hash) {
                tracing::warn!(hash = %hash, error = %e, "raw blob delete failed");
            }
        }
        Ok(())
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
    Ok(conn_row.map(|c| {
        let f = repo.get_conversation_flags(&c.id).unwrap_or((false, false));
        conversation_dto(c, 0, f)
    }))
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
