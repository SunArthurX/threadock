//! 应用设置与用户自定义脱敏规则（从 repository.rs 拆出）。

use super::Repository;
use crate::error::StorageResult;
use crate::timestamp;
use ch_domain::now_utc;
use rusqlite::{params, OptionalExtension};

/// 脱敏规则记录。
#[derive(Debug, Clone, PartialEq)]
pub struct RedactionRuleRecord {
    pub id: String,
    pub name: String,
    pub pattern: String,
    pub enabled: bool,
}

impl Repository {
    /// 读取通用设置（不存在返回 None）。
    pub fn get_setting(&self, key: &str) -> StorageResult<Option<String>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    /// 写入通用设置（upsert）。
    pub fn set_setting(&self, key: &str, value: &str) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }

    /// 添加自定义脱敏规则（幂等：按 name upsert）。
    pub fn add_redaction_rule(&self, name: &str, pattern: &str) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let now_ms = timestamp::to_millis(Some(now_utc())).expect("timestamp conversion failed");
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
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt =
            conn.prepare("SELECT id, name, pattern, enabled FROM redaction_rules ORDER BY name")?;
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
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.execute("DELETE FROM redaction_rules WHERE name = ?1", params![name])?;
        Ok(())
    }
}
