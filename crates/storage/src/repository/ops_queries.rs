//! CodeAgentOps 指标域：用量/工具调用写入、聚合查询、资产与自动化记录（从 repository.rs 拆出）。

use super::{row_to_conversation, Repository};
use crate::error::StorageResult;
use crate::timestamp;
use ch_domain::now_utc;
use rusqlite::{params, params_from_iter, types::Value as SqlValue};
use std::str::FromStr as _;

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
    pub cost_usd: f64,
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
    pub cost_usd: f64,
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

/// 资产行（M6）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct AssetRow {
    pub provider: String,
    pub kind: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub risky_hits: i64,
    pub installed_at: Option<String>,
    pub path: Option<String>,
}

/// 自动化行（M8）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct AutomationRow {
    pub provider: String,
    pub name: String,
    pub kind: String,
    pub schedule: Option<String>,
    pub status: Option<String>,
    pub detail: Option<String>,
}

/// 按目录成本（M7）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct DirCost {
    pub dir: String,
    pub tokens: i64,
    pub cost_usd: f64,
    pub requests: i64,
}

/// 缓存统计（M7）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct CacheStat {
    pub provider: String,
    pub input_tokens: i64,
    pub cache_read_tokens: i64,
    pub hit_rate: f64,
}

/// 异常行（M9）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct AnomalyRow {
    pub kind: String,
    pub agent: String,
    pub detail: String,
    pub severity: String,
    /// 会话级异常（重试风暴等）携带定位：provider 名 + 来源会话 ID（可跳转）。
    pub provider: Option<String>,
    pub source_session_id: Option<String>,
}

/// Agent 健康度（M10）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentHealth {
    pub provider: String,
    pub total_requests: i64,
    pub errors: i64,
    pub completed: i64,
    pub retries: i64,
    pub sessions: i64,
    pub success_rate: f64,
    pub error_rate: f64,
    pub retry_rate: f64,
    pub stability_score: f64,
}

/// 延迟统计（M11）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LatencyStat {
    pub provider: String,
    pub sample_count: i64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub avg_ms: f64,
}

/// Token 浪费（M12）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct TokenWaste {
    pub provider: String,
    pub session_id: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub ratio: f64,
    pub requests: i64,
    pub cache_read: i64,
    pub waste_score: f64,
}

/// Agent 横向对比基准（M13）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentBenchmark {
    pub provider: String,
    pub total_requests: i64,
    pub total_tokens: i64,
    pub cost_usd: f64,
    pub sessions: i64,
    pub success_rate: f64,
    pub cache_hit_rate: f64,
    pub avg_duration_ms: f64,
    pub cost_per_session: f64,
    pub tokens_per_session: i64,
}

/// 周报汇总（M14）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct WeeklySummary {
    pub overview: OpsOverview,
    pub health: Vec<AgentHealth>,
    pub benchmark: Vec<AgentBenchmark>,
    pub waste_sessions: i64,
}

/// 用量汇总（本月 / 本年 / 全部）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct UsageSummary {
    pub month_tokens: i64,
    pub month_cost: f64,
    pub year_tokens: i64,
    pub year_cost: f64,
    pub all_tokens: i64,
    pub all_cost: f64,
}

/// 月度用量 + 按日均外推的月底预测（预算告警用）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct MonthProjection {
    pub tokens_so_far: i64,
    pub cost_so_far: f64,
    pub days_elapsed: u32,
    pub days_in_month: u32,
    /// 按日均线性外推的月底预测。
    pub projected_tokens: i64,
    pub projected_cost: f64,
}

/// 每日缓存命中趋势行（cache_read / 总输入口径）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct CacheTrendRow {
    pub day: String,
    pub total_input: i64,
    pub cache_read: i64,
}

/// 活动节律：单日热力单元。
#[derive(Debug, Clone, serde::Serialize)]
pub struct HeatCell {
    pub day: String,
    pub calls: i64,
    pub sessions: i64,
}

/// 活动节律：单小时用量。
#[derive(Debug, Clone, serde::Serialize)]
pub struct HourBucket {
    pub hour: i64,
    pub calls: i64,
}

/// 活动节律：工具月度趋势（按月 + 工具名）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolTrend {
    pub month: String,
    pub tool: String,
    pub calls: i64,
}

/// 活动节律：日级工具分布（按天 + 工具名，限最近 90 天）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct DailyTool {
    pub day: String,
    pub tool: String,
    pub calls: i64,
}

/// 活动节律聚合六件套（heatmap / hourly 全量 / 工作日 / 周末 / 月度工具趋势 / 日级工具）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ActivityStats {
    pub heatmap: Vec<HeatCell>,
    pub hourly: Vec<HourBucket>,
    pub hourly_weekday: Vec<HourBucket>,
    pub hourly_weekend: Vec<HourBucket>,
    pub tools_trend: Vec<ToolTrend>,
    pub tool_daily: Vec<DailyTool>,
}

impl Repository {
    /// 批量写入用量记录（事务 + 幂等：UNIQUE 键冲突跳过）。
    pub fn upsert_usage_batch(&self, records: &[ch_domain::UsageRecord]) -> StorageResult<usize> {
        let mut conn = self.conn.lock().expect("mutex poisoned");
        let mut n = 0;
        // 分块事务：每 2000 条一个事务，避免长持主锁阻塞 UI 查询
        for records in records.chunks(2000) {
            let tx = conn.transaction()?;
            for r in records {
                let changed = tx.execute(
                "INSERT OR IGNORE INTO usage_records
                    (id, provider_id, source_session_id, turn_id, model, ts,
                     input_tokens, output_tokens, reasoning_tokens, cache_read_tokens,
                     cache_write_tokens, cost_usd, status, duration_ms, retry_count,
                     source_dir, context_exceeded)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
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
                    r.source_dir,
                    r.context_exceeded,
                ],
            )?;
                n += changed;
            }
            tx.commit()?;
        }
        Ok(n)
    }

    /// 批量写入工具调用记录（分块事务 + 幂等）。
    pub fn upsert_tool_call_batch(
        &self,
        records: &[ch_domain::ToolCallRecord],
    ) -> StorageResult<usize> {
        let mut conn = self.conn.lock().expect("mutex poisoned");
        let mut n = 0;
        for records in records.chunks(2000) {
            let tx = conn.transaction()?;
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
        }
        Ok(n)
    }

    /// 时间范围过滤子句：days=None 全量，否则最近 N 天。
    fn range_clause(days: Option<i64>) -> (String, Option<i64>) {
        match days {
            Some(d) => {
                let cutoff =
                    timestamp::to_millis(Some(ch_domain::now_utc())).unwrap_or(0) - d * 86_400_000;
                ("ts >= ?1".to_string(), Some(cutoff))
            }
            None => ("1=1".to_string(), None),
        }
    }

    /// 治理总览 KPI（单参数：全部子查询共用 ?1，cutoff=0 即全量）。
    pub fn ops_overview(&self, days: Option<i64>) -> StorageResult<OpsOverview> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let cutoff = match days {
            Some(d) => timestamp::to_millis(Some(now_utc())).unwrap_or(0) - d * 86_400_000,
            None => 0,
        };
        conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM usage_records WHERE ts >= ?1),
                COALESCE((SELECT SUM(input_tokens + output_tokens + reasoning_tokens) FROM usage_records WHERE ts >= ?1), 0),
                COALESCE((SELECT SUM(input_tokens) FROM usage_records WHERE ts >= ?1), 0),
                COALESCE((SELECT SUM(output_tokens) FROM usage_records WHERE ts >= ?1), 0),
                COALESCE((SELECT SUM(cost_usd) FROM usage_records WHERE cost_usd IS NOT NULL AND ts >= ?1), 0),
                COALESCE((SELECT AVG(duration_ms) FROM usage_records WHERE duration_ms IS NOT NULL AND ts >= ?1), 0),
                (SELECT COUNT(*) FROM usage_records WHERE status = 'error' AND ts >= ?1),
                (SELECT COUNT(DISTINCT source_session_id) FROM usage_records WHERE ts >= ?1),
                COALESCE((SELECT COUNT(*) FROM tool_call_records WHERE destructive = 1 AND ts >= ?1), 0),
                (SELECT COUNT(*) FROM tool_call_records WHERE ts >= ?1)",
            params![cutoff],
            |r| {
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
            },
        )
        .map_err(Into::into)
    }

    /// 按 provider 聚合。
    pub fn ops_by_provider(&self, days: Option<i64>) -> StorageResult<Vec<ProviderUsage>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT p.name, COUNT(*),
                    SUM(input_tokens + output_tokens + reasoning_tokens),
                    SUM(output_tokens),
                    SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END),
                    COALESCE(SUM(cost_usd), 0.0)
             FROM usage_records u JOIN providers p ON p.id = u.provider_id
             WHERE {clause}
             GROUP BY u.provider_id ORDER BY 3 DESC",
        );
        let args: Vec<SqlValue> = cutoff.map(std::convert::Into::into).into_iter().collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            Ok(ProviderUsage {
                provider: r.get(0)?,
                requests: r.get(1)?,
                total_tokens: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                output_tokens: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                errors: r.get::<_, Option<i64>>(4)?.unwrap_or(0),
                cost_usd: r.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
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
        let conn = self.conn.lock().expect("mutex poisoned");
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT COALESCE(model, '(unknown)'), u.provider_id, COUNT(*),
                    SUM(input_tokens), SUM(output_tokens),
                    SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END),
                    COALESCE(SUM(cost_usd), 0.0)
             FROM usage_records u
             WHERE {clause}
             GROUP BY model, u.provider_id ORDER BY 4 DESC",
        );
        let args: Vec<SqlValue> = cutoff.map(std::convert::Into::into).into_iter().collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            Ok(ModelUsage {
                model: r.get(0)?,
                provider_id: r.get(1)?,
                requests: r.get(2)?,
                input_tokens: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                output_tokens: r.get::<_, Option<i64>>(4)?.unwrap_or(0),
                errors: r.get::<_, Option<i64>>(5)?.unwrap_or(0),
                cost_usd: r.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
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
        let conn = self.conn.lock().expect("mutex poisoned");
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT date(ts/1000, 'unixepoch', 'localtime') AS day,
                    SUM(input_tokens + output_tokens + reasoning_tokens),
                    COUNT(*)
             FROM usage_records WHERE {clause}
             GROUP BY day ORDER BY day",
        );
        let args: Vec<SqlValue> = cutoff.map(std::convert::Into::into).into_iter().collect();
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
        let conn = self.conn.lock().expect("mutex poisoned");
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT tool_name, COUNT(*),
                    SUM(CASE WHEN destructive = 1 THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END),
                    COALESCE(AVG(duration_ms), 0)
             FROM tool_call_records WHERE {clause}
             GROUP BY tool_name ORDER BY 2 DESC LIMIT ?",
        );
        let mut args: Vec<SqlValue> = Vec::new();
        if let Some(c) = cutoff {
            args.push(c.into());
        }
        args.push(n.into());
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
    pub fn ops_risky_calls(
        &self,
        days: Option<i64>,
        n: i64,
    ) -> StorageResult<Vec<ch_domain::ToolCallRecord>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT t.id, p.name, source_session_id, tool_name, ts, read_only,
                    destructive, approval_status, exit_code, duration_ms, status, command_text
             FROM tool_call_records t JOIN providers p ON p.id = t.provider_id
             WHERE (destructive = 1 OR status = 'error' OR (exit_code IS NOT NULL AND exit_code != 0))
               AND {clause}
             ORDER BY ts DESC LIMIT ?",
        );
        let mut args: Vec<SqlValue> = Vec::new();
        if let Some(c) = cutoff {
            args.push(c.into());
        }
        args.push(n.into());
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
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

    /// 本月（自 cutoff 毫秒起）用量：返回 (tokens, `cost_usd`)。
    pub fn ops_month_usage_since(&self, cutoff_ms: i64) -> StorageResult<(i64, f64)> {
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.query_row(
            "SELECT COALESCE(SUM(input_tokens + output_tokens + reasoning_tokens), 0),
                    COALESCE(SUM(cost_usd), 0.0)
             FROM usage_records WHERE ts >= ?1",
            params![cutoff_ms],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(Into::into)
    }

    /// 模型 → (`input_tokens`, `output_tokens`) 汇总（成本重算用）。
    pub fn ops_model_token_totals(&self) -> StorageResult<Vec<(String, String, i64, i64)>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT COALESCE(model, '(unknown)') AS m, u.provider_id,
                    SUM(input_tokens), SUM(output_tokens)
             FROM usage_records u GROUP BY m, u.provider_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                r.get::<_, Option<i64>>(3)?.unwrap_or(0),
            ))
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 整源替换用量记录：同一 provider 的口径切换（如 zcode turn→model 级）时，
    /// 先删后插防止两种口径叠加导致总量翻倍。单事务分块执行。
    pub fn replace_provider_usage(
        &self,
        provider_id: &str,
        records: &[ch_domain::UsageRecord],
    ) -> StorageResult<usize> {
        let mut conn = self.conn.lock().expect("mutex poisoned");
        let mut n = 0;
        let mut first = true;
        for chunk in records.chunks(2000) {
            let tx = conn.transaction()?;
            if first {
                tx.execute(
                    "DELETE FROM usage_records WHERE provider_id = ?1",
                    params![provider_id],
                )?;
                first = false;
            }
            for r in chunk {
                n += tx.execute(
                    "INSERT OR IGNORE INTO usage_records
                        (id, provider_id, source_session_id, turn_id, model, ts,
                         input_tokens, output_tokens, reasoning_tokens, cache_read_tokens,
                         cache_write_tokens, cost_usd, status, duration_ms, retry_count,
                         source_dir, context_exceeded)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                    params![
                        r.id,
                        provider_id,
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
                        r.source_dir,
                        r.context_exceeded,
                    ],
                )?;
            }
            tx.commit()?;
        }
        Ok(n)
    }

    /// 按单价更新该模型每行的 `cost_usd（行内` tokens × 单价，行成本可加总）。
    pub fn update_model_pricing(
        &self,
        model: &str,
        provider_id: &str,
        input_per_mtok: f64,
        output_per_mtok: f64,
    ) -> StorageResult<usize> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let n = conn.execute(
            "UPDATE usage_records SET cost_usd =
                (input_tokens * ?1 + output_tokens * ?2) / 1e6
             WHERE COALESCE(model, '(unknown)') = ?3 AND provider_id = ?4",
            params![input_per_mtok, output_per_mtok, model, provider_id],
        )?;
        Ok(n)
    }

    /// 整源替换资产清单（先删后插，分块事务）。
    pub fn replace_provider_assets(
        &self,
        provider_id: &str,
        records: &[ch_domain::AssetRecord],
    ) -> StorageResult<usize> {
        let mut conn = self.conn.lock().expect("mutex poisoned");
        let mut first = true;
        let mut n = 0;
        for chunk in records.chunks(2000) {
            let tx = conn.transaction()?;
            if first {
                tx.execute(
                    "DELETE FROM asset_records WHERE provider_id = ?1",
                    params![provider_id],
                )?;
                first = false;
            }
            for r in chunk {
                n += tx.execute(
                    "INSERT OR IGNORE INTO asset_records
                        (id, provider_id, kind, name, version, description, risky_hits, installed_at, path)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        r.id,
                        provider_id,
                        r.kind,
                        r.name,
                        r.version,
                        r.description,
                        r.risky_hits,
                        r.installed_at,
                        r.path,
                    ],
                )?;
            }
            tx.commit()?;
        }
        Ok(n)
    }

    /// 列出全部资产（带 provider 名）。
    pub fn list_assets(&self) -> StorageResult<Vec<AssetRow>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT p.name, a.kind, a.name, a.version, a.description, a.risky_hits, a.installed_at, a.path
             FROM asset_records a JOIN providers p ON p.id = a.provider_id
             ORDER BY p.name, a.kind, a.name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(AssetRow {
                provider: r.get(0)?,
                kind: r.get(1)?,
                name: r.get(2)?,
                version: r.get(3)?,
                description: r.get(4)?,
                risky_hits: r.get(5)?,
                installed_at: r.get(6)?,
                path: r.get(7)?,
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 整源替换自动化任务。
    pub fn replace_provider_automations(
        &self,
        provider_id: &str,
        records: &[ch_domain::AutomationRecord],
    ) -> StorageResult<usize> {
        let mut conn = self.conn.lock().expect("mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM automation_records WHERE provider_id = ?1",
            params![provider_id],
        )?;
        let mut n = 0;
        for r in records {
            n += tx.execute(
                "INSERT OR IGNORE INTO automation_records
                    (id, provider_id, name, kind, schedule, status, detail)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    r.id,
                    provider_id,
                    r.name,
                    r.kind,
                    r.schedule,
                    r.status,
                    r.detail
                ],
            )?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// 列出全部自动化任务。
    pub fn list_automations(&self) -> StorageResult<Vec<AutomationRow>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT p.name, a.name, a.kind, a.schedule, a.status, a.detail
             FROM automation_records a JOIN providers p ON p.id = a.provider_id
             ORDER BY p.name, a.name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(AutomationRow {
                provider: r.get(0)?,
                name: r.get(1)?,
                kind: r.get(2)?,
                schedule: r.get(3)?,
                status: r.get(4)?,
                detail: r.get(5)?,
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// M7：按来源目录归因成本/用量（Top N）。
    pub fn ops_cost_by_dir(&self, days: Option<i64>, n: i64) -> StorageResult<Vec<DirCost>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT COALESCE(source_dir, '(未记录目录)') AS d,
                    SUM(input_tokens + output_tokens + reasoning_tokens),
                    SUM(COALESCE(cost_usd, 0)),
                    COUNT(*)
             FROM usage_records WHERE {clause}
             GROUP BY d ORDER BY 2 DESC LIMIT ?",
        );
        let mut args: Vec<SqlValue> = Vec::new();
        if let Some(c) = cutoff {
            args.push(c.into());
        }
        args.push(n.into());
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            Ok(DirCost {
                dir: r.get(0)?,
                tokens: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                cost_usd: r.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                requests: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// `M7：缓存命中率（cache_read` / (input + `cache_read)）按` provider。
    pub fn ops_cache_stats(&self, days: Option<i64>) -> StorageResult<Vec<CacheStat>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT p.name,
                    SUM(u.input_tokens),
                    SUM(u.cache_read_tokens)
             FROM usage_records u JOIN providers p ON p.id = u.provider_id
             WHERE {clause}
             GROUP BY p.name ORDER BY 2 DESC",
        );
        let args: Vec<SqlValue> = cutoff.map(std::convert::Into::into).into_iter().collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            let input: i64 = r.get::<_, Option<i64>>(1)?.unwrap_or(0);
            let cached: i64 = r.get::<_, Option<i64>>(2)?.unwrap_or(0);
            let total = input + cached;
            Ok(CacheStat {
                provider: r.get(0)?,
                input_tokens: input,
                cache_read_tokens: cached,
                hit_rate: if total > 0 {
                    cached as f64 / total as f64
                } else {
                    0.0
                },
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// M9：异常检测（错误尖峰 / 重试风暴 / context 超限），全部基于已有数据。
    pub fn ops_anomalies(&self, days: Option<i64>) -> StorageResult<Vec<AnomalyRow>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let (clause, cutoff) = Self::range_clause(days);
        let mut out = Vec::new();

        // 1) 错误尖峰：日错误数 > 3× 平均
        let sql = format!(
            "SELECT date(ts/1000,'unixepoch','localtime') AS d, COUNT(*)
             FROM usage_records WHERE status = 'error' AND {clause}
             GROUP BY d ORDER BY d",
        );
        let args: Vec<SqlValue> = cutoff.map(std::convert::Into::into).into_iter().collect();
        let daily: Vec<(String, i64)> = {
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
            let mut v = Vec::new();
            for r in rows {
                v.push(r?);
            }
            v
        };
        if !daily.is_empty() {
            let avg: f64 = daily.iter().map(|(_, c)| *c as f64).sum::<f64>() / daily.len() as f64;
            for (day, cnt) in daily {
                if avg > 0.0 && cnt as f64 > avg * 3.0 && cnt >= 3 {
                    out.push(AnomalyRow {
                        provider: None,
                        source_session_id: None,
                        kind: "error_spike".into(),
                        agent: "*".into(),
                        detail: format!(
                            "{day} 错误 {cnt} 次（均值 {avg:.1} 的 {:.1} 倍）",
                            cnt as f64 / avg
                        ),
                        severity: "high".into(),
                    });
                }
            }
        }

        // 2) 重试风暴：session 级 retry 总和 Top（≥5 次即风暴）
        let sql = format!(
            "SELECT source_session_id, SUM(retry_count) AS rc, COUNT(*) AS n
             FROM usage_records WHERE retry_count IS NOT NULL AND retry_count > 0 AND {clause}
             GROUP BY source_session_id HAVING rc >= 5 ORDER BY rc DESC LIMIT 5",
        );
        {
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    r.get::<_, i64>(2)?,
                ))
            })?;
            for r in rows {
                let (sid, rc, n) = r?;
                out.push(AnomalyRow {
                    // 会话级异常：带定位（前端可跳转对应会话）
                    provider: None,
                    source_session_id: Some(sid),
                    kind: "retry_storm".into(),
                    agent: "*".into(),
                    detail: format!("共重试 {rc} 次 / {n} 请求"),
                    severity: if rc >= 20 {
                        "high".into()
                    } else {
                        "medium".into()
                    },
                });
            }
        }

        // 3) context 超限：按 provider 汇总（ZCode 原生字段）
        let sql = format!(
            "SELECT p.name, SUM(context_exceeded), COUNT(*)
             FROM usage_records u JOIN providers p ON p.id = u.provider_id
             WHERE context_exceeded > 0 AND {clause}
             GROUP BY p.name ORDER BY 2 DESC",
        );
        {
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    r.get::<_, i64>(2)?,
                ))
            })?;
            for r in rows {
                let (name, cx, n) = r?;
                if cx > 0 {
                    out.push(AnomalyRow {
                        provider: None,
                        source_session_id: None,
                        kind: "context_exceeded".into(),
                        agent: name,
                        detail: format!("{cx} 次 context 超限 / {n} 请求"),
                        severity: "medium".into(),
                    });
                }
            }
        }
        Ok(out)
    }

    /// M10：Agent 健康度（成功率/错误率/重试率/稳定性评分 0-100）。
    pub fn ops_agent_health(&self, days: Option<i64>) -> StorageResult<Vec<AgentHealth>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT p.name, COUNT(*),
                    SUM(CASE WHEN u.status = 'error' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN u.status = 'completed' THEN 1 ELSE 0 END),
                    COALESCE(SUM(u.retry_count), 0),
                    COUNT(DISTINCT u.source_session_id)
             FROM usage_records u JOIN providers p ON p.id = u.provider_id
             WHERE {clause}
             GROUP BY p.name",
        );
        let args: Vec<SqlValue> = cutoff.map(std::convert::Into::into).into_iter().collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            let total: i64 = r.get(1)?;
            let errors: i64 = r.get::<_, Option<i64>>(2)?.unwrap_or(0);
            let completed: i64 = r.get::<_, Option<i64>>(3)?.unwrap_or(0);
            let retries: i64 = r.get::<_, Option<i64>>(4)?.unwrap_or(0);
            let sessions: i64 = r.get(5)?;
            let success_rate = if total > 0 {
                completed as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            let error_rate = if total > 0 {
                errors as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            let retry_rate = if total > 0 {
                retries as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            let stability =
                (success_rate * 0.6 - retry_rate * 0.3 - error_rate * 0.1).clamp(0.0, 100.0);
            Ok(AgentHealth {
                provider: r.get(0)?,
                total_requests: total,
                errors,
                completed,
                retries,
                sessions,
                success_rate,
                error_rate,
                retry_rate,
                stability_score: stability,
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// M11：延迟 P50/P95/平均值 per agent。
    pub fn ops_latency_stats(&self, days: Option<i64>) -> StorageResult<Vec<LatencyStat>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT p.name, u.duration_ms FROM usage_records u
             JOIN providers p ON p.id = u.provider_id
             WHERE u.duration_ms IS NOT NULL AND u.duration_ms > 0 AND {clause}",
        );
        let args: Vec<SqlValue> = cutoff.map(std::convert::Into::into).into_iter().collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        let mut by_p: std::collections::HashMap<String, Vec<i64>> =
            std::collections::HashMap::new();
        for r in rows {
            let (p, d) = r?;
            by_p.entry(p).or_default().push(d);
        }
        let mut result = Vec::new();
        for (p, mut ds) in by_p {
            ds.sort_unstable();
            let n = ds.len();
            result.push(LatencyStat {
                provider: p,
                sample_count: n as i64,
                p50_ms: ds[n * 50 / 100] as f64,
                p95_ms: ds[n * 95 / 100] as f64,
                avg_ms: ds.iter().sum::<i64>() as f64 / n as f64,
            });
        }
        result.sort_by(|a, b| {
            b.p95_ms
                .partial_cmp(&a.p95_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(result)
    }

    /// M12：Token 浪费检测（input/output > 10 = 上下文累积/全量重放模式）。
    pub fn ops_token_waste(&self, days: Option<i64>, n: i64) -> StorageResult<Vec<TokenWaste>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT p.name, u.source_session_id,
                    SUM(u.input_tokens), SUM(u.output_tokens), COUNT(*), SUM(u.cache_read_tokens)
             FROM usage_records u JOIN providers p ON p.id = u.provider_id
             WHERE {clause}
             GROUP BY u.source_session_id, p.name
             HAVING SUM(u.input_tokens) > 10000 AND SUM(u.output_tokens) > 0
                AND (CAST(SUM(u.input_tokens) AS REAL) / SUM(u.output_tokens)) > 10
             ORDER BY SUM(u.input_tokens) DESC LIMIT ?",
        );
        let mut args: Vec<SqlValue> = cutoff.map(std::convert::Into::into).into_iter().collect();
        args.push(n.into());
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            let inp: i64 = r.get::<_, Option<i64>>(2)?.unwrap_or(0);
            let outp: i64 = r.get::<_, Option<i64>>(3)?.unwrap_or(0);
            let reqs: i64 = r.get::<_, Option<i64>>(4)?.unwrap_or(0);
            let cached: i64 = r.get::<_, Option<i64>>(5)?.unwrap_or(0);
            let ratio = if outp > 0 {
                inp as f64 / outp as f64
            } else {
                0.0
            };
            let cache_ratio = if inp > 0 {
                cached as f64 / inp as f64
            } else {
                0.0
            };
            let waste = ((ratio / 100.0).min(1.0) * 60.0 + (1.0 - cache_ratio) * 40.0).min(100.0);
            Ok(TokenWaste {
                provider: r.get(0)?,
                session_id: r.get(1)?,
                input_tokens: inp,
                output_tokens: outp,
                ratio,
                requests: reqs,
                cache_read: cached,
                waste_score: waste,
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// M13：Agent 横向对比基准（全指标 side-by-side）。
    /// 聚合各 agent 的用量/成本/健康/延迟/缓存为一张对比表。
    pub fn ops_agent_benchmark(&self, days: Option<i64>) -> StorageResult<Vec<AgentBenchmark>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT p.name,
                    COUNT(*),
                    SUM(u.input_tokens + u.output_tokens + u.reasoning_tokens),
                    SUM(COALESCE(u.cost_usd, 0)),
                    SUM(CASE WHEN u.status = 'error' THEN 1 ELSE 0 END),
                    SUM(COALESCE(u.retry_count, 0)),
                    SUM(u.input_tokens),
                    SUM(u.cache_read_tokens),
                    AVG(u.duration_ms),
                    COUNT(DISTINCT u.source_session_id)
             FROM usage_records u JOIN providers p ON p.id = u.provider_id
             WHERE {clause}
             GROUP BY p.name",
        );
        let args: Vec<SqlValue> = cutoff.map(std::convert::Into::into).into_iter().collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            let total: i64 = r.get(1)?;
            let tokens: i64 = r.get::<_, Option<i64>>(2)?.unwrap_or(0);
            let cost: f64 = r.get::<_, Option<f64>>(3)?.unwrap_or(0.0);
            let errors: i64 = r.get::<_, Option<i64>>(4)?.unwrap_or(0);
            let retries: i64 = r.get::<_, Option<i64>>(5)?.unwrap_or(0);
            let _ = retries; // 保留读取占位，避免列偏移
            let input: i64 = r.get::<_, Option<i64>>(6)?.unwrap_or(0);
            let cached: i64 = r.get::<_, Option<i64>>(7)?.unwrap_or(0);
            let avg_dur: f64 = r.get::<_, Option<f64>>(8)?.unwrap_or(0.0);
            let sessions: i64 = r.get(9)?;
            let success_rate = if total > 0 {
                (total - errors) as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            let cache_hit = if input > 0 {
                cached as f64 / input as f64 * 100.0
            } else {
                0.0
            };
            let cost_per_session = if sessions > 0 {
                cost / sessions as f64
            } else {
                0.0
            };
            let tokens_per_session = if sessions > 0 { tokens / sessions } else { 0 };
            Ok(AgentBenchmark {
                provider: r.get(0)?,
                total_requests: total,
                total_tokens: tokens,
                cost_usd: cost,
                sessions,
                success_rate,
                cache_hit_rate: cache_hit,
                avg_duration_ms: avg_dur,
                cost_per_session,
                tokens_per_session,
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// M14：周报汇总数据（治理页一键导出用）。
    /// 聚合 7 天内全部治理指标为一个结构。
    pub fn ops_weekly_summary(&self) -> StorageResult<WeeklySummary> {
        let overview = self.ops_overview(Some(7))?;
        let health = self.ops_agent_health(Some(7))?;
        let benchmark = self.ops_agent_benchmark(Some(7))?;
        let waste_count = self.ops_token_waste(Some(7), 100)?.len() as i64;
        Ok(WeeklySummary {
            overview,
            health,
            benchmark,
            waste_sessions: waste_count,
        })
    }

    /// 当月用量 + 按日均外推月底（第一天返回 so_far 原值）。
    pub fn ops_month_projection(&self) -> StorageResult<MonthProjection> {
        let now = now_utc();
        let (y, m) = (now.year(), now.month() as u32);
        let days_in_month = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][(m as usize) - 1]
            + u32::from(m == 2 && (y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)));
        let day = u32::from(now.day());
        let days_elapsed = day.max(1);

        let month_start = time::Date::from_calendar_date(now.year(), now.month(), 1)
            .expect("月初日期构造仅在当时钟异常时失败")
            .midnight()
            .assume_utc();
        let cutoff = timestamp::to_millis(Some(month_start)).unwrap_or(0);

        let (tokens, cost): (i64, f64) = {
            let conn = self.conn.lock().expect("mutex poisoned");
            conn.query_row(
                "SELECT
                    COALESCE(SUM(input_tokens + output_tokens + reasoning_tokens), 0),
                    COALESCE(SUM(cost_usd), 0.0)
                 FROM usage_records WHERE ts >= ?1",
                params![cutoff],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?
        };

        let per_day_t = tokens as f64 / f64::from(days_elapsed);
        let per_day_c = cost / f64::from(days_elapsed);
        Ok(MonthProjection {
            tokens_so_far: tokens,
            cost_so_far: cost,
            days_elapsed,
            days_in_month,
            projected_tokens: (per_day_t * f64::from(days_in_month)) as i64,
            projected_cost: per_day_c * f64::from(days_in_month),
        })
    }

    /// 每日缓存命中趋势（总输入口径含 cache_read；空日由前端补零）。
    pub fn ops_cache_trend(&self, days: Option<i64>) -> StorageResult<Vec<CacheTrendRow>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let (clause, cutoff) = Self::range_clause(days);
        let sql = format!(
            "SELECT date(ts/1000, 'unixepoch', 'localtime') AS day,
                    SUM(input_tokens + output_tokens + reasoning_tokens + cache_read_tokens),
                    SUM(cache_read_tokens)
             FROM usage_records WHERE {clause}
             GROUP BY day ORDER BY day",
        );
        let args: Vec<SqlValue> = cutoff.map(std::convert::Into::into).into_iter().collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            Ok(CacheTrendRow {
                day: r.get(0)?,
                total_input: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                cache_read: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 保留策略：把 N 天前未归档的会话批量归档，返回归档数量。
    pub fn archive_conversations_older_than(&self, days: i64) -> StorageResult<usize> {
        let cutoff = timestamp::to_millis(Some(now_utc())).unwrap_or(0) - days * 86_400_000;
        let conn = self.conn.lock().expect("mutex poisoned");
        let changed = conn.execute(
            "UPDATE conversations SET is_archived = 1
             WHERE is_archived = 0
               AND COALESCE(source_status, '') != 'deleted'
               AND updated_at IS NOT NULL AND updated_at < ?1",
            params![cutoff],
        )?;
        Ok(changed)
    }

    /// GC 引用集合：所有会话挂的 raw blob hash（孤儿判定用）。
    pub fn list_raw_payload_refs(&self) -> StorageResult<std::collections::HashSet<String>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT DISTINCT raw_payload_id FROM conversations WHERE raw_payload_id IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut set = std::collections::HashSet::new();
        for r in rows {
            set.insert(r?);
        }
        Ok(set)
    }

    /// 用量汇总（本月 / 本年 / 全部），成本页顶部总览卡。
    pub fn ops_usage_summary(&self) -> StorageResult<UsageSummary> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let now = now_utc();
        let month_start = time::Date::from_calendar_date(now.year(), now.month(), 1)
            .expect("月初构造仅在当时钟异常时失败")
            .midnight()
            .assume_utc();
        let year_start = time::Date::from_calendar_date(now.year(), time::Month::January, 1)
            .expect("年初构造仅在当时钟异常时失败")
            .midnight()
            .assume_utc();
        let q = |cutoff: Option<i64>| -> StorageResult<(i64, f64)> {
            Ok(match cutoff {
                Some(c) => conn.query_row(
                    "SELECT COALESCE(SUM(input_tokens+output_tokens+reasoning_tokens),0),
                            COALESCE(SUM(cost_usd),0.0)
                     FROM usage_records WHERE ts >= ?1",
                    params![c],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?,
                None => conn.query_row(
                    "SELECT COALESCE(SUM(input_tokens+output_tokens+reasoning_tokens),0),
                            COALESCE(SUM(cost_usd),0.0)
                     FROM usage_records",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?,
            })
        };
        let month = q(Some(timestamp::to_millis(Some(month_start)).unwrap_or(0)))?;
        let year = q(Some(timestamp::to_millis(Some(year_start)).unwrap_or(0)))?;
        let all = q(None)?;
        Ok(UsageSummary {
            month_tokens: month.0,
            month_cost: month.1,
            year_tokens: year.0,
            year_cost: year.1,
            all_tokens: all.0,
            all_cost: all.1,
        })
    }

    // ── 洞察页面聚合 ─────────────────────────────────────────────────

    /// 活动节律：按天热力 / 24 小时分布 / 工具月度趋势 / 工作日-周末拆分 / 日级工具分布。
    ///
    /// 返回具名字段的对象（前端按 `{ day, calls, sessions }` 读）。
    /// 改前是 `Vec<(String, i64, i64)>`，serde 默认序列化为 `[["2026-08-10", 5, 2]]`，
    /// 前端当对象读全是 `undefined`，导致统计 NaN 且 `month.slice(2)` 抛错。
    pub fn activity_stats(&self, days: i64) -> StorageResult<ActivityStats> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let now_ms = timestamp::to_millis(Some(now_utc())).unwrap_or(0);
        let cutoff = now_ms - days * 86_400_000;
        // 日级工具分布额外限到最近 90 天（与原逻辑一致），避免数据爆炸
        let day_cutoff = now_ms - days.min(90) * 86_400_000;

        let heatmap = build_heatmap(&conn, cutoff)?;
        let (hourly, hourly_weekday, hourly_weekend) = build_hourly_buckets(&conn, cutoff)?;
        let tools_trend = build_tools_trend(&conn, cutoff)?;
        let tool_daily = build_tool_daily(&conn, day_cutoff)?;

        Ok(ActivityStats {
            heatmap,
            hourly,
            hourly_weekday,
            hourly_weekend,
            tools_trend,
            tool_daily,
        })
    }

    /// 项目中心：按 source_dir 聚合（与成本页同口径）。
    pub fn projects_overview(&self) -> StorageResult<Vec<serde_json::Value>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT COALESCE(NULLIF(u.source_dir,''), '(未知目录)') AS dir,
                    COUNT(DISTINCT u.source_session_id),
                    SUM(u.input_tokens + u.output_tokens + u.reasoning_tokens),
                    COALESCE(SUM(u.cost_usd), 0.0),
                    COUNT(*),
                    MAX(u.ts),
                    (SELECT p.name FROM usage_records u2
                      JOIN providers p ON p.id = u2.provider_id
                      WHERE u2.source_dir = u.source_dir
                      GROUP BY p.name ORDER BY COUNT(*) DESC LIMIT 1)
             FROM usage_records u
             GROUP BY dir
             ORDER BY 4 DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(serde_json::json!({
                "dir": r.get::<_, String>(0)?,
                "sessions": r.get::<_, i64>(1)?,
                "tokens": r.get::<_, i64>(2)?,
                "cost_usd": r.get::<_, f64>(3)?,
                "requests": r.get::<_, i64>(4)?,
                "last_active_ms": r.get::<_, Option<i64>>(5)?,
                "main_agent": r.get::<_, Option<String>>(6)?,
            }))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 按 source_dir 列出 conversations（项目页「查看会话」用）。
    /// 路径匹配与 projects_overview 口径一致：空串视为「(未知目录)」。
    pub fn conversations_by_source_dir(
        &self,
        dir: &str,
    ) -> StorageResult<Vec<ch_domain::Conversation>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        // projects_overview 把空/缺失归并为 (未知目录)；这里用哨兵 "__MISSING__" 走 NULL/空 匹配
        let sentinel = if dir.is_empty() || dir == "(未知目录)" {
            "__MISSING__"
        } else {
            dir
        };
        let mut stmt = conn.prepare(
            "SELECT DISTINCT c.id, c.workspace_id, p.name, c.installation_id, c.source_conversation_id,
                    c.title, c.user_title, c.status, c.model, c.started_at, c.updated_at,
                    c.completed_at, c.source_status, c.source_url, c.completeness_score,
                    c.content_hash, c.raw_payload_id, c.source_parent_id
             FROM conversations c
             JOIN providers p ON p.id = c.provider_id
             JOIN usage_records u ON u.source_session_id = c.source_conversation_id
                                  AND u.provider_id = c.provider_id
             WHERE (?1 = '__MISSING__' AND (u.source_dir IS NULL OR u.source_dir = ''))
                OR (?1 != '__MISSING__' AND u.source_dir = ?1)
             ORDER BY c.updated_at DESC NULLS LAST",
        )?;
        let rows = stmt.query_map(params![sentinel], row_to_conversation)?;
        let mut v = Vec::new();
        for row in rows {
            v.push(row?);
        }
        Ok(v)
    }

    /// 提示词库语料：最近的用户提问（含会话定位）。
    pub fn recent_user_prompts(&self, limit: i64) -> StorageResult<Vec<serde_json::Value>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT m.id, m.conversation_id,
                    substr(COALESCE(m.content_text,''), 1, 300),
                    COALESCE(c.title, c.source_conversation_id, ''),
                    m.created_at
             FROM messages m JOIN conversations c ON c.id = m.conversation_id
             WHERE m.role = 'user' AND m.content_text IS NOT NULL AND length(m.content_text) > 4
             ORDER BY COALESCE(m.created_at, 0) DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| {
            Ok(serde_json::json!({
                "message_id": r.get::<_, String>(0)?,
                "conversation_id": r.get::<_, String>(1)?,
                "text": r.get::<_, String>(2)?,
                "title": r.get::<_, String>(3)?,
                "created_at": r.get::<_, Option<i64>>(4)?,
            }))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

// ── activity_stats 拆分出的 4 个 helper（按职责分离，每个 < 30 行）────────────

/// 按天聚合：每天的 calls + sessions 计数。
fn build_heatmap(conn: &rusqlite::Connection, cutoff_ms: i64) -> StorageResult<Vec<HeatCell>> {
    let mut stmt = conn.prepare(
        "SELECT date(t.ts/1000,'unixepoch','localtime') AS d,
                COUNT(*),
                COUNT(DISTINCT t.source_session_id)
         FROM tool_call_records t
         WHERE t.ts >= ?1 AND t.ts IS NOT NULL
         GROUP BY d ORDER BY d",
    )?;
    let rows = stmt.query_map(params![cutoff_ms], |r| {
        Ok(HeatCell {
            day: r.get(0)?,
            calls: r.get(1)?,
            sessions: r.get(2)?,
        })
    })?;
    let mut heatmap = Vec::new();
    for row in rows {
        heatmap.push(row?);
    }
    Ok(heatmap)
}

/// 24 小时分布：返回 (全天, 工作日, 周末) 三个 24 槽序列。
fn build_hourly_buckets(
    conn: &rusqlite::Connection,
    cutoff_ms: i64,
) -> StorageResult<(Vec<HourBucket>, Vec<HourBucket>, Vec<HourBucket>)> {
    let mut stmt = conn.prepare(
        "SELECT CAST(strftime('%H', ts/1000,'unixepoch','localtime') AS INTEGER) AS h,
                CAST(strftime('%w', ts/1000,'unixepoch','localtime') AS INTEGER) AS dow,
                COUNT(*)
         FROM tool_call_records WHERE ts >= ?1 AND ts IS NOT NULL
         GROUP BY h, dow ORDER BY h",
    )?;
    // 固定 24 槽数组（栈分配，无堆分配），用索引累加
    let mut wd = [0i64; 24];
    let mut we = [0i64; 24];
    let mut total = [0i64; 24];
    let rows = stmt.query_map(params![cutoff_ms], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (h, dow, c) = row?;
        if (0..24).contains(&h) {
            total[h as usize] += c;
            if dow == 0 || dow == 6 {
                we[h as usize] += c;
            } else {
                wd[h as usize] += c;
            }
        }
    }
    let mut hourly = Vec::with_capacity(24);
    let mut hourly_weekday = Vec::with_capacity(24);
    let mut hourly_weekend = Vec::with_capacity(24);
    for h in 0..24 {
        hourly.push(HourBucket {
            hour: h as i64,
            calls: total[h],
        });
        hourly_weekday.push(HourBucket {
            hour: h as i64,
            calls: wd[h],
        });
        hourly_weekend.push(HourBucket {
            hour: h as i64,
            calls: we[h],
        });
    }
    Ok((hourly, hourly_weekday, hourly_weekend))
}

/// 按月聚合：每月 × 工具的 calls 计数（用于月度趋势图）。
fn build_tools_trend(conn: &rusqlite::Connection, cutoff_ms: i64) -> StorageResult<Vec<ToolTrend>> {
    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-%m', ts/1000,'unixepoch','localtime') AS m,
                tool_name, COUNT(*)
         FROM tool_call_records WHERE ts >= ?1 AND ts IS NOT NULL
         GROUP BY m, tool_name ORDER BY m, 3 DESC",
    )?;
    let rows = stmt.query_map(params![cutoff_ms], |r| {
        Ok(ToolTrend {
            month: r.get(0)?,
            tool: r.get(1)?,
            calls: r.get(2)?,
        })
    })?;
    let mut tools_trend = Vec::new();
    for row in rows {
        tools_trend.push(row?);
    }
    Ok(tools_trend)
}

/// 按天 + 工具聚合：日级工具分布（caller 自己决定 cutoff，避免数据爆炸）。
fn build_tool_daily(conn: &rusqlite::Connection, cutoff_ms: i64) -> StorageResult<Vec<DailyTool>> {
    let mut stmt = conn.prepare(
        "SELECT date(ts/1000,'unixepoch','localtime') AS d, tool_name, COUNT(*)
         FROM tool_call_records WHERE ts >= ?1 AND ts IS NOT NULL
         GROUP BY d, tool_name ORDER BY d, 3 DESC",
    )?;
    let rows = stmt.query_map(params![cutoff_ms], |r| {
        Ok(DailyTool {
            day: r.get(0)?,
            tool: r.get(1)?,
            calls: r.get(2)?,
        })
    })?;
    let mut tool_daily = Vec::new();
    for row in rows {
        tool_daily.push(row?);
    }
    Ok(tool_daily)
}
