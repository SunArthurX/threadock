//! MiniMax ops 采集：`local_runtime_token_usage` → UsageRecord（含 cost_usd）。

use crate::{ms_to_ts, open_ro, OpsResult};
use ch_domain::{Provider, UsageRecord, UsageStatus};
use std::path::Path;

pub fn collect_minimax(db_path: impl AsRef<Path>) -> OpsResult<Vec<UsageRecord>> {
    if !db_path.as_ref().exists() {
        return Ok(Vec::new());
    }
    let conn = open_ro(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT u.session_id, u.turn_id, u.model, u.ts, u.input_tokens, u.output_tokens,
                u.reasoning_tokens, u.cache_read_tokens, u.cache_write_tokens, u.cost_usd,
                json_extract(s.record_json, '$.workspaceDir')
         FROM local_runtime_token_usage u
         LEFT JOIN local_runtime_sessions s ON s.session_id = u.session_id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(UsageRecord {
            id: format!("mu_{}_{}", r.get::<_, String>(0)?, r.get::<_, i64>(3)?),
            provider: Provider::MinimaxCode,
            source_session_id: r.get(0)?,
            turn_id: r.get(1)?,
            model: r.get(2)?,
            ts: ms_to_ts(r.get(3)?),
            input_tokens: r.get(4)?,
            output_tokens: r.get(5)?,
            reasoning_tokens: r.get(6)?,
            cache_read_tokens: r.get(7)?,
            cache_write_tokens: r.get(8)?,
            cost_usd: r.get(9)?,
            status: UsageStatus::Completed,
            duration_ms: None,
            retry_count: None,
            source_dir: r.get(10)?,
            context_exceeded: 0,
        })
    })?;
    let mut v = Vec::new();
    for row in rows {
        v.push(row?);
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_reads_token_usage() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join("mm.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE local_runtime_token_usage (
                session_id TEXT, turn_id TEXT, model TEXT, ts INTEGER,
                input_tokens INTEGER, output_tokens INTEGER, reasoning_tokens INTEGER,
                cache_read_tokens INTEGER, cache_write_tokens INTEGER, cost_usd REAL);
             CREATE TABLE local_runtime_sessions (session_id TEXT PRIMARY KEY, record_json TEXT, updated_at_ms INTEGER);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO local_runtime_token_usage VALUES ('mvs_1','turn_1','MiniMax-M3',1784560908997,16705,370,0,242,0,0.02)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO local_runtime_sessions VALUES ('mvs_1','{\"workspaceDir\":\"/tmp/mmproj\"}',0)",
            [],
        )
        .unwrap();
        drop(conn);

        let usage = collect_minimax(&db).unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].input_tokens, 16705);
        assert_eq!(usage[0].cost_usd, Some(0.02));
        assert_eq!(usage[0].source_dir.as_deref(), Some("/tmp/mmproj"));
        assert_eq!(usage[0].model.as_deref(), Some("MiniMax-M3"));
    }

    #[test]
    fn missing_db_empty() {
        assert!(collect_minimax("/nonexistent/mm.db").unwrap().is_empty());
    }
}
