//! Conversation Hub 桌面应用后端：Tauri command 层。
//!
//! 通过嵌入 DaemonState（plan §8.2 单点写者）访问数据层。
//! 每个 command 是薄包装，复用 daemon/storage/knowledge 的能力。

#![allow(clippy::redundant_closure)]

use ch_domain::Workspace;
use ch_normalization::{normalize, RawConversation};
use ch_daemon::{DaemonState, DaemonStateConfig};
use ch_storage::SearchResult as DbSearchResult;
use std::path::Path;
use tauri::Manager;

// 全局状态：嵌入 DaemonState（持有 Repository + SearchIndex + RawStore）。
// plan §8.2：Daemon 是单点写者，Tauri 通过它访问所有数据层。

// ── 统一错误处理：AppError ─────────────────────────────────────────────
use ch_domain::app_error::{AppError, ErrorCode};

/// 将任何 Display 错误转为 AppError 字符串（替代裸 e.to_string()）。
/// 前端收到 `[code] message (detail)` 格式。
/// 通用内部错误
fn internal_err(e: impl std::fmt::Display) -> String {
    AppError::new(ErrorCode::Internal, "内部错误").with_detail(e.to_string()).to_string()
}


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
async fn list_workspaces(state: tauri::State<'_, DaemonState>) -> Result<Vec<WorkspaceDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    // 时间取自对话真实时间，按对话最新更新时间倒序
    let ws = repo.list_workspaces_by_conv_time().map_err(|e| internal_err(e))?;
    Ok(ws.into_iter().map(workspace_dto).collect())
}

#[tauri::command]
async fn list_conversations(
    state: tauri::State<'_, DaemonState>,
    workspace_id: Option<String>,
    provider: Option<String>,
) -> Result<Vec<ConversationDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    let convs = repo
        .list_conversations(workspace_id.as_deref())
        .map_err(|e| internal_err(e))?;
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
async fn list_child_conversations(
    state: tauri::State<'_, DaemonState>,
    parent_source_id: String,
    provider: String,
) -> Result<Vec<ConversationDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    let provider_id = format!("prov_{}", provider);
    let convs = repo
        .list_child_conversations(&parent_source_id, &provider_id)
        .map_err(|e| internal_err(e))?;
    Ok(convs.into_iter().map(|c| conversation_dto(c, 0)).collect())
}

/// 全局防重入标志：避免重置/同步并发执行导致 UI 卡顿或数据竞争。
static IS_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 取消同步标志：前端「取消更新」按钮设置，同步循环内定期检查提前退出。
static CANCEL_SYNC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 请求取消当前正在进行的同步（对话/指标），无进行中的同步也无副作用。
#[tauri::command]
fn cancel_sync() -> Result<(), String> {
    CANCEL_SYNC.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// 重置所有数据（清空 conversations/workspaces + 索引 + raw blobs）。
/// 保留 schema 和用户自定义脱敏规则。
/// 若已有重置/同步在进行中，返回错误提示前端。
#[tauri::command]
async fn reset_all_data(state: tauri::State<'_, DaemonState>) -> Result<(), String> {
    if IS_BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err(AppError::busy("重置中，请稍候…").to_string());
    }
    let result = state.wipe_all().map_err(|e| internal_err(e));
    // 重置后 ops 节流标记必须清零，否则治理页 5 分钟内拿不到新数据
    LAST_OPS_SYNC_MS.store(0, std::sync::atomic::Ordering::SeqCst);
    IS_BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    result
}

#[tauri::command]
async fn list_messages(
    state: tauri::State<'_, DaemonState>,
    conversation_id: String,
) -> Result<Vec<MessageDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    let msgs = repo.list_messages(&conversation_id).map_err(|e| internal_err(e))?;
    Ok(msgs.into_iter().map(message_dto).collect())
}

#[tauri::command]
async fn list_events(
    state: tauri::State<'_, DaemonState>,
    conversation_id: String,
) -> Result<Vec<EventDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    let events = repo.list_events(&conversation_id).map_err(|e| internal_err(e))?;
    Ok(events.into_iter().map(event_dto).collect())
}

/// 获取会话完整详情（消息 + 事件 + 完整度，plan §6.4）。
#[tauri::command]
#[tracing::instrument(skip_all, level = "debug")]
async fn get_conversation_detail(
    state: tauri::State<'_, DaemonState>,
    conversation_id: String,
) -> Result<ConversationDetailDto, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    let conv = repo
        .get_conversation(&conversation_id)
        .map_err(|e| internal_err(e))?
        .ok_or_else(|| format!("conversation not found: {conversation_id}"))?;
    let messages = repo
        .list_messages(&conversation_id)
        .map_err(|e| internal_err(e))?;
    let events = repo.list_events(&conversation_id).map_err(|e| internal_err(e))?;
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
async fn extract_knowledge(
    state: tauri::State<'_, DaemonState>,
    conversation_id: String,
) -> Result<ch_knowledge::ExtractionResult, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    let conv = repo
        .get_conversation(&conversation_id)
        .map_err(|e| internal_err(e))?
        .ok_or_else(|| format!("conversation not found: {conversation_id}"))?;
    let messages = repo.list_messages(&conversation_id).map_err(|e| internal_err(e))?;
    let events = repo.list_events(&conversation_id).map_err(|e| internal_err(e))?;
    let input = ch_knowledge::ExtractionInput {
        title: Some(conv.effective_title().to_string()),
        messages,
        events,
    };
    Ok(ch_knowledge::RuleExtractor::new().extract(&input))
}

#[tauri::command]
#[tracing::instrument(skip_all, level = "debug")]
async fn search(state: tauri::State<'_, DaemonState>, query: String) -> Result<Vec<SearchResultDto>, String> {
    // 优先走 Tantivy（plan §9.5 主检索），降级 FTS5
    let idx = state.search_index.lock().map_err(|e| internal_err(e))?;
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
            let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
            let q = ch_storage::SearchQuery::new(&query);
            let results = repo.search(&q).map_err(|e| internal_err(e))?;
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
async fn import_file(
    state: tauri::State<'_, DaemonState>,
    path: String,
    workspace_name: Option<String>,
) -> Result<ImportResultDto, String> {
    let path_ref = Path::new(&path);

    // 1. 归档原始到 Raw Store
    let bytes = std::fs::read(path_ref).map_err(|e| internal_err(e))?;
    let raw_store = state.raw_store.lock().map_err(|e| internal_err(e))?;
    let raw_payload = raw_store.put(&bytes).map_err(|e| internal_err(e))?;
    drop(raw_store);

    // 2. 解析（按扩展名）
    let raw = parse_by_extension(path_ref)?;
    let normalized = normalize(raw).map_err(|e| internal_err(e))?;

    // 3. 入库
    let repo = state.repo.lock().map_err(|e| internal_err(e))?;
    repo.upsert_provider(normalized.conversation.provider)
        .map_err(|e| internal_err(e))?;

    // workspace 归并（复用 resolver）
    let workspace_id = resolve_workspace(&repo, workspace_name.as_deref(), path_ref)?;

    let mut conv = normalized.conversation;
    conv.workspace_id = workspace_id.clone();
    conv.raw_payload_id = Some(raw_payload.hash);
    let conversation_id = repo
        .upsert_conversation(&conv)
        .map_err(|e| internal_err(e))?;
    for m in &normalized.messages {
        let mut m = m.clone();
        m.conversation_id = conversation_id.clone();
        repo.upsert_message(&m).map_err(|e| internal_err(e))?;
    }
    for e in &normalized.events {
        let mut e = e.clone();
        e.conversation_id = conversation_id.clone();
        repo.upsert_event(&e).map_err(|e| internal_err(e))?;
    }
    // 读取入库后的消息用于索引
    let messages = repo.list_messages(&conversation_id).map_err(|e| internal_err(e))?;
    let conv_title = conv.effective_title().to_string();
    let provider = conv.provider;
    drop(repo);

    // 4. 同步 Tantivy 索引（plan §9.5）
    let idx = state.search_index.lock().map_err(|e| internal_err(e))?;
    let mut writer = idx.writer(15_000_000).map_err(|e| internal_err(e))?;
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
        idx.index_message(&mut writer, &im).map_err(|e| internal_err(e))?;
    }
    idx.commit(writer).map_err(|e| internal_err(e))?;

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
    /// 已导入标记（一次 HashSet 查询批量判定，非逐条）。
    imported: bool,
}

/// 已导入判定上下文：存在集合 + 各 provider 新鲜度表。
struct ImportCtx {
    existing: std::collections::HashSet<(String, String)>,
    states: std::collections::HashMap<String, std::collections::HashMap<String, Option<i64>>>,
}

fn import_ctx(state: &DaemonState) -> ImportCtx {
    let repo = match state.repo.lock() {
        Ok(r) => r,
        Err(_) => return ImportCtx { existing: Default::default(), states: Default::default() },
    };
    let existing = repo
        .list_conversation_sources()
        .unwrap_or_default()
        .into_iter()
        .collect();
    let mut states = std::collections::HashMap::new();
    for pid in ["prov_zcode", "prov_claude-code", "prov_cursor", "prov_minimax-code", "prov_codex"] {
        if let Ok(m) = repo.import_state_map(pid) {
            states.insert(pid.to_string(), m);
        }
    }
    ImportCtx { existing, states }
}

/// 「已导入」= 存在 且 源更新时间 ≤ 导入时观察时间（源有新对话 → false，可再导入）。
fn imported_flag(ctx: &ImportCtx, provider_id: &str, source_id: &str, src_ms: Option<i64>) -> bool {
    ch_storage::Repository::is_up_to_date(
        ctx.states.get(provider_id).unwrap_or(&Default::default()),
        &ctx.existing,
        provider_id,
        source_id,
        src_ms,
    )
}

/// 列出 ZCode 会话。
#[tauri::command]
async fn list_zcode_sessions(state: tauri::State<'_, DaemonState>) -> Result<Vec<SourceSessionDto>, String> {
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    let db_path = format!("{home}/.zcode/cli/db/db.sqlite");
    let sessions = ch_adapter_zcode::discover_sessions(&db_path)
        .map_err(|e| format!("discover zcode: {e}"))?;
    let ictx = import_ctx(&state);
    Ok(sessions
        .into_iter()
        .map(|s| SourceSessionDto {
            imported: imported_flag(&ictx, "prov_zcode", &s.session_id, Some(s.time_updated)),
            session_id: s.session_id,
            title: s.title,
            detail: s.directory,
            message_count: Some(s.message_count),
        })
        .collect())
}

/// 从 ZCode 导入一条会话。
#[tauri::command]
async fn import_from_zcode(
    state: tauri::State<'_, DaemonState>,
    session_id: String,
) -> Result<ImportResultDto, String> {
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    let db_path = format!("{home}/.zcode/cli/db/db.sqlite");
    let raw = ch_adapter_zcode::parse_session(&db_path, &session_id)
        .map_err(|e| format!("parse zcode: {e}"))?;
    let observed = ch_adapter_zcode::discover_sessions(&db_path)
        .ok()
        .and_then(|v| v.into_iter().find(|s| s.session_id == session_id))
        .map(|s| s.time_updated);
    import_raw_to_state(&state, raw, Some("ZCode"), observed)
}

/// 列出 Claude Code 会话。
#[tauri::command]
async fn list_claude_code_sessions(state: tauri::State<'_, DaemonState>) -> Result<Vec<SourceSessionDto>, String> {
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    let claude_home = format!("{home}/.claude");
    let sessions = ch_adapter_claude_code::discover_sessions(&claude_home)
        .map_err(|e| format!("discover claude code: {e}"))?;
    let ictx = import_ctx(&state);
    Ok(sessions
        .into_iter()
        .map(|s| SourceSessionDto {
            imported: imported_flag(&ictx, "prov_claude-code", &s.session_id, s.mtime_ms),
            session_id: s.session_id,
            title: s.project_dir.clone(),
            detail: format!("{} KB", s.size_bytes / 1024),
            message_count: None,
        })
        .collect())
}

/// 从 Claude Code 导入一条会话。
#[tauri::command]
async fn import_from_claude_code(
    state: tauri::State<'_, DaemonState>,
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
    import_raw_to_state(&state, raw, Some("Claude Code"), session.mtime_ms)
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
async fn list_cursor_sessions(state: tauri::State<'_, DaemonState>) -> Result<Vec<SourceSessionDto>, String> {
    let db = cursor_db_path()?;
    let sessions = ch_adapter_cursor::discover_sessions(&db)
        .map_err(|e| format!("discover cursor: {e}"))?;
    let ictx = import_ctx(&state);
    Ok(sessions
        .into_iter()
        .map(|s| SourceSessionDto {
            imported: imported_flag(&ictx, "prov_cursor", &s.session_id, None),
            session_id: s.session_id,
            title: s.title,
            detail: format!("{} 条消息", s.message_count),
            message_count: Some(s.message_count as i64),
        })
        .collect())
}

/// 从 Cursor 导入一条会话。
#[tauri::command]
async fn import_from_cursor(
    state: tauri::State<'_, DaemonState>,
    session_id: String,
) -> Result<ImportResultDto, String> {
    let db = cursor_db_path()?;
    let raw = ch_adapter_cursor::parse_session(&db, &session_id)
        .map_err(|e| format!("parse cursor: {e}"))?;
    import_raw_to_state(&state, raw, Some("Cursor"), None)
}

/// 列出 MiniMax 会话。
#[tauri::command]
async fn list_minimax_sessions(state: tauri::State<'_, DaemonState>) -> Result<Vec<SourceSessionDto>, String> {
    let db = minimax_db_path()?;
    let sessions = ch_adapter_minimax::discover_sessions(&db)
        .map_err(|e| format!("discover minimax: {e}"))?;
    let ictx = import_ctx(&state);
    Ok(sessions
        .into_iter()
        .map(|s| {
            let mut detail = format!("{} 消息", s.message_count);
            if !s.agent_name.is_empty() {
                detail = format!("{} · {detail}", s.agent_name);
            }
            SourceSessionDto {
                imported: imported_flag(&ictx, "prov_minimax-code", &s.session_id, Some(s.updated_at_ms)),
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
async fn import_from_minimax(
    state: tauri::State<'_, DaemonState>,
    session_id: String,
) -> Result<ImportResultDto, String> {
    let db = minimax_db_path()?;
    let raw = ch_adapter_minimax::parse_session(&db, &session_id)
        .map_err(|e| format!("parse minimax: {e}"))?;
    let observed = ch_adapter_minimax::discover_sessions(&db)
        .ok()
        .and_then(|v| v.into_iter().find(|s| s.session_id == session_id))
        .map(|s| s.updated_at_ms);
    import_raw_to_state(&state, raw, Some("MiniMax Code"), observed)
}

// ── Codex (ChatGPT CLI/Desktop) 真实来源导入 ──────────────────────────

/// Codex home 路径。
fn codex_home() -> Result<String, String> {
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    Ok(format!("{home}/.codex"))
}

/// 列出 Codex 会话。
#[tauri::command]
async fn list_codex_sessions(state: tauri::State<'_, DaemonState>) -> Result<Vec<SourceSessionDto>, String> {
    let home = codex_home()?;
    let sessions = ch_adapter_codex::discover_sessions(&home)
        .map_err(|e| format!("discover codex: {e}"))?;
    let ictx = import_ctx(&state);
    Ok(sessions
        .into_iter()
        .map(|s| SourceSessionDto {
            imported: imported_flag(&ictx, "prov_codex", &s.session_id, s.mtime_ms),
            session_id: s.session_id,
            title: s.title,
            detail: format!("{} KB", s.size_bytes / 1024),
            message_count: None,
        })
        .collect())
}

/// 从 Codex 导入一条会话。
#[tauri::command]
async fn import_from_codex(
    state: tauri::State<'_, DaemonState>,
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
    import_raw_to_state(&state, raw, Some("Codex"), session.mtime_ms)
}

// ── CodeAgentOps：指标采集与聚合查询（plan codeagent-ops M2）──────────

/// ops_sync 上次完成时间（毫秒，进程内存缓存；真源为 app_settings.last_ops_sync_ms）。
static LAST_OPS_SYNC_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// ops 同步节流窗口：30 分钟内不重复全量扫描（跨进程持久，重启后仍生效）。
const OPS_SYNC_THROTTLE_MS: i64 = 30 * 60 * 1000;

/// 同步 ops 指标（独立于对话采集，幂等批量写入，不影响现有数据）。
/// force=false 时 5 分钟节流（进入治理页不再每次全量扫描 32MB+ JSONL）。
#[tauri::command]
#[tracing::instrument(skip_all, level = "info")]
async fn ops_sync(state: tauri::State<'_, DaemonState>, force: Option<bool>) -> Result<serde_json::Value, String> {
    let now_ms = (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
    // 持久节流：优先读库内时间戳（跨进程），内存值兜底
    let mut last = LAST_OPS_SYNC_MS.load(std::sync::atomic::Ordering::SeqCst);
    if last == 0 {
        if let Ok(repo) = state.repo.lock() {
            if let Ok(Some(v)) = repo.get_setting("last_ops_sync_ms") {
                last = v.parse().unwrap_or(0);
            }
        }
    }
    if !force.unwrap_or(false) && last > 0 && now_ms - last < OPS_SYNC_THROTTLE_MS {
        return Ok(serde_json::json!({ "usage_written": 0, "tools_written": 0, "throttled": true }));
    }
    CANCEL_SYNC.store(false, std::sync::atomic::Ordering::SeqCst);
    if IS_BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err(AppError::busy("同步中，请稍候…").to_string());
    }
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    let mut usage_written = 0usize;
    let mut tools_written = 0usize;
    let result = (|| -> Result<(), String> {
        let repo = state.repo.lock().map_err(|e| internal_err(e))?;
        // provider 表需要存在对应行（JOIN 用）
        for p in [
            ch_domain::Provider::ZCode,
            ch_domain::Provider::MinimaxCode,
            ch_domain::Provider::ClaudeCode,
            ch_domain::Provider::Codex,
        ] {
            repo.upsert_provider(p).map_err(|e| internal_err(e))?;
        }

        // ZCode: turn_usage + tool_usage
        // ZCode：model_usage 请求级口径，整源替换（与 turn 级互斥，防双算）
        if let Ok((u, t)) = ch_ops_metrics::collect_zcode(format!("{home}/.zcode/cli/db/db.sqlite")) {
            usage_written += repo.replace_provider_usage("prov_zcode", &u).map_err(|e| internal_err(e))?;
            tools_written += repo.upsert_tool_call_batch(&t).map_err(|e| internal_err(e))?;
        }
        // MiniMax: token_usage
        if let Ok(u) = ch_ops_metrics::collect_minimax(format!("{home}/.minimax/v2/sqlite/runtime-state.sqlite")) {
            usage_written += repo.replace_provider_usage("prov_minimax-code", &u).map_err(|e| internal_err(e))?;
        }
        // Claude Code: JSONL usage + tool_use
        if let Ok((u, t)) = ch_ops_metrics::collect_claude_code(format!("{home}/.claude")) {
            usage_written += repo.replace_provider_usage("prov_claude-code", &u).map_err(|e| internal_err(e))?;
            tools_written += repo.upsert_tool_call_batch(&t).map_err(|e| internal_err(e))?;
        }
        // Codex: token_count 快照 + function_call
        if let Ok((u, t)) = ch_ops_metrics::collect_codex(format!("{home}/.codex")) {
            usage_written += repo.replace_provider_usage("prov_codex", &u).map_err(|e| internal_err(e))?;
            tools_written += repo.upsert_tool_call_batch(&t).map_err(|e| internal_err(e))?;
        }
        // 自动成本重算：同步后立即按定价出数（此前需手动点重算，成本恒为 0）
        if let Ok(pricing) = ops_pricing_get_inner(&state) {
            let _ = apply_pricing(&repo, &pricing);
        }
        Ok(())
    })();
    if result.is_ok() {
        LAST_OPS_SYNC_MS.store(now_ms, std::sync::atomic::Ordering::SeqCst);
        if let Ok(repo) = state.repo.lock() {
            let _ = repo.set_setting("last_ops_sync_ms", &now_ms.to_string());
        }
    }
    IS_BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    result?;
    Ok(serde_json::json!({
        "usage_written": usage_written,
        "tools_written": tools_written,
    }))
}

#[tauri::command]
async fn ops_overview(state: tauri::State<'_, DaemonState>, days: Option<i64>) -> Result<ch_storage::OpsOverview, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.ops_overview(days).map_err(|e| internal_err(e))
}

#[tauri::command]
async fn ops_by_provider(state: tauri::State<'_, DaemonState>, days: Option<i64>) -> Result<Vec<ch_storage::ProviderUsage>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.ops_by_provider(days).map_err(|e| internal_err(e))
}

#[tauri::command]
async fn ops_by_model(state: tauri::State<'_, DaemonState>, days: Option<i64>) -> Result<Vec<ch_storage::ModelUsage>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.ops_by_model(days).map_err(|e| internal_err(e))
}

#[tauri::command]
async fn ops_timeseries(state: tauri::State<'_, DaemonState>, days: Option<i64>) -> Result<Vec<ch_storage::DailyUsage>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.ops_timeseries_daily(days).map_err(|e| internal_err(e))
}

#[tauri::command]
async fn ops_tool_toplist(state: tauri::State<'_, DaemonState>, days: Option<i64>, n: Option<i64>) -> Result<Vec<ch_storage::ToolUsageRow>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.ops_tool_toplist(days, n.unwrap_or(10)).map_err(|e| internal_err(e))
}

/// 风险调用 DTO：ts 转毫秒数（此前直接序列化 OffsetDateTime → 前端 Invalid Date）。
#[derive(serde::Serialize)]
struct RiskyCallDto {
    id: String,
    provider: String,
    source_session_id: String,
    tool_name: String,
    /// Unix 毫秒
    ts_ms: i64,
    read_only: Option<bool>,
    destructive: Option<bool>,
    approval_status: Option<String>,
    exit_code: Option<i64>,
    duration_ms: Option<i64>,
    status: String,
    command_text: Option<String>,
}

#[tauri::command]
async fn ops_risky_calls(state: tauri::State<'_, DaemonState>, days: Option<i64>, n: Option<i64>) -> Result<Vec<RiskyCallDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    let rows = repo.ops_risky_calls(days, n.unwrap_or(50)).map_err(|e| internal_err(e))?;
    Ok(rows
        .into_iter()
        .map(|r| RiskyCallDto {
            ts_ms: (r.ts - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64,
            id: r.id,
            provider: r.provider.to_string(),
            source_session_id: r.source_session_id,
            tool_name: r.tool_name,
            read_only: r.read_only,
            destructive: r.destructive,
            approval_status: r.approval_status,
            exit_code: r.exit_code,
            duration_ms: r.duration_ms,
            status: r.status.as_str().to_string(),
            command_text: r.command_text,
        })
        .collect())
}

/// 按 provider + source_conversation_id 精确查会话（审计命中跳转用，含子任务）。
#[tauri::command]
async fn get_conversation_by_source(
    state: tauri::State<'_, DaemonState>,
    provider: String,
    source_conversation_id: String,
) -> Result<Option<ConversationDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    let provider_id = format!("prov_{provider}");
    let conn_row = repo
        .find_conversation_by_source(&provider_id, &source_conversation_id)
        .map_err(|e| internal_err(e))?;
    Ok(conn_row.map(|c| conversation_dto(c, 0)))
}

// ── M4：安全审计 ───────────────────────────────────────────────────────

/// 全库审计扫描：敏感信息 + 危险命令（plan codeagent-ops M4）。
/// catch_unwind 兜底：扫描内部任何 panic 转为错误返回，绝不带崩整个应用。
#[tauri::command]
#[tracing::instrument(skip_all, level = "info")]
async fn audit_scan(state: tauri::State<'_, DaemonState>) -> Result<ch_audit::AuditReport, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ch_audit::run_audit(&repo)
    }))
    .map_err(|_| "扫描内部错误，请查看日志".to_string())?
    .map_err(|e| internal_err(e))
}

/// 渲染 HTML 审计报告（前端保存对话框落盘）。同样带 panic 兜底。
#[tauri::command]
async fn audit_export_html(state: tauri::State<'_, DaemonState>) -> Result<String, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    let report = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ch_audit::run_audit(&repo)
    }))
    .map_err(|_| "扫描内部错误，请查看日志".to_string())?
    .map_err(|e| internal_err(e))?;
    Ok(ch_audit::render_html(&report))
}

/// 策略规则 CRUD（M4/M5：命令黑名单 + 自定义敏感规则）。
#[tauri::command]
async fn policy_list(state: tauri::State<'_, DaemonState>) -> Result<Vec<ch_storage::PolicyRuleRecord>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.list_policy_rules().map_err(|e| internal_err(e))
}

#[tauri::command]
async fn policy_upsert(
    state: tauri::State<'_, DaemonState>,
    rule: ch_storage::PolicyRuleRecord,
) -> Result<(), String> {
    // 校验正则合法
    regex::Regex::new(&rule.pattern).map_err(|e| format!("正则无效: {e}"))?;
    let repo = state.repo.lock().map_err(|e| internal_err(e))?;
    repo.upsert_policy_rule(&rule).map_err(|e| internal_err(e))?;
    Ok(())
}

#[tauri::command]
async fn policy_delete(state: tauri::State<'_, DaemonState>, name: String) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| internal_err(e))?;
    repo.delete_policy_rule(&name).map_err(|e| internal_err(e))
}

// ── M5：预算设置 ───────────────────────────────────────────────────────

#[tauri::command]
async fn budget_get(state: tauri::State<'_, DaemonState>) -> Result<ch_storage::BudgetSettings, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.get_budget_settings().map_err(|e| internal_err(e))
}

#[tauri::command]
async fn budget_set(state: tauri::State<'_, DaemonState>, settings: ch_storage::BudgetSettings) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| internal_err(e))?;
    repo.set_budget_settings(&settings).map_err(|e| internal_err(e))
}

/// 本月（自然月）用量：预算告警用。
#[tauri::command]
async fn ops_month_usage(state: tauri::State<'_, DaemonState>) -> Result<serde_json::Value, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    let now = ch_domain::now_utc();
    // 本月 1 号 00:00 UTC 的毫秒
    let month_start = time::Date::from_calendar_date(
        now.year(),
        time::Month::try_from(now.month() as u8).unwrap_or(time::Month::January),
        1,
    )
    .map_err(|e| internal_err(e))?
    .with_time(time::Time::MIDNIGHT)
    .assume_utc();
    let cutoff = (month_start - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
    let row = repo
        .ops_month_usage_since(cutoff)
        .map_err(|e| internal_err(e))?;
    Ok(serde_json::json!({
        "tokens": row.0,
        "cost_usd": row.1,
    }))
}

// ── M6-M9：资产 / 自动化 / 成本归因 / 缓存 / 异常 ─────────────────────

/// 同步资产清单（30 分钟节流，force 可强制）。
#[tauri::command]
async fn assets_sync(state: tauri::State<'_, DaemonState>, force: Option<bool>) -> Result<serde_json::Value, String> {
    let now_ms = (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
    {
        let repo = state.repo.lock().map_err(|e| internal_err(e))?;
        if !force.unwrap_or(false) {
            if let Ok(Some(v)) = repo.get_setting("last_assets_sync_ms") {
                if let Ok(last) = v.parse::<i64>() {
                    if now_ms - last < 30 * 60 * 1000 {
                        return Ok(serde_json::json!({ "written": 0, "throttled": true }));
                    }
                }
            }
        }
    }
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    let assets = ch_ops_metrics::collect_assets(&home).map_err(|e| internal_err(e))?;
    let repo = state.repo.lock().map_err(|e| internal_err(e))?;
    for p in [
        ch_domain::Provider::ZCode,
        ch_domain::Provider::Codex,
        ch_domain::Provider::ClaudeCode,
        ch_domain::Provider::MinimaxCode,
    ] {
        repo.upsert_provider(p).map_err(|e| internal_err(e))?;
    }
    let mut written = 0;
    for p in [
        ch_domain::Provider::ZCode,
        ch_domain::Provider::Codex,
        ch_domain::Provider::ClaudeCode,
        ch_domain::Provider::MinimaxCode,
    ] {
        let subset: Vec<_> = assets.iter().filter(|a| a.provider == p).cloned().collect();
        written += repo
            .replace_provider_assets(&format!("prov_{}", p.as_str()), &subset)
            .map_err(|e| internal_err(e))?;
    }
    let _ = repo.set_setting("last_assets_sync_ms", &now_ms.to_string());
    Ok(serde_json::json!({ "written": written }))
}

#[tauri::command]
async fn assets_list(state: tauri::State<'_, DaemonState>) -> Result<Vec<ch_storage::AssetRow>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.list_assets().map_err(|e| internal_err(e))
}

/// 同步自动化任务（30 分钟节流）。
#[tauri::command]
async fn automations_sync(state: tauri::State<'_, DaemonState>, force: Option<bool>) -> Result<serde_json::Value, String> {
    let now_ms = (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
    {
        let repo = state.repo.lock().map_err(|e| internal_err(e))?;
        if !force.unwrap_or(false) {
            if let Ok(Some(v)) = repo.get_setting("last_automations_sync_ms") {
                if let Ok(last) = v.parse::<i64>() {
                    if now_ms - last < 30 * 60 * 1000 {
                        return Ok(serde_json::json!({ "written": 0, "throttled": true }));
                    }
                }
            }
        }
    }
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    let recs = ch_ops_metrics::collect_automations(&home).map_err(|e| internal_err(e))?;
    let repo = state.repo.lock().map_err(|e| internal_err(e))?;
    for p in [
        ch_domain::Provider::ZCode,
        ch_domain::Provider::Codex,
        ch_domain::Provider::MinimaxCode,
    ] {
        repo.upsert_provider(p).map_err(|e| internal_err(e))?;
    }
    let mut written = 0;
    for p in [
        ch_domain::Provider::ZCode,
        ch_domain::Provider::Codex,
        ch_domain::Provider::MinimaxCode,
    ] {
        let subset: Vec<_> = recs.iter().filter(|a| a.provider == p).cloned().collect();
        written += repo
            .replace_provider_automations(&format!("prov_{}", p.as_str()), &subset)
            .map_err(|e| internal_err(e))?;
    }
    let _ = repo.set_setting("last_automations_sync_ms", &now_ms.to_string());
    Ok(serde_json::json!({ "written": written }))
}

#[tauri::command]
async fn automations_list(state: tauri::State<'_, DaemonState>) -> Result<Vec<ch_storage::AutomationRow>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.list_automations().map_err(|e| internal_err(e))
}

#[tauri::command]
async fn ops_cost_by_dir(state: tauri::State<'_, DaemonState>, days: Option<i64>, n: Option<i64>) -> Result<Vec<ch_storage::DirCost>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.ops_cost_by_dir(days, n.unwrap_or(10)).map_err(|e| internal_err(e))
}

#[tauri::command]
async fn ops_cache_stats(state: tauri::State<'_, DaemonState>, days: Option<i64>) -> Result<Vec<ch_storage::CacheStat>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.ops_cache_stats(days).map_err(|e| internal_err(e))
}

#[tauri::command]
async fn ops_anomalies(state: tauri::State<'_, DaemonState>, days: Option<i64>) -> Result<Vec<ch_storage::AnomalyRow>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.ops_anomalies(days).map_err(|e| internal_err(e))
}

// ── M10-M12：健康度 / 延迟 / Token 浪费 ────────────────────────────────

#[tauri::command]
async fn ops_agent_health(state: tauri::State<'_, DaemonState>, days: Option<i64>) -> Result<Vec<ch_storage::AgentHealth>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.ops_agent_health(days).map_err(|e| internal_err(e))
}

#[tauri::command]
async fn ops_latency_stats(state: tauri::State<'_, DaemonState>, days: Option<i64>) -> Result<Vec<ch_storage::LatencyStat>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.ops_latency_stats(days).map_err(|e| internal_err(e))
}

#[tauri::command]
async fn ops_token_waste(state: tauri::State<'_, DaemonState>, days: Option<i64>, n: Option<i64>) -> Result<Vec<ch_storage::TokenWaste>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.ops_token_waste(days, n.unwrap_or(10)).map_err(|e| internal_err(e))
}

// ── M13-M14：横向对比 / 周报 ────────────────────────────────────────────

#[tauri::command]
async fn ops_agent_benchmark(state: tauri::State<'_, DaemonState>, days: Option<i64>) -> Result<Vec<ch_storage::AgentBenchmark>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.ops_agent_benchmark(days).map_err(|e| internal_err(e))
}

/// M14：生成周报 HTML（7 天治理汇总，自包含可分享）。
#[tauri::command]
async fn ops_weekly_report(state: tauri::State<'_, DaemonState>) -> Result<String, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    let s = repo.ops_weekly_summary().map_err(|e| internal_err(e))?;
    let mut html = String::new();
    use std::fmt::Write;
    writeln!(html, "<!doctype html><html lang='zh-CN'><head><meta charset='utf-8'><title>Conversation Hub 周报</title>").unwrap();
    writeln!(html, "<style>body{{font-family:-apple-system,'PingFang SC',sans-serif;margin:40px;background:#f7f8fa;color:#1a1e2e;}}").unwrap();
    writeln!(html, "h1{{font-size:20px;}} .meta{{color:#666;font-size:13px;margin-bottom:24px;}}").unwrap();
    writeln!(html, ".grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:12px;margin-bottom:24px;}}").unwrap();
    writeln!(html, ".card{{background:#fff;border:1px solid #e5e7eb;border-radius:10px;padding:16px;}}").unwrap();
    writeln!(html, ".card b{{display:block;font-size:24px;margin-bottom:4px;}}").unwrap();
    writeln!(html, "table{{width:100%;border-collapse:collapse;background:#fff;border-radius:10px;font-size:13px;}}").unwrap();
    writeln!(html, "th,td{{padding:10px 14px;border-bottom:1px solid #f0f0f0;text-align:left;}}").unwrap();
    writeln!(html, "th{{background:#f9fafb;font-size:11px;color:#6b7280;text-transform:uppercase;}}").unwrap();
    writeln!(html, ".good{{color:#059669;}} .warn{{color:#d97706;}} .bad{{color:#dc2626;}}").unwrap();
    writeln!(html, "</style></head><body>").unwrap();
    writeln!(html, "<h1>📊 Conversation Hub 治理周报</h1>").unwrap();
    writeln!(html, "<div class='meta'>{} · 覆盖最近 7 天 · {} 个 Agent</div>",
        ch_domain::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
        s.benchmark.len()).unwrap();

    // KPI
    writeln!(html, "<div class='grid'>").unwrap();
    writeln!(html, "<div class='card'><b>{}</b>模型请求</div>", s.overview.total_requests).unwrap();
    writeln!(html, "<div class='card'><b>{}</b>总 Tokens</div>",
        if s.overview.total_tokens >= 1_000_000_000 { format!("{:.2}B", s.overview.total_tokens as f64 / 1e9) }
        else if s.overview.total_tokens >= 1_000_000 { format!("{:.2}M", s.overview.total_tokens as f64 / 1e6) }
        else { s.overview.total_tokens.to_string() }).unwrap();
    writeln!(html, "<div class='card'><b>${:.2}</b>估算成本</div>", s.overview.cost_usd).unwrap();
    writeln!(html, "<div class='card'><b>{}</b>危险操作</div>", s.overview.destructive_calls).unwrap();
    writeln!(html, "<div class='card'><b>{}</b>浪费会话</div>", s.waste_sessions).unwrap();
    writeln!(html, "</div>").unwrap();

    // Agent 对比
    writeln!(html, "<h2>Agent 横向对比</h2><table><tr><th>Agent</th><th>请求</th><th>Tokens</th><th>成本</th><th>成功率</th><th>缓存命中</th><th>会话</th></tr>").unwrap();
    for b in &s.benchmark {
        writeln!(html, "<tr><td><b>{}</b></td><td>{}</td><td>{}</td><td>${:.2}</td><td class='{}'>{:.1}%</td><td>{:.1}%</td><td>{}</td></tr>",
            b.provider, b.total_requests,
            if b.total_tokens >= 1_000_000_000 { format!("{:.2}B", b.total_tokens as f64 / 1e9) }
            else if b.total_tokens >= 1_000_000 { format!("{:.2}M", b.total_tokens as f64 / 1e6) }
            else { b.total_tokens.to_string() },
            b.cost_usd,
            if b.success_rate > 95.0 {"good"} else if b.success_rate > 80.0 {"warn"} else {"bad"},
            b.success_rate, b.cache_hit_rate, b.sessions).unwrap();
    }
    writeln!(html, "</table>").unwrap();

    // 健康度
    if !s.health.is_empty() {
        writeln!(html, "<h2 style='margin-top:24px;'>Agent 健康度</h2><table><tr><th>Agent</th><th>请求</th><th>错误</th><th>重试</th><th>稳定性</th></tr>").unwrap();
        for h in &s.health {
            writeln!(html, "<tr><td>{}</td><td>{}</td><td class='{}'>{}</td><td>{}</td><td class='{}'>{:.0}</td></tr>",
                h.provider, h.total_requests, h.errors,
                if h.errors == 0 {"good"} else {"warn"},
                h.retries,
                if h.stability_score > 80.0 {"good"} else if h.stability_score > 50.0 {"warn"} else {"bad"},
                h.stability_score).unwrap();
        }
        writeln!(html, "</table>").unwrap();
    }

    writeln!(html, "<p style='margin-top:24px;color:#aaa;font-size:11px;'>由 Conversation Hub 自动生成 · 数据口径: input + output + reasoning (cache 不计费)</p>").unwrap();
    writeln!(html, "</body></html>").unwrap();
    Ok(html)
}

// ── M5：定价模型 ───────────────────────────────────────────────────────

/// 默认定价（$/M tokens，可被 app_data/pricing.json 覆盖）。
/// "zcode"/"cursor" 为 provider 兜底价（模型未细分时使用）。
const DEFAULT_PRICING: &str = r#"{
  "GLM-5.2": {"input_per_mtok": 0.5, "output_per_mtok": 2.0},
  "GLM-5.3": {"input_per_mtok": 0.5, "output_per_mtok": 2.0},
  "MiniMax-M3": {"input_per_mtok": 0.3, "output_per_mtok": 1.2},
  "codex": {"input_per_mtok": 2.0, "output_per_mtok": 8.0},
  "gpt-5": {"input_per_mtok": 1.25, "output_per_mtok": 10.0},
  "claude": {"input_per_mtok": 3.0, "output_per_mtok": 15.0},
  "zcode": {"input_per_mtok": 0.5, "output_per_mtok": 2.0},
  "cursor": {"input_per_mtok": 0.5, "output_per_mtok": 2.0}
}"#;

/// provider_id → 兜底定价键（模型未细分/unknown 时按来源计价）。
const PROVIDER_PRICING_FALLBACK: &[(&str, &str)] = &[
    ("prov_zcode", "zcode"),
    ("prov_minimax-code", "MiniMax-M3"),
    ("prov_claude-code", "claude"),
    ("prov_codex", "codex"),
    ("prov_cursor", "cursor"),
];

fn pricing_path(state: &tauri::State<DaemonState>) -> std::path::PathBuf {
    state.data_dir.join("pricing.json")
}

/// 读取定价表（不存在时写入默认值；旧文件缺新键时内存合并默认键，不回写）。
fn ops_pricing_get_inner(state: &DaemonState) -> Result<serde_json::Value, String> {
    let path = state.data_dir.join("pricing.json");
    if !path.exists() {
        std::fs::write(&path, DEFAULT_PRICING).map_err(|e| internal_err(e))?;
    }
    let content = std::fs::read_to_string(&path).map_err(|e| internal_err(e))?;
    let mut pricing: serde_json::Value = serde_json::from_str(&content).map_err(|e| internal_err(e))?;
    // 旧文件缺省键合并（如后来新增的 zcode/cursor 兜底价）
    if let (Some(dst), Ok(defs)) = (pricing.as_object_mut(), serde_json::from_str::<serde_json::Value>(DEFAULT_PRICING)) {
        if let Some(def_map) = defs.as_object() {
            for (k, v) in def_map {
                dst.entry(k.clone()).or_insert(v.clone());
            }
        }
    }
    Ok(pricing)
}

#[tauri::command]
async fn ops_pricing_get(state: tauri::State<'_, DaemonState>) -> Result<serde_json::Value, String> {
    ops_pricing_get_inner(&state)
}

/// 保存定价表（前端编辑后写回）。
#[tauri::command]
async fn ops_pricing_set(state: tauri::State<'_, DaemonState>, pricing: serde_json::Value) -> Result<(), String> {
    let path = pricing_path(&state);
    let content = serde_json::to_string_pretty(&pricing).map_err(|e| internal_err(e))?;
    std::fs::write(&path, content).map_err(|e| internal_err(e))
}

/// 定价应用核心：模型名匹配（前缀/包含）→ 命中；未命中走 provider 兜底价。
/// 返回 (更新模型数, 总成本)。ops_sync 自动调用 + 手动重算共用。
fn apply_pricing(repo: &ch_storage::Repository, pricing: &serde_json::Value) -> (i64, f64) {
    let mut table: Vec<(String, f64, f64)> = Vec::new();
    if let Some(map) = pricing.as_object() {
        for (model, v) in map {
            let pin = v.get("input_per_mtok").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let pout = v.get("output_per_mtok").and_then(|x| x.as_f64()).unwrap_or(0.0);
            table.push((model.to_lowercase(), pin, pout));
        }
    }
    let Ok(models) = repo.ops_model_token_totals() else {
        return (0, 0.0);
    };
    let mut updated = 0i64;
    let mut total_cost = 0f64;
    for (model, provider_id, in_tok, out_tok) in models {
        let m = model.to_lowercase();
        // 1) 模型名匹配
        let hit = table
            .iter()
            .find(|(k, _, _)| m.starts_with(k.as_str()) || k.starts_with(m.as_str()) || m.contains(k.as_str()))
            .map(|(k, _, _)| k.clone())
            // 2) provider 兜底（模型未细分，如 zcode turn_usage 无模型字段）
            .or_else(|| {
                PROVIDER_PRICING_FALLBACK
                    .iter()
                    .find(|(p, _)| *p == provider_id)
                    .map(|(_, k)| k.to_string())
            });
        if let Some(key) = hit {
            if let Some((_, pin, pout)) = table.iter().find(|(k, _, _)| *k == key) {
                // 按行内 tokens 单价写入：SUM(行成本) 与聚合口径一致
                if repo.update_model_pricing(&model, &provider_id, *pin, *pout).is_ok() {
                    updated += 1;
                    total_cost += (in_tok as f64 / 1e6) * pin + (out_tok as f64 / 1e6) * pout;
                }
            }
        }
    }
    (updated, total_cost)
}

/// 按定价重算 cost_usd（改 pricing.json 后调用立即生效）。
#[tauri::command]
async fn ops_cost_recalc(state: tauri::State<'_, DaemonState>) -> Result<serde_json::Value, String> {
    let pricing = ops_pricing_get_inner(&state)?;
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    let (updated, total_cost) = apply_pricing(&repo, &pricing);
    Ok(serde_json::json!({
        "models_updated": updated,
        "total_cost_usd": total_cost,
    }))
}

/// 启动时自动拉取 ZCode / Claude Code / Cursor / MiniMax 最新会话（plan §6.1 自动发现/同步）。
/// 返回导入统计。最多各导入 limit 个最新会话。
/// 若已有重置/同步在进行中，返回「同步中」标记（不阻塞 UI）。
#[tauri::command]
async fn auto_sync(
    state: tauri::State<'_, DaemonState>,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    if IS_BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err(AppError::busy("同步中，请稍候…").to_string());
    }
    let result = auto_sync_inner(&state, limit);
    IS_BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    result
}

/// auto_sync 实现：拉取 5 个来源的最新会话。
/// 性能要点：
/// - 幂等检查用一次性加载的 HashSet（旧版每会话全表扫描 → 卡顿根因之一）
/// - 已导入但缺主子链路的旧数据，用 repair_conversation_parent 补上
/// - limit 默认 500（覆盖全部会话；旧版 50 导致 MiniMax/ZCode 大量子任务丢失）
#[tracing::instrument(skip_all, level = "info")]
fn auto_sync_inner(
    state: &DaemonState,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    CANCEL_SYNC.store(false, std::sync::atomic::Ordering::SeqCst);
    let mut cancelled = false;
    let lim = limit.unwrap_or(500);
    let mut zcode_ok = 0u32;
    let mut zcode_skip = 0u32;
    let mut cc_ok = 0u32;
    let mut cc_skip = 0u32;
    let mut cursor_ok = 0u32;
    let mut cursor_skip = 0u32;
    let mut mm_ok = 0u32;
    let mut mm_skip = 0u32;
    let mut codex_ok = 0u32;
    let mut codex_skip = 0u32;

    let home = std::env::var("HOME").map_err(|_| "no HOME")?;

    // 全部导入完成后一次性提交索引（单 writer 单 commit，性能关键路径）
    let mut pending_index: Vec<ch_search::index::IndexableMessage> = Vec::new();

    // 一次性加载已导入集合 + 新鲜度表（增量导入：stale 会话也要重导入新消息）
    type SrcKey = (String, String);
    let (existing, istate): (
        std::collections::HashSet<SrcKey>,
        std::collections::HashMap<SrcKey, i64>,
    ) = {
        let repo = state.repo.lock().map_err(|e| internal_err(e))?;
        let sources = repo.list_conversation_sources().map_err(|e| internal_err(e))?;
        let mut set = std::collections::HashSet::new();
        let mut state_map = std::collections::HashMap::new();
        for (pid, sid) in sources {
            set.insert((pid.clone(), sid.clone()));
            state_map.insert((pid, sid), 0); // 默认 0 = 未知，视为 stale
        }
        // 覆盖已知 observed_ms
        for pid in ["prov_zcode", "prov_claude-code", "prov_cursor", "prov_minimax-code", "prov_codex"] {
            if let Ok(m) = repo.import_state_map(pid) {
                for (sid, obs) in m {
                    if let Some(v) = obs {
                        state_map.insert((pid.to_string(), sid), v);
                    }
                }
            }
        }
        (set, state_map)
    };


    // stale = 源更新时间 > 导入时观察时间 → 有新消息，需重导入（增量）
    let is_stale = |pid: &str, sid: &str, src_ms: i64| -> bool {
        match istate.get(&(pid.to_string(), sid.to_string())) {
            Some(&obs) => src_ms > obs,
            None => true, // 无记录 = 从未导入过
        }
    };

    // ZCode：discover_all 返回主任务 + 子任务（子任务带 source_parent_id）
    // 性能：已导入的 repair 收集后单事务批量执行（不再逐条锁）；
    // 新导入每 5 个让出 1ms，给 UI 查询抢锁窗口（消除锁饿死卡顿）
    let zcode_db = format!("{home}/.zcode/cli/db/db.sqlite");
    if std::path::Path::new(&zcode_db).exists() {
        match ch_adapter_zcode::discover_all_sessions(&zcode_db) {
            Ok(sessions) => {
                let mut repairs: Vec<(String, String)> = Vec::new();
                let mut zc_observed: Vec<(String, Option<i64>)> = Vec::new();
                let mut imported_count = 0u32;
                for s in sessions.into_iter().take(lim) {
                    zc_observed.push((s.session_id.clone(), Some(s.time_updated)));
                    if CANCEL_SYNC.load(std::sync::atomic::Ordering::SeqCst) {
                        cancelled = true;
                        break;
                    }
                    let key = ("prov_zcode".to_string(), s.session_id.clone());
                    if existing.contains(&key) && !is_stale("prov_zcode", &s.session_id, s.time_updated) {
                        if let Some(parent) = &s.parent_id {
                            repairs.push((s.session_id.clone(), parent.clone()));
                        }
                        zcode_skip += 1;
                        continue;
                    }
                    match ch_adapter_zcode::parse_session(&zcode_db, &s.session_id) {
                        Ok(raw) => {
                            match import_raw_inner(state, raw, Some("ZCode"), Some(s.time_updated)) {
                                Ok(o) => {
                                    pending_index.extend(o.indexable);
                                    zcode_ok += 1;
                                    imported_count += 1;
                                    if imported_count % 5 == 0 {
                                        std::thread::sleep(std::time::Duration::from_millis(1));
                                    }
                                }
                                Err(e) => tracing::warn!(session = %s.session_id, error = %e, "zcode import failed"),
                            }
                        }
                        Err(e) => tracing::warn!(session = %s.session_id, error = %e, "zcode parse failed"),
                    }
                }
                if !repairs.is_empty() {
                    if let Ok(repo) = state.repo.lock() {
                        let _ = repo.repair_parents_batch("prov_zcode", &repairs);
                    }
                }
                if let Ok(repo) = state.repo.lock() {
                    let _ = repo.record_import_states("prov_zcode", &zc_observed);
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
                    if CANCEL_SYNC.load(std::sync::atomic::Ordering::SeqCst) {
                        cancelled = true;
                        break;
                    }
                    let key = ("prov_claude-code".to_string(), s.session_id.clone());
                    if existing.contains(&key) && !is_stale("prov_claude-code", &s.session_id, s.mtime_ms.unwrap_or(0)) {
                        cc_skip += 1;
                        continue;
                    }
                    match ch_adapter_claude_code::parse_session(&s.file_path) {
                        Ok(raw) => {
                            match import_raw_inner(state, raw, Some("Claude Code"), s.mtime_ms) {
                                Ok(o) => {
                                    pending_index.extend(o.indexable);
                                    cc_ok += 1;
                                }
                                Err(e) => tracing::warn!(session = %s.session_id, error = %e, "cc import failed"),
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
                    if CANCEL_SYNC.load(std::sync::atomic::Ordering::SeqCst) {
                        cancelled = true;
                        break;
                    }
                    let key = ("prov_cursor".to_string(), s.session_id.clone());
                    if existing.contains(&key) && !is_stale("prov_cursor", &s.session_id, 0) {
                        cursor_skip += 1;
                        continue;
                    }
                    match ch_adapter_cursor::parse_session(&cursor_db, &s.session_id) {
                        Ok(raw) => {
                            match import_raw_inner(state, raw, Some("Cursor"), None) {
                                Ok(o) => {
                                    pending_index.extend(o.indexable);
                                    cursor_ok += 1;
                                }
                                Err(e) => tracing::warn!(session = %s.session_id, error = %e, "cursor import failed"),
                            }
                        }
                        Err(e) => tracing::warn!(session = %s.session_id, error = %e, "cursor parse failed"),
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "cursor discover failed"),
        }
    }

    // MiniMax：discover_all 返回主任务 + 子任务（过滤隐藏/归档内部残留）
    let mm_db = format!("{home}/.minimax/v2/sqlite/runtime-state.sqlite");
    if std::path::Path::new(&mm_db).exists() {
        match ch_adapter_minimax::discover_all_sessions(&mm_db) {
            Ok(sessions) => {
                let mut mm_repairs: Vec<(String, String)> = Vec::new();
                let mut mm_observed: Vec<(String, Option<i64>)> = Vec::new();
                let mut mm_imported = 0u32;
                for s in sessions.into_iter().take(lim) {
                    mm_observed.push((s.session_id.clone(), Some(s.updated_at_ms)));
                    if CANCEL_SYNC.load(std::sync::atomic::Ordering::SeqCst) {
                        cancelled = true;
                        break;
                    }
                    let key = ("prov_minimax-code".to_string(), s.session_id.clone());
                    if existing.contains(&key) && !is_stale("prov_minimax-code", &s.session_id, s.updated_at_ms) {
                        if let Some(parent) = &s.parent_session_id {
                            mm_repairs.push((s.session_id.clone(), parent.clone()));
                        }
                        mm_skip += 1;
                        continue;
                    }
                    match ch_adapter_minimax::parse_session(&mm_db, &s.session_id) {
                        Ok(raw) => {
                            match import_raw_inner(state, raw, Some("MiniMax Code"), Some(s.updated_at_ms)) {
                                Ok(o) => {
                                    pending_index.extend(o.indexable);
                                    mm_ok += 1;
                                    mm_imported += 1;
                                    if mm_imported % 5 == 0 {
                                        std::thread::sleep(std::time::Duration::from_millis(1));
                                    }
                                }
                                Err(e) => tracing::warn!(session = %s.session_id, error = %e, "mm import failed"),
                            }
                        }
                        Err(e) => tracing::warn!(session = %s.session_id, error = %e, "mm parse failed"),
                    }
                }
                if !mm_repairs.is_empty() {
                    if let Ok(repo) = state.repo.lock() {
                        let _ = repo.repair_parents_batch("prov_minimax-code", &mm_repairs);
                    }
                }
                if let Ok(repo) = state.repo.lock() {
                    let _ = repo.record_import_states("prov_minimax-code", &mm_observed);
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
                    if CANCEL_SYNC.load(std::sync::atomic::Ordering::SeqCst) {
                        cancelled = true;
                        break;
                    }
                    let key = ("prov_codex".to_string(), s.session_id.clone());
                    if existing.contains(&key) && !is_stale("prov_codex", &s.session_id, s.mtime_ms.unwrap_or(0)) {
                        codex_skip += 1;
                        continue;
                    }
                    match ch_adapter_codex::parse_session(&s.file_path) {
                        Ok(raw) => {
                            match import_raw_inner(state, raw, Some("Codex"), s.mtime_ms) {
                                Ok(o) => {
                                    pending_index.extend(o.indexable);
                                    codex_ok += 1;
                                }
                                Err(e) => tracing::warn!(session = %s.session_id, error = %e, "codex import failed"),
                            }
                        }
                        Err(e) => tracing::warn!(session = %s.session_id, error = %e, "codex parse failed"),
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "codex discover failed"),
        }
    }

    // 索引统一提交：整轮同步只有 1 次 tantivy commit
    commit_index(state, &pending_index)?;

    // 记录同步时间戳（持久化，供节流与展示）
    if let Ok(repo) = state.repo.lock() {
        let now_ms = (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds();
        let _ = repo.set_setting("last_conv_sync_ms", &now_ms.to_string());
    }

    Ok(serde_json::json!({
        "cancelled": cancelled,
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

/// 导入产出：DTO + 待索引消息（索引延后统一提交）。
struct ImportOutcome {
    dto: ImportResultDto,
    indexable: Vec<ch_search::index::IndexableMessage>,
}

/// 通用导入（不含索引）：RawConversation → DaemonState。
/// 性能：单事务批量写入（每会话一次 fsync），大幅降低主锁占用 → UI 不卡。
fn import_raw_inner(
    state: &DaemonState,
    raw: RawConversation,
    workspace_name: Option<&str>,
    observed_updated_ms: Option<i64>,
) -> Result<ImportOutcome, String> {
    let provider = raw.provider;
    let raw_bytes = serde_json::to_vec(&raw).map_err(|e| internal_err(e))?;
    let raw_store = state.raw_store.lock().map_err(|e| internal_err(e))?;
    let raw_payload = raw_store.put(&raw_bytes).map_err(|e| internal_err(e))?;
    drop(raw_store);

    let normalized = normalize(raw).map_err(|e| internal_err(e))?;

    // 挂 raw_payload + 单事务入库
    let mut conv = normalized.conversation.clone();
    conv.raw_payload_id = Some(raw_payload.hash);
    let repo = state.repo.lock().map_err(|e| internal_err(e))?;
    let conversation_id = repo
        .import_conversation_batch(
            &conv,
            &normalized.messages,
            &normalized.events,
            workspace_name,
            observed_updated_ms,
        )
        .map_err(|e| internal_err(e))?;
    let workspace_id = conv.workspace_id.clone();
    let messages = repo.list_messages(&conversation_id).map_err(|e| internal_err(e))?;
    let conv_title = conv.effective_title().to_string();
    drop(repo);

    // 构建待索引消息（调用方决定何时提交：单条立即，批量最后一次性）
    let indexable = messages
        .iter()
        .map(|m| ch_search::index::IndexableMessage {
            message_id: m.id.clone(),
            conversation_id: conversation_id.clone(),
            provider,
            workspace_id: workspace_id.clone(),
            role: m.role,
            title: Some(conv_title.clone()),
            body: m.content_text.clone(),
        })
        .collect();

    Ok(ImportOutcome {
        dto: ImportResultDto {
            conversation_id,
            workspace_id,
            messages: normalized.messages.len(),
            events: normalized.events.len(),
            completeness: normalized.completeness.label().to_string(),
        },
        indexable,
    })
}

/// 统一提交索引：一个 writer 一次 commit（批量同步的关键性能路径，
/// 旧路径每会话一次 commit，500 会话 = 500 次 segment 落盘）。
fn commit_index(
    state: &DaemonState,
    docs: &[ch_search::index::IndexableMessage],
) -> Result<(), String> {
    if docs.is_empty() {
        return Ok(());
    }
    let idx = state.search_index.lock().map_err(|e| internal_err(e))?;
    let mut writer = idx.writer(15_000_000).map_err(|e| internal_err(e))?;
    for im in docs {
        idx.index_message(&mut writer, im).map_err(|e| internal_err(e))?;
    }
    idx.commit(writer).map_err(|e| internal_err(e))?;
    Ok(())
}

/// 通用导入：RawConversation → DaemonState（repo + search_index + raw_store）。
fn import_raw_to_state(
    state: &DaemonState,
    raw: RawConversation,
    workspace_name: Option<&str>,
    observed_updated_ms: Option<i64>,
) -> Result<ImportResultDto, String> {
    let outcome = import_raw_inner(state, raw, workspace_name, observed_updated_ms)?;
    commit_index(state, &outcome.indexable)?;
    Ok(outcome.dto)
}

/// 导出单条会话为 Markdown 或 JSON 字符串（plan §6.6）。
#[derive(serde::Serialize)]
struct ExportOutput {
    content: String,
    format: String,
    filename: String,
}

#[tauri::command]
async fn export_conversation(
    state: tauri::State<'_, DaemonState>,
    conversation_id: String,
    format: String,
) -> Result<ExportOutput, String> {
    let repo = state.repo.lock().map_err(|e| internal_err(e))?;
    let conv = repo
        .get_conversation(&conversation_id)
        .map_err(|e| internal_err(e))?
        .ok_or_else(|| format!("conversation not found: {conversation_id}"))?;
    let messages = repo.list_messages(&conversation_id).map_err(|e| internal_err(e))?;
    let events = repo.list_events(&conversation_id).map_err(|e| internal_err(e))?;
    let opts = ch_export::ExportOptions::everything();
    let safe_title: String = conv
        .effective_title()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(40)
        .collect();
    let (content, ext) = match format.as_str() {
        "json" => (
            ch_export::to_json(None, &conv, &messages, &events, &opts).map_err(|e| internal_err(e))?,
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
    std::fs::write(&path, content.as_bytes()).map_err(|e| internal_err(e))
}

#[tauri::command]
async fn set_favorite(state: tauri::State<'_, DaemonState>, id: String, favorite: bool) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| internal_err(e))?;
    repo.set_favorite(&id, favorite).map_err(|e| internal_err(e))
}

#[tauri::command]
async fn add_tag(state: tauri::State<'_, DaemonState>, id: String, tag: String) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| internal_err(e))?;
    repo.add_tag(&id, &tag).map_err(|e| internal_err(e))
}

#[tauri::command]
async fn list_tags(state: tauri::State<'_, DaemonState>, id: String) -> Result<Vec<String>, String> {
    let repo = state.read_repo.lock().map_err(|e| internal_err(e))?;
    repo.list_tags(&id).map_err(|e| internal_err(e))
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
        "jsonl" | "ndjson" => ch_adapter_jsonl::parse_file(path).map_err(|e| internal_err(e)),
        _ => ch_adapter_markdown::parse_file(path).map_err(|e| internal_err(e)),
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
        .map_err(|e| internal_err(e))?
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
            let id = repo.upsert_workspace(&ws).map_err(|e| internal_err(e))?;
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
            // 日志：导入失败等 warn 输出到 stderr（此前静默，事故排查困难）
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::INFO)
                .with_target(false)
                .init();

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
            assets_sync,
            assets_list,
            automations_sync,
            automations_list,
            ops_cost_by_dir,
            ops_cache_stats,
            ops_anomalies,
            ops_agent_health,
            ops_latency_stats,
            ops_token_waste,
            ops_agent_benchmark,
            ops_weekly_report,
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
            cancel_sync,
            export_conversation,
            save_text_file,
            set_favorite,
            add_tag,
            list_tags,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Conversation Hub");
}
