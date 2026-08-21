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
        started_after_ms: None,
        started_before_ms: None,
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

/// 按日期范围列出主任务会话（活动页「查看当日会话」用）。
///
/// `from_ms` / `to_ms` 是闭区间的毫秒时间戳；任一为 None 表示该端不限。
#[tauri::command]
pub(crate) async fn list_conversations_by_date(
    state: tauri::State<'_, DaemonState>,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> Result<Vec<ConversationDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let mut filter = ch_storage::ConversationFilter::new().exclude_deleted();
    if let (Some(f), Some(t)) = (from_ms, to_ms) {
        filter = filter.with_started_range_ms(f, t);
    } else if let Some(f) = from_ms {
        filter.started_after_ms = Some(f);
    } else if let Some(t) = to_ms {
        filter.started_before_ms = Some(t);
    }
    let convs = repo
        .list_conversations_filtered(&filter)
        .map_err(|e| storage_err(e))?;
    let child_counts = repo.child_counts_bulk().map_err(|e| storage_err(e))?;
    let flags = repo.conversation_flags_bulk().map_err(|e| storage_err(e))?;
    let dtos = convs
        .into_iter()
        .filter(|c| c.source_parent_id.is_none())
        .map(|c| {
            let provider_id = format!("prov_{}", c.provider.as_str());
            let child_count = child_counts
                .get(&(c.source_conversation_id.clone(), provider_id))
                .copied()
                .unwrap_or(0);
            let f = flags.get(&c.id).copied().unwrap_or_default();
            conversation_dto(c, child_count, f)
        })
        .collect();
    Ok(dtos)
}

/// 按 source_dir 列出主任务会话（项目页卡片「查看会话」用）。
///
/// `dir` 精确匹配 usage_records.source_dir；空字符串 / None 视为「未知目录」回退。
#[tauri::command]
pub(crate) async fn list_conversations_by_dir(
    state: tauri::State<'_, DaemonState>,
    dir: String,
) -> Result<Vec<ConversationDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    // 通过 projects_overview 拿到 dir → conv ids 反查：简化为按 dir 模糊匹配
    // 这里直接用 SQL：conversations 自身没存 source_dir，但 raw_payload_id → usage_records
    // 关联能拿到。为了快速落地，先用 conversations.id 在 messages/usage_records
    // 里的最新 source_dir。
    let convs = repo
        .conversations_by_source_dir(&dir)
        .map_err(|e| storage_err(e))?;
    let child_counts = repo.child_counts_bulk().map_err(|e| storage_err(e))?;
    let flags = repo.conversation_flags_bulk().map_err(|e| storage_err(e))?;
    let dtos = convs
        .into_iter()
        .filter(|c| c.source_parent_id.is_none())
        .map(|c| {
            let provider_id = format!("prov_{}", c.provider.as_str());
            let child_count = child_counts
                .get(&(c.source_conversation_id.clone(), provider_id))
                .copied()
                .unwrap_or(0);
            let f = flags.get(&c.id).copied().unwrap_or_default();
            conversation_dto(c, child_count, f)
        })
        .collect();
    Ok(dtos)
}

/// 重置所有数据（清空 conversations/workspaces + 索引 + raw blobs）。
/// 保留 schema 和用户自定义脱敏规则。
/// 若已有重置/同步在进行中，返回错误提示前端。
#[tauri::command]
pub(crate) async fn reset_all_data(state: tauri::State<'_, DaemonState>) -> Result<(), String> {
    let _guard = BusyGuard::acquire()?;
    // 物理重建含大量文件删除，移出 runtime worker 线程
    let result = run_blocking(|| state.wipe_all().map_err(|e| storage_err(e)));
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

/// 列出库中实际存在会话的 provider（按 name 去重）。比 list_conversations 轻量：
/// 不拉 messages/events，单独的 DISTINCT 查询，用于过滤栏「仅显示有数据的来源」。
#[tauri::command]
pub(crate) async fn available_providers(
    state: tauri::State<'_, DaemonState>,
) -> Result<Vec<String>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.list_active_providers().map_err(|e| storage_err(e))
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
/// `engine`：`None`/`"rule"` → 规则引擎（默认，确定性离线）；`"llm"` →
/// 大模型引擎（需在设置中显式启用，见 `llm_cmd::extract_with_llm`）。
#[tauri::command]
pub(crate) async fn extract_knowledge(
    state: tauri::State<'_, DaemonState>,
    conversation_id: String,
    engine: Option<String>,
) -> Result<ch_knowledge::ExtractionResult, String> {
    let input = run_blocking(|| {
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
        Ok::<_, String>(ch_knowledge::ExtractionInput {
            title: Some(conv.effective_title().to_string()),
            messages,
            events,
        })
    })?;
    match engine.as_deref().unwrap_or("rule") {
        "llm" => run_blocking(|| super::llm_cmd::extract_with_llm(&state, &input)),
        _ => Ok(ch_knowledge::RuleExtractor::new().extract(&input)),
    }
}

/// 知识跨会话引用：给定文件/命令关键词，返回各关键词在多少个其他会话里被提到。
/// 复用 storage.search（FTS5 + 角色过滤），比前端逐条 search 快一个数量级。
#[derive(Debug, Clone, serde::Serialize)]
pub struct XrefConv {
    pub id: String,
    pub title: Option<String>,
    pub provider: String,
    pub updated_at_ms: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct XrefEntry {
    pub keyword: String,
    pub kind: String, // "file" | "command"
    pub other_count: i64,
    pub other_conversations: Vec<XrefConv>,
}

#[tauri::command]
pub(crate) async fn knowledge_xref(
    state: tauri::State<'_, DaemonState>,
    conversation_id: String,
    keywords: Vec<KnowledgeXrefKeyword>,
) -> Result<Vec<XrefEntry>, String> {
    use ch_storage::SearchQuery;
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let mut out = Vec::with_capacity(keywords.len());
    for kw in keywords {
        if kw.text.trim().is_empty() {
            continue;
        }
        // FTS5 模糊匹配（加上前缀通配符 * 走 MATCH 表达式）
        let q = SearchQuery {
            query: format!("\"{}\"*", kw.text.replace('"', " ")),
            provider: None,
            role: None,
            workspace_id: None,
            limit: 50,
        };
        let results = repo.search(&q).map_err(|e| storage_err(e))?;
        // 聚合 by conversation_id（排除自身）
        use std::collections::BTreeMap;
        let mut by_conv: BTreeMap<String, i64> = BTreeMap::new();
        for r in &results {
            if r.conversation_id == conversation_id {
                continue;
            }
            *by_conv.entry(r.conversation_id.clone()).or_insert(0) += 1;
        }
        // 取 Top 10
        let mut ranked: Vec<(String, i64)> = by_conv.into_iter().collect();
        ranked.sort_by_key(|x| std::cmp::Reverse(x.1));
        ranked.truncate(10);
        let mut convs = Vec::with_capacity(ranked.len());
        for (cid, _n) in &ranked {
            if let Ok(Some(conv)) = repo.get_conversation(cid) {
                convs.push(XrefConv {
                    id: conv.id.clone(),
                    title: conv.user_title.clone().or(conv.title.clone()),
                    provider: conv.provider.to_string(),
                    updated_at_ms: ch_storage::timestamp::to_millis(conv.updated_at),
                });
            }
        }
        out.push(XrefEntry {
            keyword: kw.text,
            kind: kw.kind,
            other_count: convs.len() as i64,
            other_conversations: convs,
        });
    }
    Ok(out)
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct KnowledgeXrefKeyword {
    pub text: String,
    pub kind: String, // "file" | "command"
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

/// 设置/清除会话的用户自定义标题（user_title）。
/// - title 传 null 或空串 → 清除（恢复使用 agent 提取的原始 title）
/// - 非空 → 保存为 user_title
#[tauri::command]
pub(crate) async fn set_user_title(
    state: tauri::State<'_, DaemonState>,
    id: String,
    title: Option<String>,
) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    repo.set_user_title(&id, title.as_deref())
        .map_err(|e| storage_err(e))
}

/// 读取会话的私有笔记 + 最后修改时间。
/// 返回 Option<(text, updated_at_ms)>：None 表示未写过。
#[tauri::command]
pub(crate) async fn get_conversation_note(
    state: tauri::State<'_, DaemonState>,
    id: String,
) -> Result<Option<ch_storage::NoteDto>, String> {
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    Ok(repo
        .get_note(&id)
        .map_err(|e| storage_err(e))?
        .map(|(note, updated_at)| ch_storage::NoteDto { note, updated_at }))
}

/// 列出全部会话标签（去重 + 频次倒序）—— 供前端标签自动补全。
#[tauri::command]
pub(crate) async fn list_all_tags(
    state: tauri::State<'_, DaemonState>,
    limit: Option<i64>,
) -> Result<Vec<ch_storage::TagCountDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let rows = repo
        .list_all_tags(limit.unwrap_or(50))
        .map_err(|e| storage_err(e))?;
    Ok(rows
        .into_iter()
        .map(|(tag, count)| ch_storage::TagCountDto { tag, count })
        .collect())
}

/// 设置/清除会话的私有笔记。
/// - note 为 None 或空串 → 删除
/// - 非空 → 保存并返回 updated_at 毫秒时间戳（0 = 已删除）
#[tauri::command]
pub(crate) async fn set_conversation_note(
    state: tauri::State<'_, DaemonState>,
    id: String,
    note: Option<String>,
) -> Result<i64, String> {
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    repo.set_note(&id, note.as_deref())
        .map_err(|e| storage_err(e))
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
