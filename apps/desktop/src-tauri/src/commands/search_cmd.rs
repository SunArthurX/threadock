//! 搜索域命令：双引擎消息级搜索 + 按主对话分组 + 会话树内命中步进。
//!
//! - [`search`]：消息级命中（原有命令，从 conversations.rs 迁入，行为不变）。
//! - [`search_grouped`]：GUI 搜索模式左栏用——命中按「主对话」聚合，
//!   子任务命中折叠到所属主对话之下（fix：搜索后左栏不再被平铺的子对话刷屏）。
//! - [`search_tree_hits`]：GUI 右栏命中步进用——某主对话及其全部子任务内
//!   的命中，按「主对话 → 子任务（时间升序）→ 消息序号」阅读顺序返回。

use super::*;
use ch_daemon::DaemonState;

/// 双引擎归一化后的单条命中（Tantivy / FTS5 字段一致，仅来源不同）。
#[derive(Debug, Clone)]
pub(crate) struct EngineHit {
    pub message_id: String,
    pub conversation_id: String,
    pub provider: String,
    pub role: String,
    pub title: Option<String>,
    pub snippet: String,
}

impl EngineHit {
    fn into_dto(self) -> SearchResultDto {
        SearchResultDto {
            message_id: self.message_id,
            conversation_id: self.conversation_id,
            provider: self.provider,
            role: self.role,
            title: self.title,
            snippet: self.snippet,
        }
    }
}

/// 双引擎搜索核心：Tantivy 优先（plan §9.5 主检索），失败/空结果降级 FTS5。
/// 从原 `search` 命令抽出，供消息级 / 分组 / 树内步进三类命令复用。
///
/// `base_limit` 为无 DB 后过滤时的条数上限；有过滤时超量拉取（≥200）避免
/// 过滤后不足一页。空关键词 + 无过滤 + 无 workspace 前缀时走 FTS5 全量。
fn engine_search(
    state: &tauri::State<'_, DaemonState>,
    query: &str,
    role: Option<&str>,
    base_limit: usize,
) -> Result<Vec<EngineHit>, String> {
    // ── 查询语法（plan §13.2）：workspace 名字解析 + DB 级后过滤集合 ──
    let parsed = ch_domain::query_syntax::parse(query);
    let (ws_ids, db_filter) = {
        let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
        let ws_ids = match &parsed.workspace {
            Some(w) => repo
                .workspace_ids_by_name_or_id(w)
                .map_err(|e| storage_err(e))?,
            None => Vec::new(),
        };
        let db_filter = repo
            .search_filter_conversation_ids(&parsed)
            .map_err(|e| storage_err(e))?;
        (ws_ids, db_filter)
    };
    // workspace: 名字解析不出任何候选 → 语义上就是无命中，短路返回
    if parsed.workspace.is_some() && ws_ids.is_empty() {
        return Ok(Vec::new());
    }

    // 空关键词 + 角色筛选 = 全量该角色（如「所有我的提问」）：直接走 FTS5
    if parsed.text.is_empty() && !parsed.needs_db_filter() && parsed.workspace.is_none() {
        let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
        let mut q = ch_storage::SearchQuery::new("").with_limit(base_limit.max(200));
        if let Some(r) = role {
            q = q.with_role(r.to_string());
        }
        let results = repo.search(&q).map_err(|e| storage_err(e))?;
        return Ok(results.into_iter().map(fts_hit).collect());
    }
    // 优先走 Tantivy，降级 FTS5
    let idx = state.search_index.lock().map_err(|e| search_err(e))?;
    let mut q = ch_search::SearchQuery::new(query).with_workspace_ids(ws_ids);
    if let Some(r) = role {
        q = q.with_role(r.to_string());
    }
    // 有 DB 后过滤时超量拉取，避免过滤后不足一页
    if db_filter.is_some() {
        q = q.with_limit(base_limit.max(200));
    }
    let tantivy_hits = idx.search(&q).ok().map(|hits| {
        hits.into_iter()
            .filter(|h| db_filter.as_ref().is_none_or(|set| set.contains(&h.conversation_id)))
            .map(|h| EngineHit {
                message_id: h.message_id,
                conversation_id: h.conversation_id,
                provider: h.provider.to_string(),
                role: h.role.to_string(),
                title: h.title,
                snippet: h.snippet,
            })
            .collect::<Vec<_>>()
    });
    // 有 DB 过滤时结果以过滤后为准（降级 FTS5 不会更优）；否则空结果降级 FTS5
    let use_tantivy = match &tantivy_hits {
        Some(hits) => !hits.is_empty() || db_filter.is_some(),
        None => false,
    };
    if use_tantivy {
        return Ok(tantivy_hits.unwrap_or_default());
    }
    drop(idx);
    fts_search(state, query, role, base_limit)
}

/// FTS5 降级路径（与原 `search` 命令的降级行为一致）。
fn fts_search(
    state: &tauri::State<'_, DaemonState>,
    query: &str,
    role: Option<&str>,
    limit: usize,
) -> Result<Vec<EngineHit>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let mut q = ch_storage::SearchQuery::new(query).with_limit(limit);
    if let Some(r) = role {
        q = q.with_role(r.to_string());
    }
    let results = repo.search(&q).map_err(|e| storage_err(e))?;
    Ok(results.into_iter().map(fts_hit).collect())
}

/// storage::SearchResult → 归一化命中。
fn fts_hit(r: ch_storage::SearchResult) -> EngineHit {
    EngineHit {
        message_id: r.message_id,
        conversation_id: r.conversation_id,
        provider: r.provider.to_string(),
        role: r.role.to_string(),
        title: r.title,
        snippet: r.snippet,
    }
}

#[tauri::command]
#[tracing::instrument(skip_all, level = "debug")]
pub(crate) async fn search(
    state: tauri::State<'_, DaemonState>,
    query: String,
    role: Option<String>,
) -> Result<Vec<SearchResultDto>, String> {
    let hits = engine_search(&state, &query, role.as_deref(), 50)?;
    Ok(hits.into_iter().map(EngineHit::into_dto).collect())
}

/// 一个会话（可能是子任务）在本次搜索中的聚合。
struct ConvAcc {
    root_conversation_id: String,
    root_title: Option<String>,
    root_updated_at_ms: Option<i64>,
    provider: String,
    conversation_id: String,
    title: Option<String>,
    is_child: bool,
    hit_count: i64,
    best_message_id: String,
    best_role: String,
    snippet: String,
}

impl ConvAcc {
    fn into_dto(self) -> SearchHitGroupDto {
        SearchHitGroupDto {
            root_conversation_id: self.root_conversation_id,
            root_title: self.root_title,
            root_updated_at_ms: self.root_updated_at_ms,
            provider: self.provider,
            conversation_id: self.conversation_id,
            title: self.title,
            is_child: self.is_child,
            hit_count: self.hit_count,
            best_message_id: self.best_message_id,
            best_role: self.best_role,
            snippet: self.snippet,
        }
    }
}

/// 搜索结果按主对话分组（GUI 搜索模式左栏）：一次取齐涉及的会话与父会话，
/// 命中按会话聚合；组间顺序保持引擎相关序（首次命中出现顺序）。
#[tauri::command]
#[tracing::instrument(skip_all, level = "debug")]
pub(crate) async fn search_grouped(
    state: tauri::State<'_, DaemonState>,
    query: String,
    role: Option<String>,
) -> Result<Vec<SearchHitGroupDto>, String> {
    let hits = engine_search(&state, &query, role.as_deref(), 500)?;
    if hits.is_empty() {
        return Ok(Vec::new());
    }
    let convs = load_conversation_meta(&state, &hits)?;
    let parents = load_parent_meta(&state, &convs)?;

    // 聚合：key = 命中会话 id；顺序 = 首次命中出现顺序（引擎相关序）
    let mut accs: Vec<ConvAcc> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for h in &hits {
        let Some(conv) = convs.get(&h.conversation_id) else {
            continue; // 会话已被硬删（FTS 残留）：跳过
        };
        let parent = conv
            .source_parent_id
            .as_ref()
            .and_then(|spid| parents.get(&(format!("prov_{}", conv.provider.as_str()), spid.clone())));
        let (root_id, root_title, root_updated_ms, is_child) = match parent {
            Some(p) => (
                p.id.clone(),
                p.user_title.clone().or(p.title.clone()),
                ts_to_ms(p.updated_at),
                true,
            ),
            None => (
                conv.id.clone(),
                conv.user_title.clone().or(conv.title.clone()),
                ts_to_ms(conv.updated_at),
                false,
            ),
        };
        match index.get(&h.conversation_id) {
            Some(&i) => accs[i].hit_count += 1,
            None => {
                index.insert(h.conversation_id.clone(), accs.len());
                accs.push(ConvAcc {
                    root_conversation_id: root_id,
                    root_title,
                    root_updated_at_ms: root_updated_ms,
                    provider: h.provider.clone(),
                    conversation_id: h.conversation_id.clone(),
                    title: conv.user_title.clone().or(conv.title.clone()),
                    is_child,
                    hit_count: 1,
                    best_message_id: h.message_id.clone(),
                    best_role: h.role.clone(),
                    snippet: h.snippet.clone(),
                });
            }
        }
    }
    Ok(accs.into_iter().map(ConvAcc::into_dto).collect())
}

/// 会话树内命中（GUI 右栏步进）：root 主对话 + 其全部子任务内的命中，
/// 按「主对话 → 子任务（更新时间升序）→ 消息序号」排序。
#[tauri::command]
#[tracing::instrument(skip_all, level = "debug")]
pub(crate) async fn search_tree_hits(
    state: tauri::State<'_, DaemonState>,
    query: String,
    root_conversation_id: String,
    role: Option<String>,
) -> Result<Vec<SearchResultDto>, String> {
    let (root, children) = {
        let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
        let root = repo
            .get_conversation(&root_conversation_id)
            .map_err(|e| storage_err(e))?
            .ok_or_else(|| "未找到会话".to_string())?;
        let children = repo
            .list_child_conversations(
                &root.source_conversation_id,
                &format!("prov_{}", root.provider.as_str()),
            )
            .map_err(|e| storage_err(e))?;
        (root, children)
    };

    let hits = engine_search(&state, &query, role.as_deref(), 500)?;
    // 阅读顺序：主对话 0，子任务按更新时间升序 1..n；未知的排最后
    let mut conv_rank: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    conv_rank.insert(root.id.clone(), 0);
    for c in children.iter().rev() {
        // list_child_conversations 返回 updated_at DESC，步进取升序
        conv_rank.insert(c.id.clone(), conv_rank.len());
    }
    let mut filtered: Vec<EngineHit> = hits
        .into_iter()
        .filter(|h| conv_rank.contains_key(&h.conversation_id))
        .collect();
    let order = {
        let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
        repo.message_order_by_ids(&filtered.iter().map(|h| h.message_id.clone()).collect::<Vec<_>>())
            .map_err(|e| storage_err(e))?
    };
    filtered.sort_by_key(|h| {
        let rank = conv_rank.get(&h.conversation_id).copied().unwrap_or(usize::MAX);
        let seq = order
            .get(&h.message_id)
            .map(|(_, s)| *s)
            .unwrap_or(i64::MAX);
        (rank, seq)
    });
    Ok(filtered.into_iter().map(EngineHit::into_dto).collect())
}

/// 命中涉及会话的元信息（id → Conversation）。
fn load_conversation_meta(
    state: &tauri::State<'_, DaemonState>,
    hits: &[EngineHit],
) -> Result<std::collections::HashMap<String, ch_domain::Conversation>, String> {
    let mut ids: Vec<String> = hits.iter().map(|h| h.conversation_id.clone()).collect();
    ids.sort();
    ids.dedup();
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let convs = repo
        .conversations_by_ids(&ids)
        .map_err(|e| storage_err(e))?;
    Ok(convs.into_iter().map(|c| (c.id.clone(), c)).collect())
}

/// 子任务命中的父会话元信息：(provider, source_parent_id) → 父 Conversation。
fn load_parent_meta(
    state: &tauri::State<'_, DaemonState>,
    convs: &std::collections::HashMap<String, ch_domain::Conversation>,
) -> Result<std::collections::HashMap<(String, String), ch_domain::Conversation>, String> {
    // 按 provider 分组收集需要回溯的父 source id，避免逐条查询
    let mut wanted: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for c in convs.values() {
        if let Some(spid) = &c.source_parent_id {
            wanted
                .entry(format!("prov_{}", c.provider.as_str()))
                .or_default()
                .push(spid.clone());
        }
    }
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let mut out = std::collections::HashMap::new();
    for (provider_id, mut source_ids) in wanted {
        source_ids.sort();
        source_ids.dedup();
        let parents = repo
            .conversations_by_source_ids(&provider_id, &source_ids)
            .map_err(|e| storage_err(e))?;
        for p in parents {
            out.insert((provider_id.clone(), p.source_conversation_id.clone()), p);
        }
    }
    Ok(out)
}

/// Prompt 复用推荐（round 25）：用 FTS5 找相似历史 user 消息，
/// 返回「你之前 N 个会话问过类似问题」+ 那次 cost / model。
/// 空 query 返回空列表（防误传）。
#[tauri::command]
#[tracing::instrument(skip_all, level = "debug")]
pub(crate) async fn prompt_reuse_search(
    state: tauri::State<'_, DaemonState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<ch_storage::PromptReuseHit>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.prompt_reuse_search(q, limit.unwrap_or(5))
        .map_err(|e| storage_err(e))
}
