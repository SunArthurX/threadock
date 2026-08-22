//! LLM 提取运行记录（V15）：成功与失败均留痕。
//! 「知识」弹窗的 AI 记录 tab 展示历史与失败原因；重复提取确认依据最近一次成功。

use super::Repository;
use crate::error::StorageResult;
use crate::timestamp;
use ch_domain::now_utc;
use rusqlite::params;

/// 一次 AI 提取运行（成功或失败）。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LlmRunRecord {
    pub id: String,
    pub conversation_id: String,
    /// `success` | `failed`
    pub status: String,
    /// 失败原因（成功为 None）
    pub error: Option<String>,
    /// 引擎标签（`llm:glm-5.3@prompt-v2`）
    pub extractor: String,
    pub input_messages: i64,
    pub input_chars: i64,
    /// 提取条目总数（六类之和；失败为 0）
    pub items_total: i64,
    pub duration_ms: i64,
    pub created_at_ms: i64,
}

impl Repository {
    /// 记录一次 AI 提取运行（成功/失败都记）。
    pub fn record_llm_run(&self, rec: &LlmRunRecord) -> StorageResult<String> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let now_ms = timestamp::to_millis(Some(now_utc())).expect("timestamp conversion failed");
        conn.execute(
            "INSERT INTO llm_extract_runs
                (id, conversation_id, status, error, extractor, input_messages,
                 input_chars, items_total, duration_ms, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                rec.id,
                rec.conversation_id,
                rec.status,
                rec.error,
                rec.extractor,
                rec.input_messages,
                rec.input_chars,
                rec.items_total,
                rec.duration_ms,
                now_ms,
            ],
        )?;
        Ok(rec.id.clone())
    }

    /// 某会话的 AI 提取历史（时间倒序，含成功与失败）。
    pub fn list_llm_runs(
        &self,
        conversation_id: &str,
        limit: i64,
    ) -> StorageResult<Vec<LlmRunRecord>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, status, error, extractor, input_messages,
                    input_chars, items_total, duration_ms, created_at
             FROM llm_extract_runs
             WHERE conversation_id = ?1
             ORDER BY created_at DESC, rowid DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![conversation_id, limit], |r| {
            Ok(LlmRunRecord {
                id: r.get(0)?,
                conversation_id: r.get(1)?,
                status: r.get(2)?,
                error: r.get(3)?,
                extractor: r.get(4)?,
                input_messages: r.get(5)?,
                input_chars: r.get(6)?,
                items_total: r.get(7)?,
                duration_ms: r.get(8)?,
                created_at_ms: r.get(9)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Repository;

    fn repo() -> Repository {
        let dir = tempfile::TempDir::new().expect("tempdir");
        Repository::open(dir.path().join("t.db")).expect("open")
    }

    fn run(status: &str, error: Option<&str>, conv: &str) -> LlmRunRecord {
        LlmRunRecord {
            id: ch_domain::new_id("lrun"),
            conversation_id: conv.into(),
            status: status.into(),
            error: error.map(String::from),
            extractor: "llm:glm-5.3@prompt-v2".into(),
            input_messages: 12,
            input_chars: 24_000,
            items_total: if status == "success" { 30 } else { 0 },
            duration_ms: 45_000,
            created_at_ms: 0,
        }
    }

    #[test]
    fn records_success_and_failure_in_order() {
        let r = repo();
        let conv = setup_conv(&r);
        r.record_llm_run(&run("failed", Some("网络请求失败：超时"), &conv))
            .expect("record 1");
        r.record_llm_run(&run("success", None, &conv))
            .expect("record 2");

        let history = r.list_llm_runs(&conv, 10).expect("list");
        assert_eq!(history.len(), 2);
        // 时间倒序：最近在前
        assert_eq!(history[0].status, "success");
        assert!(history[0].error.is_none());
        assert_eq!(history[0].items_total, 30);
        assert_eq!(history[1].status, "failed");
        assert_eq!(history[1].error.as_deref(), Some("网络请求失败：超时"));
    }

    #[test]
    fn respects_limit_and_conversation_scope() {
        let r = repo();
        let conv_a = setup_conv(&r);
        let conv_b = setup_conv(&r);
        for _ in 0..3 {
            r.record_llm_run(&run("success", None, &conv_a))
                .expect("record");
        }
        r.record_llm_run(&run("failed", Some("x"), &conv_b))
            .expect("record b");
        assert_eq!(
            r.list_llm_runs(&conv_a, 2).expect("a").len(),
            2,
            "limit 生效"
        );
        assert_eq!(
            r.list_llm_runs(&conv_b, 10).expect("b").len(),
            1,
            "会话隔离"
        );
    }

    /// 建一个最小会话（外键约束需要）。
    fn setup_conv(r: &Repository) -> String {
        r.upsert_provider(ch_domain::Provider::ZCode)
            .expect("provider");
        let id = ch_domain::new_id("conv");
        r.conn
            .lock()
            .expect("mutex poisoned")
            .execute(
                "INSERT INTO conversations (id, provider_id, source_conversation_id, title, status, started_at, updated_at)
                 VALUES (?1, (SELECT id FROM providers LIMIT 1), ?1, 't', 'completed', 0, 0)",
                params![id],
            )
            .expect("insert conv");
        id
    }
}
