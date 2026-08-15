//! 导入域：手动导入、5 来源会话列表与单条导入、auto_sync 表驱动同步、
//! 单事务批量入库与索引提交。

use super::*;
use ch_daemon::DaemonState;
use ch_normalization::{normalize, RawConversation};
use std::path::Path;

/// 导入一个会话文件（plan §8.3 流水线）。
/// 按扩展名自动选择 markdown/jsonl adapter。
#[tauri::command]
pub(crate) async fn import_file(
    state: tauri::State<'_, DaemonState>,
    path: String,
    workspace_name: Option<String>,
) -> Result<ImportResultDto, String> {
    // 文件读取 + zstd 归档 + 解析 + 批量入库 + 索引均为同步重活，移出 runtime worker 线程
    run_blocking(|| import_file_inner(&state, &path, workspace_name.as_deref()))
}

pub(crate) fn import_file_inner(
    state: &DaemonState,
    path: &str,
    workspace_name: Option<&str>,
) -> Result<ImportResultDto, String> {
    let path_ref = Path::new(path);

    // 1. 归档原始到 Raw Store
    let bytes = std::fs::read(path_ref).map_err(|e| io_err(e))?;
    let raw_store = state.raw_store.lock().map_err(|e| io_err(e))?;
    let raw_payload = raw_store.put(&bytes).map_err(|e| io_err(e))?;
    drop(raw_store);

    // 2. 解析（按扩展名）
    let raw = parse_by_extension(path_ref)?;
    let normalized = normalize(raw).map_err(|e| io_err(e))?;

    // 3. 入库：workspace 归并（复用 resolver）+ 单事务批量写（provider/会话/消息/事件一次提交）
    let repo = state.repo.lock().map_err(|e| import_err(e))?;
    let workspace_id = resolve_workspace(&repo, workspace_name, path_ref)?;

    let mut conv = normalized.conversation;
    conv.workspace_id = workspace_id.clone();
    conv.raw_payload_id = Some(raw_payload.hash);
    let conversation_id = repo
        .import_conversation_batch(&conv, &normalized.messages, &normalized.events, None, None)
        .map_err(|e| storage_err(e))?;
    // 读取入库后的消息用于索引
    let messages = repo
        .list_messages(&conversation_id)
        .map_err(|e| storage_err(e))?;
    let conv_title = conv.effective_title().to_string();
    let provider = conv.provider;
    drop(repo);

    // 4. 同步 Tantivy 索引（plan §9.5）
    let idx = state.search_index.lock().map_err(|e| search_err(e))?;
    let mut writer = idx
        .writer(ch_search::index::DEFAULT_WRITER_HEAP)
        .map_err(|e| search_err(e))?;
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
        idx.index_message(&mut writer, &im)
            .map_err(|e| search_err(e))?;
    }
    idx.commit(writer).map_err(|e| search_err(e))?;

    Ok(ImportResultDto {
        conversation_id,
        workspace_id,
        messages: normalized.messages.len(),
        events: normalized.events.len(),
        completeness: normalized.completeness.label().to_string(),
    })
}

/// 已导入判定上下文：存在集合 + 各 provider 新鲜度表。
pub(crate) struct ImportCtx {
    existing: std::collections::HashSet<(String, String)>,
    states: std::collections::HashMap<String, std::collections::HashMap<String, Option<i64>>>,
}

pub(crate) fn import_ctx(state: &DaemonState) -> ImportCtx {
    let repo = match state.repo.lock() {
        Ok(r) => r,
        Err(_) => {
            return ImportCtx {
                existing: Default::default(),
                states: Default::default(),
            }
        }
    };
    let existing = repo
        .list_conversation_sources()
        .unwrap_or_default()
        .into_iter()
        .collect();
    let mut states = std::collections::HashMap::new();
    for pid in [
        "prov_zcode",
        "prov_claude-code",
        "prov_cursor",
        "prov_minimax-code",
        "prov_codex",
    ] {
        if let Ok(m) = repo.import_state_map(pid) {
            states.insert(pid.to_string(), m);
        }
    }
    ImportCtx { existing, states }
}

/// 「已导入」= 存在 且 源更新时间 ≤ 导入时观察时间（源有新对话 → false，可再导入）。
pub(crate) fn imported_flag(
    ctx: &ImportCtx,
    provider_id: &str,
    source_id: &str,
    src_ms: Option<i64>,
) -> bool {
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
pub(crate) async fn list_zcode_sessions(
    state: tauri::State<'_, DaemonState>,
) -> Result<Vec<SourceSessionDto>, String> {
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
pub(crate) async fn import_from_zcode(
    state: tauri::State<'_, DaemonState>,
    session_id: String,
) -> Result<ImportResultDto, String> {
    run_blocking(|| {
        let home = std::env::var("HOME").map_err(|_| "no HOME")?;
        let db_path = format!("{home}/.zcode/cli/db/db.sqlite");
        let raw = ch_adapter_zcode::parse_session(&db_path, &session_id)
            .map_err(|e| format!("parse zcode: {e}"))?;
        let observed = ch_adapter_zcode::discover_sessions(&db_path)
            .ok()
            .and_then(|v| v.into_iter().find(|s| s.session_id == session_id))
            .map(|s| s.time_updated);
        import_raw_to_state(&state, raw, Some("ZCode"), observed)
    })
}

/// 列出 Claude Code 会话。
#[tauri::command]
pub(crate) async fn list_claude_code_sessions(
    state: tauri::State<'_, DaemonState>,
) -> Result<Vec<SourceSessionDto>, String> {
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
pub(crate) async fn import_from_claude_code(
    state: tauri::State<'_, DaemonState>,
    session_id: String,
) -> Result<ImportResultDto, String> {
    run_blocking(|| {
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
    })
}

/// Cursor state.vscdb 路径。
pub(crate) fn cursor_db_path() -> Result<String, String> {
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    Ok(format!(
        "{home}/Library/Application Support/Cursor/User/globalStorage/state.vscdb"
    ))
}

/// MiniMax runtime-state.sqlite 路径。
pub(crate) fn minimax_db_path() -> Result<String, String> {
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    Ok(format!("{home}/.minimax/v2/sqlite/runtime-state.sqlite"))
}

/// 列出 Cursor 会话。
#[tauri::command]
pub(crate) async fn list_cursor_sessions(
    state: tauri::State<'_, DaemonState>,
) -> Result<Vec<SourceSessionDto>, String> {
    let db = cursor_db_path()?;
    let sessions =
        ch_adapter_cursor::discover_sessions(&db).map_err(|e| format!("discover cursor: {e}"))?;
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
pub(crate) async fn import_from_cursor(
    state: tauri::State<'_, DaemonState>,
    session_id: String,
) -> Result<ImportResultDto, String> {
    run_blocking(|| {
        let db = cursor_db_path()?;
        let raw = ch_adapter_cursor::parse_session(&db, &session_id)
            .map_err(|e| format!("parse cursor: {e}"))?;
        import_raw_to_state(&state, raw, Some("Cursor"), None)
    })
}

/// 列出 MiniMax 会话。
#[tauri::command]
pub(crate) async fn list_minimax_sessions(
    state: tauri::State<'_, DaemonState>,
) -> Result<Vec<SourceSessionDto>, String> {
    let db = minimax_db_path()?;
    let sessions =
        ch_adapter_minimax::discover_sessions(&db).map_err(|e| format!("discover minimax: {e}"))?;
    let ictx = import_ctx(&state);
    Ok(sessions
        .into_iter()
        .map(|s| {
            let mut detail = format!("{} 消息", s.message_count);
            if !s.agent_name.is_empty() {
                detail = format!("{} · {detail}", s.agent_name);
            }
            SourceSessionDto {
                imported: imported_flag(
                    &ictx,
                    "prov_minimax-code",
                    &s.session_id,
                    Some(s.updated_at_ms),
                ),
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
pub(crate) async fn import_from_minimax(
    state: tauri::State<'_, DaemonState>,
    session_id: String,
) -> Result<ImportResultDto, String> {
    run_blocking(|| {
        let db = minimax_db_path()?;
        let raw = ch_adapter_minimax::parse_session(&db, &session_id)
            .map_err(|e| format!("parse minimax: {e}"))?;
        let observed = ch_adapter_minimax::discover_sessions(&db)
            .ok()
            .and_then(|v| v.into_iter().find(|s| s.session_id == session_id))
            .map(|s| s.updated_at_ms);
        import_raw_to_state(&state, raw, Some("MiniMax Code"), observed)
    })
}

/// Codex home 路径。
pub(crate) fn codex_home() -> Result<String, String> {
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    Ok(format!("{home}/.codex"))
}

/// 列出 Codex 会话。
#[tauri::command]
pub(crate) async fn list_codex_sessions(
    state: tauri::State<'_, DaemonState>,
) -> Result<Vec<SourceSessionDto>, String> {
    let home = codex_home()?;
    let sessions =
        ch_adapter_codex::discover_sessions(&home).map_err(|e| format!("discover codex: {e}"))?;
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
pub(crate) async fn import_from_codex(
    state: tauri::State<'_, DaemonState>,
    session_id: String,
) -> Result<ImportResultDto, String> {
    run_blocking(|| {
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
    })
}

/// 启动时自动拉取 ZCode / Claude Code / Cursor / MiniMax 最新会话（plan §6.1 自动发现/同步）。
/// 返回导入统计。最多各导入 limit 个最新会话。
/// 若已有重置/同步在进行中，返回「同步中」标记（不阻塞 UI）。
#[tauri::command]
pub(crate) async fn auto_sync(
    state: tauri::State<'_, DaemonState>,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let _guard = BusyGuard::acquire()?;
    // 同步重活（5 来源发现+解析+批量入库，可达数分钟）移出 runtime worker 线程
    run_blocking(|| auto_sync_inner(&state, limit))
}

/// 解析闭包类型别名（单来源会话 → RawConversation）。
type ParseFn<'a> =
    Box<dyn Fn(&SourceItem) -> Result<ch_normalization::RawConversation, String> + 'a>;

/// 单个来源会话的归一化描述（表驱动同步的统一中间形态）。
pub(crate) struct SourceItem {
    session_id: String,
    /// 源侧更新时间（毫秒）；0 = 源不提供（如 Cursor）。
    src_ms: i64,
    /// 传给 import_raw_inner 的观察时间（None = 源无可靠时间）。
    observed_ms: Option<i64>,
    /// 文件型来源（Claude Code / Codex）的会话文件路径。
    file_path: Option<String>,
    /// 子任务 → 主任务的父链（ZCode / MiniMax），用于 repair。
    parent_id: Option<String>,
}

/// 来源描述：发现 + 解析闭包，驱动统一同步循环（旧实现为 5 段复制粘贴）。
pub(crate) struct SourceSync<'a> {
    /// providers 表 id（如 prov_zcode）。
    provider_id: &'static str,
    /// 统计 JSON 键前缀（如 zcode → zcode_imported / zcode_skipped）。
    stat_key: &'static str,
    /// 归并用 workspace 显示名。
    workspace: &'static str,
    /// 是否把全部发现结果（含跳过）写回 import_state 观察表
    /// （ZCode / MiniMax 有子任务层级，需要观察表支撑增量判定）。
    records_observed: bool,
    /// 发现（含来源存在性检查；不存在返回空列表）。
    discover: Box<dyn FnOnce() -> Result<Vec<SourceItem>, String> + 'a>,
    /// 解析单个会话为 RawConversation。
    parse: ParseFn<'a>,
}

/// 5 个来源的同步描述表（home = 用户主目录）。
pub(crate) fn source_table(home: &str) -> Vec<SourceSync<'_>> {
    let mk = |exists: bool| -> bool { exists };
    let mut out: Vec<SourceSync> = Vec::new();

    // ZCode：SQLite 直读，discover_all 含子任务（parent 链）
    let zcode_db = format!("{home}/.zcode/cli/db/db.sqlite");
    if mk(std::path::Path::new(&zcode_db).exists()) {
        let db = zcode_db.clone();
        let db2 = zcode_db;
        out.push(SourceSync {
            provider_id: "prov_zcode",
            stat_key: "zcode",
            workspace: "ZCode",
            records_observed: true,
            discover: Box::new(move || {
                ch_adapter_zcode::discover_all_sessions(&db)
                    .map(|v| {
                        v.into_iter()
                            .map(|s| SourceItem {
                                src_ms: s.time_updated,
                                observed_ms: Some(s.time_updated),
                                session_id: s.session_id,
                                file_path: None,
                                parent_id: s.parent_id,
                            })
                            .collect()
                    })
                    .map_err(|e| e.to_string())
            }),
            parse: Box::new(move |it| {
                ch_adapter_zcode::parse_session(&db2, &it.session_id).map_err(|e| e.to_string())
            }),
        });
    }

    // Claude Code：projects 目录下 JSONL（mtime 即源更新时间）
    let claude_home = format!("{home}/.claude");
    if mk(std::path::Path::new(&claude_home).join("projects").exists()) {
        let ch = claude_home;
        out.push(SourceSync {
            provider_id: "prov_claude-code",
            stat_key: "claude_code",
            workspace: "Claude Code",
            records_observed: false,
            discover: Box::new(move || {
                ch_adapter_claude_code::discover_sessions(&ch)
                    .map(|v| {
                        v.into_iter()
                            .map(|s| SourceItem {
                                session_id: s.session_id,
                                src_ms: s.mtime_ms.unwrap_or(0),
                                observed_ms: s.mtime_ms,
                                file_path: Some(s.file_path.to_string_lossy().into_owned()),
                                parent_id: None,
                            })
                            .collect()
                    })
                    .map_err(|e| e.to_string())
            }),
            parse: Box::new(move |it| {
                ch_adapter_claude_code::parse_session(it.file_path.as_deref().unwrap_or_default())
                    .map_err(|e| e.to_string())
            }),
        });
    }

    // Cursor：state.vscdb（源侧无更新时间，恒视为可导入但靠内容幂等）
    let cursor_db =
        format!("{home}/Library/Application Support/Cursor/User/globalStorage/state.vscdb");
    if mk(std::path::Path::new(&cursor_db).exists()) {
        let db = cursor_db.clone();
        let db2 = cursor_db;
        out.push(SourceSync {
            provider_id: "prov_cursor",
            stat_key: "cursor",
            workspace: "Cursor",
            records_observed: false,
            discover: Box::new(move || {
                ch_adapter_cursor::discover_sessions(&db)
                    .map(|v| {
                        v.into_iter()
                            .map(|s| SourceItem {
                                session_id: s.session_id,
                                src_ms: 0,
                                observed_ms: None,
                                file_path: None,
                                parent_id: None,
                            })
                            .collect()
                    })
                    .map_err(|e| e.to_string())
            }),
            parse: Box::new(move |it| {
                ch_adapter_cursor::parse_session(&db2, &it.session_id).map_err(|e| e.to_string())
            }),
        });
    }

    // MiniMax：runtime-state.sqlite，discover_all 含子任务（parent 链）
    let mm_db = format!("{home}/.minimax/v2/sqlite/runtime-state.sqlite");
    if mk(std::path::Path::new(&mm_db).exists()) {
        let db = mm_db.clone();
        let db2 = mm_db;
        out.push(SourceSync {
            provider_id: "prov_minimax-code",
            stat_key: "minimax",
            workspace: "MiniMax Code",
            records_observed: true,
            discover: Box::new(move || {
                ch_adapter_minimax::discover_all_sessions(&db)
                    .map(|v| {
                        v.into_iter()
                            .map(|s| SourceItem {
                                session_id: s.session_id,
                                src_ms: s.updated_at_ms,
                                observed_ms: Some(s.updated_at_ms),
                                file_path: None,
                                parent_id: s.parent_session_id,
                            })
                            .collect()
                    })
                    .map_err(|e| e.to_string())
            }),
            parse: Box::new(move |it| {
                ch_adapter_minimax::parse_session(&db2, &it.session_id).map_err(|e| e.to_string())
            }),
        });
    }

    // Codex (ChatGPT CLI/Desktop)：~/.codex/sessions 下 JSONL
    if mk(std::path::Path::new(&format!("{home}/.codex/sessions")).exists()) {
        let codex_root = format!("{home}/.codex");
        out.push(SourceSync {
            provider_id: "prov_codex",
            stat_key: "codex",
            workspace: "Codex",
            records_observed: false,
            discover: Box::new(move || {
                ch_adapter_codex::discover_sessions(&codex_root)
                    .map(|v| {
                        v.into_iter()
                            .map(|s| SourceItem {
                                session_id: s.session_id,
                                src_ms: s.mtime_ms.unwrap_or(0),
                                observed_ms: s.mtime_ms,
                                file_path: Some(s.file_path),
                                parent_id: None,
                            })
                            .collect()
                    })
                    .map_err(|e| e.to_string())
            }),
            parse: Box::new(move |it| {
                ch_adapter_codex::parse_session(it.file_path.as_deref().unwrap_or_default())
                    .map_err(|e| e.to_string())
            }),
        });
    }
    out
}

/// auto_sync 实现：按 [`source_table`] 表驱动拉取各来源最新会话。
/// 性能要点：
/// - 幂等检查用一次性加载的 HashSet（旧版每会话全表扫描 → 卡顿根因之一）
/// - 已导入但缺主子链路的旧数据，用 repair_parents_batch 补上
/// - limit 默认 500（覆盖全部会话；旧版 50 导致 MiniMax/ZCode 大量子任务丢失）
#[tracing::instrument(skip_all, level = "info")]
pub(crate) fn auto_sync_inner(
    state: &DaemonState,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    CANCEL_SYNC.store(false, std::sync::atomic::Ordering::SeqCst);
    let mut cancelled = false;
    let lim = limit.unwrap_or(500);

    let home = std::env::var("HOME").map_err(|_| "no HOME")?;

    // 全部导入完成后一次性提交索引（单 writer 单 commit，性能关键路径）
    let mut pending_index: Vec<ch_search::index::IndexableMessage> = Vec::new();

    // 一次性加载已导入集合 + 新鲜度表（增量导入：stale 会话也要重导入新消息）
    type SrcKey = (String, String);
    let (existing, istate): (
        std::collections::HashSet<SrcKey>,
        std::collections::HashMap<SrcKey, i64>,
    ) = {
        let repo = state.repo.lock().map_err(|e| storage_err(e))?;
        let sources = repo
            .list_conversation_sources()
            .map_err(|e| storage_err(e))?;
        let mut set = std::collections::HashSet::new();
        let mut state_map = std::collections::HashMap::new();
        for (pid, sid) in sources {
            set.insert((pid.clone(), sid.clone()));
            state_map.insert((pid, sid), 0); // 默认 0 = 未知，视为 stale
        }
        // 覆盖已知 observed_ms
        for src in source_table(&home) {
            if let Ok(m) = repo.import_state_map(src.provider_id) {
                for (sid, obs) in m {
                    if let Some(v) = obs {
                        state_map.insert((src.provider_id.to_string(), sid), v);
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

    // ── 统一同步循环（旧实现为 5 段结构相同的复制粘贴）───────────────────
    // 每 5 个新导入让出 1ms，给 UI 查询抢锁窗口（消除锁饿死卡顿）
    // 全部来源的统计键预置为 0（输出契约与旧版一致：来源不存在也输出 0 计数）
    let mut stats: std::collections::BTreeMap<&'static str, (u32, u32)> = [
        ("zcode", (0, 0)),
        ("claude_code", (0, 0)),
        ("cursor", (0, 0)),
        ("minimax", (0, 0)),
        ("codex", (0, 0)),
    ]
    .into_iter()
    .collect();
    for src in source_table(&home) {
        let items = match (src.discover)() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(provider = src.provider_id, error = %e, "discover failed");
                stats.insert(src.stat_key, (0, 0));
                continue;
            }
        };
        let mut ok = 0u32;
        let mut skip = 0u32;
        let mut imported_count = 0u32;
        let mut repairs: Vec<(String, String)> = Vec::new();
        let mut observed: Vec<(String, Option<i64>)> = Vec::new();
        // 配额语义修复（2026-08-15 真实事故）：lim 只限「本轮新导入」条数，
        // 已最新的跳过不占额度。旧实现 take(lim) 按更新时间降序截断——
        // 源 641 条 / 库 506 条时，未导入的 135 条永远排在尾部轮不到，
        // 表现为「增量同步没同步 / 红点永不灭」。
        for item in items {
            if src.records_observed {
                observed.push((item.session_id.clone(), Some(item.src_ms)));
            }
            if CANCEL_SYNC.load(std::sync::atomic::Ordering::SeqCst) {
                cancelled = true;
                break;
            }
            let key = (src.provider_id.to_string(), item.session_id.clone());
            if existing.contains(&key) && !is_stale(src.provider_id, &item.session_id, item.src_ms)
            {
                if let Some(parent) = item.parent_id {
                    repairs.push((item.session_id.clone(), parent));
                }
                skip += 1;
                continue;
            }
            // 本轮导入配额已满：剩余新会话留到下一轮（observed/repair 继续收集）
            if imported_count >= lim as u32 {
                continue;
            }
            match (src.parse)(&item) {
                Ok(raw) => {
                    match import_raw_inner(state, raw, Some(src.workspace), item.observed_ms) {
                        Ok(o) => {
                            pending_index.extend(o.indexable);
                            ok += 1;
                            imported_count += 1;
                            if imported_count.is_multiple_of(5) {
                                std::thread::sleep(std::time::Duration::from_millis(1));
                            }
                        }
                        Err(e) => {
                            tracing::warn!(session = %item.session_id, error = %e, "import failed")
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(session = %item.session_id, error = %e, "parse failed")
                }
            }
        }
        // 跳过的子任务补主子链路（单事务批量）
        if !repairs.is_empty() {
            if let Ok(repo) = state.repo.lock() {
                if let Err(e) = repo.repair_parents_batch(src.provider_id, &repairs) {
                    tracing::warn!(error = %e, "repair parents failed");
                }
            }
        }
        if src.records_observed {
            if let Ok(repo) = state.repo.lock() {
                if let Err(e) = repo.record_import_states(src.provider_id, &observed) {
                    tracing::warn!(error = %e, "record import states failed (stale 会话将重复导入)");
                }
            }
        }
        stats.insert(src.stat_key, (ok, skip));
    }

    // 索引统一提交：整轮同步只有 1 次 tantivy commit
    commit_index(state, &pending_index)?;

    // 记录同步时间戳（持久化，供节流与展示）
    if let Ok(repo) = state.repo.lock() {
        let now_ms = (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds();
        if let Err(e) = repo.set_setting("last_conv_sync_ms", &now_ms.to_string()) {
            tracing::warn!(error = %e, "persist last_conv_sync_ms failed");
        }
    }

    // 输出键与旧版完全一致（前端契约不变）
    let mut map = serde_json::Map::new();
    map.insert("cancelled".into(), serde_json::json!(cancelled));
    for (key, (ok, skip)) in stats {
        map.insert(format!("{key}_imported"), serde_json::json!(ok));
        map.insert(format!("{key}_skipped"), serde_json::json!(skip));
    }
    Ok(serde_json::Value::Object(map))
}

/// 导入产出：DTO + 待索引消息（索引延后统一提交）。
pub(crate) struct ImportOutcome {
    dto: ImportResultDto,
    indexable: Vec<ch_search::index::IndexableMessage>,
}

/// 通用导入（不含索引）：RawConversation → DaemonState。
/// 性能：单事务批量写入（每会话一次 fsync），大幅降低主锁占用 → UI 不卡。
pub(crate) fn import_raw_inner(
    state: &DaemonState,
    raw: RawConversation,
    workspace_name: Option<&str>,
    observed_updated_ms: Option<i64>,
) -> Result<ImportOutcome, String> {
    let provider = raw.provider;
    let raw_bytes = serde_json::to_vec(&raw).map_err(|e| storage_err(e))?;
    let raw_store = state.raw_store.lock().map_err(|e| io_err(e))?;
    let raw_payload = raw_store.put(&raw_bytes).map_err(|e| io_err(e))?;
    drop(raw_store);

    let normalized = normalize(raw).map_err(|e| io_err(e))?;

    // 挂 raw_payload + 单事务入库
    let mut conv = normalized.conversation.clone();
    conv.raw_payload_id = Some(raw_payload.hash);
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    let conversation_id = repo
        .import_conversation_batch(
            &conv,
            &normalized.messages,
            &normalized.events,
            workspace_name,
            observed_updated_ms,
        )
        .map_err(|e| storage_err(e))?;
    let workspace_id = conv.workspace_id.clone();
    let messages = repo
        .list_messages(&conversation_id)
        .map_err(|e| storage_err(e))?;
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
pub(crate) fn commit_index(
    state: &DaemonState,
    docs: &[ch_search::index::IndexableMessage],
) -> Result<(), String> {
    if docs.is_empty() {
        return Ok(());
    }
    let idx = state.search_index.lock().map_err(|e| search_err(e))?;
    let mut writer = idx
        .writer(ch_search::index::DEFAULT_WRITER_HEAP)
        .map_err(|e| search_err(e))?;
    for im in docs {
        idx.index_message(&mut writer, im)
            .map_err(|e| search_err(e))?;
    }
    idx.commit(writer).map_err(|e| search_err(e))?;
    Ok(())
}

/// 通用导入：RawConversation → DaemonState（repo + search_index + raw_store）。
pub(crate) fn import_raw_to_state(
    state: &DaemonState,
    raw: RawConversation,
    workspace_name: Option<&str>,
    observed_updated_ms: Option<i64>,
) -> Result<ImportResultDto, String> {
    let outcome = import_raw_inner(state, raw, workspace_name, observed_updated_ms)?;
    commit_index(state, &outcome.indexable)?;
    Ok(outcome.dto)
}

/// 按扩展名选择 adapter（plan §10.5）。
pub(crate) fn parse_by_extension(path: &Path) -> Result<RawConversation, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jsonl" | "ndjson" => ch_adapter_jsonl::parse_file(path).map_err(|e| storage_err(e)),
        _ => ch_adapter_markdown::parse_file(path).map_err(|e| storage_err(e)),
    }
}

/// workspace 归并（复用 identity-resolver，plan §4.3）。
pub(crate) fn resolve_workspace(
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
        .map_err(|e| storage_err(e))?
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
        ch_identity_resolver::Resolution::NeedsConfirmation {
            candidate: Some(m), ..
        } => Ok(Some(m.workspace_id)),
        _ => {
            let mut ws = ch_domain::Workspace::new(name);
            ws.canonical_path = path.parent().map(|p| p.to_string_lossy().into_owned());
            let id = repo.upsert_workspace(&ws).map_err(|e| storage_err(e))?;
            Ok(Some(id))
        }
    }
}

/// 各来源「未导入新内容」计数（导入按钮红点数据源）。
///
/// 判定口径与列表页一致（imported_flag：未导入或源有更新 = 新）。
/// discover 全量但只读不写，属于同步级别的重活 → run_blocking。
#[tauri::command]
pub(crate) async fn sources_new_count(
    state: tauri::State<'_, DaemonState>,
) -> Result<serde_json::Value, String> {
    run_blocking(move || sources_new_count_inner(&state))
}

/// 活跃宽限：已导入且源在近 N 分钟内更新过的会话（正在使用的活跃会话）
/// 不计为「新内容」——否则正在运行的 agent 会话会让红点永不熄灭。
const ACTIVE_GRACE_MS: i64 = 5 * 60 * 1000;

fn sources_new_count_inner(state: &DaemonState) -> Result<serde_json::Value, String> {
    let home = std::env::var("HOME").map_err(|_| "no HOME".to_string())?;
    let ctx = import_ctx(state);
    let now_ms =
        (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
    let mut map = serde_json::Map::new();
    let mut total = 0u64;
    for src in source_table(&home) {
        let items = match (src.discover)() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(provider = src.provider_id, error = %e, "discover failed");
                continue;
            }
        };
        let empty: std::collections::HashMap<String, Option<i64>> = Default::default();
        let states = ctx.states.get(src.provider_id).unwrap_or(&empty);
        let n = items
            .iter()
            .filter(|it| {
                if !ctx
                    .existing
                    .contains(&(src.provider_id.to_string(), it.session_id.clone()))
                {
                    return true; // 从未导入 = 真·新内容
                }
                // 已导入：仅当「源有更新且已冷却」才算新（活跃会话不闪红点）
                let up_to_date = ch_storage::Repository::is_up_to_date(
                    states,
                    &ctx.existing,
                    src.provider_id,
                    &it.session_id,
                    Some(it.src_ms),
                );
                !up_to_date && (now_ms - it.src_ms) > ACTIVE_GRACE_MS
            })
            .count() as u64;
        map.insert(src.stat_key.to_string(), serde_json::json!(n));
        total += n;
    }
    map.insert("total".into(), serde_json::json!(total));
    Ok(serde_json::Value::Object(map))
}

#[cfg(test)]
mod new_count_tests {
    use super::*;

    #[test]
    fn sources_new_count_empty_home_is_zero() {
        // 空环境：无来源可发现 → total = 0（红点灭）
        let state = DaemonState::open_in_memory().expect("state open");
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        std::env::set_var("HOME", dir.path());
        let v = sources_new_count_inner(&state).expect("count");
        assert_eq!(v.get("total"), Some(&serde_json::json!(0)));
    }
}

#[cfg(test)]
mod backlog_e2e_tests {
    use super::*;
    use ch_daemon::{DaemonState, DaemonStateConfig};

    /// 真实环境回归（手动跑）：拷贝真实 app 库 + 真实 HOME 源，
    /// 验证配额修复后积压的未导入会话能被消化。
    /// cargo test --lib backlog -- --ignored --nocapture
    #[test]
    #[ignore = "依赖本机真实 ~/.zcode 数据与 app 库副本，CI 不跑"]
    fn sync_consumes_backlog_on_real_copy() {
        let app_db = std::path::PathBuf::from(std::env::var("HOME").expect("unexpected None"))
            .join("Library/Application Support/com.threadock.desktop/threadock.db");
        if !app_db.exists() {
            panic!("本机无真实 app 库，跳过语义不适用");
        }
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        std::fs::copy(&app_db, dir.path().join("threadock.db")).expect("file I/O failed");
        let state = DaemonState::open(DaemonStateConfig {
            data_dir: dir.path().to_path_buf(),
        })
        .expect("state open");

        let before = sources_new_count_inner(&state).expect("count before");
        let v = auto_sync_inner(&state, None).expect("sync");
        let after = sources_new_count_inner(&state).expect("count after");
        println!("before={before}");
        println!("sync={v}");
        println!("after={after}");
        let before_total = before.get("total").and_then(|t| t.as_u64()).unwrap_or(0);
        let after_total = after.get("total").and_then(|t| t.as_u64()).unwrap_or(0);
        let imported = v
            .get("zcode_imported")
            .and_then(|t| t.as_u64())
            .unwrap_or(0);
        // 有积压时本轮必须消化（不再被 take(lim) 截断挡住）
        if before_total > 0 {
            assert!(
                imported > 0,
                "backlog {before_total} 存在时本轮必须导入，实际 {imported}"
            );
            assert!(
                after_total < before_total,
                "未导入计数必须下降: {before_total} -> {after_total}"
            );
        }
    }
}
