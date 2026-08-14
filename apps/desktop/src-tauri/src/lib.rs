//! Conversation Hub 桌面应用后端：Tauri command 层。
//!
//! 通过嵌入 DaemonState（plan §8.2 单点写者）访问数据层。
//! 每个 command 是薄包装，复用 daemon/storage/knowledge 的能力。

use ch_domain::Workspace;
use ch_normalization::{normalize, RawConversation};
use ch_daemon::{DaemonState, DaemonStateConfig};
use ch_storage::SearchResult as DbSearchResult;
use std::path::Path;
use tauri::Manager;

/// 全局状态：嵌入 DaemonState（持有 Repository + SearchIndex + RawStore）。
/// plan §8.2：Daemon 是单点写者，Tauri 通过它访问所有数据层。

// ── 前端返回类型（serde 自动派生）──────────────────────────────────────

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

// ── Tauri commands ──────────────────────────────────────────────────────

#[tauri::command]
fn list_workspaces(state: tauri::State<DaemonState>) -> Result<Vec<WorkspaceDto>, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    // 时间取自对话真实时间，按对话最新更新时间倒序
    let ws = repo.list_workspaces_by_conv_time().map_err(|e| e.to_string())?;
    Ok(ws.into_iter().map(workspace_dto).collect())
}

#[tauri::command]
fn list_conversations(
    state: tauri::State<DaemonState>,
    workspace_id: Option<String>,
    provider: Option<String>,
) -> Result<Vec<ConversationDto>, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let convs = repo
        .list_conversations(workspace_id.as_deref())
        .map_err(|e| e.to_string())?;
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
            let child_count = repo
                .count_children(&c.source_conversation_id, &provider_id)
                .unwrap_or(0);
            conversation_dto(c, child_count)
        })
        .collect();
    Ok(dtos)
}

/// 列出指定父会话的子任务（中栏展开时调用）。
#[tauri::command]
fn list_child_conversations(
    state: tauri::State<DaemonState>,
    parent_source_id: String,
    provider: String,
) -> Result<Vec<ConversationDto>, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let provider_id = format!("prov_{}", provider);
    let convs = repo
        .list_child_conversations(&parent_source_id, &provider_id)
        .map_err(|e| e.to_string())?;
    Ok(convs.into_iter().map(|c| conversation_dto(c, 0)).collect())
}

/// 全局防重入标志：避免重置/同步并发执行导致 UI 卡顿或数据竞争。
static IS_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 重置所有数据（清空 conversations/workspaces + 索引 + raw blobs）。
/// 保留 schema 和用户自定义脱敏规则。
/// 若已有重置/同步在进行中，返回错误提示前端。
#[tauri::command]
fn reset_all_data(state: tauri::State<DaemonState>) -> Result<(), String> {
    if IS_BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("重置中，请稍候…".into());
    }
    let result = state.wipe_all().map_err(|e| e.to_string());
    IS_BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    result
}

#[tauri::command]
fn list_messages(
    state: tauri::State<DaemonState>,
    conversation_id: String,
) -> Result<Vec<MessageDto>, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let msgs = repo.list_messages(&conversation_id).map_err(|e| e.to_string())?;
    Ok(msgs.into_iter().map(message_dto).collect())
}

#[tauri::command]
fn list_events(
    state: tauri::State<DaemonState>,
    conversation_id: String,
) -> Result<Vec<EventDto>, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let events = repo.list_events(&conversation_id).map_err(|e| e.to_string())?;
    Ok(events.into_iter().map(event_dto).collect())
}

/// 获取会话完整详情（消息 + 事件 + 完整度，plan §6.4）。
#[tauri::command]
fn get_conversation_detail(
    state: tauri::State<DaemonState>,
    conversation_id: String,
) -> Result<ConversationDetailDto, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let conv = repo
        .get_conversation(&conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("conversation not found: {conversation_id}"))?;
    let messages = repo
        .list_messages(&conversation_id)
        .map_err(|e| e.to_string())?;
    let events = repo.list_events(&conversation_id).map_err(|e| e.to_string())?;
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
fn extract_knowledge(
    state: tauri::State<DaemonState>,
    conversation_id: String,
) -> Result<ch_knowledge::ExtractionResult, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let conv = repo
        .get_conversation(&conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("conversation not found: {conversation_id}"))?;
    let messages = repo.list_messages(&conversation_id).map_err(|e| e.to_string())?;
    let events = repo.list_events(&conversation_id).map_err(|e| e.to_string())?;
    let input = ch_knowledge::ExtractionInput {
        title: Some(conv.effective_title().to_string()),
        messages,
        events,
    };
    Ok(ch_knowledge::RuleExtractor::new().extract(&input))
}

#[tauri::command]
fn search(state: tauri::State<DaemonState>, query: String) -> Result<Vec<SearchResultDto>, String> {
    // 优先走 Tantivy（plan §9.5 主检索），降级 FTS5
    let idx = state.search_index.lock().map_err(|e| e.to_string())?;
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
            let repo = state.repo.lock().map_err(|e| e.to_string())?;
            let q = ch_storage::SearchQuery::new(&query);
            let results = repo.search(&q).map_err(|e| e.to_string())?;
            Ok(results.into_iter().map(search_result_dto).collect())
        }
    }
}

#[derive(serde::Serialize)]
struct ImportResultDto {
    conversation_id: String,
    workspace_id: Option<String>,
    messages: usize,
    events: usize,
    completeness: String,
}

/// 导入一个会话文件（plan §8.3 流水线）。
/// 按扩展名自动选择 markdown/jsonl adapter。
#[tauri::command]
fn import_file(
    state: tauri::State<DaemonState>,
    path: String,
    workspace_name: Option<String>,
) -> Result<ImportResultDto, String> {
    let path_ref = Path::new(&path);

    // 1. 归档原始到 Raw Store
    let bytes = std::fs::read(path_ref).map_err(|e| e.to_string())?;
    let raw_store = state.raw_store.lock().map_err(|e| e.to_string())?;
    let raw_payload = raw_store.put(&bytes).map_err(|e| e.to_string())?;
    drop(raw_store);

    // 2. 解析（按扩展名）
    let raw = parse_by_extension(path_ref)?;
    let normalized = normalize(raw).map_err(|e| e.to_string())?;

    // 3. 入库
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.upsert_provider(normalized.conversation.provider)
        .map_err(|e| e.to_string())?;

    // workspace 归并（复用 resolver）
    let workspace_id = resolve_workspace(&repo, workspace_name.as_deref(), path_ref)?;

    let mut conv = normalized.conversation;
    conv.workspace_id = workspace_id.clone();
    conv.raw_payload_id = Some(raw_payload.hash);
    let conversation_id = repo
        .upsert_conversation(&conv)
        .map_err(|e| e.to_string())?;
    for m in &normalized.messages {
        let mut m = m.clone();
        m.conversation_id = conversation_id.clone();
        repo.upsert_message(&m).map_err(|e| e.to_string())?;
    }
    for e in &normalized.events {
        let mut e = e.clone();
        e.conversation_id = conversation_id.clone();
        repo.upsert_event(&e).map_err(|e| e.to_string())?;
    }
    // 读取入库后的消息用于索引
    let messages = repo.list_messages(&conversation_id).map_err(|e| e.to_string())?;
    let conv_title = conv.effective_title().to_string();
    let provider = conv.provider;
    drop(repo);

    // 4. 同步 Tantivy 索引（plan §9.5）
    let idx = state.search_index.lock().map_err(|e| e.to_string())?;
    let mut writer = idx.writer(15_000_000).map_err(|e| e.to_string())?;
    for m in &messages {
        let im = ch_search::index::IndexableMessage {
            message_id: m.id.clone(),
            conversation_id: conversation_id.clone(),
            provider,
            workspace_id: workspace_id.clone(),
            role: m.role,
            title: Some(conv_title.clone()),
            body: m.content_text.clone(),
        };
        idx.index_message(&mut writer, &im).map_err(|e| e.to_string())?;
    }
    idx.commit(writer).map_err(|e| e.to_string())?;

    Ok(ImportResultDto {
        conversation_id,
        workspace_id,
        messages: normalized.messages.len(),
        events: normalized.events.len(),
        completeness: normalized.completeness.label().to_string(),
    })
}

// ── ZCode / Claude Code 真实来源导入（plan §10.5）─────────────────────────

#[derive(serde::Serialize)]
struct SourceSessionDto {
    session_id: String,
    title: String,
    detail: String,
    message_count: Option<i64>,
}

/// 列出 ZCode 会话。
#[tauri::command]
fn list_zcode_sessions() -> Result<Vec<SourceSessionDto>, String> {
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    let db_path = format!("{home}/.zcode/cli/db/db.sqlite");
    let sessions = ch_adapter_zcode::discover_sessions(&db_path)
        .map_err(|e| format!("discover zcode: {e}"))?;
    Ok(sessions
        .into_iter()
        .map(|s| SourceSessionDto {
            session_id: s.session_id,
            title: s.title,
            detail: s.directory,
            message_count: Some(s.message_count),
        })
        .collect())
}

/// 从 ZCode 导入一条会话。
#[tauri::command]
fn import_from_zcode(
    state: tauri::State<DaemonState>,
    session_id: String,
) -> Result<ImportResultDto, String> {
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    let db_path = format!("{home}/.zcode/cli/db/db.sqlite");
    let raw = ch_adapter_zcode::parse_session(&db_path, &session_id)
        .map_err(|e| format!("parse zcode: {e}"))?;
    import_raw_to_state(&state, raw, Some("ZCode"))
}

/// 列出 Claude Code 会话。
#[tauri::command]
fn list_claude_code_sessions() -> Result<Vec<SourceSessionDto>, String> {
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    let claude_home = format!("{home}/.claude");
    let sessions = ch_adapter_claude_code::discover_sessions(&claude_home)
        .map_err(|e| format!("discover claude code: {e}"))?;
    Ok(sessions
        .into_iter()
        .map(|s| SourceSessionDto {
            session_id: s.session_id,
            title: s.project_dir.clone(),
            detail: format!("{} KB", s.size_bytes / 1024),
            message_count: None,
        })
        .collect())
}

/// 从 Claude Code 导入一条会话。
#[tauri::command]
fn import_from_claude_code(
    state: tauri::State<DaemonState>,
    session_id: String,
) -> Result<ImportResultDto, String> {
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    let claude_home = format!("{home}/.claude");
    let sessions = ch_adapter_claude_code::discover_sessions(&claude_home)
        .map_err(|e| format!("discover: {e}"))?;
    let session = sessions
        .into_iter()
        .find(|s| s.session_id == session_id)
        .ok_or_else(|| format!("session not found: {session_id}"))?;
    let raw = ch_adapter_claude_code::parse_session(&session.file_path)
        .map_err(|e| format!("parse: {e}"))?;
    import_raw_to_state(&state, raw, Some("Claude Code"))
}

// ── Cursor / MiniMax 真实来源导入 ──────────────────────────────────────

/// Cursor state.vscdb 路径。
fn cursor_db_path() -> Result<String, String> {
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    Ok(format!("{home}/Library/Application Support/Cursor/User/globalStorage/state.vscdb"))
}

/// MiniMax runtime-state.sqlite 路径。
fn minimax_db_path() -> Result<String, String> {
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    Ok(format!("{home}/.minimax/v2/sqlite/runtime-state.sqlite"))
}

/// 列出 Cursor 会话。
#[tauri::command]
fn list_cursor_sessions() -> Result<Vec<SourceSessionDto>, String> {
    let db = cursor_db_path()?;
    let sessions = ch_adapter_cursor::discover_sessions(&db)
        .map_err(|e| format!("discover cursor: {e}"))?;
    Ok(sessions
        .into_iter()
        .map(|s| SourceSessionDto {
            session_id: s.session_id,
            title: s.title,
            detail: format!("{} 条消息", s.message_count),
            message_count: Some(s.message_count as i64),
        })
        .collect())
}

/// 从 Cursor 导入一条会话。
#[tauri::command]
fn import_from_cursor(
    state: tauri::State<DaemonState>,
    session_id: String,
) -> Result<ImportResultDto, String> {
    let db = cursor_db_path()?;
    let raw = ch_adapter_cursor::parse_session(&db, &session_id)
        .map_err(|e| format!("parse cursor: {e}"))?;
    import_raw_to_state(&state, raw, Some("Cursor"))
}

/// 列出 MiniMax 会话。
#[tauri::command]
fn list_minimax_sessions() -> Result<Vec<SourceSessionDto>, String> {
    let db = minimax_db_path()?;
    let sessions = ch_adapter_minimax::discover_sessions(&db)
        .map_err(|e| format!("discover minimax: {e}"))?;
    Ok(sessions
        .into_iter()
        .map(|s| {
            let mut detail = format!("{} 消息", s.message_count);
            if !s.agent_name.is_empty() {
                detail = format!("{} · {detail}", s.agent_name);
            }
            SourceSessionDto {
                session_id: s.session_id,
                title: s.title,
                detail: if s.child_count > 0 {
                    format!("{detail} · {} 子任务", s.child_count)
                } else {
                    detail
                },
                message_count: Some(s.message_count),
            }
        })
        .collect())
}

/// 从 MiniMax 导入一条会话。
#[tauri::command]
fn import_from_minimax(
    state: tauri::State<DaemonState>,
    session_id: String,
) -> Result<ImportResultDto, String> {
    let db = minimax_db_path()?;
    let raw = ch_adapter_minimax::parse_session(&db, &session_id)
        .map_err(|e| format!("parse minimax: {e}"))?;
    import_raw_to_state(&state, raw, Some("MiniMax Code"))
}

// ── Codex (ChatGPT CLI/Desktop) 真实来源导入 ──────────────────────────

/// Codex home 路径。
fn codex_home() -> Result<String, String> {
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    Ok(format!("{home}/.codex"))
}

/// 列出 Codex 会话。
#[tauri::command]
fn list_codex_sessions() -> Result<Vec<SourceSessionDto>, String> {
    let home = codex_home()?;
    let sessions = ch_adapter_codex::discover_sessions(&home)
        .map_err(|e| format!("discover codex: {e}"))?;
    Ok(sessions
        .into_iter()
        .map(|s| SourceSessionDto {
            session_id: s.session_id,
            title: s.title,
            detail: format!("{} KB", s.size_bytes / 1024),
            message_count: None,
        })
        .collect())
}

/// 从 Codex 导入一条会话。
#[tauri::command]
fn import_from_codex(
    state: tauri::State<DaemonState>,
    session_id: String,
) -> Result<ImportResultDto, String> {
    let home = codex_home()?;
    let sessions = ch_adapter_codex::discover_sessions(&home)
        .map_err(|e| format!("discover codex: {e}"))?;
    let session = sessions
        .into_iter()
        .find(|s| s.session_id == session_id)
        .ok_or_else(|| format!("session not found: {session_id}"))?;
    let raw = ch_adapter_codex::parse_session(&session.file_path)
        .map_err(|e| format!("parse codex: {e}"))?;
    import_raw_to_state(&state, raw, Some("Codex"))
}

// ── CodeAgentOps：指标采集与聚合查询（plan codeagent-ops M2）──────────

/// 同步 ops 指标（独立于对话采集，幂等批量写入，不影响现有数据）。
#[tauri::command]
fn ops_sync(state: tauri::State<DaemonState>) -> Result<serde_json::Value, String> {
    if IS_BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("同步中，请稍候…".into());
    }
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    let mut usage_written = 0usize;
    let mut tools_written = 0usize;
    let result = (|| -> Result<(), String> {
        let repo = state.repo.lock().map_err(|e| e.to_string())?;
        // provider 表需要存在对应行（JOIN 用）
        for p in [
            ch_domain::Provider::ZCode,
            ch_domain::Provider::MinimaxCode,
            ch_domain::Provider::ClaudeCode,
            ch_domain::Provider::Codex,
        ] {
            repo.upsert_provider(p).map_err(|e| e.to_string())?;
        }

        // ZCode: turn_usage + tool_usage
        if let Ok((u, t)) = ch_ops_metrics::collect_zcode(format!("{home}/.zcode/cli/db/db.sqlite")) {
            usage_written += repo.upsert_usage_batch(&u).map_err(|e| e.to_string())?;
            tools_written += repo.upsert_tool_call_batch(&t).map_err(|e| e.to_string())?;
        }
        // MiniMax: token_usage
        if let Ok(u) = ch_ops_metrics::collect_minimax(format!("{home}/.minimax/v2/sqlite/runtime-state.sqlite")) {
            usage_written += repo.upsert_usage_batch(&u).map_err(|e| e.to_string())?;
        }
        // Claude Code: JSONL usage + tool_use
        if let Ok((u, t)) = ch_ops_metrics::collect_claude_code(format!("{home}/.claude")) {
            usage_written += repo.upsert_usage_batch(&u).map_err(|e| e.to_string())?;
            tools_written += repo.upsert_tool_call_batch(&t).map_err(|e| e.to_string())?;
        }
        // Codex: token_count 快照 + function_call
        if let Ok((u, t)) = ch_ops_metrics::collect_codex(format!("{home}/.codex")) {
            usage_written += repo.upsert_usage_batch(&u).map_err(|e| e.to_string())?;
            tools_written += repo.upsert_tool_call_batch(&t).map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    IS_BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    result?;
    Ok(serde_json::json!({
        "usage_written": usage_written,
        "tools_written": tools_written,
    }))
}

#[tauri::command]
fn ops_overview(state: tauri::State<DaemonState>, days: Option<i64>) -> Result<ch_storage::OpsOverview, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.ops_overview(days).map_err(|e| e.to_string())
}

#[tauri::command]
fn ops_by_provider(state: tauri::State<DaemonState>, days: Option<i64>) -> Result<Vec<ch_storage::ProviderUsage>, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.ops_by_provider(days).map_err(|e| e.to_string())
}

#[tauri::command]
fn ops_by_model(state: tauri::State<DaemonState>, days: Option<i64>) -> Result<Vec<ch_storage::ModelUsage>, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.ops_by_model(days).map_err(|e| e.to_string())
}

#[tauri::command]
fn ops_timeseries(state: tauri::State<DaemonState>, days: Option<i64>) -> Result<Vec<ch_storage::DailyUsage>, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.ops_timeseries_daily(days).map_err(|e| e.to_string())
}

#[tauri::command]
fn ops_tool_toplist(state: tauri::State<DaemonState>, days: Option<i64>, n: Option<i64>) -> Result<Vec<ch_storage::ToolUsageRow>, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.ops_tool_toplist(days, n.unwrap_or(10)).map_err(|e| e.to_string())
}

#[tauri::command]
fn ops_risky_calls(state: tauri::State<DaemonState>, days: Option<i64>, n: Option<i64>) -> Result<Vec<ch_domain::ToolCallRecord>, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.ops_risky_calls(days, n.unwrap_or(50)).map_err(|e| e.to_string())
}

/// 按 provider + source_conversation_id 精确查会话（审计命中跳转用，含子任务）。
#[tauri::command]
fn get_conversation_by_source(
    state: tauri::State<DaemonState>,
    provider: String,
    source_conversation_id: String,
) -> Result<Option<ConversationDto>, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let provider_id = format!("prov_{provider}");
    let conn_row = repo
        .find_conversation_by_source(&provider_id, &source_conversation_id)
        .map_err(|e| e.to_string())?;
    Ok(conn_row.map(|c| conversation_dto(c, 0)))
}

// ── M4：安全审计 ───────────────────────────────────────────────────────

/// 全库审计扫描：敏感信息 + 危险命令（plan codeagent-ops M4）。
#[tauri::command]
fn audit_scan(state: tauri::State<DaemonState>) -> Result<ch_audit::AuditReport, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    ch_audit::run_audit(&repo).map_err(|e| e.to_string())
}

/// 渲染 HTML 审计报告（前端保存对话框落盘）。
#[tauri::command]
fn audit_export_html(state: tauri::State<DaemonState>) -> Result<String, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let report = ch_audit::run_audit(&repo).map_err(|e| e.to_string())?;
    Ok(ch_audit::render_html(&report))
}

/// 策略规则 CRUD（M4/M5：命令黑名单 + 自定义敏感规则）。
#[tauri::command]
fn policy_list(state: tauri::State<DaemonState>) -> Result<Vec<ch_storage::PolicyRuleRecord>, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.list_policy_rules().map_err(|e| e.to_string())
}

#[tauri::command]
fn policy_upsert(
    state: tauri::State<DaemonState>,
    rule: ch_storage::PolicyRuleRecord,
) -> Result<(), String> {
    // 校验正则合法
    regex::Regex::new(&rule.pattern).map_err(|e| format!("正则无效: {e}"))?;
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.upsert_policy_rule(&rule).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn policy_delete(state: tauri::State<DaemonState>, name: String) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.delete_policy_rule(&name).map_err(|e| e.to_string())
}

// ── M5：预算设置 ───────────────────────────────────────────────────────

#[tauri::command]
fn budget_get(state: tauri::State<DaemonState>) -> Result<ch_storage::BudgetSettings, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.get_budget_settings().map_err(|e| e.to_string())
}

#[tauri::command]
fn budget_set(state: tauri::State<DaemonState>, settings: ch_storage::BudgetSettings) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.set_budget_settings(&settings).map_err(|e| e.to_string())
}

/// 本月（自然月）用量：预算告警用。
#[tauri::command]
fn ops_month_usage(state: tauri::State<DaemonState>) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let now = ch_domain::now_utc();
    // 本月 1 号 00:00 UTC 的毫秒
    let month_start = time::Date::from_calendar_date(
        now.year(),
        time::Month::try_from(u8::try_from(now.month() as u8).unwrap_or(1)).unwrap_or(time::Month::January),
        1,
    )
    .map_err(|e| e.to_string())?
    .with_time(time::Time::MIDNIGHT)
    .assume_utc();
    let cutoff = (month_start - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
    let row = repo
        .ops_month_usage_since(cutoff)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "tokens": row.0,
        "cost_usd": row.1,
    }))
}

// ── M5：定价模型 ───────────────────────────────────────────────────────

/// 默认定价（$/M tokens，可被 app_data/pricing.json 覆盖）。
const DEFAULT_PRICING: &str = r#"{
  "GLM-5.2": {"input_per_mtok": 0.5, "output_per_mtok": 2.0},
  "GLM-5.3": {"input_per_mtok": 0.5, "output_per_mtok": 2.0},
  "MiniMax-M3": {"input_per_mtok": 0.3, "output_per_mtok": 1.2},
  "codex": {"input_per_mtok": 2.0, "output_per_mtok": 8.0},
  "gpt-5": {"input_per_mtok": 1.25, "output_per_mtok": 10.0},
  "claude": {"input_per_mtok": 3.0, "output_per_mtok": 15.0}
}"#;

fn pricing_path(state: &tauri::State<DaemonState>) -> std::path::PathBuf {
    state.data_dir.join("pricing.json")
}

/// 读取定价表（不存在时写入默认值）。返回 {model: {input_per_mtok, output_per_mtok}}。
#[tauri::command]
fn ops_pricing_get(state: tauri::State<DaemonState>) -> Result<serde_json::Value, String> {
    let path = pricing_path(&state);
    if !path.exists() {
        std::fs::write(&path, DEFAULT_PRICING).map_err(|e| e.to_string())?;
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

/// 保存定价表（前端编辑后写回）。
#[tauri::command]
fn ops_pricing_set(state: tauri::State<DaemonState>, pricing: serde_json::Value) -> Result<(), String> {
    let path = pricing_path(&state);
    let content = serde_json::to_string_pretty(&pricing).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

/// 按定价重算 cost_usd（模型名前缀匹配；改 pricing.json 后调用立即生效）。
#[tauri::command]
fn ops_cost_recalc(state: tauri::State<DaemonState>) -> Result<serde_json::Value, String> {
    let pricing = ops_pricing_get(state.clone())?;
    // 展开为 Vec<(小写模型名, in 价, out 价)>
    let mut table: Vec<(String, f64, f64)> = Vec::new();
    if let Some(map) = pricing.as_object() {
        for (model, v) in map {
            let pin = v.get("input_per_mtok").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let pout = v.get("output_per_mtok").and_then(|x| x.as_f64()).unwrap_or(0.0);
            table.push((model.to_lowercase(), pin, pout));
        }
    }
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    // 模型 → 累计 in/out
    let models = repo
        .ops_model_token_totals()
        .map_err(|e| e.to_string())?;
    let mut updated = 0i64;
    let mut total_cost = 0f64;
    for (model, in_tok, out_tok) in models {
        // 前缀匹配（双向：定价键是模型名前缀，或模型名包含定价键）
        let hit = table.iter().find(|(k, _, _)| {
            let m = model.to_lowercase();
            m.starts_with(k.as_str()) || k.starts_with(m.as_str()) || m.contains(k.as_str())
        });
        if let Some((_, pin, pout)) = hit {
            let cost = (in_tok as f64 / 1e6) * pin + (out_tok as f64 / 1e6) * pout;
            repo.update_model_cost(&model, cost).map_err(|e| e.to_string())?;
            updated += 1;
            total_cost += cost;
        }
    }
    Ok(serde_json::json!({
        "models_updated": updated,
        "total_cost_usd": total_cost,
    }))
}

/// 启动时自动拉取 ZCode / Claude Code / Cursor / MiniMax 最新会话（plan §6.1 自动发现/同步）。
/// 返回导入统计。最多各导入 limit 个最新会话。
/// 若已有重置/同步在进行中，返回「同步中」标记（不阻塞 UI）。
#[tauri::command]
fn auto_sync(
    state: tauri::State<DaemonState>,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    if IS_BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("同步中，请稍候…".into());
    }
    let lim = limit.unwrap_or(50);
    let mut zcode_ok = 0u32;
    let mut zcode_skip = 0u32;
    let mut cc_ok = 0u32;
    let mut cc_skip = 0u32;
    let mut cursor_ok = 0u32;
    let mut cursor_skip = 0u32;
    let mut mm_ok = 0u32;
    let mut codex_ok = 0u32;
    let mut codex_skip = 0u32;
    let mut mm_skip = 0u32;

    let home = std::env::var("HOME").map_err(|_| "no HOME")?;

    // ZCode：discover_all 返回主任务 + 子任务（子任务带 source_parent_id）
    let zcode_db = format!("{home}/.zcode/cli/db/db.sqlite");
    if std::path::Path::new(&zcode_db).exists() {
        match ch_adapter_zcode::discover_all_sessions(&zcode_db) {
            Ok(sessions) => {
                for s in sessions.into_iter().take(lim) {
                    // 跳过已导入的（幂等检查）
                    let repo = state.repo.lock().map_err(|e| e.to_string())?;
                    let exists = repo
                        .list_conversations(None)
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .any(|c| c.source_conversation_id == s.session_id && c.provider == ch_domain::Provider::ZCode);
                    drop(repo);
                    if exists {
                        zcode_skip += 1;
                        continue;
                    }
                    match ch_adapter_zcode::parse_session(&zcode_db, &s.session_id) {
                        Ok(raw) => {
                            if import_raw_to_state(&state, raw, Some("ZCode")).is_ok() {
                                zcode_ok += 1;
                            }
                        }
                        Err(e) => tracing::warn!(session = %s.session_id, error = %e, "zcode parse failed"),
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "zcode discover failed"),
        }
    }

    // Claude Code
    let claude_home = format!("{home}/.claude");
    if std::path::Path::new(&claude_home).join("projects").exists() {
        match ch_adapter_claude_code::discover_sessions(&claude_home) {
            Ok(sessions) => {
                for s in sessions.into_iter().take(lim) {
                    let repo = state.repo.lock().map_err(|e| e.to_string())?;
                    let exists = repo
                        .list_conversations(None)
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .any(|c| c.source_conversation_id == s.session_id && c.provider == ch_domain::Provider::ClaudeCode);
                    drop(repo);
                    if exists {
                        cc_skip += 1;
                        continue;
                    }
                    match ch_adapter_claude_code::parse_session(&s.file_path) {
                        Ok(raw) => {
                            if import_raw_to_state(&state, raw, Some("Claude Code")).is_ok() {
                                cc_ok += 1;
                            }
                        }
                        Err(e) => tracing::warn!(session = %s.session_id, error = %e, "cc parse failed"),
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "cc discover failed"),
        }
    }

    // Cursor
    let cursor_db = format!("{home}/Library/Application Support/Cursor/User/globalStorage/state.vscdb");
    if std::path::Path::new(&cursor_db).exists() {
        match ch_adapter_cursor::discover_sessions(&cursor_db) {
            Ok(sessions) => {
                for s in sessions.into_iter().take(lim) {
                    let repo = state.repo.lock().map_err(|e| e.to_string())?;
                    let exists = repo
                        .list_conversations(None)
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .any(|c| c.source_conversation_id == s.session_id && c.provider == ch_domain::Provider::Cursor);
                    drop(repo);
                    if exists {
                        cursor_skip += 1;
                        continue;
                    }
                    match ch_adapter_cursor::parse_session(&cursor_db, &s.session_id) {
                        Ok(raw) => {
                            if import_raw_to_state(&state, raw, Some("Cursor")).is_ok() {
                                cursor_ok += 1;
                            }
                        }
                        Err(e) => tracing::warn!(session = %s.session_id, error = %e, "cursor parse failed"),
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "cursor discover failed"),
        }
    }

    // MiniMax：discover_all 返回主任务 + 子任务（子任务带 source_parent_id）
    let mm_db = format!("{home}/.minimax/v2/sqlite/runtime-state.sqlite");
    if std::path::Path::new(&mm_db).exists() {
        match ch_adapter_minimax::discover_all_sessions(&mm_db) {
            Ok(sessions) => {
                for s in sessions.into_iter().take(lim) {
                    let repo = state.repo.lock().map_err(|e| e.to_string())?;
                    let exists = repo
                        .list_conversations(None)
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .any(|c| c.source_conversation_id == s.session_id && c.provider == ch_domain::Provider::MinimaxCode);
                    drop(repo);
                    if exists {
                        mm_skip += 1;
                        continue;
                    }
                    match ch_adapter_minimax::parse_session(&mm_db, &s.session_id) {
                        Ok(raw) => {
                            if import_raw_to_state(&state, raw, Some("MiniMax Code")).is_ok() {
                                mm_ok += 1;
                            }
                        }
                        Err(e) => tracing::warn!(session = %s.session_id, error = %e, "mm parse failed"),
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "mm discover failed"),
        }
    }

    // Codex (ChatGPT CLI/Desktop)：JSONL 会话
    let codex_home_dir = format!("{home}/.codex/sessions");
    if std::path::Path::new(&codex_home_dir).exists() {
        match ch_adapter_codex::discover_sessions(format!("{home}/.codex")) {
            Ok(sessions) => {
                for s in sessions.into_iter().take(lim) {
                    let repo = state.repo.lock().map_err(|e| e.to_string())?;
                    let exists = repo
                        .list_conversations(None)
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .any(|c| c.source_conversation_id == s.session_id && c.provider == ch_domain::Provider::Codex);
                    drop(repo);
                    if exists {
                        codex_skip += 1;
                        continue;
                    }
                    match ch_adapter_codex::parse_session(&s.file_path) {
                        Ok(raw) => {
                            if import_raw_to_state(&state, raw, Some("Codex")).is_ok() {
                                codex_ok += 1;
                            }
                        }
                        Err(e) => tracing::warn!(session = %s.session_id, error = %e, "codex parse failed"),
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "codex discover failed"),
        }
    }

    IS_BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    Ok(serde_json::json!({
        "zcode_imported": zcode_ok,
        "zcode_skipped": zcode_skip,
        "claude_code_imported": cc_ok,
        "claude_code_skipped": cc_skip,
        "cursor_imported": cursor_ok,
        "cursor_skipped": cursor_skip,
        "minimax_imported": mm_ok,
        "minimax_skipped": mm_skip,
        "codex_imported": codex_ok,
        "codex_skipped": codex_skip,
    }))
}

/// 通用导入：RawConversation → DaemonState（repo + search_index + raw_store）。
fn import_raw_to_state(
    state: &DaemonState,
    raw: RawConversation,
    workspace_name: Option<&str>,
) -> Result<ImportResultDto, String> {
    let provider = raw.provider;
    let raw_bytes = serde_json::to_vec(&raw).map_err(|e| e.to_string())?;
    let raw_store = state.raw_store.lock().map_err(|e| e.to_string())?;
    let raw_payload = raw_store.put(&raw_bytes).map_err(|e| e.to_string())?;
    drop(raw_store);

    let normalized = normalize(raw).map_err(|e| e.to_string())?;
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.upsert_provider(provider).map_err(|e| e.to_string())?;

    let workspace_id = workspace_name.map(|name| {
        if let Ok(Some(existing)) = repo.find_workspace_by_name(name) {
            existing.id
        } else {
            let ws = Workspace::new(name);
            repo.upsert_workspace(&ws).unwrap_or_default()
        }
    });

    let mut conv = normalized.conversation.clone();
    conv.workspace_id = workspace_id.clone();
    conv.raw_payload_id = Some(raw_payload.hash);
    let conversation_id = repo.upsert_conversation(&conv).map_err(|e| e.to_string())?;
    for m in &normalized.messages {
        let mut m = m.clone();
        m.conversation_id = conversation_id.clone();
        repo.upsert_message(&m).map_err(|e| e.to_string())?;
    }
    for e in &normalized.events {
        let mut e = e.clone();
        e.conversation_id = conversation_id.clone();
        repo.upsert_event(&e).map_err(|e| e.to_string())?;
    }
    let messages = repo.list_messages(&conversation_id).map_err(|e| e.to_string())?;
    let conv_title = conv.effective_title().to_string();
    drop(repo);

    // Tantivy 索引
    let idx = state.search_index.lock().map_err(|e| e.to_string())?;
    let mut writer = idx.writer(15_000_000).map_err(|e| e.to_string())?;
    for m in &messages {
        let im = ch_search::index::IndexableMessage {
            message_id: m.id.clone(),
            conversation_id: conversation_id.clone(),
            provider,
            workspace_id: workspace_id.clone(),
            role: m.role,
            title: Some(conv_title.clone()),
            body: m.content_text.clone(),
        };
        idx.index_message(&mut writer, &im).map_err(|e| e.to_string())?;
    }
    idx.commit(writer).map_err(|e| e.to_string())?;

    Ok(ImportResultDto {
        conversation_id,
        workspace_id,
        messages: normalized.messages.len(),
        events: normalized.events.len(),
        completeness: normalized.completeness.label().to_string(),
    })
}

/// 导出单条会话为 Markdown 或 JSON 字符串（plan §6.6）。
#[derive(serde::Serialize)]
struct ExportOutput {
    content: String,
    format: String,
    filename: String,
}

#[tauri::command]
fn export_conversation(
    state: tauri::State<DaemonState>,
    conversation_id: String,
    format: String,
) -> Result<ExportOutput, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let conv = repo
        .get_conversation(&conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("conversation not found: {conversation_id}"))?;
    let messages = repo.list_messages(&conversation_id).map_err(|e| e.to_string())?;
    let events = repo.list_events(&conversation_id).map_err(|e| e.to_string())?;
    let opts = ch_export::ExportOptions::everything();
    let safe_title: String = conv
        .effective_title()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(40)
        .collect();
    let (content, ext) = match format.as_str() {
        "json" => (
            ch_export::to_json(None, &conv, &messages, &events, &opts).map_err(|e| e.to_string())?,
            "json",
        ),
        _ => (
            ch_export::to_markdown(&conv, &messages, &events, &opts),
            "md",
        ),
    };
    let filename = format!("{}.{}", if safe_title.is_empty() { "conversation".into() } else { safe_title }, ext);
    Ok(ExportOutput {
        content,
        format: ext.to_string(),
        filename,
    })
}

/// 写文本文件到指定路径（前端 save 对话框返回的路径）。
/// 避免引入 tauri-plugin-fs，用最简方式把导出内容落盘。
#[tauri::command]
fn save_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content.as_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_favorite(state: tauri::State<DaemonState>, id: String, favorite: bool) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.set_favorite(&id, favorite).map_err(|e| e.to_string())
}

#[tauri::command]
fn add_tag(state: tauri::State<DaemonState>, id: String, tag: String) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.add_tag(&id, &tag).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_tags(state: tauri::State<DaemonState>, id: String) -> Result<Vec<String>, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.list_tags(&id).map_err(|e| e.to_string())
}

// ── DTO 转换 ────────────────────────────────────────────────────────────

/// 按扩展名选择 adapter（plan §10.5）。
fn parse_by_extension(path: &Path) -> Result<RawConversation, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jsonl" | "ndjson" => ch_adapter_jsonl::parse_file(path).map_err(|e| e.to_string()),
        _ => ch_adapter_markdown::parse_file(path).map_err(|e| e.to_string()),
    }
}

/// workspace 归并（复用 identity-resolver，plan §4.3）。
fn resolve_workspace(
    repo: &ch_storage::Repository,
    name: Option<&str>,
    path: &Path,
) -> Result<Option<String>, String> {
    let Some(name) = name else {
        return Ok(None);
    };
    let parent_path = path.parent().map(|p| p.to_string_lossy().into_owned());
    let mut candidate = ch_identity_resolver::SourceWorkspaceCandidate::new(name);
    candidate.canonical_path = parent_path;

    let known: Vec<_> = repo
        .list_workspaces()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|ws| ch_identity_resolver::IdentityKey {
            workspace_id: ws.id,
            display_name: ws.display_name,
            manifest_id: None,
            git_remote: ws.git_remote,
            git_common_dir: ws.git_common_dir,
            canonical_path: ws.canonical_path,
            filesystem_id: None,
        })
        .collect();

    let resolution = ch_identity_resolver::resolve(&candidate, &known);
    match resolution {
        ch_identity_resolver::Resolution::AutoMerge(m) => Ok(Some(m.workspace_id)),
        ch_identity_resolver::Resolution::NeedsConfirmation { candidate: Some(m), .. } => {
            Ok(Some(m.workspace_id))
        }
        _ => {
            let mut ws = ch_domain::Workspace::new(name);
            ws.canonical_path = path.parent().map(|p| p.to_string_lossy().into_owned());
            let id = repo.upsert_workspace(&ws).map_err(|e| e.to_string())?;
            Ok(Some(id))
        }
    }
}

fn workspace_dto(ws: Workspace) -> WorkspaceDto {
    WorkspaceDto {
        id: ws.id,
        display_name: ws.display_name,
        user_title: ws.user_title,
        status: ws.status.as_str().to_string(),
        created_at_ms: ts_to_ms(Some(ws.created_at)),
        updated_at_ms: ts_to_ms(Some(ws.updated_at)),
    }
}

fn ts_to_ms(ts: Option<ch_domain::Timestamp>) -> Option<i64> {
    ts.map(|t| (t - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64)
}

fn conversation_dto(c: ch_domain::Conversation, child_count: i64) -> ConversationDto {
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

fn message_dto(m: ch_domain::Message) -> MessageDto {
    MessageDto {
        id: m.id,
        role: m.role.to_string(),
        content_text: m.content_text,
        sequence_number: m.sequence_number,
        created_at_ms: ts_to_ms(m.created_at),
    }
}

fn event_dto(e: ch_domain::Event) -> EventDto {
    EventDto {
        id: e.id,
        event_type: e.event_type.to_string(),
        summary: e.summary,
        sequence_number: e.sequence_number,
    }
}

fn search_result_dto(r: DbSearchResult) -> SearchResultDto {
    SearchResultDto {
        message_id: r.message_id,
        conversation_id: r.conversation_id,
        provider: r.provider.to_string(),
        role: r.role.to_string(),
        title: r.title,
        snippet: r.snippet,
    }
}

// ── 应用启动 ────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 数据库放在 app data 目录（plan §9.6 布局）
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("app data dir should be available");
            std::fs::create_dir_all(&data_dir).expect("create data dir");
            // 嵌入 DaemonState（plan §8.2 单点写者），统一持有 repo + search_index + raw_store
            let daemon_state =
                DaemonState::open(DaemonStateConfig { data_dir: data_dir.clone() })
                    .expect("open daemon state");
            app.manage(daemon_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_workspaces,
            list_conversations,
            list_child_conversations,
            list_messages,
            list_events,
            get_conversation_detail,
            extract_knowledge,
            search,
            import_file,
            list_zcode_sessions,
            import_from_zcode,
            list_claude_code_sessions,
            import_from_claude_code,
            list_cursor_sessions,
            import_from_cursor,
            list_minimax_sessions,
            import_from_minimax,
            list_codex_sessions,
            import_from_codex,
            ops_sync,
            ops_overview,
            ops_by_provider,
            ops_by_model,
            ops_timeseries,
            ops_tool_toplist,
            ops_risky_calls,
            get_conversation_by_source,
            audit_scan,
            audit_export_html,
            policy_list,
            policy_upsert,
            policy_delete,
            budget_get,
            budget_set,
            ops_month_usage,
            ops_pricing_get,
            ops_pricing_set,
            ops_cost_recalc,
            auto_sync,
            reset_all_data,
            export_conversation,
            save_text_file,
            set_favorite,
            add_tag,
            list_tags,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Conversation Hub");
}
