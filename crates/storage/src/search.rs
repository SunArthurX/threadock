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
pub(super) fn search(
    conn: &MutexGuard<'_, rusqlite::Connection>,
    q: &SearchQuery,
) -> StorageResult<Vec<SearchResult>> {
    let mut where_clauses: Vec<String> = Vec::new();
    let mut args: Vec<rusqlite::types::Value> = Vec::new();

    // MATCH：在 title 和 body 两列上检索
    let match_expr = build_match_expr(&q.query);
    if !match_expr.is_empty() {
        where_clauses.push("messages_fts MATCH ?".to_string());
        // MATCH 表达式必须作为整体字符串绑定到虚拟表列；
        // 这里用 `{title body} : <expr>` 语法限定列范围
        args.push(format!("{{title body}} : {match_expr}").into());
    }
    if let Some(p) = q.provider {
        where_clauses.push("provider = ?".to_string());
        args.push(p.as_str().to_string().into());
    }
    if let Some(wsid) = &q.workspace_id {
        where_clauses.push("workspace_id = ?".to_string());
        args.push(wsid.clone().into());
    }
    if let Some(role) = &q.role {
        where_clauses.push("role = ?".to_string());
        args.push(role.clone().into());
    }

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
}
