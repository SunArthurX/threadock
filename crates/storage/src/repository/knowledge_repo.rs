//! 知识提取持久化：保存/读取/版本列表（从 repository.rs 拆出）。

use super::Repository;
use crate::error::StorageResult;
use crate::timestamp;
use ch_domain::now_utc;
use rusqlite::{params, OptionalExtension};

/// 知识提取记录（从库读回的行）。
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeRecord {
    pub id: String,
    pub conversation_id: String,
    pub version: i64,
    pub extractor: String,
    /// `ExtractionResult` 的 JSON 字符串。
    pub result_json: String,
    pub created_at: ch_domain::Timestamp,
    pub updated_at: ch_domain::Timestamp,
}

impl Repository {
    /// 保存一条知识提取结果（plan §13.5「人工编辑后保留版本」）。
    ///
    /// 把该 conversation 的旧版本标记 `is_current=0，新版本作为` current。
    /// `result_json` 是 `ExtractionResult` 的序列化字符串。
    pub fn save_knowledge(
        &self,
        conversation_id: &str,
        extractor: &str,
        result_json: &str,
    ) -> StorageResult<String> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let now_ms = timestamp::to_millis(Some(now_utc())).expect("timestamp conversion failed");
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
        let conn = self.conn.lock().expect("mutex poisoned");
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
    pub fn list_knowledge_versions(
        &self,
        conversation_id: &str,
    ) -> StorageResult<Vec<KnowledgeRecord>> {
        let conn = self.conn.lock().expect("mutex poisoned");
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
}
