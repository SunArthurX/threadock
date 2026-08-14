//! ZCode ops 采集：`turn_usage` + `model_usage` → UsageRecord；
//! `tool_usage` → ToolCallRecord（原生含 destructive/approval）。

use crate::{ms_to_ts, open_ro, OpsResult};
use ch_domain::{Provider, ToolCallRecord, UsageRecord, UsageStatus};

use std::path::Path;

/// 采集 ZCode 用量（turn 级）+ 工具调用。
/// 返回 (usage, tool_calls)。库不存在时返回空（静默）。
pub fn collect_zcode(db_path: impl AsRef<Path>) -> OpsResult<(Vec<UsageRecord>, Vec<ToolCallRecord>)> {
    if !db_path.as_ref().exists() {
        return Ok((Vec::new(), Vec::new()));
    }
    let conn = open_ro(db_path)?;

    // 1. turn_usage → UsageRecord（turn 级汇总）
    let mut usage = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT session_id, turn_id, started_at, status, duration_ms, model_retry_count,
                    input_tokens, output_tokens, reasoning_tokens,
                    cache_read_input_tokens, cache_creation_input_tokens
             FROM turn_usage",
        )?;
        let rows = stmt.query_map([], |r| {
            let status: String = r.get(3)?;
            Ok(UsageRecord {
                id: format!("zu_{}_{}", r.get::<_, String>(1)?, r.get::<_, i64>(2)?),
                provider: Provider::ZCode,
                source_session_id: r.get(0)?,
                turn_id: Some(r.get(1)?),
                model: None,
                ts: ms_to_ts(r.get(2)?),
                input_tokens: r.get(6)?,
                output_tokens: r.get(7)?,
                reasoning_tokens: r.get(8)?,
                cache_read_tokens: r.get(9)?,
                cache_write_tokens: r.get(10)?,
                cost_usd: None,
                status: UsageStatus::parse(&status),
                duration_ms: Some(r.get(4)?),
                retry_count: Some(r.get(5)?),
            })
        })?;
        for row in rows {
            usage.push(row?);
        }
    }

    // 2. tool_usage → ToolCallRecord（含原生治理字段）
    let mut tools = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT session_id, tool_name, started_at, read_only, destructive,
                    approval_status, exit_code, duration_ms, status
             FROM tool_usage",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ToolCallRecord {
                id: format!("zt_{}_{}_{}", r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?),
                provider: Provider::ZCode,
                source_session_id: r.get(0)?,
                tool_name: r.get(1)?,
                ts: ms_to_ts(r.get(2)?),
                read_only: r.get::<_, Option<i64>>(3)?.map(|v| v != 0),
                destructive: r.get::<_, Option<i64>>(4)?.map(|v| v != 0),
                approval_status: r.get(5)?,
                exit_code: r.get(6)?,
                duration_ms: r.get(7)?,
                status: UsageStatus::parse(&r.get::<_, String>(8)?),
                command_text: None,
            })
        })?;
        for row in rows {
            tools.push(row?);
        }
    }

    Ok((usage, tools))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_from_missing_db_returns_empty() {
        let (u, t) = collect_zcode("/nonexistent/zcode.db").unwrap();
        assert!(u.is_empty());
        assert!(t.is_empty());
    }

    #[test]
    fn collect_reads_turn_and_tool_usage() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join("zc.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE turn_usage (session_id TEXT, turn_id TEXT, started_at INTEGER,
                status TEXT, duration_ms INTEGER, model_retry_count INTEGER,
                input_tokens INTEGER, output_tokens INTEGER, reasoning_tokens INTEGER,
                cache_read_input_tokens INTEGER, cache_creation_input_tokens INTEGER);
             CREATE TABLE tool_usage (session_id TEXT, tool_name TEXT, started_at INTEGER,
                read_only INTEGER, destructive INTEGER, approval_status TEXT,
                exit_code INTEGER, duration_ms INTEGER, status TEXT);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO turn_usage VALUES ('s1','t1',1000000,'completed',5000,0,100,50,10,200,0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tool_usage VALUES ('s1','Bash',1000000,0,1,'none',0,300,'completed')",
            [],
        )
        .unwrap();
        drop(conn);

        let (usage, tools) = collect_zcode(&db).unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].input_tokens, 100);
        assert_eq!(usage[0].cache_read_tokens, 200);
        assert_eq!(usage[0].billable_tokens(), 160);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool_name, "Bash");
        assert_eq!(tools[0].destructive, Some(true));
    }
}
