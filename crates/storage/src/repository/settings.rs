//! 应用设置与用户自定义脱敏规则（从 repository.rs 拆出）。

use super::Repository;
use crate::error::StorageResult;
use crate::timestamp;
use ch_domain::now_utc;

/// 私有笔记 DTO（前后端契约）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct NoteDto {
    pub note: String,
    pub updated_at: i64,
}

/// 标签频次 DTO（前后端契约）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct TagCountDto {
    pub tag: String,
    pub count: i64,
}
use rusqlite::{params, OptionalExtension};

/// 脱敏规则记录。
#[derive(Debug, Clone, PartialEq)]
pub struct RedactionRuleRecord {
    pub id: String,
    pub name: String,
    pub pattern: String,
    pub enabled: bool,
}

/// 保存搜索记录（plan §13.2，前后端契约）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct SavedSearchRecord {
    pub id: String,
    pub name: String,
    pub query_text: String,
    pub created_at: i64,
    pub updated_at: i64,
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

    /// 批量回写脱敏规则（重置后恢复用户自定义规则，保留 enabled 状态）。
    pub fn restore_redaction_rules(&self, rules: &[RedactionRuleRecord]) -> StorageResult<()> {
        if rules.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().expect("mutex poisoned");
        let now_ms = timestamp::to_millis(Some(now_utc())).unwrap_or(0);
        for r in rules {
            conn.execute(
                "INSERT INTO redaction_rules (id, name, pattern, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                 ON CONFLICT(name) DO UPDATE SET pattern = ?3, enabled = ?4, updated_at = ?5",
                params![r.id, r.name, r.pattern, i64::from(r.enabled), now_ms],
            )?;
        }
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

    // ── 保存搜索条件（plan §13.2，V14）────────────────────────────

    /// 新增/覆盖一条保存搜索（按 name 幂等）。返回记录 id。
    pub fn upsert_saved_search(&self, name: &str, query_text: &str) -> StorageResult<String> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let id = ch_domain::new_id("ssearch");
        let now = crate::timestamp::to_millis(Some(ch_domain::now_utc()))
            .expect("timestamp conversion failed");
        conn.execute(
            "INSERT INTO saved_searches (id, name, query_text, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(name) DO UPDATE SET
                query_text = ?3, updated_at = ?4",
            params![id, name, query_text, now],
        )?;
        let actual: String = conn.query_row(
            "SELECT id FROM saved_searches WHERE name = ?1",
            params![name],
            |r| r.get(0),
        )?;
        Ok(actual)
    }

    /// 列出全部保存搜索（按创建时间倒序）。
    pub fn list_saved_searches(&self) -> StorageResult<Vec<SavedSearchRecord>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn
            .prepare("SELECT id, name, query_text, created_at, updated_at FROM saved_searches ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok(SavedSearchRecord {
                id: r.get(0)?,
                name: r.get(1)?,
                query_text: r.get(2)?,
                created_at: r.get(3)?,
                updated_at: r.get(4)?,
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 按 id 删除保存搜索。
    pub fn delete_saved_search(&self, id: &str) -> StorageResult<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.execute("DELETE FROM saved_searches WHERE id = ?1", params![id])?;
        Ok(())
    }
}
