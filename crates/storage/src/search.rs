//! 全文搜索，对应 plan §13「检索与知识化能力」与 §9.5「FTS5 作为 MVP/降级方案」。
//!
//! MVP 实现：
//! - 用 `SQLite` `FTS5（messages_fts` 虚拟表，见 schema）。
//! - 支持关键词 + provider / workspace 过滤。
//! - 返回命中片段（snippet）与所属 conversation/message。
//! - 中文靠 `unicode61` 分词 + 字符级匹配兜底（plan §13：N-gram 兜底在 Tantivy 阶段完善）。
//!
//! 未来切换到 Tantivy 时，本模块的 `SearchQuery`/`SearchResult` 契约保持不变。

use crate::error::StorageResult;
use ch_domain::{Provider, Role, Timestamp};
use rusqlite::params_from_iter;
use std::sync::MutexGuard;

/// 搜索条件。
#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    /// FTS5 MATCH 表达式，例如 `"tauri android"`。
    /// 若为空则退化为全量（仅按过滤条件）。
    pub query: String,
    pub provider: Option<Provider>,
    pub workspace_id: Option<String>,
    /// 角色过滤（"user" = 仅用户提问；空 query + role = 全量该角色）。
    pub role: Option<String>,
    pub limit: usize,
}

impl SearchQuery {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            provider: None,
            workspace_id: None,
            role: None,
            limit: 50,
        }
    }
    #[must_use]
    pub fn with_provider(mut self, p: Provider) -> Self {
        self.provider = Some(p);
        self
    }
    #[must_use]
    pub fn with_workspace(mut self, id: impl Into<String>) -> Self {
        self.workspace_id = Some(id.into());
        self
    }
    /// 角色过滤（"user" = 仅我的提问）。
    #[must_use]
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.role = Some(role.into());
        self
    }
    #[must_use]
    pub fn with_limit(mut self, n: usize) -> Self {
        self.limit = n;
        self
    }
}

/// 单条命中。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SearchResult {
    pub message_id: String,
    pub conversation_id: String,
    pub provider: Provider,
    pub workspace_id: Option<String>,
    pub role: Role,
    pub title: Option<String>,
    /// 命中片段（FTS5 snippet，已 HTML 转义并高亮 `<b>...</b>`）。
    pub snippet: String,
    /// 该消息原文（便于直接展示）。
    pub body: Option<String>,
    pub created_at: Option<Timestamp>,
}

/// Prompt 复用推荐命中（round 25）：相似历史 user 消息 + 当时 cost/model。
/// 用于「你之前 3 个会话问过类似问题」+ 一键跳到原会话复用。
#[derive(Debug, Clone, serde::Serialize)]
pub struct PromptReuseHit {
    pub message_id: String,
    pub conversation_id: String,
    pub title: Option<String>,
    pub user_title: Option<String>,
    pub model: Option<String>,
    pub provider_name: String,
    /// 命中片段（FTS5 snippet，已 HTML 转义并高亮 `<b>...</b>`）。
    pub snippet: String,
    /// 该消息原文（用于复制到剪贴板）。
    pub body: String,
    /// 该会话累计 cost（USD；按 source_session_id 聚合）。
    pub cost_usd: f64,
}

/// 把用户输入的裸关键词转成 FTS5 安全的 MATCH 表达式。
///
/// 策略：对每个 token 加双引号包裹，避免被当 FTS5 语法（如 `OR`、`*`、`:`）误解析。
/// 这保证「用户搜什么就匹配什么字面量」，符合 MVP 的可预期性。
#[must_use]
pub fn build_match_expr(user_input: &str) -> String {
    user_input
        .split_whitespace()
        .map(|tok| {
            // 去掉 token 内部的双引号，避免破坏 FTS5 字符串语法
            let clean: String = tok.chars().filter(|c| *c != '"').collect();
            format!("\"{clean}\"")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// 执行搜索。`conn_guard` 由 Repository 转入，保证锁一致。
///
/// `q.query` 支持查询语法（plan §13.2）：`provider:` `workspace:` `type:` `role:`
/// `status:` `file:` `model:` `after:` `before:` 前缀会被解析为过滤条件，
/// 其余作为自由文本进入 FTS5 MATCH（解析见 `ch_domain::query_syntax`）。
pub(super) fn search(
    conn: &MutexGuard<'_, rusqlite::Connection>,
    q: &SearchQuery,
) -> StorageResult<Vec<SearchResult>> {
    let parsed = ch_domain::query_syntax::parse(&q.query);
    let mut where_clauses: Vec<String> = Vec::new();
    let mut args: Vec<rusqlite::types::Value> = Vec::new();

    // MATCH：在 title 和 body 两列上检索（仅自由文本部分）
    let match_expr = build_match_expr(&parsed.text);
    if !match_expr.is_empty() {
        where_clauses.push("messages_fts MATCH ?".to_string());
        // MATCH 表达式必须作为整体字符串绑定到虚拟表列；
        // 这里用 `{title body} : <expr>` 语法限定列范围
        args.push(format!("{{title body}} : {match_expr}").into());
    }
    if let Some(p) = q.provider {
        where_clauses.push("provider = ?".to_string());
        args.push(p.as_str().to_string().into());
    } else if let Some(p) = &parsed.provider {
        // 语法前缀 provider:xxx —— 按原始字符串比较，未知来源自然无命中
        where_clauses.push("provider = ?".to_string());
        args.push(p.clone().into());
    }
    if let Some(wsid) = &q.workspace_id {
        where_clauses.push("workspace_id = ?".to_string());
        args.push(wsid.clone().into());
    } else if let Some(ws) = &parsed.workspace {
        // 语法前缀 workspace:xxx —— id 或 display_name（大小写不敏感）都接受
        where_clauses.push(
            "(workspace_id = ? OR workspace_id IN \
             (SELECT id FROM workspaces WHERE display_name = ? COLLATE NOCASE))"
                .to_string(),
        );
        args.push(ws.clone().into());
        args.push(ws.clone().into());
    }
    let role = q.role.clone().or_else(|| parsed.role.clone());
    if let Some(role) = role {
        where_clauses.push("role = ?".to_string());
        args.push(role.into());
    }
    push_conversation_level_filters(&parsed, &mut where_clauses, &mut args);

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    // rank() 让 FTS5 按相关性排序；snippet() 生成带高亮的命中片段。
    // 高亮标记用控制字符 char(1)/char(2)（正文中不会出现），
    // 取回后先整体 HTML 转义再替换为 <b>，防止正文中的 HTML 注入（存储型 XSS）。
    let sql = format!(
        "SELECT message_id, conversation_id, provider, workspace_id, role, title,
                snippet(messages_fts, 6, char(1), char(2), '…', 16) AS snip,
                body, ''
         FROM messages_fts
         {where_sql}
         ORDER BY rank
         LIMIT ?"
    );
    args.push((q.limit as i64).into());

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
        let provider_str: String = r.get(2)?;
        let provider = Provider::from_str(&provider_str).unwrap_or(Provider::Unknown);
        let role_str: String = r.get(4)?;
        let role = match role_str.as_str() {
            "assistant" => Role::Assistant,
            "system" => Role::System,
            "tool" => Role::Tool,
            _ => Role::User,
        };
        let body: Option<String> = r.get(7)?;
        let raw_snip: String = r.get(6)?;
        let snippet = ch_domain::html::escape_html(&raw_snip)
            .replace('\u{1}', "<b>")
            .replace('\u{2}', "</b>");
        Ok(SearchResult {
            message_id: r.get(0)?,
            conversation_id: r.get(1)?,
            provider,
            workspace_id: r.get::<_, Option<String>>(3)?,
            role,
            title: r.get(5)?,
            snippet,
            body,
            created_at: None,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

// 让 domain 的 FromStr 在本模块可见
use std::str::FromStr as _;

/// 语法过滤中需要查 conversations / events 的条件（plan §13.2 全量语法），
/// 以子查询形式叠加到 WHERE。从 [`search`] 拆出以控制主函数行数。
fn push_conversation_level_filters(
    parsed: &ch_domain::query_syntax::ParsedQuery,
    where_clauses: &mut Vec<String>,
    args: &mut Vec<rusqlite::types::Value>,
) {
    if let Some(after) = parsed.after_ms {
        where_clauses.push(
            "conversation_id IN (SELECT id FROM conversations WHERE updated_at >= ?)".to_string(),
        );
        args.push(after.into());
    }
    if let Some(before) = parsed.before_ms {
        where_clauses.push(
            "conversation_id IN (SELECT id FROM conversations WHERE updated_at <= ?)".to_string(),
        );
        args.push(before.into());
    }
    if let Some(model) = &parsed.model {
        where_clauses.push(
            "conversation_id IN (SELECT id FROM conversations WHERE model LIKE ?)".to_string(),
        );
        args.push(format!("%{model}%").into());
    }
    if let Some(file) = &parsed.file {
        where_clauses.push(
            "conversation_id IN (SELECT conversation_id FROM events \
             WHERE summary LIKE ? OR payload_json LIKE ?)"
                .to_string(),
        );
        args.push(format!("%{file}%").into());
        args.push(format!("%{file}%").into());
    }
    match parsed.status.as_deref() {
        Some("favorite") => where_clauses.push(
            "conversation_id IN (SELECT id FROM conversations WHERE favorite = 1)".to_string(),
        ),
        Some("archived") => where_clauses.push(
            "conversation_id IN (SELECT id FROM conversations WHERE is_archived = 1)".to_string(),
        ),
        Some("deleted") => where_clauses.push(
            "conversation_id IN (SELECT id FROM conversations WHERE source_status = 'deleted')"
                .to_string(),
        ),
        Some("active") => where_clauses.push(
            "conversation_id IN (SELECT id FROM conversations \
             WHERE source_status != 'deleted' AND is_archived = 0)"
                .to_string(),
        ),
        _ => {}
    }
}

/// Tantivy 路径的数据库后过滤（plan §13.2 全量语法）。
///
/// Tantivy 索引内只覆盖 provider/workspace/role；`status:` `file:` `model:`
/// `after:` `before:` 需要查 SQLite。本函数返回允许通过的 conversation id 集合；
/// 无任何 DB 级过滤时返回 `None`（调用方跳过后过滤）。
///
/// 返回 `Some(空集合)` 表示过滤条件存在但无任何会话满足（命中应清空）。
pub fn filter_conversation_ids(
    conn: &MutexGuard<'_, rusqlite::Connection>,
    p: &ch_domain::query_syntax::ParsedQuery,
) -> StorageResult<Option<std::collections::HashSet<String>>> {
    if !p.needs_db_filter() {
        return Ok(None);
    }
    let mut where_clauses: Vec<String> = Vec::new();
    let mut args: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(after) = p.after_ms {
        where_clauses.push("updated_at >= ?".to_string());
        args.push(after.into());
    }
    if let Some(before) = p.before_ms {
        where_clauses.push("updated_at <= ?".to_string());
        args.push(before.into());
    }
    if let Some(model) = &p.model {
        where_clauses.push("model LIKE ?".to_string());
        args.push(format!("%{model}%").into());
    }
    if let Some(file) = &p.file {
        // file 过滤跨表：会话的事件摘要/payload 里含该路径子串
        where_clauses.push(
            "id IN (SELECT conversation_id FROM events WHERE summary LIKE ? OR payload_json LIKE ?)"
                .to_string(),
        );
        args.push(format!("%{file}%").into());
        args.push(format!("%{file}%").into());
    }
    match p.status.as_deref() {
        Some("favorite") => where_clauses.push("favorite = 1".to_string()),
        Some("archived") => where_clauses.push("is_archived = 1".to_string()),
        Some("deleted") => where_clauses.push("source_status = 'deleted'".to_string()),
        Some("active") => {
            where_clauses.push("source_status != 'deleted' AND is_archived = 0".to_string())
        }
        _ => {}
    }

    let sql = format!(
        "SELECT id FROM conversations WHERE {}",
        where_clauses.join(" AND ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
        let id: String = r.get(0)?;
        Ok(id)
    })?;
    let mut set = std::collections::HashSet::new();
    for row in rows {
        set.insert(row?);
    }
    Ok(Some(set))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::Repository;
    use ch_domain::{Conversation, Message, Provider, Role};

    fn seed() -> Repository {
        let r = Repository::open_in_memory().expect("unexpected None");
        r.upsert_provider(Provider::Generic).expect("upsert failed");
        r.upsert_provider(Provider::Codex).expect("upsert failed");

        // workspace
        let mut ws = ch_domain::Workspace::new("tauri-project");
        let wid = r.upsert_workspace(&ws).expect("upsert failed");
        ws.id = wid.clone();

        // conversation 1 (Generic) — 谈 Tauri Android
        let mut c1 = Conversation::new(Provider::Generic, "src-1");
        c1.workspace_id = Some(wid.clone());
        c1.title = Some("Tauri Android 后台任务".into());
        let cid1 = r.upsert_conversation(&c1).expect("upsert failed");
        let mut m1 = Message::new(&cid1, Role::User, 1);
        m1.content_text = Some("如何实现 Tauri 的 Android 后台任务？".into());
        r.upsert_message(&m1).expect("upsert failed");
        let mut m2 = Message::new(&cid1, Role::Assistant, 2);
        m2.content_text = Some("用 WorkManager，不要用 Foreground Service。".into());
        r.upsert_message(&m2).expect("upsert failed");

        // conversation 2 (Codex) — 谈 Rust 错误处理
        let mut c2 = Conversation::new(Provider::Codex, "src-2");
        c2.title = Some("Rust 错误处理".into());
        let cid2 = r.upsert_conversation(&c2).expect("upsert failed");
        let mut m3 = Message::new(&cid2, Role::User, 1);
        m3.content_text = Some("thiserror 和 anyhow 怎么选？".into());
        r.upsert_message(&m3).expect("upsert failed");

        r
    }

    #[test]
    fn search_finds_by_keyword() {
        let r = seed();
        let results = r
            .search(&SearchQuery::new("tauri"))
            .expect("SQL execution failed");
        assert!(!results.is_empty(), "should find tauri matches");
        assert!(results
            .iter()
            .all(|sr| sr.conversation_id.starts_with("conv")));
        // 第一条命中应来自 Android 那条会话
        assert!(results[0].title.as_deref().unwrap_or("").contains("Tauri"));
    }

    #[test]
    fn search_filters_by_provider() {
        let r = seed();
        // 用 Codex 那条消息里真实存在的词
        let results = r
            .search(&SearchQuery::new("thiserror").with_provider(Provider::Codex))
            .expect("unexpected None");
        assert!(!results.is_empty());
        assert!(results.iter().all(|sr| sr.provider == Provider::Codex));
    }

    #[test]
    fn search_filters_by_workspace() {
        let r = seed();
        let wid = r.list_workspaces().expect("unexpected None")[0].id.clone();
        let results = r
            .search(&SearchQuery::new("android").with_workspace(&wid))
            .expect("unexpected None");
        assert!(!results.is_empty());
        assert!(results
            .iter()
            .all(|sr| sr.workspace_id.as_deref() == Some(wid.as_str())));
    }

    #[test]
    fn search_returns_snippet() {
        let r = seed();
        let results = r
            .search(&SearchQuery::new("workmanager"))
            .expect("SQL execution failed");
        assert!(!results.is_empty());
        // snippet 非空
        assert!(!results[0].snippet.is_empty());
    }

    #[test]
    fn search_no_match_returns_empty() {
        let r = seed();
        let results = r
            .search(&SearchQuery::new("zzznotfound"))
            .expect("SQL execution failed");
        assert!(results.is_empty());
    }

    #[test]
    fn search_chinese_keyword() {
        let r = seed();
        let results = r
            .search(&SearchQuery::new("后台任务"))
            .expect("SQL execution failed");
        assert!(!results.is_empty());
    }

    #[test]
    fn build_match_expr_quotes_tokens() {
        // 防注入：OR/* 等 FTS5 语法字符被引号包裹为字面量
        let expr = build_match_expr("tauri OR android");
        assert!(expr.contains("\"tauri\""));
        assert!(expr.contains("\"OR\""));
        assert!(expr.contains("\"android\""));
    }

    #[test]
    fn build_match_expr_strips_internal_quotes() {
        let expr = build_match_expr("he\"llo");
        assert_eq!(expr, "\"hello\"");
    }

    #[test]
    fn build_match_expr_empty_input() {
        assert_eq!(build_match_expr(""), "");
        assert_eq!(build_match_expr("   "), "");
    }

    #[test]
    fn search_snippet_escapes_html() {
        // 防存储型 XSS：正文中的 HTML 不能原样进入 snippet（前端 innerHTML 渲染）
        let r = Repository::open_in_memory().expect("unexpected None");
        r.upsert_provider(Provider::Generic).expect("upsert failed");
        let c = Conversation::new(Provider::Generic, "src-xss");
        let cid = r.upsert_conversation(&c).expect("upsert failed");
        let mut m = Message::new(&cid, Role::User, 1);
        m.content_text = Some("<img src=x onerror=alert(1)> tauri 攻击载荷".into());
        r.upsert_message(&m).expect("upsert failed");

        let results = r
            .search(&SearchQuery::new("tauri"))
            .expect("SQL execution failed");
        assert!(!results.is_empty());
        let snip = &results[0].snippet;
        assert!(!snip.contains("<img"), "raw HTML must not survive: {snip}");
        assert!(snip.contains("&lt;img"), "HTML must be escaped: {snip}");
    }

    #[test]
    fn search_filters_by_role_user_only() {
        // 「仅我的提问」：role=user 只返回用户消息
        let r = Repository::open_in_memory().expect("unexpected None");
        r.upsert_provider(Provider::Generic).expect("upsert failed");
        let c = Conversation::new(Provider::Generic, "src-role");
        let cid = r.upsert_conversation(&c).expect("upsert failed");
        let mut mu = Message::new(&cid, Role::User, 1);
        mu.content_text = Some("用户问 rust role 过滤".into());
        r.upsert_message(&mu).expect("upsert failed");
        let mut ma = Message::new(&cid, Role::Assistant, 2);
        ma.content_text = Some("助手答 rust role 过滤".into());
        r.upsert_message(&ma).expect("upsert failed");

        let hits = r
            .search(&SearchQuery::new("rust").with_role("user"))
            .expect("SQL execution failed");
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.role == Role::User));

        // 空 query + role = 全量该角色（所有我的提问）
        let all = r
            .search(&SearchQuery::new("").with_role("user"))
            .expect("SQL execution failed");
        assert!(!all.is_empty(), "空关键词也应返回用户消息");
        assert!(all.iter().all(|h| h.role == Role::User));
    }

    // ── 查询语法集成测试（plan §13.2）──────────────────────────────────

    /// 拿到 seed 里「Tauri Android」那条会话（list 顺序不保证，按标题找）。
    fn tauri_conversation(r: &Repository) -> ch_domain::Conversation {
        r.list_conversations(None)
            .expect("unexpected None")
            .into_iter()
            .find(|c| c.title.as_deref().unwrap_or("").contains("Tauri"))
            .expect("seed must contain Tauri conversation")
    }

    #[test]
    fn syntax_provider_prefix() {
        let r = seed();
        let hits = r
            .search(&SearchQuery::new("provider:codex thiserror"))
            .expect("SQL execution failed");
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.provider == Provider::Codex));
    }

    #[test]
    fn syntax_workspace_by_display_name() {
        let r = seed();
        // seed 里 workspace display_name 是 "tauri-project"
        let hits = r
            .search(&SearchQuery::new("workspace:tauri-project android"))
            .expect("SQL execution failed");
        assert!(!hits.is_empty());
        let wid = r.list_workspaces().expect("unexpected None")[0].id.clone();
        assert!(hits
            .iter()
            .all(|h| h.workspace_id.as_deref() == Some(wid.as_str())));
    }

    #[test]
    fn syntax_type_role_prefix() {
        let r = seed();
        let hits = r
            .search(&SearchQuery::new("type:assistant workmanager"))
            .expect("SQL execution failed");
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.role == Role::Assistant));
    }

    #[test]
    fn syntax_status_favorite() {
        let r = seed();
        let target = tauri_conversation(&r);
        r.set_favorite(&target.id, true).expect("unexpected None");
        let hits = r
            .search(&SearchQuery::new("status:favorite tauri"))
            .expect("SQL execution failed");
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.conversation_id == target.id));
    }

    #[test]
    fn syntax_after_before_range() {
        let r = seed();
        // seed 的 updated_at 为 NULL，先补上「现在」
        let mut target = tauri_conversation(&r);
        target.updated_at = Some(Timestamp::now_utc());
        r.upsert_conversation(&target).expect("upsert failed");
        // after:2020-01-01 命中；before:2020-01-01 空
        let hits = r
            .search(&SearchQuery::new("after:2020-01-01 tauri"))
            .expect("SQL execution failed");
        assert!(!hits.is_empty());
        let none = r
            .search(&SearchQuery::new("before:2020-01-01 tauri"))
            .expect("SQL execution failed");
        assert!(none.is_empty());
    }

    #[test]
    fn syntax_model_substring() {
        let r = seed();
        let mut target = tauri_conversation(&r);
        target.model = Some("gpt-5.3-mini".into());
        r.upsert_conversation(&target).expect("upsert failed");

        let hits = r
            .search(&SearchQuery::new("model:gpt-5.3 tauri"))
            .expect("SQL execution failed");
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.conversation_id == target.id));
    }

    #[test]
    fn syntax_file_filter() {
        let r = seed();
        let target = tauri_conversation(&r);
        let mut ev = ch_domain::Event::new(&target.id, ch_domain::EventType::FileUpdated, 1);
        ev.summary = Some("edit src/main.rs".into());
        r.upsert_event(&ev).expect("upsert failed");

        let hits = r
            .search(&SearchQuery::new("file:main.rs tauri"))
            .expect("SQL execution failed");
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.conversation_id == target.id));
    }

    #[test]
    fn syntax_pure_filter_without_text() {
        // 纯过滤（无关键词）：status:archived 无命中（没归档过）
        let r = seed();
        let hits = r
            .search(&SearchQuery::new("status:archived"))
            .expect("SQL execution failed");
        assert!(hits.is_empty());
    }
}
