//! Repository：存储层的统一入口，对应 plan §9.4「所有写操作由 Daemon 单点负责」。
//!
//! 关键能力：
//! - 打开/初始化数据库（含 WAL + 4 项 PRAGMA）。
//! - 幂等写入：Conversation / Message / Event 重复 upsert 不产生重复。
//! - 事务包裹：见 plan §11.2「SQLite 事务写入」。

use crate::error::{StorageError, StorageResult};
use crate::filter::ConversationFilter;
use crate::migration;
use crate::search;
use crate::timestamp;
use ch_domain::{
    now_utc, Conversation, Event, EventType, Message, Provider, Role, Status, Workspace,
};
#[allow(unused_imports)]
use ch_domain::{Installation, MatchMethod, Timestamp};
use rusqlite::{params, params_from_iter, types::Value as SqlValue, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

/// 主数据存储。`Mutex` 保证 Daemon 单点写（plan §9.4）。
///
/// 按域拆分的 impl 分布在子模块（同名目录下），此处仅保留核心会话/消息/导入域：
/// [`ops_queries`]（指标聚合）、[`audit_repo`]（审计/策略/预算）、
/// [`knowledge_repo`]（知识持久化）、[`settings`]（应用设置/脱敏规则）。
pub struct Repository {
    conn: Mutex<Connection>,
}

mod audit_repo;
mod knowledge_repo;
mod ops_queries;
mod settings;

pub use audit_repo::{
    AuditFindingState, AuditMessageRow, BudgetSettings, GovernanceLogRow, PolicyRuleRecord,
};
pub use knowledge_repo::KnowledgeRecord;
pub use ops_queries::{
    ActivityStats, AgentBenchmark, AgentHealth, AnomalyRow, AssetRow, AutomationRow, CacheStat,
    CacheTrendRow, DailyTool, DailyUsage, DirCost, HeatCell, HourBucket, LatencyStat, ModelUsage,
    MonthProjection, OpsOverview, ProviderUsage, TokenWaste, ToolTrend, ToolUsageRow, UsageSummary,
    WeeklySummary,
};
pub use settings::{NoteDto, RedactionRuleRecord, TagCountDto};

impl Repository {
    /// 打开文件库，应用 PRAGMA 并迁移到最新版本。
    pub fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        let mut conn = Connection::open(path)?;
        Self::init_pragmas(&conn)?;
        migration::migrate_to_latest(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 打开内存库（主要用于测试）。
    pub fn open_in_memory() -> StorageResult<Self> {
        let mut conn = Connection::open_in_memory()?;
        Self::init_pragmas(&conn)?;
        migration::migrate_to_latest(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn init_pragmas(conn: &Connection) -> StorageResult<()> {
        // plan §9.4 的 4 项 PRAGMA
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // WAL 下 NORMAL 为官方推荐档：事务提交不再逐条 fsync（FULL 在
        // autocommit 路径上每语句一次 fsync，批量导入慢 1-2 个数量级）。
        // NORMAL 仅在断电时可能丢最后一个检查点（应用崩溃不丢），本地工具可接受。
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        Ok(())
    }

    /// 完整性检查，对应 plan §7.3 与上线清单。
    pub fn integrity_check(&self) -> StorageResult<bool> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let result: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        Ok(result == "ok")
    }

    /// 全文搜索，对应 plan §13。委托给 search 模块。
    pub fn search(&self, q: &search::SearchQuery) -> StorageResult<Vec<search::SearchResult>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        search::search(&conn, q)
    }

    /// Prompt 复用推荐（round 25）：用 FTS5 找相似历史 user 消息，
    /// JOIN usage_records 拿当时的 cost / model / provider，让用户看到
    /// 「你之前 3 个会话问过类似问题 + 那次花了多少钱」。
    pub fn prompt_reuse_search(
        &self,
        query: &str,
        limit: usize,
    ) -> StorageResult<Vec<search::PromptReuseHit>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let match_expr = search::build_match_expr(query);
        if match_expr.is_empty() {
            return Ok(Vec::new());
        }
        // FTS5 命中后 JOIN 取会话元数据 + 聚合 cost（按 source_session_id）
        let mut stmt = conn.prepare(
            "SELECT m.id, m.conversation_id, c.title, c.user_title, c.model,
                    p.name as provider_name,
                    snippet(messages_fts, 6, char(1), char(2), '…', 16) AS snip,
                    m.content_text,
                    (SELECT COALESCE(SUM(u.cost_usd), 0.0)
                       FROM usage_records u
                      WHERE u.source_session_id = c.source_conversation_id
                        AND u.provider_id = c.provider_id) AS cost
             FROM messages m
             JOIN messages_fts fts ON fts.rowid = m.rowid
             JOIN conversations c ON c.id = m.conversation_id
             JOIN providers p ON p.id = c.provider_id
             WHERE m.role = 'user' AND messages_fts MATCH ?1
             ORDER BY rank, m.created_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![match_expr, limit as i64], |r| {
            let raw_snip: String = r.get(6)?;
            let snippet = ch_domain::html::escape_html(&raw_snip)
                .replace('\u{1}', "<b>")
                .replace('\u{2}', "</b>");
            let body: Option<String> = r.get(7)?;
            Ok(search::PromptReuseHit {
                message_id: r.get(0)?,
                conversation_id: r.get(1)?,
                title: r.get(2)?,
                user_title: r.get(3)?,
                model: r.get(4)?,
                provider_name: r.get(5)?,
                snippet,
                body: body.unwrap_or_default(),
                cost_usd: r.get(8)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    // ── Provider / Installation ──────────────────────────────────────────

    /// 写入或更新 provider 记录（按 name 去重）。
    pub fn upsert_provider(&self, p: Provider) -> StorageResult<String> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let now_ms = timestamp::to_millis(Some(now_utc())).expect("timestamp conversion failed");
        let id = format!("prov_{}", p.as_str());
        conn.execute(
            "INSERT INTO providers (id, name, adapter_id, adapter_version, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(id) DO UPDATE SET updated_at = ?5",
            params![
                &id,
                p.as_str(),
                format!("{}-adapter", p.as_str()),
                "0.1.0",
                now_ms
            ],
        )?;
        Ok(id)
    }

    /// 写入 installation（幂等：相同 provider+device+path 则更新）。
    pub fn upsert_installation(&self, inst: &Installation) -> StorageResult<String> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let now_ms = timestamp::to_millis(Some(now_utc())).expect("timestamp conversion failed");
        conn.execute(
            "INSERT INTO installations
                (id, provider_id, device_id, app_version, executable_path, data_path,
                 schema_fingerprint, status, last_seen_at, created_at, updated_at)
             VALUES
                (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
             ON CONFLICT(id) DO UPDATE SET
                app_version = ?4, executable_path = ?5, data_path = ?6,
                schema_fingerprint = ?7, status = ?8, last_seen_at = ?9, updated_at = ?10",
            params![
                inst.id,
                format!("prov_{}", inst.provider.as_str()),
                inst.device_id,
                inst.app_version,
                inst.executable_path,
                inst.data_path,
                inst.schema_fingerprint,
                inst.status.as_str(),
                timestamp::to_millis(inst.last_seen_at),
                now_ms,
            ],
        )?;
        Ok(inst.id.clone())
    }

    // ── Workspace ────────────────────────────────────────────────────────

    /// 写入 workspace（幂等：相同 id 更新）。
    pub fn upsert_workspace(&self, ws: &Workspace) -> StorageResult<String> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let now_ms = timestamp::to_millis(Some(now_utc())).expect("timestamp conversion failed");
        conn.execute(
            "INSERT INTO workspaces
                (id, display_name, user_title, canonical_path, git_remote, git_common_dir,
                 status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             ON CONFLICT(id) DO UPDATE SET
                display_name = ?2, user_title = ?3, canonical_path = ?4,
                git_remote = ?5, git_common_dir = ?6, status = ?7, updated_at = ?8",
            params![
                ws.id,
                ws.display_name,
                ws.user_title,
                ws.canonical_path,
                ws.git_remote,
                ws.git_common_dir,
                ws.status.as_str(),
                now_ms,
            ],
        )?;
        Ok(ws.id.clone())
    }

    pub fn get_workspace(&self, id: &str) -> StorageResult<Option<Workspace>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let row = conn
            .query_row(
                "SELECT id, display_name, user_title, canonical_path, git_remote, git_common_dir,
                        status, created_at, updated_at
                 FROM workspaces WHERE id = ?",
                [id],
                row_to_workspace,
            )
            .optional()?;
        Ok(row)
    }

    /// 按显示名查找 active workspace。用于导入时复用已有 workspace（幂等）。
    pub fn find_workspace_by_name(&self, name: &str) -> StorageResult<Option<Workspace>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let row = conn
            .query_row(
                "SELECT id, display_name, user_title, canonical_path, git_remote, git_common_dir,
                        status, created_at, updated_at
                 FROM workspaces
                 WHERE display_name = ? AND status = 'active'
                 ORDER BY created_at ASC LIMIT 1",
                [name],
                row_to_workspace,
            )
            .optional()?;
        Ok(row)
    }

    pub fn list_workspaces(&self) -> StorageResult<Vec<Workspace>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, display_name, user_title, canonical_path, git_remote, git_common_dir,
                    status, created_at, updated_at
             FROM workspaces ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_workspace)?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 列出所有 workspace，时间字段取自其下属对话的真实时间（而非导入时间），
    /// 并按对话最新更新时间倒序排列。用于左侧列表展示。
    pub fn list_workspaces_by_conv_time(&self) -> StorageResult<Vec<Workspace>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        // COALESCE：有对话时取对话时间，否则回退到 workspace 自身时间
        let mut stmt = conn.prepare(
            "SELECT w.id, w.display_name, w.user_title, w.canonical_path, w.git_remote,
                    w.git_common_dir, w.status,
                    COALESCE(MIN(c.started_at), w.created_at) AS eff_created,
                    COALESCE(MAX(c.updated_at), w.updated_at) AS eff_updated
             FROM workspaces w
             LEFT JOIN conversations c ON c.workspace_id = w.id
             GROUP BY w.id
             ORDER BY eff_updated DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Workspace {
                id: r.get(0)?,
                display_name: r.get(1)?,
                user_title: r.get(2)?,
                canonical_path: r.get(3)?,
                git_remote: r.get(4)?,
                git_common_dir: r.get(5)?,
                status: parse_status(&r.get::<_, String>(6)?),
                created_at: timestamp::from_millis(Some(r.get::<_, Option<i64>>(7)?.unwrap_or(0)))
                    .unwrap_or_else(now_utc),
                updated_at: timestamp::from_millis(Some(r.get::<_, Option<i64>>(8)?.unwrap_or(0)))
                    .unwrap_or_else(now_utc),
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    // ── Conversation（幂等核心） ─────────────────────────────────────────

    /// 写入 `conversation。幂等键：(provider_id`, `installation_id`, `source_conversation_id`)。
    /// 重复写入更新内容字段，但保留 `user_title（plan` §11.5：用户数据优先）。
    pub fn upsert_conversation(&self, c: &Conversation) -> StorageResult<String> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let provider_id = format!("prov_{}", c.provider.as_str());

        // 先尝试按幂等键查找现有 id
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM conversations
                 WHERE provider_id = ?1 AND
                       COALESCE(installation_id, '') = COALESCE(?2, '') AND
                       source_conversation_id = ?3",
                params![&provider_id, c.installation_id, c.source_conversation_id],
                |r| r.get(0),
            )
            .optional()?;

        let id = existing.clone().unwrap_or_else(|| c.id.clone());

        conn.execute(
            "INSERT INTO conversations
                (id, workspace_id, provider_id, installation_id, source_conversation_id,
                 title, user_title, status, model, started_at, updated_at, completed_at,
                 source_status, source_url, completeness_score, content_hash, raw_payload_id,
                 source_parent_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
             ON CONFLICT(id) DO UPDATE SET
                workspace_id = ?2, title = ?6, status = ?8, model = ?9,
                started_at = ?10, updated_at = ?11, completed_at = ?12,
                source_status = ?13, source_url = ?14, completeness_score = ?15,
                content_hash = ?16, raw_payload_id = ?17, source_parent_id = ?18,
                updated_at = ?11
             /* user_title 不在 UPDATE 列表中，保留用户自定义 */",
            params![
                id,
                c.workspace_id,
                provider_id,
                c.installation_id,
                c.source_conversation_id,
                c.title,
                c.user_title,
                c.status.map(|s| s.as_str()),
                c.model,
                timestamp::to_millis(c.started_at),
                timestamp::to_millis(c.updated_at),
                timestamp::to_millis(c.completed_at),
                c.source_status.as_str(),
                c.source_url,
                c.completeness_score,
                c.content_hash,
                c.raw_payload_id,
                c.source_parent_id,
            ],
        )?;
        Ok(id)
    }

    pub fn get_conversation(&self, id: &str) -> StorageResult<Option<Conversation>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.query_row(
            "SELECT c.id, c.workspace_id, p.name, c.installation_id, c.source_conversation_id,
                    c.title, c.user_title, c.status, c.model, c.started_at, c.updated_at,
                    c.completed_at, c.source_status, c.source_url, c.completeness_score,
                    c.content_hash, c.raw_payload_id, c.source_parent_id
             FROM conversations c JOIN providers p ON p.id = c.provider_id
             WHERE c.id = ?",
            [id],
            row_to_conversation,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_conversations(
        &self,
        workspace_id: Option<&str>,
    ) -> StorageResult<Vec<Conversation>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut sql = String::from(
            "SELECT c.id, c.workspace_id, p.name, c.installation_id, c.source_conversation_id,
                    c.title, c.user_title, c.status, c.model, c.started_at, c.updated_at,
                    c.completed_at, c.source_status, c.source_url, c.completeness_score,
                    c.content_hash, c.raw_payload_id, c.source_parent_id
             FROM conversations c JOIN providers p ON p.id = c.provider_id",
        );
        let mut args: Vec<SqlValue> = Vec::new();
        if let Some(wsid) = workspace_id {
            sql.push_str(" WHERE c.workspace_id = ?1");
            args.push(wsid.to_string().into());
        }
        sql.push_str(" ORDER BY c.updated_at DESC NULLS LAST");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), row_to_conversation)?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 多维度过滤会话（plan §6.4：按来源/Workspace/状态/标签筛选）。
    pub fn list_conversations_filtered(
        &self,
        filter: &ConversationFilter,
    ) -> StorageResult<Vec<Conversation>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut where_clauses: Vec<String> = Vec::new();
        let mut args: Vec<SqlValue> = Vec::new();
        let mut next_idx = 1usize;

        let push = |clause: String,
                    val: SqlValue,
                    wc: &mut Vec<String>,
                    a: &mut Vec<SqlValue>,
                    idx: &mut usize| {
            wc.push(clause.replace('?', &format!("?{idx}")));
            a.push(val);
            *idx += 1;
        };

        if let Some(p) = filter.provider {
            push(
                "p.name = ?".to_string(),
                p.as_str().to_string().into(),
                &mut where_clauses,
                &mut args,
                &mut next_idx,
            );
        }
        if let Some(wsid) = &filter.workspace_id {
            push(
                "c.workspace_id = ?".to_string(),
                wsid.clone().into(),
                &mut where_clauses,
                &mut args,
                &mut next_idx,
            );
        }
        if let Some(fav) = filter.favorite {
            push(
                "c.favorite = ?".to_string(),
                i64::from(i32::from(fav)).into(),
                &mut where_clauses,
                &mut args,
                &mut next_idx,
            );
        }
        if let Some(arch) = filter.archived {
            push(
                "c.is_archived = ?".to_string(),
                i64::from(i32::from(arch)).into(),
                &mut where_clauses,
                &mut args,
                &mut next_idx,
            );
        }
        if let Some(del) = filter.deleted {
            // 无参数子句：直接拼接（不能走 push，否则占位索引错位）
            where_clauses.push(if del {
                "c.source_status = 'deleted'".to_string()
            } else {
                "COALESCE(c.source_status, '') != 'deleted'".to_string()
            });
        }
        if let Some(after) = filter.started_after_ms {
            push(
                "c.started_at >= ?".to_string(),
                after.into(),
                &mut where_clauses,
                &mut args,
                &mut next_idx,
            );
        }
        if let Some(before) = filter.started_before_ms {
            push(
                "c.started_at <= ?".to_string(),
                before.into(),
                &mut where_clauses,
                &mut args,
                &mut next_idx,
            );
        }

        let mut sql = String::from(
            "SELECT c.id, c.workspace_id, p.name, c.installation_id, c.source_conversation_id,
                    c.title, c.user_title, c.status, c.model, c.started_at, c.updated_at,
                    c.completed_at, c.source_status, c.source_url, c.completeness_score,
                    c.content_hash, c.raw_payload_id, c.source_parent_id
             FROM conversations c JOIN providers p ON p.id = c.provider_id",
        );
        if !where_clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY c.updated_at DESC NULLS LAST");

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), row_to_conversation)?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 统计 conversation 数量（基准测试用）。
    pub fn count_conversations(&self) -> StorageResult<i64> {
        let conn = self.conn.lock().expect("mutex poisoned");
        Ok(conn.query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))?)
    }

    /// 列出指定父会话的子任务（按 `source_parent_id` 关联）。
    /// `parent_source_id` 是父会话的 `source_conversation_id`，`provider_id` 形如 `prov_zcode`。
    pub fn list_child_conversations(
        &self,
        parent_source_id: &str,
        provider_id: &str,
    ) -> StorageResult<Vec<Conversation>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT c.id, c.workspace_id, p.name, c.installation_id, c.source_conversation_id,
                    c.title, c.user_title, c.status, c.model, c.started_at, c.updated_at,
                    c.completed_at, c.source_status, c.source_url, c.completeness_score,
                    c.content_hash, c.raw_payload_id, c.source_parent_id
             FROM conversations c JOIN providers p ON p.id = c.provider_id
             WHERE c.source_parent_id = ?1 AND c.provider_id = ?2
             ORDER BY c.updated_at DESC NULLS LAST",
        )?;
        let rows = stmt.query_map(params![parent_source_id, provider_id], row_to_conversation)?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 统计指定父会话的子任务数量。
    pub fn count_children(&self, parent_source_id: &str, provider_id: &str) -> StorageResult<i64> {
        let conn = self.conn.lock().expect("mutex poisoned");
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM conversations
             WHERE source_parent_id = ?1 AND provider_id = ?2",
            params![parent_source_id, provider_id],
            |r| r.get(0),
        )?)
    }

    /// 一次查询统计所有父会话的子任务数（key = (source_parent_id, provider_id)）。
    ///
    /// 会话列表页的 child_count 批量来源：替代每会话一次 count_children 的 N+1。
    pub fn child_counts_bulk(
        &self,
    ) -> StorageResult<std::collections::HashMap<(String, String), i64>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT source_parent_id, provider_id, COUNT(*) FROM conversations
                          WHERE source_parent_id IS NOT NULL
                          GROUP BY source_parent_id, provider_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                (r.get::<_, String>(0)?, r.get::<_, String>(1)?),
                r.get::<_, i64>(2)?,
            ))
        })?;
        let mut map = std::collections::HashMap::new();
        for row in rows {
            let (k, v) = row?;
            map.insert(k, v);
        }
        Ok(map)
    }

    /// 时间范围重置：库中最早数据时间戳（用于 UI 限制 `min`）。
    /// 取 usage_records / tool_call_records / conversations 三表最小 ts；
    /// 空库返回 None（前端 fallback 到今天）。
    pub fn reset_range_min_ts(&self) -> StorageResult<Option<i64>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        // 单条 SQL 同时查三表最小值；任一为空用 NULL 兜底，最终取整体 MIN
        let v: Option<i64> = conn
            .query_row(
                "SELECT MIN(min_ts) FROM (
                     SELECT MIN(ts) AS min_ts FROM usage_records
                     UNION ALL
                     SELECT MIN(ts) AS min_ts FROM tool_call_records
                     UNION ALL
                     SELECT MIN(updated_at) AS min_ts FROM conversations
                 )",
                [],
                |r| r.get(0),
            )
            .map_err(crate::StorageError::from)?;
        Ok(v)
    }

    /// 时间范围重置预览：统计 [start_ms, now] 内将被删除的数据量。
    pub fn reset_range_stats(&self, start_ms: i64) -> StorageResult<(i64, i64, i64)> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let convs: i64 = conn.query_row(
            "SELECT COUNT(*) FROM conversations WHERE updated_at >= ?1",
            params![start_ms],
            |r| r.get(0),
        )?;
        let msgs: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages m JOIN conversations c ON c.id = m.conversation_id
             WHERE c.updated_at >= ?1",
            params![start_ms],
            |r| r.get(0),
        )?;
        let usage: i64 = conn.query_row(
            "SELECT COUNT(*) FROM usage_records WHERE ts >= ?1",
            params![start_ms],
            |r| r.get(0),
        )?;
        Ok((convs, msgs, usage))
    }

    /// 时间范围重置：删除 [start_ms, now] 内的会话及其级联数据 + 指标记录。
    /// 返回删除的会话数与消息 id 列表（供调用方同步删搜索索引）。
    ///
    /// 约束：开始时间不得晚于当前时间（防误传未来时间），不限制最早日期——
    /// 库中有 1 年前数据也允许重置整段。性能（真实库实测迭代）：
    /// FK CASCADE + FTS5 触发器单事务 686s → 分批 228s → 禁触发器删除 +
    /// FTS 一次性 rebuild 秒级。
    pub fn reset_range(&self, start_ms: i64) -> StorageResult<(i64, Vec<String>)> {
        let now_ms = timestamp::to_millis(Some(now_utc())).unwrap_or(0);
        if start_ms > now_ms {
            return Err(StorageError::NotFound {
                entity: "reset_range",
                id: "开始时间晚于当前时间：参数错误".into(),
            });
        }
        let mut conn = self.conn.lock().expect("mutex poisoned");
        // ① 收集被删 conversations 的本地 id + 源侧 source_conversation_id
        // （后者用于清 import_state——否则重置后 autoSync 走增量会全 skip，导致"重置 = 数据丢失"）
        let conv_ids: Vec<String> = {
            let mut stmt =
                conn.prepare("SELECT id FROM conversations WHERE updated_at >= ?1")?;
            let rows = stmt.query_map(params![start_ms], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let source_ids: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT source_conversation_id FROM conversations
                 WHERE updated_at >= ?1 AND source_conversation_id IS NOT NULL
                   AND source_conversation_id != ''",
            )?;
            let rows = stmt.query_map(params![start_ms], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let n = conv_ids.len() as i64;
        if conv_ids.is_empty() {
            return Ok((0, Vec::new()));
        }
        // ② 删除前收集消息 id（只读，供索引级联删除）
        let msg_ids: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT id FROM messages WHERE conversation_id IN (
                     SELECT id FROM conversations WHERE updated_at >= ?1)",
            )?;
            let rows = stmt.query_map(params![start_ms], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        // ③ 禁用 FTS 行触发器（删除走批量，索引删除改由 rebuild 承担）
        for trig in ["messages_ai_fts", "messages_ad_fts", "messages_au_fts"] {
            let _ = conn.execute(&format!("DROP TRIGGER IF EXISTS {trig}"), []);
        }
        let del_result: StorageResult<()> = (|| {
            // ④ 分批删子表 + 主表 + import_state（每批独立短事务）
            for chunk in conv_ids.chunks(200) {
                let tx = conn.transaction()?;
                let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                for table in [
                    "messages",
                    "events",
                    "conversation_tags",
                    "knowledge_extractions",
                ] {
                    let sql =
                        format!("DELETE FROM {table} WHERE conversation_id IN ({placeholders})");
                    tx.execute(&sql, rusqlite::params_from_iter(chunk.iter()))?;
                }
                let sql_del = format!("DELETE FROM conversations WHERE id IN ({placeholders})");
                tx.execute(&sql_del, rusqlite::params_from_iter(chunk.iter()))?;
                drop(tx.commit());
            }
            // ⑤ 同步清 import_state（用 source_conversation_id 匹配）—— 否则 autoSync 增量会全 skip
            if !source_ids.is_empty() {
                for chunk in source_ids.chunks(500) {
                    let placeholders =
                        chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                    let sql = format!(
                        "DELETE FROM import_state WHERE source_id IN ({placeholders})"
                    );
                    conn.execute(&sql, rusqlite::params_from_iter(chunk.iter()))?;
                }
            }
            // ⑥ 指标按时间分批
            for table in ["usage_records", "tool_call_records"] {
                loop {
                    let deleted = conn.execute(
                        &format!(
                            "DELETE FROM {table} WHERE rowid IN (
                                 SELECT rowid FROM {table} WHERE ts >= ?1 LIMIT 2000)"
                        ),
                        params![start_ms],
                    )?;
                    if deleted == 0 {
                        break;
                    }
                }
            }
            Ok(())
        })();
        // ⑦ 无论成败：恢复触发器 + 全文索引一次性重建（保持结构一致）
        conn.execute_batch(crate::schema::SCHEMA_FTS_TRIGGERS)?;
        conn.execute(
            "INSERT INTO messages_fts(messages_fts) VALUES('rebuild')",
            [],
        )?;
        del_result?;
        Ok((n, msg_ids))
    }

    /// 单会话 UI 标志位（favorite/is_archived，DB-only 字段）。
    pub fn get_conversation_flags(&self, id: &str) -> StorageResult<(bool, bool)> {
        let conn = self.conn.lock().expect("mutex poisoned");
        Ok(conn.query_row(
            "SELECT favorite, is_archived FROM conversations WHERE id = ?1",
            params![id],
            |r| Ok((r.get::<_, i64>(0)? != 0, r.get::<_, i64>(1)? != 0)),
        )?)
    }

    /// 全部会话标志位（id → (favorite, is_archived)），列表页一次取齐。
    pub fn conversation_flags_bulk(
        &self,
    ) -> StorageResult<std::collections::HashMap<String, (bool, bool)>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare("SELECT id, favorite, is_archived FROM conversations")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (r.get::<_, i64>(1)? != 0, r.get::<_, i64>(2)? != 0),
            ))
        })?;
        let mut map = std::collections::HashMap::new();
        for r in rows {
            let (k, v) = r?;
            map.insert(k, v);
        }
        Ok(map)
    }

    /// 清空所有数据（conversations 级联删除 messages/events/tags/knowledge）。
    /// 保留 schema 和 `redaction_rules（用户自定义规则`）。
    /// 用于「重置数据」功能。
    ///
    /// ⚠️ 必须同步清 `import_state`（V10 新鲜度表）—— 否则 autoSync 走增量时
    /// `existing.contains(&key)` 为真就全 skip，导致「重置后数据全丢」
    /// （round 24 复盘：用户报告"全部重置后 zcode 少了 142 条，重新点导入才解决"）
    pub fn clear_all(&self) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        // conversations 有 ON DELETE CASCADE，会自动清理 messages/events/turns/tags/knowledge
        conn.execute("DELETE FROM conversations", [])?;
        conn.execute("DELETE FROM workspaces", [])?;
        conn.execute("DELETE FROM source_workspaces", [])?;
        conn.execute("DELETE FROM providers", [])?;
        conn.execute("DELETE FROM installations", [])?;
        conn.execute("DELETE FROM usage_records", [])?;
        conn.execute("DELETE FROM tool_call_records", [])?;
        // 关键：清 import_state（不在 FK CASCADE 链上）
        conn.execute("DELETE FROM import_state", [])?;
        Ok(())
    }

    // ── CodeAgentOps：用量/工具调用指标（plan codeagent-ops §3.2）────────

    /// 单事务批量导入一条会话（会话 + 全部消息 + 事件）。
    ///
    /// 性能关键路径：逐条 upsert 每次独立提交（WAL + synchronous=FULL 下
    /// 每条一次 fsync），大批量导入会拖垮主锁 → UI 卡顿。这里整会话一个事务，
    /// 只在提交时 fsync 一次，快 1-2 个数量级。
    /// `workspace_name` 非空时按名查找/创建并挂到会话上。
    #[allow(clippy::too_many_lines)] // 单事务批量导入：provider+workspace+会话+消息+事件一体，拆分损害原子性
    pub fn import_conversation_batch(
        &self,
        conv: &Conversation,
        messages: &[Message],
        events: &[Event],
        workspace_name: Option<&str>,
        observed_updated_ms: Option<i64>,
    ) -> StorageResult<String> {
        let mut conn_guard = self.conn.lock().expect("mutex poisoned");
        let tx = conn_guard.transaction()?;

        // provider（幂等；adapter_id/adapter_version NOT NULL，与 upsert_provider 同口径）
        let provider_id = format!("prov_{}", conv.provider.as_str());
        let now_ms = timestamp::to_millis(Some(now_utc())).unwrap_or(0);
        tx.execute(
            "INSERT INTO providers (id, name, adapter_id, adapter_version, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(id) DO UPDATE SET updated_at = ?5",
            params![
                provider_id,
                conv.provider.as_str(),
                format!("{}-adapter", conv.provider.as_str()),
                "0.1.0",
                now_ms
            ],
        )?;

        // workspace 查找/创建（未提供名称时保留调用方在 conv 上已设置的 workspace，
        // 避免把已有归属覆盖成 NULL）
        let workspace_id: Option<String> = workspace_name
            .map(|name| {
                tx.query_row(
                    "SELECT id FROM workspaces WHERE display_name = ?1 AND status = 'active'
                 ORDER BY created_at ASC LIMIT 1",
                    params![name],
                    |r| r.get(0),
                )
                .optional()
                .ok()
                .flatten()
                .unwrap_or_else(|| {
                    let ws = Workspace::new(name);
                    let ws_id = ws.id.clone();
                    let ws_now = timestamp::to_millis(Some(ws.created_at)).unwrap_or(0);
                    let _ = tx.execute(
                        "INSERT OR IGNORE INTO workspaces
                        (id, display_name, user_title, canonical_path, git_remote, git_common_dir,
                         status, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                        params![
                            ws.id,
                            ws.display_name,
                            ws.user_title,
                            ws.canonical_path,
                            ws.git_remote,
                            ws.git_common_dir,
                            ws.status.as_str(),
                            ws_now,
                        ],
                    );
                    ws_id
                })
            })
            .or_else(|| conv.workspace_id.clone());

        // 会话幂等查找 + upsert（与 upsert_conversation 同口径）
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM conversations
                 WHERE provider_id = ?1
                   AND COALESCE(installation_id, '') = COALESCE(?2, '')
                   AND source_conversation_id = ?3",
                params![
                    provider_id,
                    conv.installation_id,
                    conv.source_conversation_id
                ],
                |r| r.get(0),
            )
            .optional()?;
        let id = existing.unwrap_or_else(|| conv.id.clone());

        tx.execute(
            "INSERT INTO conversations
                (id, workspace_id, provider_id, installation_id, source_conversation_id,
                 title, user_title, status, model, started_at, updated_at, completed_at,
                 source_status, source_url, completeness_score, content_hash, raw_payload_id,
                 source_parent_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
             ON CONFLICT(id) DO UPDATE SET
                workspace_id = ?2, title = ?6, status = ?8, model = ?9,
                started_at = ?10, updated_at = ?11, completed_at = ?12,
                source_status = ?13, source_url = ?14, completeness_score = ?15,
                content_hash = ?16, raw_payload_id = ?17, source_parent_id = ?18",
            params![
                id,
                workspace_id,
                provider_id,
                conv.installation_id,
                conv.source_conversation_id,
                conv.title,
                conv.user_title,
                conv.status.map(|s| s.as_str()),
                conv.model,
                timestamp::to_millis(conv.started_at),
                timestamp::to_millis(conv.updated_at),
                timestamp::to_millis(conv.completed_at),
                conv.source_status.as_str(),
                conv.source_url,
                conv.completeness_score,
                conv.content_hash,
                conv.raw_payload_id,
                conv.source_parent_id,
            ],
        )?;

        // 消息（幂等：conversation_id + sequence，与 upsert_message 同列集）
        for m in messages {
            let existing_msg: Option<String> = tx
                .query_row(
                    "SELECT id FROM messages WHERE conversation_id = ?1 AND sequence_number = ?2",
                    params![id, m.sequence_number],
                    |r| r.get(0),
                )
                .optional()?;
            let msg_id = existing_msg.unwrap_or_else(|| m.id.clone());
            tx.execute(
                "INSERT INTO messages
                    (id, conversation_id, turn_id, source_message_id, role, content_text,
                     content_json, sequence_number, created_at, content_hash, raw_payload_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(id) DO UPDATE SET
                    turn_id = ?3, source_message_id = ?4, role = ?5, content_text = ?6,
                    content_json = ?7, created_at = ?9, content_hash = ?10, raw_payload_id = ?11",
                params![
                    msg_id,
                    id,
                    m.turn_id,
                    m.source_message_id,
                    m.role.as_str(),
                    m.content_text,
                    m.content_json
                        .as_ref()
                        .map(std::string::ToString::to_string),
                    m.sequence_number,
                    timestamp::to_millis(m.created_at),
                    m.content_hash,
                    m.raw_payload_id,
                ],
            )?;
        }
        // 事件（幂等：conversation_id + sequence）
        for e in events {
            let existing_ev: Option<String> = tx
                .query_row(
                    "SELECT id FROM events WHERE conversation_id = ?1 AND sequence_number = ?2",
                    params![id, e.sequence_number],
                    |r| r.get(0),
                )
                .optional()?;
            let ev_id = existing_ev.unwrap_or_else(|| e.id.clone());
            tx.execute(
                "INSERT INTO events
                    (id, conversation_id, turn_id, source_event_id, event_type, status, summary,
                     payload_json, sequence_number, created_at, completed_at, raw_payload_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(id) DO UPDATE SET
                    turn_id = ?3, source_event_id = ?4, event_type = ?5, status = ?6, summary = ?7,
                    payload_json = ?8, created_at = ?10, completed_at = ?11, raw_payload_id = ?12",
                params![
                    ev_id,
                    id,
                    e.turn_id,
                    e.source_event_id,
                    e.event_type.as_str(),
                    e.status.map(|s| s.as_str()),
                    e.summary,
                    e.payload_json
                        .as_ref()
                        .map(std::string::ToString::to_string),
                    e.sequence_number,
                    timestamp::to_millis(e.created_at),
                    timestamp::to_millis(e.completed_at),
                    e.raw_payload_id,
                ],
            )?;
        }

        // 记录导入新鲜度（「已导入」判定依据：源更新时间 ≤ observed_ms）
        tx.execute(
            "INSERT INTO import_state (source_pk, provider_id, source_id, observed_ms, imported_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(source_pk) DO UPDATE SET observed_ms = ?4, imported_at = ?5",
            params![
                format!("{provider_id}:{}", conv.source_conversation_id),
                provider_id,
                conv.source_conversation_id,
                observed_updated_ms,
                timestamp::to_millis(Some(now_utc())).unwrap_or(0),
            ],
        )?;

        tx.commit()?;
        Ok(id)
    }

    /// `批量回填/刷新导入新鲜度（auto_sync` 对已存在会话也记录当前源时间）。
    pub fn record_import_states(
        &self,
        provider_id: &str,
        entries: &[(String, Option<i64>)],
    ) -> StorageResult<usize> {
        if entries.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn.lock().expect("mutex poisoned");
        let tx = conn.transaction()?;
        let now_ms = timestamp::to_millis(Some(now_utc())).unwrap_or(0);
        let mut n = 0;
        for (source_id, observed) in entries {
            n += tx.execute(
                "INSERT INTO import_state (source_pk, provider_id, source_id, observed_ms, imported_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(source_pk) DO UPDATE SET
                    observed_ms = COALESCE(?4, observed_ms), imported_at = ?5",
                params![
                    format!("{provider_id}:{source_id}"),
                    provider_id,
                    source_id,
                    observed,
                    now_ms,
                ],
            )?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// 读取某 provider 的 {`source_id`: `observed_ms`} 新鲜度表。
    pub fn import_state_map(
        &self,
        provider_id: &str,
    ) -> StorageResult<std::collections::HashMap<String, Option<i64>>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt =
            conn.prepare("SELECT source_id, observed_ms FROM import_state WHERE provider_id = ?1")?;
        let rows = stmt.query_map(params![provider_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?))
        })?;
        let mut m = std::collections::HashMap::new();
        for r in rows {
            let (k, v) = r?;
            m.insert(k, v);
        }
        Ok(m)
    }

    /// 「已导入」判定：会话存在且源的更新时间不晚于导入时观察时间。
    /// 源时间未知（None）时退化为存在性判断。
    #[must_use]
    pub fn is_up_to_date(
        state: &std::collections::HashMap<String, Option<i64>>,
        existing: &std::collections::HashSet<(String, String)>,
        provider_id: &str,
        source_id: &str,
        source_updated_ms: Option<i64>,
    ) -> bool {
        if !existing.contains(&(provider_id.to_string(), source_id.to_string())) {
            return false;
        }
        match (source_updated_ms, state.get(source_id).copied()) {
            (Some(src_ms), Some(Some(obs))) => obs >= src_ms,
            _ => true,
        }
    }

    /// 修复旧数据的主子链路：当来源侧有 parent 而库内为 NULL/不一致时更新。
    /// 返回是否实际更新。用于 `auto_sync` 对已存在会话补 `source_parent_id`。
    pub fn repair_conversation_parent(
        &self,
        provider_id: &str,
        source_conversation_id: &str,
        parent: Option<&str>,
    ) -> StorageResult<bool> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let n = conn.execute(
            "UPDATE conversations SET source_parent_id = ?1
             WHERE provider_id = ?2 AND source_conversation_id = ?3
               AND source_parent_id IS NOT ?1",
            params![parent, provider_id, source_conversation_id],
        )?;
        Ok(n > 0)
    }

    /// 批量修复主子链路：单事务执行整批 UPDATE（替代逐条锁循环，
    /// 消除启动同步时 800+ 次密集锁循环导致的 UI 查询饿死）。
    pub fn repair_parents_batch(
        &self,
        provider_id: &str,
        pairs: &[(String, String)],
    ) -> StorageResult<usize> {
        if pairs.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn.lock().expect("mutex poisoned");
        let tx = conn.transaction()?;
        let mut n = 0;
        for (source_id, parent) in pairs {
            n += tx.execute(
                "UPDATE conversations SET source_parent_id = ?1
                 WHERE provider_id = ?2 AND source_conversation_id = ?3
                   AND source_parent_id IS NOT ?1",
                params![parent, provider_id, source_id],
            )?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// 已导入会话的 (`provider_id`, `source_id`) `全集（auto_sync` 幂等快速检查用）。
    pub fn list_conversation_sources(&self) -> StorageResult<Vec<(String, String)>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt =
            conn.prepare("SELECT provider_id, source_conversation_id FROM conversations")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 按 `provider_id` + `source_conversation_id` 精确查会话（审计跳转用）。
    pub fn find_conversation_by_source(
        &self,
        provider_id: &str,
        source_conversation_id: &str,
    ) -> StorageResult<Option<Conversation>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.query_row(
            "SELECT c.id, c.workspace_id, p.name, c.installation_id, c.source_conversation_id,
                    c.title, c.user_title, c.status, c.model, c.started_at, c.updated_at,
                    c.completed_at, c.source_status, c.source_url, c.completeness_score,
                    c.content_hash, c.raw_payload_id, c.source_parent_id
             FROM conversations c JOIN providers p ON p.id = c.provider_id
             WHERE c.provider_id = ?1 AND c.source_conversation_id = ?2",
            params![provider_id, source_conversation_id],
            row_to_conversation,
        )
        .optional()
        .map_err(Into::into)
    }

    // ── M6-M9：资产 / 自动化 / 成本归因 / 缓存 / 异常 ─────────────────────

    // ── M10-M12：健康度 / 延迟 / Token 浪费 ─────────────────────────────

    // ── M13-M14：横向对比 / 周报数据 ─────────────────────────────────────

    // ── 审计：策略规则 + 预算设置 + 消息扫描流（plan codeagent-ops M4/M5）──

    // ── Message（幂等：按 content_hash + sequence 去重） ──────────────────

    /// 写入 `message。幂等键：(conversation_id`, `sequence_number`)。
    pub fn upsert_message(&self, m: &Message) -> StorageResult<String> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM messages WHERE conversation_id = ?1 AND sequence_number = ?2",
                params![m.conversation_id, m.sequence_number],
                |r| r.get(0),
            )
            .optional()?;
        let id = existing.unwrap_or_else(|| m.id.clone());

        conn.execute(
            "INSERT INTO messages
                (id, conversation_id, turn_id, source_message_id, role, content_text,
                 content_json, sequence_number, created_at, content_hash, raw_payload_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                turn_id = ?3, source_message_id = ?4, role = ?5, content_text = ?6,
                content_json = ?7, created_at = ?9, content_hash = ?10, raw_payload_id = ?11",
            params![
                id,
                m.conversation_id,
                m.turn_id,
                m.source_message_id,
                m.role.as_str(),
                m.content_text,
                m.content_json
                    .as_ref()
                    .map(std::string::ToString::to_string),
                m.sequence_number,
                timestamp::to_millis(m.created_at),
                m.content_hash,
                m.raw_payload_id,
            ],
        )?;
        Ok(id)
    }

    pub fn list_messages(&self, conversation_id: &str) -> StorageResult<Vec<Message>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, turn_id, source_message_id, role, content_text,
                    content_json, sequence_number, created_at, content_hash, raw_payload_id
             FROM messages WHERE conversation_id = ? ORDER BY sequence_number ASC",
        )?;
        let rows = stmt.query_map([conversation_id], row_to_message)?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    // ── Event（幂等：按 conversation_id + sequence_number） ───────────────

    pub fn upsert_event(&self, e: &Event) -> StorageResult<String> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM events WHERE conversation_id = ?1 AND sequence_number = ?2",
                params![e.conversation_id, e.sequence_number],
                |r| r.get(0),
            )
            .optional()?;
        let id = existing.unwrap_or_else(|| e.id.clone());

        conn.execute(
            "INSERT INTO events
                (id, conversation_id, turn_id, source_event_id, event_type, status, summary,
                 payload_json, sequence_number, created_at, completed_at, raw_payload_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(id) DO UPDATE SET
                turn_id = ?3, source_event_id = ?4, event_type = ?5, status = ?6, summary = ?7,
                payload_json = ?8, created_at = ?10, completed_at = ?11, raw_payload_id = ?12",
            params![
                id,
                e.conversation_id,
                e.turn_id,
                e.source_event_id,
                e.event_type.as_str(),
                e.status.map(|s| s.as_str()),
                e.summary,
                e.payload_json
                    .as_ref()
                    .map(std::string::ToString::to_string),
                e.sequence_number,
                timestamp::to_millis(e.created_at),
                timestamp::to_millis(e.completed_at),
                e.raw_payload_id,
            ],
        )?;
        Ok(id)
    }

    pub fn list_events(&self, conversation_id: &str) -> StorageResult<Vec<Event>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, turn_id, source_event_id, event_type, status, summary,
                    payload_json, sequence_number, created_at, completed_at, raw_payload_id
             FROM events WHERE conversation_id = ? ORDER BY sequence_number ASC",
        )?;
        let rows = stmt.query_map([conversation_id], row_to_event)?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    // ── Sync cursor ──────────────────────────────────────────────────────

    /// `写入同步游标。幂等：(provider_id`, `installation_id`, `cursor_type`)。
    pub fn upsert_cursor(
        &self,
        provider: Provider,
        installation_id: Option<&str>,
        cursor_type: &str,
        value: &str,
        schema_fingerprint: Option<&str>,
    ) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let provider_id = format!("prov_{}", provider.as_str());
        conn.execute(
            "INSERT INTO sync_cursors
                (provider_id, installation_id, cursor_type, cursor_value, schema_fingerprint,
                 last_success_at)
             VALUES (?1, COALESCE(?2, ''), ?3, ?4, ?5, ?6)
             ON CONFLICT(provider_id, installation_id, cursor_type) DO UPDATE SET
                cursor_value = ?4, schema_fingerprint = ?5, last_success_at = ?6",
            params![
                provider_id,
                installation_id,
                cursor_type,
                value,
                schema_fingerprint,
                timestamp::to_millis(Some(now_utc())).expect("timestamp conversion failed"),
            ],
        )?;
        Ok(())
    }

    pub fn get_cursor(
        &self,
        provider: Provider,
        installation_id: Option<&str>,
        cursor_type: &str,
    ) -> StorageResult<Option<String>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let provider_id = format!("prov_{}", provider.as_str());
        let v: Option<String> = conn
            .query_row(
                "SELECT cursor_value FROM sync_cursors
                 WHERE provider_id = ?1 AND installation_id = COALESCE(?2, '') AND cursor_type = ?3",
                params![provider_id, installation_id, cursor_type],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v)
    }

    // ── 收藏 / 标签 / 归档（plan §6.3/§6.4/§6.5）─────────────────────────

    /// 设置会话收藏状态。
    pub fn set_favorite(&self, conversation_id: &str, favorite: bool) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let changed = conn.execute(
            "UPDATE conversations SET favorite = ?1 WHERE id = ?2",
            params![i32::from(favorite), conversation_id],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound {
                entity: "conversation",
                id: conversation_id.to_string(),
            });
        }
        Ok(())
    }

    /// 设置用户自定义标题（user_title）。传空串或 null 表示清除。
    /// 原始 title（agent 提取）保留不变；前端展示用 COALESCE(user_title, title)。
    pub fn set_user_title(&self, conversation_id: &str, user_title: Option<&str>) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        // 空串视为清除
        let normalized = user_title.map(|s| s.trim()).filter(|s| !s.is_empty());
        let changed = conn.execute(
            "UPDATE conversations SET user_title = ?1 WHERE id = ?2",
            params![normalized, conversation_id],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound {
                entity: "conversation",
                id: conversation_id.to_string(),
            });
        }
        Ok(())
    }

    /// 读取会话的私有笔记（None = 没写过）。
    /// 返回 (note_text, updated_at_ms)。
    pub fn get_note(&self, conversation_id: &str) -> StorageResult<Option<(String, i64)>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let r: Option<(String, i64)> = conn
            .query_row(
                "SELECT note, updated_at FROM conversation_notes WHERE conversation_id = ?1",
                [conversation_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(r)
    }

    /// 设置会话的私有笔记（覆盖或删除）。
    /// - note 为 None 或空串 → 删除该会话的笔记行
    /// - 非空 → UPSERT (note, updated_at)
    /// 返回笔记 updated_at 毫秒时间戳（0 = 已删除）
    pub fn set_note(&self, conversation_id: &str, note: Option<&str>) -> StorageResult<i64> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let now_ms = timestamp::to_millis(Some(now_utc())).expect("timestamp conversion failed");
        let trimmed = note.map(|s| s.trim()).filter(|s| !s.is_empty());
        if trimmed.is_none() {
            conn.execute("DELETE FROM conversation_notes WHERE conversation_id = ?1", [conversation_id])?;
            return Ok(0);
        }
        let text = trimmed.unwrap();
        // UPSERT: 插入或更新
        conn.execute(
            "INSERT INTO conversation_notes (conversation_id, note, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(conversation_id) DO UPDATE SET note = ?2, updated_at = ?3",
            params![conversation_id, text, now_ms],
        )?;
        Ok(now_ms)
    }

    /// 查询会话是否已收藏。
    pub fn is_favorite(&self, conversation_id: &str) -> StorageResult<bool> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let v: i64 = conn
            .query_row(
                "SELECT favorite FROM conversations WHERE id = ?",
                [conversation_id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        Ok(v != 0)
    }

    /// 设置归档状态。
    pub fn set_archived(&self, conversation_id: &str, archived: bool) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let changed = conn.execute(
            "UPDATE conversations SET is_archived = ?1 WHERE id = ?2",
            params![i32::from(archived), conversation_id],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound {
                entity: "conversation",
                id: conversation_id.to_string(),
            });
        }
        Ok(())
    }

    pub fn is_archived(&self, conversation_id: &str) -> StorageResult<bool> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let v: i64 = conn
            .query_row(
                "SELECT is_archived FROM conversations WHERE id = ?",
                [conversation_id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        Ok(v != 0)
    }

    /// 给会话添加标签（幂等）。
    pub fn add_tag(&self, conversation_id: &str, tag: &str) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let now_ms = timestamp::to_millis(Some(now_utc())).expect("timestamp conversion failed");
        conn.execute(
            "INSERT INTO conversation_tags (conversation_id, tag, created_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(conversation_id, tag) DO NOTHING",
            params![conversation_id, tag, now_ms],
        )?;
        Ok(())
    }

    /// 移除标签。
    pub fn remove_tag(&self, conversation_id: &str, tag: &str) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.execute(
            "DELETE FROM conversation_tags WHERE conversation_id = ?1 AND tag = ?2",
            params![conversation_id, tag],
        )?;
        Ok(())
    }

    /// 列出会话的所有标签。
    pub fn list_tags(&self, conversation_id: &str) -> StorageResult<Vec<String>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn
            .prepare("SELECT tag FROM conversation_tags WHERE conversation_id = ? ORDER BY tag")?;
        let rows = stmt.query_map([conversation_id], |r| r.get::<_, String>(0))?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 列出全部会话标签（去重 + 按使用频次倒序）。
    /// 返回 (tag, count) 列表。
    pub fn list_all_tags(&self, limit: i64) -> StorageResult<Vec<(String, i64)>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT tag, COUNT(DISTINCT conversation_id) AS cnt
             FROM conversation_tags
             GROUP BY tag
             ORDER BY cnt DESC, tag ASC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 列出收藏的会话 ID。
    pub fn list_favorite_conversation_ids(&self) -> StorageResult<Vec<String>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn
            .prepare("SELECT id FROM conversations WHERE favorite = 1 ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    // ── 知识提取持久化（plan §13.5）──────────────────────────────────────

    // ── 删除（plan §11.4 删除语义 / §3 用户可完全删除）──────────────────

    /// 软删除：标记 `source_status=deleted，保留数据（plan` §11.4 默认行为）。
    pub fn soft_delete_conversation(&self, conversation_id: &str) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let changed = conn.execute(
            "UPDATE conversations SET source_status = 'deleted' WHERE id = ?1",
            params![conversation_id],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound {
                entity: "conversation",
                id: conversation_id.to_string(),
            });
        }
        Ok(())
    }

    /// 恢复软删除的会话。
    pub fn restore_conversation(&self, conversation_id: &str) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let changed = conn.execute(
            "UPDATE conversations SET source_status = 'active' WHERE id = ?1",
            params![conversation_id],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound {
                entity: "conversation",
                id: conversation_id.to_string(),
            });
        }
        Ok(())
    }

    /// 硬删除：物理移除会话及其所有消息、事件、标签、知识提取（级联）。
    /// plan §3「用户可完全删除数据」。
    pub fn hard_delete_conversation(&self, conversation_id: &str) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        // 外键 CASCADE 会自动清理 messages/events（FK 定义含 ON DELETE CASCADE 的表）。
        // conversation_tags 和 knowledge_extractions 也定义了 CASCADE。
        // 但 SQLite 的 CASCADE 需要开启 foreign_keys pragma（已在 init_pragmas 开启）。
        let changed = conn.execute(
            "DELETE FROM conversations WHERE id = ?1",
            params![conversation_id],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound {
                entity: "conversation",
                id: conversation_id.to_string(),
            });
        }
        Ok(())
    }

    // ── 自定义脱敏规则（plan §14.6）──────────────────────────────────────
}

// ── Row 映射函数 ────────────────────────────────────────────────────────

fn row_to_workspace(r: &rusqlite::Row<'_>) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: r.get(0)?,
        display_name: r.get(1)?,
        user_title: r.get(2)?,
        canonical_path: r.get(3)?,
        git_remote: r.get(4)?,
        git_common_dir: r.get(5)?,
        status: parse_status(&r.get::<_, String>(6)?),
        // workspaces.created_at/updated_at 均为 NOT NULL，直接取 i64。
        created_at: timestamp::from_millis(Some(r.get::<_, i64>(7)?))
            .expect("timestamp conversion failed"),
        updated_at: timestamp::from_millis(Some(r.get::<_, i64>(8)?))
            .expect("timestamp conversion failed"),
    })
}

fn row_to_conversation(r: &rusqlite::Row<'_>) -> rusqlite::Result<Conversation> {
    let provider_str: String = r.get(2)?;
    let provider = Provider::from_str(&provider_str).unwrap_or(Provider::Unknown);
    let mut c = Conversation::new(provider, r.get::<_, String>(4)?);
    c.id = r.get(0)?;
    c.workspace_id = r.get(1)?;
    c.installation_id = r.get(3)?;
    c.title = r.get(5)?;
    c.user_title = r.get(6)?;
    c.status = r.get::<_, Option<String>>(7)?.as_deref().map(parse_status);
    c.model = r.get(8)?;
    c.started_at = timestamp::from_millis(r.get(9)?);
    c.updated_at = timestamp::from_millis(r.get(10)?);
    c.completed_at = timestamp::from_millis(r.get(11)?);
    c.source_status = parse_status(&r.get::<_, String>(12)?);
    c.source_url = r.get(13)?;
    c.completeness_score = r.get(14)?;
    c.content_hash = r.get(15)?;
    c.raw_payload_id = r.get(16)?;
    c.source_parent_id = r.get(17)?;
    Ok(c)
}

fn row_to_message(r: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    let role_str: String = r.get(4)?;
    let role = match role_str.as_str() {
        "assistant" => Role::Assistant,
        "system" => Role::System,
        "tool" => Role::Tool,
        _ => Role::User,
    };
    let json_str: Option<String> = r.get(6)?;
    let content_json = json_str.and_then(|s| serde_json::from_str(&s).ok());
    let mut m = Message::new(r.get::<_, String>(1)?, role, r.get(7)?);
    m.id = r.get(0)?;
    m.turn_id = r.get(2)?;
    m.source_message_id = r.get(3)?;
    m.content_text = r.get(5)?;
    m.content_json = content_json;
    m.created_at = timestamp::from_millis(r.get(8)?);
    m.content_hash = r.get(9)?;
    m.raw_payload_id = r.get(10)?;
    Ok(m)
}

fn row_to_event(r: &rusqlite::Row<'_>) -> rusqlite::Result<Event> {
    let type_str: String = r.get(4)?;
    let event_type = parse_event_type(&type_str);
    let json_str: Option<String> = r.get(7)?;
    let payload_json = json_str.and_then(|s| serde_json::from_str(&s).ok());
    let mut e = Event::new(r.get::<_, String>(1)?, event_type, r.get(8)?);
    e.id = r.get(0)?;
    e.turn_id = r.get(2)?;
    e.source_event_id = r.get(3)?;
    e.status = r.get::<_, Option<String>>(5)?.as_deref().map(parse_status);
    e.summary = r.get(6)?;
    e.payload_json = payload_json;
    e.created_at = timestamp::from_millis(r.get(9)?);
    e.completed_at = timestamp::from_millis(r.get(10)?);
    e.raw_payload_id = r.get(11)?;
    Ok(e)
}

// ── CodeAgentOps 聚合结果结构（plan codeagent-ops §3.2）────────────────

fn parse_status(s: &str) -> Status {
    match s {
        "completed" => Status::Completed,
        "failed" => Status::Failed,
        "cancelled" => Status::Cancelled,
        "archived" => Status::Archived,
        "deleted" => Status::Deleted,
        _ => Status::Active,
    }
}

fn parse_event_type(s: &str) -> EventType {
    match s {
        "tool_call_started" => EventType::ToolCallStarted,
        "tool_call_completed" => EventType::ToolCallCompleted,
        "command_started" => EventType::CommandStarted,
        "command_completed" => EventType::CommandCompleted,
        "file_read" => EventType::FileRead,
        "file_created" => EventType::FileCreated,
        "file_updated" => EventType::FileUpdated,
        "file_deleted" => EventType::FileDeleted,
        "diff_generated" => EventType::DiffGenerated,
        "approval_requested" => EventType::ApprovalRequested,
        "approval_granted" => EventType::ApprovalGranted,
        "approval_denied" => EventType::ApprovalDenied,
        "browser_action" => EventType::BrowserAction,
        "mcp_call" => EventType::McpCall,
        "subagent_started" => EventType::SubagentStarted,
        "subagent_completed" => EventType::SubagentCompleted,
        "plan_created" => EventType::PlanCreated,
        "error" => EventType::Error,
        "artifact_created" => EventType::ArtifactCreated,
        _ => EventType::StatusChanged,
    }
}

// 让 domain 的 FromStr 在本模块可见（pub(crate) 不够，需要 use）
use std::str::FromStr as _;

#[cfg(test)]
mod tests {
    use super::*;
    use ch_domain::{
        Conversation, Event, EventType, Installation, Message, Provider, Role, Status, Workspace,
    };
    use time::OffsetDateTime;

    fn repo() -> Repository {
        let r = Repository::open_in_memory().expect("unexpected None");
        r.upsert_provider(Provider::Generic).expect("upsert failed");
        r
    }

    fn ts(s: i64) -> Timestamp {
        OffsetDateTime::from_unix_timestamp(s).expect("unexpected None")
    }

    #[test]
    fn workspace_upsert_and_get() {
        let r = repo();
        let mut ws = Workspace::new("my-web-app");
        ws.canonical_path = Some("/tmp/my-web-app".into());
        let id = r.upsert_workspace(&ws).expect("upsert failed");
        let got = r
            .get_workspace(&id)
            .expect("unexpected None")
            .expect("unexpected None");
        assert_eq!(got.display_name, "my-web-app");
        assert_eq!(got.canonical_path.as_deref(), Some("/tmp/my-web-app"));

        // 更新
        let mut ws2 = ws.clone();
        ws2.user_title = Some("custom".into());
        ws2.id = id.clone();
        r.upsert_workspace(&ws2).expect("upsert failed");
        let got2 = r
            .get_workspace(&id)
            .expect("unexpected None")
            .expect("unexpected None");
        assert_eq!(got2.user_title.as_deref(), Some("custom"));
    }

    #[test]
    fn conversation_upsert_is_idempotent() {
        let r = repo();
        let mut c = Conversation::new(Provider::Generic, "src-conv-1");
        c.title = Some("hello".into());
        c.workspace_id = None;
        let id1 = r.upsert_conversation(&c).expect("upsert failed");
        // 再次写入，不指定 workspace，应更新而非新建
        let id2 = r.upsert_conversation(&c).expect("upsert failed");
        assert_eq!(id1, id2, "idempotent upsert should return same id");
        assert_eq!(r.count_conversations().expect("unexpected None"), 1);
    }

    #[test]
    fn conversation_preserves_user_title_on_update() {
        // plan §11.5：用户数据优先
        let r = repo();
        let mut c = Conversation::new(Provider::Generic, "src-conv-2");
        c.user_title = Some("my custom title".into());
        let id = r.upsert_conversation(&c).expect("upsert failed");

        // 模拟来源标题变化后再次同步
        let mut c2 = c.clone();
        c2.id = ch_domain::new_id("conv"); // 新对象，但幂等键相同
        c2.user_title = None; // 这次同步没带 user_title
        c2.title = Some("source changed".into());
        let id2 = r.upsert_conversation(&c2).expect("upsert failed");
        assert_eq!(id, id2);

        let got = r
            .get_conversation(&id)
            .expect("unexpected None")
            .expect("unexpected None");
        assert_eq!(got.user_title.as_deref(), Some("my custom title")); // 保留
        assert_eq!(got.title.as_deref(), Some("source changed")); // 更新
    }

    #[test]
    fn message_upsert_dedup_by_sequence() {
        let r = repo();
        let mut c = Conversation::new(Provider::Generic, "src-conv-m");
        let cid = r.upsert_conversation(&c).expect("upsert failed");
        c.id = cid.clone();

        let mut m1 = Message::new(&cid, Role::User, 1);
        m1.content_text = Some("hi".into());
        r.upsert_message(&m1).expect("upsert failed");

        // 同序号再次写入 → 更新
        let mut m2 = Message::new(&cid, Role::User, 1);
        m2.content_text = Some("hi edited".into());
        r.upsert_message(&m2).expect("upsert failed");

        let msgs = r.list_messages(&cid).expect("unexpected None");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content_text.as_deref(), Some("hi edited"));
    }

    #[test]
    fn event_upsert_dedup_by_sequence() {
        let r = repo();
        let cid = r
            .upsert_conversation(&Conversation::new(Provider::Generic, "src-conv-e"))
            .expect("unexpected None");

        let mut e1 = Event::new(&cid, EventType::CommandStarted, 1);
        e1.summary = Some("cargo build".into());
        r.upsert_event(&e1).expect("upsert failed");

        let mut e2 = Event::new(&cid, EventType::CommandCompleted, 1);
        e2.summary = Some("cargo build done".into());
        r.upsert_event(&e2).expect("upsert failed");

        let events = r.list_events(&cid).expect("unexpected None");
        assert_eq!(events.len(), 1);
        // 同序号被覆盖
        assert_eq!(events[0].event_type, EventType::CommandCompleted);
    }

    #[test]
    fn list_conversations_by_workspace() {
        let r = repo();
        let ws_id = r
            .upsert_workspace(&Workspace::new("ws-a"))
            .expect("upsert failed");

        let mut c1 = Conversation::new(Provider::Generic, "c1");
        c1.workspace_id = Some(ws_id.clone());
        let mut c2 = Conversation::new(Provider::Generic, "c2");
        c2.workspace_id = Some(ws_id.clone());
        let c3 = Conversation::new(Provider::Generic, "c3"); // 无 workspace
        r.upsert_conversation(&c1).expect("upsert failed");
        r.upsert_conversation(&c2).expect("upsert failed");
        r.upsert_conversation(&c3).expect("upsert failed");

        let in_ws = r.list_conversations(Some(&ws_id)).expect("unexpected None");
        assert_eq!(in_ws.len(), 2);
        let all = r.list_conversations(None).expect("unexpected None");
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn cursor_roundtrip() {
        let r = repo();
        r.upsert_cursor(
            Provider::Generic,
            None,
            "default",
            "2026-08-02T00:00:00Z",
            None,
        )
        .expect("unexpected None");
        let v = r
            .get_cursor(Provider::Generic, None, "default")
            .expect("unexpected None");
        assert_eq!(v.as_deref(), Some("2026-08-02T00:00:00Z"));

        // 更新
        r.upsert_cursor(Provider::Generic, None, "default", "v2", None)
            .expect("unexpected None");
        let v2 = r
            .get_cursor(Provider::Generic, None, "default")
            .expect("unexpected None");
        assert_eq!(v2.as_deref(), Some("v2"));
    }

    #[test]
    fn integrity_check_passes_on_fresh_db() {
        let r = repo();
        assert!(r.integrity_check().expect("unexpected None"));
    }

    #[test]
    fn full_pipeline_import() {
        // 端到端：workspace → conversation → messages + events，重复导入幂等
        let r = repo();
        let ws_id = r
            .upsert_workspace(&Workspace::new("proj"))
            .expect("upsert failed");

        let mut conv = Conversation::new(Provider::Generic, "src-pipe");
        conv.workspace_id = Some(ws_id.clone());
        conv.title = Some("pipe test".into());
        conv.started_at = Some(ts(1_785_000_000));
        let cid = r.upsert_conversation(&conv).expect("upsert failed");

        let mut m_user = Message::new(&cid, Role::User, 1);
        m_user.content_text = Some("please build".into());
        let mut m_asst = Message::new(&cid, Role::Assistant, 2);
        m_asst.content_text = Some("done".into());
        r.upsert_message(&m_user).expect("upsert failed");
        r.upsert_message(&m_asst).expect("upsert failed");

        let mut ev = Event::new(&cid, EventType::CommandCompleted, 1);
        ev.summary = Some("cargo build".into());
        r.upsert_event(&ev).expect("upsert failed");

        // 验证读回
        let got = r
            .get_conversation(&cid)
            .expect("unexpected None")
            .expect("unexpected None");
        assert_eq!(got.title.as_deref(), Some("pipe test"));
        assert_eq!(r.list_messages(&cid).expect("unexpected None").len(), 2);
        assert_eq!(r.list_events(&cid).expect("unexpected None").len(), 1);

        // 重复导入整套 → 数量不变
        r.upsert_conversation(&conv).expect("upsert failed");
        r.upsert_message(&m_user).expect("upsert failed");
        r.upsert_message(&m_asst).expect("upsert failed");
        r.upsert_event(&ev).expect("upsert failed");
        assert_eq!(r.count_conversations().expect("unexpected None"), 1);
        assert_eq!(r.list_messages(&cid).expect("unexpected None").len(), 2);
        assert_eq!(r.list_events(&cid).expect("unexpected None").len(), 1);
    }

    #[test]
    #[allow(dead_code)]
    fn installation_unused_for_now() {
        // installation 写入路径在 Phase 1 后续启用；这里仅保证类型可构造。
        let _i = Installation::new(Provider::Generic, "device-1");
    }

    // ── 收藏 / 标签 / 归档（plan §6.3/§6.4/§6.5）─────────────────────────

    fn conv_id(repo: &Repository, src: &str) -> String {
        repo.upsert_conversation(&Conversation::new(Provider::Generic, src))
            .expect("unexpected None")
    }

    #[test]
    fn favorite_toggle_and_query() {
        let r = repo();
        let cid = conv_id(&r, "fav-1");
        assert!(!r.is_favorite(&cid).expect("unexpected None"));
        r.set_favorite(&cid, true).expect("unexpected None");
        assert!(r.is_favorite(&cid).expect("unexpected None"));
        r.set_favorite(&cid, false).expect("unexpected None");
        assert!(!r.is_favorite(&cid).expect("unexpected None"));
    }

    #[test]
    fn list_favorites() {
        let r = repo();
        let a = conv_id(&r, "fav-a");
        let b = conv_id(&r, "fav-b");
        let _c = conv_id(&r, "fav-c");
        r.set_favorite(&a, true).expect("unexpected None");
        r.set_favorite(&b, true).expect("unexpected None");
        let favs = r.list_favorite_conversation_ids().expect("unexpected None");
        assert_eq!(favs.len(), 2);
        assert!(favs.contains(&a));
        assert!(favs.contains(&b));
    }

    #[test]
    fn favorite_nonexistent_errors() {
        let r = repo();
        assert!(r.set_favorite("conv_nope", true).is_err());
    }

    #[test]
    fn archived_toggle_and_query() {
        let r = repo();
        let cid = conv_id(&r, "arch-1");
        assert!(!r.is_archived(&cid).expect("unexpected None"));
        r.set_archived(&cid, true).expect("unexpected None");
        assert!(r.is_archived(&cid).expect("unexpected None"));
    }

    #[test]
    fn tags_add_remove_list() {
        let r = repo();
        let cid = conv_id(&r, "tag-1");
        assert!(r.list_tags(&cid).expect("unexpected None").is_empty());

        r.add_tag(&cid, "rust").expect("unexpected None");
        r.add_tag(&cid, "tauri").expect("unexpected None");
        r.add_tag(&cid, "rust").expect("unexpected None"); // 幂等：重复添加不报错

        let tags = r.list_tags(&cid).expect("unexpected None");
        assert_eq!(tags, vec!["rust".to_string(), "tauri".to_string()]);

        r.remove_tag(&cid, "rust").expect("unexpected None");
        let tags2 = r.list_tags(&cid).expect("unexpected None");
        assert_eq!(tags2, vec!["tauri".to_string()]);
    }

    #[test]
    fn tags_chinese() {
        let r = repo();
        let cid = conv_id(&r, "tag-zh");
        r.add_tag(&cid, "后端").expect("unexpected None");
        r.add_tag(&cid, "架构").expect("unexpected None");
        let tags = r.list_tags(&cid).expect("unexpected None");
        assert_eq!(tags.len(), 2);
        assert!(tags.contains(&"后端".to_string()));
    }

    #[test]
    fn remove_nonexistent_tag_no_error() {
        let r = repo();
        let cid = conv_id(&r, "tag-rm");
        // 删除不存在的标签不报错
        assert!(r.remove_tag(&cid, "nope").is_ok());
        assert!(r.list_tags(&cid).expect("unexpected None").is_empty());
    }

    // ── 多维过滤（plan §6.4）─────────────────────────────────────────────

    use crate::filter::ConversationFilter;

    /// activity_stats 6 件套返回：heatmap / hourly 全量 / weekday / weekend / tools_trend / tool_daily。
    /// 用「现在 + 偏移」避免时区硬编码（SQLite `localtime` 跟测试环境 TZ 耦合）。
    #[test]
    fn activity_stats_six_fields() {
        let r = repo();
        r.upsert_provider(Provider::Codex).expect("upsert failed");
        let now_ms = {
            let t = ch_domain::now_utc();
            (t - OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64
        };
        // 在「昨天 23:30」「今天 00:30」「今天 01:00」三处插 3 条（localtime 下覆盖跨日）
        let day_ms = 86_400_000_i64;
        let ts1 = now_ms - day_ms - 30 * 60_000;     // 昨天 23:30（UTC）
        let ts2 = now_ms - 30 * 60_000;              // 今天 00:30（UTC）
        let ts3 = now_ms;                              // 现在
        {
            let conn = r.conn.lock().expect("mutex");
            conn.execute(
                "INSERT INTO tool_call_records (id, provider_id, source_session_id, tool_name, ts, status)
                 VALUES ('t1','prov_codex','s1','Bash',?1,'completed'),
                        ('t2','prov_codex','s1','Bash',?2,'completed'),
                        ('t3','prov_codex','s1','Read',?3,'completed')",
                rusqlite::params![ts1, ts2, ts3],
            ).expect("insert tool_call");
        }
        let stats = r.activity_stats(365).expect("activity");
        // 3 条 → 至少 1 个 heatmap 桶
        assert!(!stats.heatmap.is_empty(), "应至少有 heatmap 数据");
        let total: i64 = stats.heatmap.iter().map(|c| c.calls).sum();
        assert_eq!(total, 3, "3 条记录总和应为 3 次调用");
        // hourly 全 24 槽
        assert_eq!(stats.hourly.len(), 24);
        // 工作日+周末拆分总数应等于 3
        let weekend_total: i64 = stats.hourly_weekend.iter().map(|h| h.calls).sum();
        let weekday_total: i64 = stats.hourly_weekday.iter().map(|h| h.calls).sum();
        assert_eq!(weekend_total + weekday_total, 3, "工作日+周末 总数应等于 3 条");
        // tools_trend 按月聚合：应至少有 1 行
        assert!(!stats.tools_trend.is_empty());
        // tool_daily 应含 Bash 工具
        assert!(stats.tool_daily.iter().any(|t| t.tool == "Bash"));
        assert!(stats.tool_daily.iter().any(|t| t.tool == "Read"));
    }

    #[test]
    fn filter_by_favorite() {
        let r = repo();
        r.upsert_provider(Provider::Codex).expect("upsert failed");
        let a = conv_id(&r, "f-a");
        let _b = conv_id(&r, "f-b");
        r.set_favorite(&a, true).expect("unexpected None");

        let favs = r
            .list_conversations_filtered(&ConversationFilter::new().favorites_only())
            .expect("unexpected None");
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].id, a);
    }

    #[test]
    fn filter_by_archived() {
        let r = repo();
        let a = conv_id(&r, "ar-a");
        let b = conv_id(&r, "ar-b");
        r.set_archived(&a, true).expect("unexpected None");

        let unarchived = r
            .list_conversations_filtered(&ConversationFilter::new().unarchived_only())
            .expect("unexpected None");
        assert!(unarchived.iter().all(|c| c.id != a));
        assert!(unarchived.iter().any(|c| c.id == b));

        let archived = r
            .list_conversations_filtered(&ConversationFilter::new().archived_only())
            .expect("unexpected None");
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].id, a);
    }

    #[test]
    fn filter_by_provider() {
        let r = repo();
        r.upsert_provider(Provider::Codex).expect("upsert failed");
        let _generic_conv = conv_id(&r, "p-generic"); // Generic（由 repo() 创建）
        let codex_conv = r
            .upsert_conversation(&Conversation::new(Provider::Codex, "p-codex"))
            .expect("unexpected None");

        let codex_only = r
            .list_conversations_filtered(&ConversationFilter::new().with_provider(Provider::Codex))
            .expect("unexpected None");
        assert_eq!(codex_only.len(), 1);
        assert_eq!(codex_only[0].id, codex_conv);
    }

    #[test]
    fn filter_by_workspace() {
        let r = repo();
        let ws_id = r
            .upsert_workspace(&Workspace::new("ws-f"))
            .expect("upsert failed");
        let mut c = Conversation::new(Provider::Generic, "ws-conv");
        c.workspace_id = Some(ws_id.clone());
        let in_ws = r.upsert_conversation(&c).expect("upsert failed");
        let _other = conv_id(&r, "ws-other");

        let in_workspace = r
            .list_conversations_filtered(&ConversationFilter::new().with_workspace(ws_id.clone()))
            .expect("unexpected None");
        assert_eq!(in_workspace.len(), 1);
        assert_eq!(in_workspace[0].id, in_ws);
    }

    #[test]
    fn filter_by_date_range() {
        let r = repo();
        // ts(N) 是 unix 秒；存储转毫秒。设三条会话分别对应 unix 100/500/900 秒。
        let mut c1 = Conversation::new(Provider::Generic, "date-old");
        c1.started_at = Some(ts(100));
        r.upsert_conversation(&c1).expect("upsert failed");
        let mut c2 = Conversation::new(Provider::Generic, "date-in");
        c2.started_at = Some(ts(500));
        r.upsert_conversation(&c2).expect("upsert failed");
        let mut c3 = Conversation::new(Provider::Generic, "date-new");
        c3.started_at = Some(ts(900));
        r.upsert_conversation(&c3).expect("upsert failed");

        // 闭区间 [300s, 700s] → 只命中 c2（毫秒 300_000..=700_000）
        let in_range = r
            .list_conversations_filtered(
                &ConversationFilter::new().with_started_range_ms(300_000, 700_000),
            )
            .expect("unexpected None");
        assert_eq!(in_range.len(), 1, "应只命中 started_at ∈ [300s, 700s] 的会话");
        assert!(in_range[0].source_conversation_id.contains("date-in"));

        // 只有 from ≥ 300s → c2 + c3
        let after = r
            .list_conversations_filtered(
                &ConversationFilter {
                    started_after_ms: Some(300_000),
                    ..ConversationFilter::new()
                },
            )
            .expect("unexpected None");
        assert_eq!(after.len(), 2);

        // 只有 to ≤ 700s → c1 + c2
        let before = r
            .list_conversations_filtered(
                &ConversationFilter {
                    started_before_ms: Some(700_000),
                    ..ConversationFilter::new()
                },
            )
            .expect("unexpected None");
        assert_eq!(before.len(), 2);
    }

    #[test]
    fn filter_combined() {
        let r = repo();
        r.upsert_provider(Provider::Codex).expect("upsert failed");
        let ws_id = r
            .upsert_workspace(&Workspace::new("ws-c"))
            .expect("upsert failed");

        // 一个满足全部条件的
        let mut c = Conversation::new(Provider::Codex, "combined");
        c.workspace_id = Some(ws_id.clone());
        let target = r.upsert_conversation(&c).expect("upsert failed");
        r.set_favorite(&target, true).expect("unexpected None");

        // 不满足 favorite 的
        let mut c2 = Conversation::new(Provider::Codex, "combined-nofav");
        c2.workspace_id = Some(ws_id.clone());
        let _nofav = r.upsert_conversation(&c2).expect("upsert failed");

        let filter = ConversationFilter::new()
            .with_provider(Provider::Codex)
            .with_workspace(ws_id.clone())
            .favorites_only();
        let result = r
            .list_conversations_filtered(&filter)
            .expect("unexpected None");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, target);
    }

    #[test]
    fn filter_empty_returns_all() {
        let r = repo();
        let _a = conv_id(&r, "all-a");
        let _b = conv_id(&r, "all-b");
        let all = r
            .list_conversations_filtered(&ConversationFilter::new())
            .expect("unexpected None");
        assert_eq!(all.len(), 2);
    }

    // ── 删除（plan §11.4 / §3）────────────────────────────────────────────

    #[test]
    fn soft_delete_marks_deleted() {
        let r = repo();
        let cid = conv_id(&r, "del-soft");
        // 软删除
        r.soft_delete_conversation(&cid).expect("unexpected None");
        // 仍存在，但 source_status=deleted
        let conv = r
            .get_conversation(&cid)
            .expect("unexpected None")
            .expect("unexpected None");
        assert_eq!(conv.source_status, Status::Deleted);
    }

    #[test]
    fn restore_soft_deleted() {
        let r = repo();
        let cid = conv_id(&r, "del-restore");
        r.soft_delete_conversation(&cid).expect("unexpected None");
        r.restore_conversation(&cid).expect("unexpected None");
        let conv = r
            .get_conversation(&cid)
            .expect("unexpected None")
            .expect("unexpected None");
        assert_eq!(conv.source_status, Status::Active);
    }

    #[test]
    fn import_conversation_batch_works_after_clear_all() {
        // 回归：providers.adapter_id NOT NULL，批量导入曾绑 NULL 导致
        // 重置后所有导入静默失败（2026-08-14 真实事故）
        let r = Repository::open_in_memory().expect("unexpected None");
        r.clear_all().expect("unexpected None");
        let conv = ch_domain::Conversation::new(Provider::ZCode, "src-batch-regress");
        let msgs = vec![ch_domain::Message::new(&conv.id, ch_domain::Role::User, 1)];
        let id = r
            .import_conversation_batch(&conv, &msgs, &[], Some("ZCode"), Some(1000))
            .expect("unexpected None");
        let got = r
            .get_conversation(&id)
            .expect("unexpected None")
            .expect("unexpected None");
        assert_eq!(got.source_conversation_id, "src-batch-regress");
        assert_eq!(r.list_messages(&id).expect("unexpected None").len(), 1);
        // 幂等重放
        let id2 = r
            .import_conversation_batch(&conv, &msgs, &[], Some("ZCode"), Some(1000))
            .expect("unexpected None");
        assert_eq!(id, id2);
        assert_eq!(
            r.list_messages(&id).expect("unexpected None").len(),
            1,
            "重放不产生重复消息"
        );
    }

    #[test]
    fn import_batch_preserves_conv_workspace_when_no_name() {
        // workspace_name=None 时不得把 conv 上已设置的 workspace 覆盖成 NULL
        let r = Repository::open_in_memory().expect("unexpected None");
        let ws_id = r
            .upsert_workspace(&ch_domain::Workspace::new("proj"))
            .expect("unexpected None");
        let mut conv = ch_domain::Conversation::new(Provider::ZCode, "src-ws-keep");
        conv.workspace_id = Some(ws_id.clone());
        let msgs = vec![ch_domain::Message::new(&conv.id, ch_domain::Role::User, 1)];
        let id = r
            .import_conversation_batch(&conv, &msgs, &[], None, None)
            .expect("unexpected None");
        let got = r
            .get_conversation(&id)
            .expect("unexpected None")
            .expect("unexpected None");
        assert_eq!(got.workspace_id.as_deref(), Some(ws_id.as_str()));
    }

    #[test]
    fn child_counts_bulk_matches_count_children() {
        let r = Repository::open_in_memory().expect("unexpected None");
        r.upsert_provider(Provider::ZCode).expect("upsert failed");
        let parent = ch_domain::Conversation::new(Provider::ZCode, "src-parent");
        r.upsert_conversation(&parent).expect("upsert failed");
        for i in 0..3 {
            let mut child = ch_domain::Conversation::new(Provider::ZCode, format!("src-child-{i}"));
            child.source_parent_id = Some("src-parent".into());
            r.upsert_conversation(&child).expect("upsert failed");
        }
        let bulk = r.child_counts_bulk().expect("unexpected None");
        assert_eq!(
            bulk.get(&("src-parent".to_string(), "prov_zcode".to_string())),
            Some(&3),
            "bulk count must match"
        );
        assert_eq!(
            r.count_children("src-parent", "prov_zcode")
                .expect("unexpected None"),
            3
        );
    }

    #[test]
    fn is_up_to_date_staleness_logic() {
        use std::collections::{HashMap, HashSet};
        let mut existing = HashSet::new();
        existing.insert(("prov_zcode".to_string(), "s1".to_string()));
        let mut st = HashMap::new();
        st.insert("s1".to_string(), Some(1000i64));

        // 源无更新（同时间/更早）→ 已导入
        assert!(Repository::is_up_to_date(
            &st,
            &existing,
            "prov_zcode",
            "s1",
            Some(1000)
        ));
        assert!(Repository::is_up_to_date(
            &st,
            &existing,
            "prov_zcode",
            "s1",
            Some(900)
        ));
        // 源有新对话（更新时间更晚）→ 可再导入
        assert!(!Repository::is_up_to_date(
            &st,
            &existing,
            "prov_zcode",
            "s1",
            Some(1001)
        ));
        // 源时间未知 → 退化存在性
        assert!(Repository::is_up_to_date(
            &st,
            &existing,
            "prov_zcode",
            "s1",
            None
        ));
        // 不存在 → 未导入
        assert!(!Repository::is_up_to_date(
            &st,
            &existing,
            "prov_zcode",
            "s2",
            Some(1)
        ));
        // 无观察记录（历史遗留）→ 存在即已导入
        let st2: HashMap<String, Option<i64>> = HashMap::new();
        assert!(Repository::is_up_to_date(
            &st2,
            &existing,
            "prov_zcode",
            "s1",
            Some(9999)
        ));
    }

    #[test]
    fn hard_delete_removes_conversation_and_cascade() {
        let r = repo();
        let cid = conv_id(&r, "del-hard");
        // 加消息和标签
        let mut m = Message::new(&cid, Role::User, 1);
        m.content_text = Some("to be deleted".into());
        r.upsert_message(&m).expect("upsert failed");
        r.add_tag(&cid, "temp").expect("unexpected None");

        r.hard_delete_conversation(&cid).expect("unexpected None");
        // 会话已不存在
        assert!(r.get_conversation(&cid).expect("unexpected None").is_none());
        // 消息级联删除
        assert!(r.list_messages(&cid).expect("unexpected None").is_empty());
        // 标签级联删除
        assert!(r.list_tags(&cid).expect("unexpected None").is_empty());
        // 总数减少
        assert_eq!(r.count_conversations().expect("unexpected None"), 0);
    }

    #[test]
    fn delete_nonexistent_errors() {
        let r = repo();
        assert!(r.soft_delete_conversation("conv_nope").is_err());
        assert!(r.hard_delete_conversation("conv_nope").is_err());
        assert!(r.restore_conversation("conv_nope").is_err());
    }

    // ── 自定义脱敏规则（plan §14.6）──────────────────────────────────────

    #[test]
    fn redaction_rule_add_list_remove() {
        let r = repo();
        r.add_redaction_rule("emp_id", r"EMP\d{6}")
            .expect("unexpected None");
        r.add_redaction_rule("id_card", r"\d{17}[\dXx]")
            .expect("unexpected None");

        let rules = r.list_redaction_rules().expect("unexpected None");
        assert_eq!(rules.len(), 2);
        assert!(rules.iter().any(|x| x.name == "emp_id"));
        assert!(rules.iter().any(|x| x.name == "id_card"));
        assert!(rules.iter().all(|x| x.enabled));

        r.remove_redaction_rule("emp_id").expect("unexpected None");
        let rules2 = r.list_redaction_rules().expect("unexpected None");
        assert_eq!(rules2.len(), 1);
        assert_eq!(rules2[0].name, "id_card");
    }

    #[test]
    fn redaction_rule_upsert_by_name() {
        let r = repo();
        r.add_redaction_rule("test", r"v1")
            .expect("unexpected None");
        r.add_redaction_rule("test", r"v2")
            .expect("unexpected None"); // 更新 pattern
        let rules = r.list_redaction_rules().expect("unexpected None");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, "v2");
    }

    #[test]
    fn redaction_rule_remove_nonexistent_no_error() {
        let r = repo();
        assert!(r.remove_redaction_rule("nope").is_ok());
        assert!(r
            .list_redaction_rules()
            .expect("unexpected None")
            .is_empty());
    }

    // ── 知识提取持久化（plan §13.5）──────────────────────────────────────

    #[test]
    fn save_and_get_knowledge() {
        let r = repo();
        let cid = conv_id(&r, "know-1");
        let json = r#"{"summary":"测试摘要","decisions":[],"todos":[],"errors":[],"commands":[],"files":[],"extractor":"rule-v1"}"#;
        let kid = r
            .save_knowledge(&cid, "rule-v1", json)
            .expect("unexpected None");

        let rec = r
            .get_knowledge(&cid)
            .expect("unexpected None")
            .expect("unexpected None");
        assert_eq!(rec.id, kid);
        assert_eq!(rec.version, 1);
        assert_eq!(rec.extractor, "rule-v1");
        assert!(rec.result_json.contains("测试摘要"));
    }

    #[test]
    fn save_increments_version_and_marks_current() {
        let r = repo();
        let cid = conv_id(&r, "know-2");

        r.save_knowledge(&cid, "rule-v1", "{\"v\":1}")
            .expect("unexpected None");
        r.save_knowledge(&cid, "rule-v1", "{\"v\":2}")
            .expect("unexpected None");
        r.save_knowledge(&cid, "rule-v1", "{\"v\":3}")
            .expect("unexpected None");

        // 当前版本应是 3
        let current = r
            .get_knowledge(&cid)
            .expect("unexpected None")
            .expect("unexpected None");
        assert_eq!(current.version, 3);
        assert!(current.result_json.contains("\"v\":3"));

        // 历史应有 3 个版本
        let versions = r.list_knowledge_versions(&cid).expect("unexpected None");
        assert_eq!(versions.len(), 3);
        // 降序：第一个是最新
        assert_eq!(versions[0].version, 3);
        assert_eq!(versions[2].version, 1);
    }

    #[test]
    fn get_knowledge_none_when_not_saved() {
        let r = repo();
        let cid = conv_id(&r, "know-empty");
        assert!(r.get_knowledge(&cid).expect("unexpected None").is_none());
    }

    #[test]
    fn knowledge_separate_per_conversation() {
        let r = repo();
        let a = conv_id(&r, "know-a");
        let b = conv_id(&r, "know-b");
        r.save_knowledge(&a, "rule-v1", "{\"conv\":\"a\"}")
            .expect("unexpected None");
        r.save_knowledge(&b, "rule-v1", "{\"conv\":\"b\"}")
            .expect("unexpected None");

        let rec_a = r
            .get_knowledge(&a)
            .expect("unexpected None")
            .expect("unexpected None");
        let rec_b = r
            .get_knowledge(&b)
            .expect("unexpected None")
            .expect("unexpected None");
        assert!(rec_a.result_json.contains("\"a\""));
        assert!(rec_b.result_json.contains("\"b\""));
        assert_eq!(rec_a.version, 1);
        assert_eq!(rec_b.version, 1);
    }

    // ── V11 治理闭环新增能力 ─────────────────────────────────────────

    fn seeded_usage(r: &Repository) {
        r.upsert_provider(Provider::Codex).expect("upsert failed");
        let u = ch_domain::UsageRecord {
            id: "u1".into(),
            provider: Provider::Codex,
            source_session_id: "s1".into(),
            turn_id: Some("t1".into()),
            model: Some("gpt-test".into()),
            ts: ch_domain::now_utc(),
            input_tokens: 1000,
            output_tokens: 500,
            reasoning_tokens: 0,
            cache_read_tokens: 800,
            cache_write_tokens: 0,
            cost_usd: Some(1.5),
            status: ch_domain::UsageStatus::Completed,
            duration_ms: Some(100),
            retry_count: None,
            source_dir: None,
            context_exceeded: 0,
        };
        r.upsert_usage_batch(&[u]).expect("upsert failed");
    }

    #[test]
    fn audit_finding_states_roundtrip() {
        let r = Repository::open_in_memory().expect("unexpected None");
        r.set_audit_finding_state("abc123", "ignored", Some("测试"))
            .expect("SQL execution failed");
        r.set_audit_finding_state("def456", "false_positive", None)
            .expect("SQL execution failed");
        let states = r.list_audit_finding_states().expect("SQL execution failed");
        assert_eq!(states.len(), 2);
        // upsert 覆盖
        r.set_audit_finding_state("abc123", "false_positive", None)
            .expect("SQL execution failed");
        let s = &r.list_audit_finding_states().expect("SQL execution failed")[0];
        assert_eq!(s.status, "false_positive");
        // 清除
        r.clear_audit_finding_state("def456")
            .expect("SQL execution failed");
        assert_eq!(
            r.list_audit_finding_states()
                .expect("SQL execution failed")
                .len(),
            1
        );
    }

    #[test]
    fn governance_log_roundtrip() {
        let r = Repository::open_in_memory().expect("unexpected None");
        r.log_governance_action("reset_all_data", None, None, "ok", None)
            .expect("SQL execution failed");
        r.log_governance_action(
            "hard_delete_conversation",
            Some("conversation"),
            Some("c1"),
            "ok",
            None,
        )
        .expect("SQL execution failed");
        let log = r.list_governance_log(10).expect("SQL execution failed");
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].action, "hard_delete_conversation"); // 倒序
        assert_eq!(log[1].action, "reset_all_data");
    }

    #[test]
    fn month_projection_extrapolates() {
        let r = Repository::open_in_memory().expect("unexpected None");
        seeded_usage(&r);
        let p = r.ops_month_projection().expect("SQL execution failed");
        assert_eq!(p.tokens_so_far, 1500);
        assert!((p.cost_so_far - 1.5).abs() < 1e-9);
        // 日均外推：days_elapsed >= 1 → 预测 >= 已用
        assert!(p.projected_tokens >= p.tokens_so_far);
        assert!(p.projected_cost >= p.cost_so_far - 1e-9);
        assert!(p.days_in_month >= 28);
    }

    #[test]
    fn cache_trend_rows() {
        let r = Repository::open_in_memory().expect("unexpected None");
        seeded_usage(&r);
        let rows = r.ops_cache_trend(Some(30)).expect("SQL execution failed");
        assert_eq!(rows.len(), 1);
        // total 口径含 cache_read：1000+500+0+800 = 2300
        assert_eq!(rows[0].total_input, 2300);
        assert_eq!(rows[0].cache_read, 800);
    }

    #[test]
    fn archive_older_than_targets_stale() {
        let r = Repository::open_in_memory().expect("unexpected None");
        r.upsert_provider(Provider::Generic).expect("upsert failed");
        let mut old = Conversation::new(Provider::Generic, "src-old");
        old.updated_at = Some(ch_domain::now_utc() - time::Duration::days(400));
        let mut fresh = Conversation::new(Provider::Generic, "src-fresh");
        fresh.updated_at = Some(ch_domain::now_utc());
        r.upsert_conversation(&old).expect("upsert failed");
        r.upsert_conversation(&fresh).expect("upsert failed");
        let n = r
            .archive_conversations_older_than(90)
            .expect("SQL execution failed");
        assert_eq!(n, 1, "只归档 400 天前的");
        let (fav_old, arch_old) = r
            .get_conversation_flags(&old.id)
            .expect("SQL execution failed");
        assert!(arch_old);
        assert!(!fav_old);
        let (_, arch_fresh) = r
            .get_conversation_flags(&fresh.id)
            .expect("SQL execution failed");
        assert!(!arch_fresh);
    }

    #[test]
    fn filter_deleted_dimension() {
        let r = Repository::open_in_memory().expect("unexpected None");
        r.upsert_provider(Provider::Generic).expect("upsert failed");
        let c1 = Conversation::new(Provider::Generic, "src-d1");
        let c2 = Conversation::new(Provider::Generic, "src-d2");
        r.upsert_conversation(&c1).expect("upsert failed");
        r.upsert_conversation(&c2).expect("upsert failed");
        r.soft_delete_conversation(&c1.id)
            .expect("SQL execution failed");

        let visible = r
            .list_conversations_filtered(&ConversationFilter::default().exclude_deleted())
            .expect("SQL execution failed");
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].source_conversation_id, "src-d2");

        let deleted = r
            .list_conversations_filtered(&ConversationFilter::default().deleted_only())
            .expect("SQL execution failed");
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].source_conversation_id, "src-d1");

        // 恢复
        r.restore_conversation(&c1.id)
            .expect("SQL execution failed");
        assert_eq!(
            r.list_conversations_filtered(&ConversationFilter::default().deleted_only())
                .expect("SQL execution failed")
                .len(),
            0
        );
    }

    #[test]
    fn raw_payload_refs_and_flags_bulk() {
        let r = Repository::open_in_memory().expect("unexpected None");
        r.upsert_provider(Provider::Generic).expect("upsert failed");
        let mut c = Conversation::new(Provider::Generic, "src-refs");
        c.raw_payload_id = Some("a".repeat(64));
        r.upsert_conversation(&c).expect("upsert failed");
        let refs = r.list_raw_payload_refs().expect("SQL execution failed");
        assert!(refs.contains(&"a".repeat(64)));
        let flags = r.conversation_flags_bulk().expect("SQL execution failed");
        assert!(flags.contains_key(&c.id));
    }
}
