//! Repository：存储层的统一入口，对应 plan §9.4「所有写操作由 Daemon 单点负责」。
//!
//! 关键能力：
//! - 打开/初始化数据库（含 WAL + 4 项 PRAGMA）。
//! - 幂等写入：Conversation / Message / Event 重复 upsert 不产生重复。
//! - 事务包裹：见 plan §11.2「SQLite 事务写入」。

use crate::error::{StorageError, StorageResult};
use crate::filter::ConversationFilter;
use crate::search;
use crate::migration;
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
pub struct Repository {
    conn: Mutex<Connection>,
}

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
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        Ok(())
    }

    /// 完整性检查，对应 plan §7.3 与上线清单。
    pub fn integrity_check(&self) -> StorageResult<bool> {
        let conn = self.conn.lock().unwrap();
        let result: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        Ok(result == "ok")
    }

    /// 全文搜索，对应 plan §13。委托给 search 模块。
    pub fn search(&self, q: &search::SearchQuery) -> StorageResult<Vec<search::SearchResult>> {
        let conn = self.conn.lock().unwrap();
        search::search(&conn, q)
    }

    // ── Provider / Installation ──────────────────────────────────────────

    /// 写入或更新 provider 记录（按 name 去重）。
    pub fn upsert_provider(&self, p: Provider) -> StorageResult<String> {
        let conn = self.conn.lock().unwrap();
        let now_ms = timestamp::to_millis(Some(now_utc())).unwrap();
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
        let conn = self.conn.lock().unwrap();
        let now_ms = timestamp::to_millis(Some(now_utc())).unwrap();
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
        let conn = self.conn.lock().unwrap();
        let now_ms = timestamp::to_millis(Some(now_utc())).unwrap();
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
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
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
                created_at: timestamp::from_millis(Some(r.get::<_, Option<i64>>(7)?.unwrap_or(0))).unwrap_or_else(now_utc),
                updated_at: timestamp::from_millis(Some(r.get::<_, Option<i64>>(8)?.unwrap_or(0))).unwrap_or_else(now_utc),
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    // ── Conversation（幂等核心） ─────────────────────────────────────────

    /// 写入 conversation。幂等键：(provider_id, installation_id, source_conversation_id)。
    /// 重复写入更新内容字段，但保留 user_title（plan §11.5：用户数据优先）。
    pub fn upsert_conversation(&self, c: &Conversation) -> StorageResult<String> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
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

    pub fn list_conversations(&self, workspace_id: Option<&str>) -> StorageResult<Vec<Conversation>> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        let mut where_clauses: Vec<String> = Vec::new();
        let mut args: Vec<SqlValue> = Vec::new();
        let mut next_idx = 1usize;

        let push = |clause: String, val: SqlValue, wc: &mut Vec<String>, a: &mut Vec<SqlValue>, idx: &mut usize| {
            wc.push(clause.replace("?", &format!("?{idx}")));
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
                wsid.to_string().into(),
                &mut where_clauses,
                &mut args,
                &mut next_idx,
            );
        }
        if let Some(fav) = filter.favorite {
            push(
                "c.favorite = ?".to_string(),
                (if fav { 1 } else { 0 } as i64).into(),
                &mut where_clauses,
                &mut args,
                &mut next_idx,
            );
        }
        if let Some(arch) = filter.archived {
            push(
                "c.is_archived = ?".to_string(),
                (if arch { 1 } else { 0 } as i64).into(),
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
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))?)
    }

    /// 列出指定父会话的子任务（按 source_parent_id 关联）。
    /// `parent_source_id` 是父会话的 source_conversation_id，`provider_id` 形如 `prov_zcode`。
    pub fn list_child_conversations(
        &self,
        parent_source_id: &str,
        provider_id: &str,
    ) -> StorageResult<Vec<Conversation>> {
        let conn = self.conn.lock().unwrap();
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
    pub fn count_children(
        &self,
        parent_source_id: &str,
        provider_id: &str,
    ) -> StorageResult<i64> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM conversations
             WHERE source_parent_id = ?1 AND provider_id = ?2",
            params![parent_source_id, provider_id],
            |r| r.get(0),
        )?)
    }

    /// 清空所有数据（conversations 级联删除 messages/events/tags/knowledge）。
    /// 保留 schema 和 redaction_rules（用户自定义规则）。
    /// 用于「重置数据」功能。
    pub fn clear_all(&self) -> StorageResult<()> {
        let conn = self.conn.lock().unwrap();
        // conversations 有 ON DELETE CASCADE，会自动清理 messages/events/turns/tags/knowledge
        conn.execute("DELETE FROM conversations", [])?;
        conn.execute("DELETE FROM workspaces", [])?;
        conn.execute("DELETE FROM source_workspaces", [])?;
        conn.execute("DELETE FROM providers", [])?;
        conn.execute("DELETE FROM installations", [])?;
        conn.execute("DELETE FROM usage_records", [])?;
        conn.execute("DELETE FROM tool_call_records", [])?;
        Ok(())
    }

    // ── CodeAgentOps：用量/工具调用指标（plan codeagent-ops §3.2）────────

    /// 批量写入用量记录（事务 + 幂等：UNIQUE 键冲突跳过）。
    pub fn upsert_usage_batch(&self, records: &[ch_domain::UsageRecord]) -> StorageResult<usize> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut n = 0;
        for r in records {
            let changed = tx.execute(
                "INSERT OR IGNORE INTO usage_records
                    (id, provider_id, source_session_id, turn_id, model, ts,
                     input_tokens, output_tokens, reasoning_tokens, cache_read_tokens,
                     cache_write_tokens, cost_usd, status, duration_ms, retry_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    r.id,
                    format!("prov_{}", r.provider.as_str()),
                    r.source_session_id,
                    r.turn_id,
                    r.model,
                    timestamp::to_millis(Some(r.ts)).unwrap_or(0),
                    r.input_tokens,
                    r.output_tokens,
                    r.reasoning_tokens,
                    r.cache_read_tokens,
                    r.cache_write_tokens,
                    r.cost_usd,
                    r.status.as_str(),
                    r.duration_ms,
                    r.retry_count,
                ],
            )?;
            n += changed;
        }
        tx.commit()?;
        Ok(n)
    }

    /// 批量写入工具调用记录（事务 + 幂等）。
    pub fn upsert_tool_call_batch(
        &self,
        records: &[ch_domain::ToolCallRecord],
    ) -> StorageResult<usize> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut n = 0;
        for r in records {
            let changed = tx.execute(
                "INSERT OR IGNORE INTO tool_call_records
                    (id, provider_id, source_session_id, tool_name, ts, read_only,
                     destructive, approval_status, exit_code, duration_ms, status, command_text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    r.id,
                    format!("prov_{}", r.provider.as_str()),
                    r.source_session_id,
                    r.tool_name,
                    timestamp::to_millis(Some(r.ts)).unwrap_or(0),
                    r.read_only,
                    r.destructive,
                    r.approval_status,
                    r.exit_code,
                    r.duration_ms,
                    r.status.as_str(),
                    r.command_text,
                ],
            )?;
            n += changed;
        }
        tx.commit()?;
        Ok(n)
    }

    /// 时间范围过滤子句：days=None 全量，否则最近 N 天。
    fn range_clause(days: Option<i64>) -> (String, Option<i64>) {
        match days {
            Some(d) => {
                let cutoff = timestamp::to_millis(Some(ch_domain::now_utc())).unwrap_or(0)
                    - d * 86_400_000;
                ("ts >= ?1".to_string(), Some(cutoff))
            }
            None => ("1=1".to_string(), None),
        }
    }

    /// 治理总览 KPI。
    pub fn ops_overview(&self, days: Option<i64>) -> StorageResult<OpsOverview> {
        let conn = self.conn.lock().unwrap();
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT
                (SELECT COUNT(*) FROM usage_records WHERE {clause_u}),
                COALESCE((SELECT SUM(input_tokens + output_tokens + reasoning_tokens) FROM usage_records WHERE {clause_u}), 0),
                COALESCE((SELECT SUM(input_tokens) FROM usage_records WHERE {clause_u}), 0),
                COALESCE((SELECT SUM(output_tokens) FROM usage_records WHERE {clause_u}), 0),
                COALESCE((SELECT SUM(cost_usd) FROM usage_records WHERE cost_usd IS NOT NULL AND {clause_u}), 0),
                COALESCE((SELECT AVG(duration_ms) FROM usage_records WHERE duration_ms IS NOT NULL AND {clause_u}), 0),
                (SELECT COUNT(*) FROM usage_records WHERE status = 'error' AND {clause_u}),
                (SELECT COUNT(DISTINCT source_session_id) FROM usage_records WHERE {clause_u}),
                COALESCE((SELECT COUNT(*) FROM tool_call_records WHERE destructive = 1 AND {clause_t}), 0),
                (SELECT COUNT(*) FROM tool_call_records WHERE {clause_t})",
            clause_u = clause, clause_t = clause,
        );
        let args: Vec<SqlValue> = [cutoff, cutoff, cutoff, cutoff, cutoff, cutoff, cutoff, cutoff, cutoff, cutoff]
            .iter()
            .filter_map(|c| c.map(|v| v.into()))
            .collect();
        conn.query_row(&sql, params_from_iter(args.iter()), |r| {
            Ok(OpsOverview {
                total_requests: r.get(0)?,
                total_tokens: r.get(1)?,
                input_tokens: r.get(2)?,
                output_tokens: r.get(3)?,
                cost_usd: r.get(4)?,
                avg_duration_ms: r.get(5)?,
                error_count: r.get(6)?,
                session_count: r.get(7)?,
                destructive_calls: r.get(8)?,
                total_tool_calls: r.get(9)?,
            })
        })
        .map_err(Into::into)
    }

    /// 按 provider 聚合。
    pub fn ops_by_provider(&self, days: Option<i64>) -> StorageResult<Vec<ProviderUsage>> {
        let conn = self.conn.lock().unwrap();
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT p.name, COUNT(*),
                    SUM(input_tokens + output_tokens + reasoning_tokens),
                    SUM(output_tokens),
                    SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END)
             FROM usage_records u JOIN providers p ON p.id = u.provider_id
             WHERE {clause}
             GROUP BY u.provider_id ORDER BY 3 DESC",
            clause = clause,
        );
        let args: Vec<SqlValue> = cutoff.map(|v| v.into()).into_iter().collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            Ok(ProviderUsage {
                provider: r.get(0)?,
                requests: r.get(1)?,
                total_tokens: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                output_tokens: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                errors: r.get::<_, Option<i64>>(4)?.unwrap_or(0),
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 按模型聚合。
    pub fn ops_by_model(&self, days: Option<i64>) -> StorageResult<Vec<ModelUsage>> {
        let conn = self.conn.lock().unwrap();
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT COALESCE(model, '(unknown)'), u.provider_id, COUNT(*),
                    SUM(input_tokens), SUM(output_tokens),
                    SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END)
             FROM usage_records u
             WHERE {clause}
             GROUP BY model, u.provider_id ORDER BY 4 DESC",
            clause = clause,
        );
        let args: Vec<SqlValue> = cutoff.map(|v| v.into()).into_iter().collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            Ok(ModelUsage {
                model: r.get(0)?,
                provider_id: r.get(1)?,
                requests: r.get(2)?,
                input_tokens: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                output_tokens: r.get::<_, Option<i64>>(4)?.unwrap_or(0),
                errors: r.get::<_, Option<i64>>(5)?.unwrap_or(0),
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 每日用量时间序列。
    pub fn ops_timeseries_daily(&self, days: Option<i64>) -> StorageResult<Vec<DailyUsage>> {
        let conn = self.conn.lock().unwrap();
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT date(ts/1000, 'unixepoch', 'localtime') AS day,
                    SUM(input_tokens + output_tokens + reasoning_tokens),
                    COUNT(*)
             FROM usage_records WHERE {clause}
             GROUP BY day ORDER BY day",
            clause = clause,
        );
        let args: Vec<SqlValue> = cutoff.map(|v| v.into()).into_iter().collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            Ok(DailyUsage {
                day: r.get(0)?,
                total_tokens: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                requests: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 工具调用 Top N。
    pub fn ops_tool_toplist(&self, days: Option<i64>, n: i64) -> StorageResult<Vec<ToolUsageRow>> {
        let conn = self.conn.lock().unwrap();
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT tool_name, COUNT(*),
                    SUM(CASE WHEN destructive = 1 THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END),
                    COALESCE(AVG(duration_ms), 0)
             FROM tool_call_records WHERE {clause}
             GROUP BY tool_name ORDER BY 2 DESC LIMIT ?2",
            clause = clause,
        );
        let args: Vec<SqlValue> = [cutoff.map(|v| v.into()), Some(n.into())]
            .into_iter()
            .flatten()
            .collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            Ok(ToolUsageRow {
                tool_name: r.get(0)?,
                calls: r.get(1)?,
                destructive: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                errors: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                avg_duration_ms: r.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 风险调用列表（破坏性 / 出错 / 需审批）。
    pub fn ops_risky_calls(&self, days: Option<i64>, n: i64) -> StorageResult<Vec<ch_domain::ToolCallRecord>> {
        let conn = self.conn.lock().unwrap();
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT id, p.name, source_session_id, tool_name, ts, read_only,
                    destructive, approval_status, exit_code, duration_ms, status, command_text
             FROM tool_call_records t JOIN providers p ON p.id = t.provider_id
             WHERE (destructive = 1 OR status = 'error' OR (exit_code IS NOT NULL AND exit_code != 0))
               AND {clause}
             ORDER BY ts DESC LIMIT ?2",
            clause = clause,
        );
        let args: Vec<SqlValue> = [cutoff.map(|v| v.into()), Some(n.into())]
            .into_iter()
            .flatten()
            .collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            Ok(ch_domain::ToolCallRecord {
                id: r.get(0)?,
                provider: ch_domain::Provider::from_str(&r.get::<_, String>(1)?)
                    .unwrap_or(ch_domain::Provider::Unknown),
                source_session_id: r.get(2)?,
                tool_name: r.get(3)?,
                ts: timestamp::from_millis(Some(r.get::<_, i64>(4)?)).unwrap_or_else(ch_domain::now_utc),
                read_only: r.get::<_, Option<i64>>(5)?.map(|v| v != 0),
                destructive: r.get::<_, Option<i64>>(6)?.map(|v| v != 0),
                approval_status: r.get(7)?,
                exit_code: r.get(8)?,
                duration_ms: r.get(9)?,
                status: ch_domain::UsageStatus::parse(&r.get::<_, String>(10)?),
                command_text: r.get(11)?,
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    // ── Message（幂等：按 content_hash + sequence 去重） ──────────────────

    /// 写入 message。幂等键：(conversation_id, sequence_number)。
    pub fn upsert_message(&self, m: &Message) -> StorageResult<String> {
        let conn = self.conn.lock().unwrap();
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
                m.content_json.as_ref().map(|v| v.to_string()),
                m.sequence_number,
                timestamp::to_millis(m.created_at),
                m.content_hash,
                m.raw_payload_id,
            ],
        )?;
        Ok(id)
    }

    pub fn list_messages(&self, conversation_id: &str) -> StorageResult<Vec<Message>> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
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
                e.payload_json.as_ref().map(|v| v.to_string()),
                e.sequence_number,
                timestamp::to_millis(e.created_at),
                timestamp::to_millis(e.completed_at),
                e.raw_payload_id,
            ],
        )?;
        Ok(id)
    }

    pub fn list_events(&self, conversation_id: &str) -> StorageResult<Vec<Event>> {
        let conn = self.conn.lock().unwrap();
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

    /// 写入同步游标。幂等：(provider_id, installation_id, cursor_type)。
    pub fn upsert_cursor(
        &self,
        provider: Provider,
        installation_id: Option<&str>,
        cursor_type: &str,
        value: &str,
        schema_fingerprint: Option<&str>,
    ) -> StorageResult<()> {
        let conn = self.conn.lock().unwrap();
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
                timestamp::to_millis(Some(now_utc())).unwrap(),
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
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE conversations SET favorite = ?1 WHERE id = ?2",
            params![if favorite { 1 } else { 0 }, conversation_id],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound {
                entity: "conversation",
                id: conversation_id.to_string(),
            });
        }
        Ok(())
    }

    /// 查询会话是否已收藏。
    pub fn is_favorite(&self, conversation_id: &str) -> StorageResult<bool> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE conversations SET is_archived = ?1 WHERE id = ?2",
            params![if archived { 1 } else { 0 }, conversation_id],
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
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        let now_ms = timestamp::to_millis(Some(now_utc())).unwrap();
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
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM conversation_tags WHERE conversation_id = ?1 AND tag = ?2",
            params![conversation_id, tag],
        )?;
        Ok(())
    }

    /// 列出会话的所有标签。
    pub fn list_tags(&self, conversation_id: &str) -> StorageResult<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT tag FROM conversation_tags WHERE conversation_id = ? ORDER BY tag",
        )?;
        let rows = stmt.query_map([conversation_id], |r| r.get::<_, String>(0))?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 列出收藏的会话 ID。
    pub fn list_favorite_conversation_ids(&self) -> StorageResult<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT id FROM conversations WHERE favorite = 1 ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    // ── 知识提取持久化（plan §13.5）──────────────────────────────────────

    /// 保存一条知识提取结果（plan §13.5「人工编辑后保留版本」）。
    ///
    /// 把该 conversation 的旧版本标记 is_current=0，新版本作为 current。
    /// `result_json` 是 ExtractionResult 的序列化字符串。
    pub fn save_knowledge(
        &self,
        conversation_id: &str,
        extractor: &str,
        result_json: &str,
    ) -> StorageResult<String> {
        let conn = self.conn.lock().unwrap();
        let now_ms = timestamp::to_millis(Some(now_utc())).unwrap();
        let id = ch_domain::new_id("know");

        // 旧版本取消 current
        conn.execute(
            "UPDATE knowledge_extractions SET is_current = 0 WHERE conversation_id = ?1",
            params![conversation_id],
        )?;

        // 计算新版本号
        let max_version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM knowledge_extractions WHERE conversation_id = ?1",
                params![conversation_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        conn.execute(
            "INSERT INTO knowledge_extractions
                (id, conversation_id, version, is_current, extractor, result_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?6)",
            params![&id, conversation_id, max_version + 1, extractor, result_json, now_ms],
        )?;
        Ok(id)
    }

    /// 获取某会话的当前知识提取结果（JSON 字符串 + 版本号）。
    pub fn get_knowledge(&self, conversation_id: &str) -> StorageResult<Option<KnowledgeRecord>> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, conversation_id, version, extractor, result_json, created_at, updated_at
                 FROM knowledge_extractions
                 WHERE conversation_id = ?1 AND is_current = 1",
                params![conversation_id],
                |r| {
                    Ok(KnowledgeRecord {
                        id: r.get(0)?,
                        conversation_id: r.get(1)?,
                        version: r.get(2)?,
                        extractor: r.get(3)?,
                        result_json: r.get(4)?,
                        created_at: timestamp::from_millis(r.get(5)?).unwrap_or_else(now_utc),
                        updated_at: timestamp::from_millis(r.get(6)?).unwrap_or_else(now_utc),
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// 列出某会话的所有历史版本（按版本降序）。
    pub fn list_knowledge_versions(&self, conversation_id: &str) -> StorageResult<Vec<KnowledgeRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, version, extractor, result_json, created_at, updated_at
             FROM knowledge_extractions
             WHERE conversation_id = ?1
             ORDER BY version DESC",
        )?;
        let rows = stmt.query_map(params![conversation_id], |r| {
            Ok(KnowledgeRecord {
                id: r.get(0)?,
                conversation_id: r.get(1)?,
                version: r.get(2)?,
                extractor: r.get(3)?,
                result_json: r.get(4)?,
                created_at: timestamp::from_millis(r.get(5)?).unwrap_or_else(now_utc),
                updated_at: timestamp::from_millis(r.get(6)?).unwrap_or_else(now_utc),
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    // ── 删除（plan §11.4 删除语义 / §3 用户可完全删除）──────────────────

    /// 软删除：标记 source_status=deleted，保留数据（plan §11.4 默认行为）。
    pub fn soft_delete_conversation(&self, conversation_id: &str) -> StorageResult<()> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
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

    /// 添加自定义脱敏规则（幂等：按 name upsert）。
    pub fn add_redaction_rule(&self, name: &str, pattern: &str) -> StorageResult<()> {
        let conn = self.conn.lock().unwrap();
        let now_ms = timestamp::to_millis(Some(now_utc())).unwrap();
        conn.execute(
            "INSERT INTO redaction_rules (id, name, pattern, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, 1, ?4, ?4)
             ON CONFLICT(name) DO UPDATE SET pattern = ?3, updated_at = ?4",
            params![ch_domain::new_id("rule"), name, pattern, now_ms],
        )?;
        Ok(())
    }

    /// 列出所有已启用的脱敏规则。
    pub fn list_redaction_rules(&self) -> StorageResult<Vec<RedactionRuleRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, pattern, enabled FROM redaction_rules ORDER BY name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(RedactionRuleRecord {
                id: r.get(0)?,
                name: r.get(1)?,
                pattern: r.get(2)?,
                enabled: r.get::<_, i64>(3)? != 0,
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 按名称删除脱敏规则。
    pub fn remove_redaction_rule(&self, name: &str) -> StorageResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM redaction_rules WHERE name = ?1", params![name])?;
        Ok(())
    }
}

/// 脱敏规则记录。
#[derive(Debug, Clone, PartialEq)]
pub struct RedactionRuleRecord {
    pub id: String,
    pub name: String,
    pub pattern: String,
    pub enabled: bool,
}

/// 知识提取记录（从库读回的行）。
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeRecord {
    pub id: String,
    pub conversation_id: String,
    pub version: i64,
    pub extractor: String,
    /// ExtractionResult 的 JSON 字符串。
    pub result_json: String,
    pub created_at: ch_domain::Timestamp,
    pub updated_at: ch_domain::Timestamp,
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
        created_at: timestamp::from_millis(Some(r.get::<_, i64>(7)?)).unwrap(),
        updated_at: timestamp::from_millis(Some(r.get::<_, i64>(8)?)).unwrap(),
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
        "user" => Role::User,
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

/// 治理总览 KPI。
#[derive(Debug, Clone, serde::Serialize)]
pub struct OpsOverview {
    pub total_requests: i64,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: f64,
    pub avg_duration_ms: f64,
    pub error_count: i64,
    pub session_count: i64,
    pub destructive_calls: i64,
    pub total_tool_calls: i64,
}

/// 按 provider 聚合。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderUsage {
    pub provider: String,
    pub requests: i64,
    pub total_tokens: i64,
    pub output_tokens: i64,
    pub errors: i64,
}

/// 按模型聚合。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelUsage {
    pub model: String,
    pub provider_id: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub errors: i64,
}

/// 每日用量。
#[derive(Debug, Clone, serde::Serialize)]
pub struct DailyUsage {
    pub day: String,
    pub total_tokens: i64,
    pub requests: i64,
}

/// 工具调用统计行。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolUsageRow {
    pub tool_name: String,
    pub calls: i64,
    pub destructive: i64,
    pub errors: i64,
    pub avg_duration_ms: f64,
}

fn parse_status(s: &str) -> Status {
    match s {
        "active" => Status::Active,
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
        "status_changed" => EventType::StatusChanged,
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
        let r = Repository::open_in_memory().unwrap();
        r.upsert_provider(Provider::Generic).unwrap();
        r
    }

    fn ts(s: i64) -> Timestamp {
        OffsetDateTime::from_unix_timestamp(s).unwrap()
    }

    #[test]
    fn workspace_upsert_and_get() {
        let r = repo();
        let mut ws = Workspace::new("my-web-app");
        ws.canonical_path = Some("/tmp/my-web-app".into());
        let id = r.upsert_workspace(&ws).unwrap();
        let got = r.get_workspace(&id).unwrap().unwrap();
        assert_eq!(got.display_name, "my-web-app");
        assert_eq!(got.canonical_path.as_deref(), Some("/tmp/my-web-app"));

        // 更新
        let mut ws2 = ws.clone();
        ws2.user_title = Some("custom".into());
        ws2.id = id.clone();
        r.upsert_workspace(&ws2).unwrap();
        let got2 = r.get_workspace(&id).unwrap().unwrap();
        assert_eq!(got2.user_title.as_deref(), Some("custom"));
    }

    #[test]
    fn conversation_upsert_is_idempotent() {
        let r = repo();
        let mut c = Conversation::new(Provider::Generic, "src-conv-1");
        c.title = Some("hello".into());
        c.workspace_id = None;
        let id1 = r.upsert_conversation(&c).unwrap();
        // 再次写入，不指定 workspace，应更新而非新建
        let id2 = r.upsert_conversation(&c).unwrap();
        assert_eq!(id1, id2, "idempotent upsert should return same id");
        assert_eq!(r.count_conversations().unwrap(), 1);
    }

    #[test]
    fn conversation_preserves_user_title_on_update() {
        // plan §11.5：用户数据优先
        let r = repo();
        let mut c = Conversation::new(Provider::Generic, "src-conv-2");
        c.user_title = Some("my custom title".into());
        let id = r.upsert_conversation(&c).unwrap();

        // 模拟来源标题变化后再次同步
        let mut c2 = c.clone();
        c2.id = ch_domain::new_id("conv"); // 新对象，但幂等键相同
        c2.user_title = None; // 这次同步没带 user_title
        c2.title = Some("source changed".into());
        let id2 = r.upsert_conversation(&c2).unwrap();
        assert_eq!(id, id2);

        let got = r.get_conversation(&id).unwrap().unwrap();
        assert_eq!(got.user_title.as_deref(), Some("my custom title")); // 保留
        assert_eq!(got.title.as_deref(), Some("source changed")); // 更新
    }

    #[test]
    fn message_upsert_dedup_by_sequence() {
        let r = repo();
        let mut c = Conversation::new(Provider::Generic, "src-conv-m");
        let cid = r.upsert_conversation(&c).unwrap();
        c.id = cid.clone();

        let mut m1 = Message::new(&cid, Role::User, 1);
        m1.content_text = Some("hi".into());
        r.upsert_message(&m1).unwrap();

        // 同序号再次写入 → 更新
        let mut m2 = Message::new(&cid, Role::User, 1);
        m2.content_text = Some("hi edited".into());
        r.upsert_message(&m2).unwrap();

        let msgs = r.list_messages(&cid).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content_text.as_deref(), Some("hi edited"));
    }

    #[test]
    fn event_upsert_dedup_by_sequence() {
        let r = repo();
        let cid = r
            .upsert_conversation(&Conversation::new(Provider::Generic, "src-conv-e"))
            .unwrap();

        let mut e1 = Event::new(&cid, EventType::CommandStarted, 1);
        e1.summary = Some("cargo build".into());
        r.upsert_event(&e1).unwrap();

        let mut e2 = Event::new(&cid, EventType::CommandCompleted, 1);
        e2.summary = Some("cargo build done".into());
        r.upsert_event(&e2).unwrap();

        let events = r.list_events(&cid).unwrap();
        assert_eq!(events.len(), 1);
        // 同序号被覆盖
        assert_eq!(events[0].event_type, EventType::CommandCompleted);
    }

    #[test]
    fn list_conversations_by_workspace() {
        let r = repo();
        let ws_id = r.upsert_workspace(&Workspace::new("ws-a")).unwrap();

        let mut c1 = Conversation::new(Provider::Generic, "c1");
        c1.workspace_id = Some(ws_id.clone());
        let mut c2 = Conversation::new(Provider::Generic, "c2");
        c2.workspace_id = Some(ws_id.clone());
        let c3 = Conversation::new(Provider::Generic, "c3"); // 无 workspace
        r.upsert_conversation(&c1).unwrap();
        r.upsert_conversation(&c2).unwrap();
        r.upsert_conversation(&c3).unwrap();

        let in_ws = r.list_conversations(Some(&ws_id)).unwrap();
        assert_eq!(in_ws.len(), 2);
        let all = r.list_conversations(None).unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn cursor_roundtrip() {
        let r = repo();
        r.upsert_cursor(Provider::Generic, None, "default", "2026-08-02T00:00:00Z", None)
            .unwrap();
        let v = r.get_cursor(Provider::Generic, None, "default").unwrap();
        assert_eq!(v.as_deref(), Some("2026-08-02T00:00:00Z"));

        // 更新
        r.upsert_cursor(Provider::Generic, None, "default", "v2", None)
            .unwrap();
        let v2 = r.get_cursor(Provider::Generic, None, "default").unwrap();
        assert_eq!(v2.as_deref(), Some("v2"));
    }

    #[test]
    fn integrity_check_passes_on_fresh_db() {
        let r = repo();
        assert!(r.integrity_check().unwrap());
    }

    #[test]
    fn full_pipeline_import() {
        // 端到端：workspace → conversation → messages + events，重复导入幂等
        let r = repo();
        let ws_id = r.upsert_workspace(&Workspace::new("proj")).unwrap();

        let mut conv = Conversation::new(Provider::Generic, "src-pipe");
        conv.workspace_id = Some(ws_id.clone());
        conv.title = Some("pipe test".into());
        conv.started_at = Some(ts(1_785_000_000));
        let cid = r.upsert_conversation(&conv).unwrap();

        let mut m_user = Message::new(&cid, Role::User, 1);
        m_user.content_text = Some("please build".into());
        let mut m_asst = Message::new(&cid, Role::Assistant, 2);
        m_asst.content_text = Some("done".into());
        r.upsert_message(&m_user).unwrap();
        r.upsert_message(&m_asst).unwrap();

        let mut ev = Event::new(&cid, EventType::CommandCompleted, 1);
        ev.summary = Some("cargo build".into());
        r.upsert_event(&ev).unwrap();

        // 验证读回
        let got = r.get_conversation(&cid).unwrap().unwrap();
        assert_eq!(got.title.as_deref(), Some("pipe test"));
        assert_eq!(r.list_messages(&cid).unwrap().len(), 2);
        assert_eq!(r.list_events(&cid).unwrap().len(), 1);

        // 重复导入整套 → 数量不变
        r.upsert_conversation(&conv).unwrap();
        r.upsert_message(&m_user).unwrap();
        r.upsert_message(&m_asst).unwrap();
        r.upsert_event(&ev).unwrap();
        assert_eq!(r.count_conversations().unwrap(), 1);
        assert_eq!(r.list_messages(&cid).unwrap().len(), 2);
        assert_eq!(r.list_events(&cid).unwrap().len(), 1);
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
            .unwrap()
    }

    #[test]
    fn favorite_toggle_and_query() {
        let r = repo();
        let cid = conv_id(&r, "fav-1");
        assert!(!r.is_favorite(&cid).unwrap());
        r.set_favorite(&cid, true).unwrap();
        assert!(r.is_favorite(&cid).unwrap());
        r.set_favorite(&cid, false).unwrap();
        assert!(!r.is_favorite(&cid).unwrap());
    }

    #[test]
    fn list_favorites() {
        let r = repo();
        let a = conv_id(&r, "fav-a");
        let b = conv_id(&r, "fav-b");
        let _c = conv_id(&r, "fav-c");
        r.set_favorite(&a, true).unwrap();
        r.set_favorite(&b, true).unwrap();
        let favs = r.list_favorite_conversation_ids().unwrap();
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
        assert!(!r.is_archived(&cid).unwrap());
        r.set_archived(&cid, true).unwrap();
        assert!(r.is_archived(&cid).unwrap());
    }

    #[test]
    fn tags_add_remove_list() {
        let r = repo();
        let cid = conv_id(&r, "tag-1");
        assert!(r.list_tags(&cid).unwrap().is_empty());

        r.add_tag(&cid, "rust").unwrap();
        r.add_tag(&cid, "tauri").unwrap();
        r.add_tag(&cid, "rust").unwrap(); // 幂等：重复添加不报错

        let tags = r.list_tags(&cid).unwrap();
        assert_eq!(tags, vec!["rust".to_string(), "tauri".to_string()]);

        r.remove_tag(&cid, "rust").unwrap();
        let tags2 = r.list_tags(&cid).unwrap();
        assert_eq!(tags2, vec!["tauri".to_string()]);
    }

    #[test]
    fn tags_chinese() {
        let r = repo();
        let cid = conv_id(&r, "tag-zh");
        r.add_tag(&cid, "后端").unwrap();
        r.add_tag(&cid, "架构").unwrap();
        let tags = r.list_tags(&cid).unwrap();
        assert_eq!(tags.len(), 2);
        assert!(tags.contains(&"后端".to_string()));
    }

    #[test]
    fn remove_nonexistent_tag_no_error() {
        let r = repo();
        let cid = conv_id(&r, "tag-rm");
        // 删除不存在的标签不报错
        assert!(r.remove_tag(&cid, "nope").is_ok());
        assert!(r.list_tags(&cid).unwrap().is_empty());
    }

    // ── 多维过滤（plan §6.4）─────────────────────────────────────────────

    use crate::filter::ConversationFilter;

    #[test]
    fn filter_by_favorite() {
        let r = repo();
        r.upsert_provider(Provider::Codex).unwrap();
        let a = conv_id(&r, "f-a");
        let _b = conv_id(&r, "f-b");
        r.set_favorite(&a, true).unwrap();

        let favs = r
            .list_conversations_filtered(&ConversationFilter::new().favorites_only())
            .unwrap();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].id, a);
    }

    #[test]
    fn filter_by_archived() {
        let r = repo();
        let a = conv_id(&r, "ar-a");
        let b = conv_id(&r, "ar-b");
        r.set_archived(&a, true).unwrap();

        let unarchived = r
            .list_conversations_filtered(&ConversationFilter::new().unarchived_only())
            .unwrap();
        assert!(unarchived.iter().all(|c| c.id != a));
        assert!(unarchived.iter().any(|c| c.id == b));

        let archived = r
            .list_conversations_filtered(&ConversationFilter::new().archived_only())
            .unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].id, a);
    }

    #[test]
    fn filter_by_provider() {
        let r = repo();
        r.upsert_provider(Provider::Codex).unwrap();
        let _generic_conv = conv_id(&r, "p-generic"); // Generic（由 repo() 创建）
        let codex_conv = r
            .upsert_conversation(&Conversation::new(Provider::Codex, "p-codex"))
            .unwrap();

        let codex_only = r
            .list_conversations_filtered(&ConversationFilter::new().with_provider(Provider::Codex))
            .unwrap();
        assert_eq!(codex_only.len(), 1);
        assert_eq!(codex_only[0].id, codex_conv);
    }

    #[test]
    fn filter_by_workspace() {
        let r = repo();
        let ws_id = r.upsert_workspace(&Workspace::new("ws-f")).unwrap();
        let mut c = Conversation::new(Provider::Generic, "ws-conv");
        c.workspace_id = Some(ws_id.clone());
        let in_ws = r.upsert_conversation(&c).unwrap();
        let _other = conv_id(&r, "ws-other");

        let in_workspace = r
            .list_conversations_filtered(&ConversationFilter::new().with_workspace(ws_id.clone()))
            .unwrap();
        assert_eq!(in_workspace.len(), 1);
        assert_eq!(in_workspace[0].id, in_ws);
    }

    #[test]
    fn filter_combined() {
        let r = repo();
        r.upsert_provider(Provider::Codex).unwrap();
        let ws_id = r.upsert_workspace(&Workspace::new("ws-c")).unwrap();

        // 一个满足全部条件的
        let mut c = Conversation::new(Provider::Codex, "combined");
        c.workspace_id = Some(ws_id.clone());
        let target = r.upsert_conversation(&c).unwrap();
        r.set_favorite(&target, true).unwrap();

        // 不满足 favorite 的
        let mut c2 = Conversation::new(Provider::Codex, "combined-nofav");
        c2.workspace_id = Some(ws_id.clone());
        let _nofav = r.upsert_conversation(&c2).unwrap();

        let filter = ConversationFilter::new()
            .with_provider(Provider::Codex)
            .with_workspace(ws_id.clone())
            .favorites_only();
        let result = r.list_conversations_filtered(&filter).unwrap();
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
            .unwrap();
        assert_eq!(all.len(), 2);
    }

    // ── 删除（plan §11.4 / §3）────────────────────────────────────────────

    #[test]
    fn soft_delete_marks_deleted() {
        let r = repo();
        let cid = conv_id(&r, "del-soft");
        // 软删除
        r.soft_delete_conversation(&cid).unwrap();
        // 仍存在，但 source_status=deleted
        let conv = r.get_conversation(&cid).unwrap().unwrap();
        assert_eq!(conv.source_status, Status::Deleted);
    }

    #[test]
    fn restore_soft_deleted() {
        let r = repo();
        let cid = conv_id(&r, "del-restore");
        r.soft_delete_conversation(&cid).unwrap();
        r.restore_conversation(&cid).unwrap();
        let conv = r.get_conversation(&cid).unwrap().unwrap();
        assert_eq!(conv.source_status, Status::Active);
    }

    #[test]
    fn hard_delete_removes_conversation_and_cascade() {
        let r = repo();
        let cid = conv_id(&r, "del-hard");
        // 加消息和标签
        let mut m = Message::new(&cid, Role::User, 1);
        m.content_text = Some("to be deleted".into());
        r.upsert_message(&m).unwrap();
        r.add_tag(&cid, "temp").unwrap();

        r.hard_delete_conversation(&cid).unwrap();
        // 会话已不存在
        assert!(r.get_conversation(&cid).unwrap().is_none());
        // 消息级联删除
        assert!(r.list_messages(&cid).unwrap().is_empty());
        // 标签级联删除
        assert!(r.list_tags(&cid).unwrap().is_empty());
        // 总数减少
        assert_eq!(r.count_conversations().unwrap(), 0);
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
        r.add_redaction_rule("emp_id", r"EMP\d{6}").unwrap();
        r.add_redaction_rule("id_card", r"\d{17}[\dXx]").unwrap();

        let rules = r.list_redaction_rules().unwrap();
        assert_eq!(rules.len(), 2);
        assert!(rules.iter().any(|x| x.name == "emp_id"));
        assert!(rules.iter().any(|x| x.name == "id_card"));
        assert!(rules.iter().all(|x| x.enabled));

        r.remove_redaction_rule("emp_id").unwrap();
        let rules2 = r.list_redaction_rules().unwrap();
        assert_eq!(rules2.len(), 1);
        assert_eq!(rules2[0].name, "id_card");
    }

    #[test]
    fn redaction_rule_upsert_by_name() {
        let r = repo();
        r.add_redaction_rule("test", r"v1").unwrap();
        r.add_redaction_rule("test", r"v2").unwrap(); // 更新 pattern
        let rules = r.list_redaction_rules().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, "v2");
    }

    #[test]
    fn redaction_rule_remove_nonexistent_no_error() {
        let r = repo();
        assert!(r.remove_redaction_rule("nope").is_ok());
        assert!(r.list_redaction_rules().unwrap().is_empty());
    }

    // ── 知识提取持久化（plan §13.5）──────────────────────────────────────

    #[test]
    fn save_and_get_knowledge() {
        let r = repo();
        let cid = conv_id(&r, "know-1");
        let json = r#"{"summary":"测试摘要","decisions":[],"todos":[],"errors":[],"commands":[],"files":[],"extractor":"rule-v1"}"#;
        let kid = r.save_knowledge(&cid, "rule-v1", json).unwrap();

        let rec = r.get_knowledge(&cid).unwrap().unwrap();
        assert_eq!(rec.id, kid);
        assert_eq!(rec.version, 1);
        assert_eq!(rec.extractor, "rule-v1");
        assert!(rec.result_json.contains("测试摘要"));
    }

    #[test]
    fn save_increments_version_and_marks_current() {
        let r = repo();
        let cid = conv_id(&r, "know-2");

        r.save_knowledge(&cid, "rule-v1", "{\"v\":1}").unwrap();
        r.save_knowledge(&cid, "rule-v1", "{\"v\":2}").unwrap();
        r.save_knowledge(&cid, "rule-v1", "{\"v\":3}").unwrap();

        // 当前版本应是 3
        let current = r.get_knowledge(&cid).unwrap().unwrap();
        assert_eq!(current.version, 3);
        assert!(current.result_json.contains("\"v\":3"));

        // 历史应有 3 个版本
        let versions = r.list_knowledge_versions(&cid).unwrap();
        assert_eq!(versions.len(), 3);
        // 降序：第一个是最新
        assert_eq!(versions[0].version, 3);
        assert_eq!(versions[2].version, 1);
    }

    #[test]
    fn get_knowledge_none_when_not_saved() {
        let r = repo();
        let cid = conv_id(&r, "know-empty");
        assert!(r.get_knowledge(&cid).unwrap().is_none());
    }

    #[test]
    fn knowledge_separate_per_conversation() {
        let r = repo();
        let a = conv_id(&r, "know-a");
        let b = conv_id(&r, "know-b");
        r.save_knowledge(&a, "rule-v1", "{\"conv\":\"a\"}").unwrap();
        r.save_knowledge(&b, "rule-v1", "{\"conv\":\"b\"}").unwrap();

        let rec_a = r.get_knowledge(&a).unwrap().unwrap();
        let rec_b = r.get_knowledge(&b).unwrap().unwrap();
        assert!(rec_a.result_json.contains("\"a\""));
        assert!(rec_b.result_json.contains("\"b\""));
        assert_eq!(rec_a.version, 1);
        assert_eq!(rec_b.version, 1);
    }
}
