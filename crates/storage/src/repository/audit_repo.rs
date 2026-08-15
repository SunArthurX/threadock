//! 审计与治理域：策略规则、预算设置、审计扫描取数（从 repository.rs 拆出）。

use super::Repository;
use crate::error::StorageResult;
use crate::timestamp;
use ch_domain::now_utc;
use rusqlite::{params, OptionalExtension};
use std::str::FromStr as _;

/// 审计策略规则（M4）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PolicyRuleRecord {
    pub id: String,
    pub name: String,
    pub pattern: String,
    pub kind: String,
    pub severity: String,
    pub enabled: bool,
}

/// 预算设置（M5）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BudgetSettings {
    pub monthly_token_limit: Option<i64>,
    pub monthly_cost_limit: Option<f64>,
    pub notify_on_exceed: bool,
}

/// 审计扫描用消息行。
#[derive(Debug, Clone)]
pub struct AuditMessageRow {
    pub message_id: String,
    pub provider: String,
    pub source_conversation_id: String,
    pub conversation_title: Option<String>,
    pub content_text: String,
}

/// 审计发现处置状态（fingerprint → 忽略/误报白名单）。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AuditFindingState {
    pub fingerprint: String,
    /// `ignored`（不再提示）或 `false_positive`（误报，规则命中但非敏感）。
    pub status: String,
    pub note: Option<String>,
    pub created_at: i64,
}

/// 治理操作流水行（audit_logs 表启用后的读取形态）。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GovernanceLogRow {
    pub id: String,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub result: String,
    pub metadata_json: Option<String>,
    pub created_at: i64,
}

impl Repository {
    /// 列出策略规则。
    pub fn list_policy_rules(&self) -> StorageResult<Vec<PolicyRuleRecord>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, name, pattern, kind, severity, enabled FROM policy_rules
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(PolicyRuleRecord {
                id: r.get(0)?,
                name: r.get(1)?,
                pattern: r.get(2)?,
                kind: r.get(3)?,
                severity: r.get(4)?,
                enabled: r.get::<_, i64>(5)? != 0,
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 新增/更新策略规则（按 name 幂等）。
    pub fn upsert_policy_rule(&self, rule: &PolicyRuleRecord) -> StorageResult<String> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let now_ms = timestamp::to_millis(Some(now_utc())).unwrap_or(0);
        conn.execute(
            "INSERT INTO policy_rules (id, name, pattern, kind, severity, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
             ON CONFLICT(name) DO UPDATE SET
                pattern = ?3, kind = ?4, severity = ?5, enabled = ?6, updated_at = ?7",
            params![rule.id, rule.name, rule.pattern, rule.kind, rule.severity, i64::from(rule.enabled), now_ms],
        )?;
        Ok(rule.id.clone())
    }

    /// 删除策略规则。
    pub fn delete_policy_rule(&self, name: &str) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.execute("DELETE FROM policy_rules WHERE name = ?1", params![name])?;
        Ok(())
    }

    /// 读取预算设置（无行返回默认值）。
    pub fn get_budget_settings(&self) -> StorageResult<BudgetSettings> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let row = conn
            .query_row(
                "SELECT monthly_token_limit, monthly_cost_limit, notify_on_exceed FROM budget_settings WHERE id = 1",
                [],
                |r| {
                    Ok(BudgetSettings {
                        monthly_token_limit: r.get(0)?,
                        monthly_cost_limit: r.get(1)?,
                        notify_on_exceed: r.get::<_, i64>(2)? != 0,
                    })
                },
            )
            .optional()?;
        Ok(row.unwrap_or(BudgetSettings {
            monthly_token_limit: None,
            monthly_cost_limit: None,
            notify_on_exceed: true,
        }))
    }

    /// 保存预算设置（单行 upsert）。
    pub fn set_budget_settings(&self, s: &BudgetSettings) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let now_ms = timestamp::to_millis(Some(now_utc())).unwrap_or(0);
        conn.execute(
            "INSERT INTO budget_settings (id, monthly_token_limit, monthly_cost_limit, notify_on_exceed, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                monthly_token_limit = ?1, monthly_cost_limit = ?2, notify_on_exceed = ?3, updated_at = ?4",
            params![s.monthly_token_limit, s.monthly_cost_limit, i64::from(s.notify_on_exceed), now_ms],
        )?;
        Ok(())
    }

    /// 审计扫描用：keyset 分页遍历消息（带会话来源信息）。
    ///
    /// `after_id` 传空串从头开始。keyset（`m.id > ?`）代替 OFFSET：
    /// OFFSET 在第 k 批要跳过 k*batch 行，全库扫描是 O(n²)；keyset 每批恒定成本。
    pub fn list_messages_for_audit(
        &self,
        after_id: &str,
        limit: i64,
    ) -> StorageResult<Vec<AuditMessageRow>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT m.id, p.name, c.source_conversation_id, c.title, m.content_text
             FROM messages m
             JOIN conversations c ON c.id = m.conversation_id
             JOIN providers p ON p.id = c.provider_id
             WHERE m.content_text IS NOT NULL AND length(m.content_text) > 0
               AND m.id > ?1
             ORDER BY m.id
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![after_id, limit], |r| {
            Ok(AuditMessageRow {
                message_id: r.get(0)?,
                provider: r.get(1)?,
                source_conversation_id: r.get(2)?,
                conversation_title: r.get(3)?,
                content_text: r.get(4)?,
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 审计扫描用：所有含命令文本的工具调用记录。
    pub fn list_tool_calls_for_audit(&self) -> StorageResult<Vec<ch_domain::ToolCallRecord>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT t.id, p.name, source_session_id, tool_name, ts, read_only,
                    destructive, approval_status, exit_code, duration_ms, status, command_text
             FROM tool_call_records t JOIN providers p ON p.id = t.provider_id
             WHERE command_text IS NOT NULL AND length(command_text) > 0
             ORDER BY ts DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ch_domain::ToolCallRecord {
                id: r.get(0)?,
                provider: ch_domain::Provider::from_str(&r.get::<_, String>(1)?)
                    .unwrap_or(ch_domain::Provider::Unknown),
                source_session_id: r.get(2)?,
                tool_name: r.get(3)?,
                ts: timestamp::from_millis(Some(r.get::<_, i64>(4)?))
                    .unwrap_or_else(ch_domain::now_utc),
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

    // ── 审计发现处置（M4 闭环：忽略/误报白名单）──────────────────────

    /// 列出全部处置状态（key = fingerprint）。
    pub fn list_audit_finding_states(&self) -> StorageResult<Vec<AuditFindingState>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT fingerprint, status, note, created_at FROM audit_finding_states ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(AuditFindingState {
                fingerprint: r.get(0)?,
                status: r.get(1)?,
                note: r.get(2)?,
                created_at: r.get(3)?,
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 设置/更新处置状态（upsert）。
    pub fn set_audit_finding_state(
        &self,
        fingerprint: &str,
        status: &str,
        note: Option<&str>,
    ) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let now = crate::timestamp::to_millis(Some(ch_domain::now_utc())).unwrap_or(0);
        conn.execute(
            "INSERT INTO audit_finding_states (fingerprint, status, note, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(fingerprint) DO UPDATE SET status = ?2, note = ?3",
            params![fingerprint, status, note, now],
        )?;
        Ok(())
    }

    /// 清除处置状态（恢复提示）。
    pub fn clear_audit_finding_state(&self, fingerprint: &str) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.execute(
            "DELETE FROM audit_finding_states WHERE fingerprint = ?1",
            params![fingerprint],
        )?;
        Ok(())
    }

    // ── 治理操作流水（audit_logs 表启用）─────────────────────────────

    /// 记录一条治理操作（重置/删除/GC/策略变更等敏感动作）。
    pub fn log_governance_action(
        &self,
        action: &str,
        target_type: Option<&str>,
        target_id: Option<&str>,
        result: &str,
        metadata_json: Option<&str>,
    ) -> StorageResult<String> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let id = format!("gl_{}", ch_domain::now_utc().unix_timestamp_nanos());
        let now = crate::timestamp::to_millis(Some(ch_domain::now_utc())).unwrap_or(0);
        conn.execute(
            "INSERT INTO audit_logs (id, actor_type, actor_id, action, target_type, target_id, result, metadata_json, created_at)
             VALUES (?1, 'local-user', NULL, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, action, target_type, target_id, result, metadata_json, now],
        )?;
        Ok(id)
    }

    /// 最近 N 条治理操作（倒序）。
    pub fn list_governance_log(&self, limit: i64) -> StorageResult<Vec<GovernanceLogRow>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, action, target_type, target_id, result, metadata_json, created_at
             FROM audit_logs ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| {
            Ok(GovernanceLogRow {
                id: r.get(0)?,
                action: r.get(1)?,
                target_type: r.get(2)?,
                target_id: r.get(3)?,
                result: r.get(4)?,
                metadata_json: r.get(5)?,
                created_at: r.get(6)?,
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }
}
