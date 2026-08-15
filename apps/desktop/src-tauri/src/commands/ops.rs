//! 治理域（CodeAgentOps）：指标同步与聚合查询、资产/自动化、定价与成本、周报。

use super::*;
use ch_daemon::DaemonState;
use tauri::Emitter;

/// 同步 ops 指标（独立于对话采集，幂等批量写入，不影响现有数据）。
/// force=false 时 30 分钟节流（进入治理页不再每次全量扫描 32MB+ JSONL）。
#[tauri::command]
#[tracing::instrument(skip_all, level = "info")]
pub(crate) async fn ops_sync(
    state: tauri::State<'_, DaemonState>,
    app: tauri::AppHandle,
    force: Option<bool>,
) -> Result<serde_json::Value, String> {
    let now_ms =
        (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
    // 持久节流：优先读库内时间戳（跨进程），内存值兜底
    let mut last = LAST_OPS_SYNC_MS.load(std::sync::atomic::Ordering::SeqCst);
    if last == 0 {
        if let Ok(repo) = state.repo.lock() {
            if let Ok(Some(v)) = repo.get_setting("last_ops_sync_ms") {
                last = v.parse().unwrap_or(0);
            }
        }
    }
    if !force.unwrap_or(false) && last > 0 && now_ms - last < OPS_SYNC_THROTTLE_MS {
        return Ok(
            serde_json::json!({ "usage_written": 0, "tools_written": 0, "throttled": true }),
        );
    }
    CANCEL_SYNC.store(false, std::sync::atomic::Ordering::SeqCst);
    let _guard = BusyGuard::acquire()?;
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    let mut usage_written = 0usize;
    let mut tools_written = 0usize;
    // 同步重活移出 runtime worker 线程（采集要扫外部 DB/32MB+ JSONL，秒级）
    let result = run_blocking(|| -> Result<(), String> {
        // 1. 先采集（无锁）：外部源全量扫描不占用 repo 写锁，
        //    期间其他写命令（导入/收藏等）不受阻塞
        let zcode = ch_ops_metrics::collect_zcode(format!("{home}/.zcode/cli/db/db.sqlite"));
        if let Err(ref e) = zcode {
            tracing::warn!(error = %e, "ops collect zcode failed");
        }
        let minimax = ch_ops_metrics::collect_minimax(format!(
            "{home}/.minimax/v2/sqlite/runtime-state.sqlite"
        ));
        if let Err(ref e) = minimax {
            tracing::warn!(error = %e, "ops collect minimax failed");
        }
        let cc = ch_ops_metrics::collect_claude_code(format!("{home}/.claude"));
        if let Err(ref e) = cc {
            tracing::warn!(error = %e, "ops collect claude code failed");
        }
        let codex = ch_ops_metrics::collect_codex(format!("{home}/.codex"));
        if let Err(ref e) = codex {
            tracing::warn!(error = %e, "ops collect codex failed");
        }

        // 2. 后写入（短临界区：只做批量入库）；每完成一个来源 emit 阶段进度
        let emit_stage = |done: u64, detail: &str| {
            let _ = app.emit(
                "sync_progress",
                serde_json::json!({ "current": done, "total": 4, "detail": detail, "finished": false }),
            );
        };
        let repo = state.repo.lock().map_err(|e| storage_err(e))?;
        // provider 表需要存在对应行（JOIN 用）
        for p in [
            ch_domain::Provider::ZCode,
            ch_domain::Provider::MinimaxCode,
            ch_domain::Provider::ClaudeCode,
            ch_domain::Provider::Codex,
        ] {
            repo.upsert_provider(p).map_err(|e| storage_err(e))?;
        }

        // ZCode：model_usage 请求级口径，整源替换（与 turn 级互斥，防双算）
        if let Ok((u, t)) = zcode {
            usage_written += repo
                .replace_provider_usage("prov_zcode", &u)
                .map_err(|e| storage_err(e))?;
            tools_written += repo
                .upsert_tool_call_batch(&t)
                .map_err(|e| storage_err(e))?;
            emit_stage(1, "ZCode 指标");
        }
        if let Ok(u) = minimax {
            usage_written += repo
                .replace_provider_usage("prov_minimax-code", &u)
                .map_err(|e| storage_err(e))?;
            emit_stage(2, "MiniMax 指标");
        }
        if let Ok((u, t)) = cc {
            usage_written += repo
                .replace_provider_usage("prov_claude-code", &u)
                .map_err(|e| storage_err(e))?;
            tools_written += repo
                .upsert_tool_call_batch(&t)
                .map_err(|e| storage_err(e))?;
            emit_stage(3, "Claude Code 指标");
        }
        if let Ok((u, t)) = codex {
            usage_written += repo
                .replace_provider_usage("prov_codex", &u)
                .map_err(|e| storage_err(e))?;
            tools_written += repo
                .upsert_tool_call_batch(&t)
                .map_err(|e| storage_err(e))?;
            emit_stage(4, "Codex 指标");
        }
        // 自动成本重算：同步后立即按定价出数（此前需手动点重算，成本恒为 0）
        if let Ok(pricing) = ops_pricing_get_inner(&state) {
            let _stats = apply_pricing(&repo, &pricing);
        }
        Ok(())
    });
    if result.is_ok() {
        LAST_OPS_SYNC_MS.store(now_ms, std::sync::atomic::Ordering::SeqCst);
        if let Ok(repo) = state.repo.lock() {
            if let Err(e) = repo.set_setting("last_ops_sync_ms", &now_ms.to_string()) {
                tracing::warn!(error = %e, "persist last_ops_sync_ms failed");
            }
        }
    }
    result?;
    let _ = app.emit(
        "sync_progress",
        serde_json::json!({ "current": 4, "total": 4, "detail": "done", "finished": true }),
    );
    Ok(serde_json::json!({
        "usage_written": usage_written,
        "tools_written": tools_written,
    }))
}

#[tauri::command]
pub(crate) async fn ops_overview(
    state: tauri::State<'_, DaemonState>,
    days: Option<i64>,
) -> Result<ch_storage::OpsOverview, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.ops_overview(days).map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn ops_by_provider(
    state: tauri::State<'_, DaemonState>,
    days: Option<i64>,
) -> Result<Vec<ch_storage::ProviderUsage>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.ops_by_provider(days).map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn ops_by_model(
    state: tauri::State<'_, DaemonState>,
    days: Option<i64>,
) -> Result<Vec<ch_storage::ModelUsage>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.ops_by_model(days).map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn ops_timeseries(
    state: tauri::State<'_, DaemonState>,
    days: Option<i64>,
) -> Result<Vec<ch_storage::DailyUsage>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.ops_timeseries_daily(days).map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn ops_tool_toplist(
    state: tauri::State<'_, DaemonState>,
    days: Option<i64>,
    n: Option<i64>,
) -> Result<Vec<ch_storage::ToolUsageRow>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.ops_tool_toplist(days, n.unwrap_or(10))
        .map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn ops_risky_calls(
    state: tauri::State<'_, DaemonState>,
    days: Option<i64>,
    n: Option<i64>,
) -> Result<Vec<RiskyCallDto>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let rows = repo
        .ops_risky_calls(days, n.unwrap_or(50))
        .map_err(|e| storage_err(e))?;
    Ok(rows
        .into_iter()
        .map(|r| RiskyCallDto {
            ts_ms: (r.ts - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64,
            id: r.id,
            provider: r.provider.to_string(),
            source_session_id: r.source_session_id,
            tool_name: r.tool_name,
            read_only: r.read_only,
            destructive: r.destructive,
            approval_status: r.approval_status,
            exit_code: r.exit_code,
            duration_ms: r.duration_ms,
            status: r.status.as_str().to_string(),
            command_text: r.command_text,
        })
        .collect())
}

/// 本月（自然月）用量：预算告警用。
#[tauri::command]
pub(crate) async fn ops_month_usage(
    state: tauri::State<'_, DaemonState>,
) -> Result<serde_json::Value, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let now = ch_domain::now_utc();
    // 本月 1 号 00:00 UTC 的毫秒
    let month_start = time::Date::from_calendar_date(
        now.year(),
        time::Month::try_from(now.month() as u8).unwrap_or(time::Month::January),
        1,
    )
    .map_err(|e| storage_err(e))?
    .with_time(time::Time::MIDNIGHT)
    .assume_utc();
    let cutoff = (month_start - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
    let row = repo
        .ops_month_usage_since(cutoff)
        .map_err(|e| storage_err(e))?;
    Ok(serde_json::json!({
        "tokens": row.0,
        "cost_usd": row.1,
    }))
}

/// 同步资产清单（30 分钟节流，force 可强制）。
#[tauri::command]
pub(crate) async fn assets_sync(
    state: tauri::State<'_, DaemonState>,
    force: Option<bool>,
) -> Result<serde_json::Value, String> {
    let now_ms =
        (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
    {
        let repo = state.repo.lock().map_err(|e| storage_err(e))?;
        if !force.unwrap_or(false) {
            if let Ok(Some(v)) = repo.get_setting("last_assets_sync_ms") {
                if let Ok(last) = v.parse::<i64>() {
                    if now_ms - last < 30 * 60 * 1000 {
                        return Ok(serde_json::json!({ "written": 0, "throttled": true }));
                    }
                }
            }
        }
    }
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    let assets = ch_ops_metrics::collect_assets(&home).map_err(|e| storage_err(e))?;
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    for p in [
        ch_domain::Provider::ZCode,
        ch_domain::Provider::Codex,
        ch_domain::Provider::ClaudeCode,
        ch_domain::Provider::MinimaxCode,
    ] {
        repo.upsert_provider(p).map_err(|e| storage_err(e))?;
    }
    let mut written = 0;
    for p in [
        ch_domain::Provider::ZCode,
        ch_domain::Provider::Codex,
        ch_domain::Provider::ClaudeCode,
        ch_domain::Provider::MinimaxCode,
    ] {
        let subset: Vec<_> = assets.iter().filter(|a| a.provider == p).cloned().collect();
        written += repo
            .replace_provider_assets(&format!("prov_{}", p.as_str()), &subset)
            .map_err(|e| storage_err(e))?;
    }
    if let Err(e) = repo.set_setting("last_assets_sync_ms", &now_ms.to_string()) {
        tracing::warn!(error = %e, "persist last_assets_sync_ms failed");
    }
    Ok(serde_json::json!({ "written": written }))
}

#[tauri::command]
pub(crate) async fn assets_list(
    state: tauri::State<'_, DaemonState>,
) -> Result<Vec<ch_storage::AssetRow>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.list_assets().map_err(|e| storage_err(e))
}

/// 同步自动化任务（30 分钟节流）。
#[tauri::command]
pub(crate) async fn automations_sync(
    state: tauri::State<'_, DaemonState>,
    force: Option<bool>,
) -> Result<serde_json::Value, String> {
    let now_ms =
        (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
    {
        let repo = state.repo.lock().map_err(|e| storage_err(e))?;
        if !force.unwrap_or(false) {
            if let Ok(Some(v)) = repo.get_setting("last_automations_sync_ms") {
                if let Ok(last) = v.parse::<i64>() {
                    if now_ms - last < 30 * 60 * 1000 {
                        return Ok(serde_json::json!({ "written": 0, "throttled": true }));
                    }
                }
            }
        }
    }
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    let recs = ch_ops_metrics::collect_automations(&home).map_err(|e| storage_err(e))?;
    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
    for p in [
        ch_domain::Provider::ZCode,
        ch_domain::Provider::Codex,
        ch_domain::Provider::MinimaxCode,
    ] {
        repo.upsert_provider(p).map_err(|e| storage_err(e))?;
    }
    let mut written = 0;
    for p in [
        ch_domain::Provider::ZCode,
        ch_domain::Provider::Codex,
        ch_domain::Provider::MinimaxCode,
    ] {
        let subset: Vec<_> = recs.iter().filter(|a| a.provider == p).cloned().collect();
        written += repo
            .replace_provider_automations(&format!("prov_{}", p.as_str()), &subset)
            .map_err(|e| storage_err(e))?;
    }
    if let Err(e) = repo.set_setting("last_automations_sync_ms", &now_ms.to_string()) {
        tracing::warn!(error = %e, "persist last_automations_sync_ms failed");
    }
    Ok(serde_json::json!({ "written": written }))
}

#[tauri::command]
pub(crate) async fn automations_list(
    state: tauri::State<'_, DaemonState>,
) -> Result<Vec<ch_storage::AutomationRow>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.list_automations().map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn ops_cost_by_dir(
    state: tauri::State<'_, DaemonState>,
    days: Option<i64>,
    n: Option<i64>,
) -> Result<Vec<ch_storage::DirCost>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.ops_cost_by_dir(days, n.unwrap_or(10))
        .map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn ops_cache_stats(
    state: tauri::State<'_, DaemonState>,
    days: Option<i64>,
) -> Result<Vec<ch_storage::CacheStat>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.ops_cache_stats(days).map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn ops_anomalies(
    state: tauri::State<'_, DaemonState>,
    days: Option<i64>,
) -> Result<Vec<ch_storage::AnomalyRow>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.ops_anomalies(days).map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn ops_agent_health(
    state: tauri::State<'_, DaemonState>,
    days: Option<i64>,
) -> Result<Vec<ch_storage::AgentHealth>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.ops_agent_health(days).map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn ops_latency_stats(
    state: tauri::State<'_, DaemonState>,
    days: Option<i64>,
) -> Result<Vec<ch_storage::LatencyStat>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.ops_latency_stats(days).map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn ops_token_waste(
    state: tauri::State<'_, DaemonState>,
    days: Option<i64>,
    n: Option<i64>,
) -> Result<Vec<ch_storage::TokenWaste>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.ops_token_waste(days, n.unwrap_or(10))
        .map_err(|e| storage_err(e))
}

#[tauri::command]
pub(crate) async fn ops_agent_benchmark(
    state: tauri::State<'_, DaemonState>,
    days: Option<i64>,
) -> Result<Vec<ch_storage::AgentBenchmark>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.ops_agent_benchmark(days).map_err(|e| storage_err(e))
}

/// M14：生成周报 HTML（7 天治理汇总，自包含可分享）。
#[tauri::command]
pub(crate) async fn ops_weekly_report(
    state: tauri::State<'_, DaemonState>,
) -> Result<String, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    weekly_report_html(&repo)
}

/// 周报 HTML 生成（command 与自动生成都走这里）。
pub(crate) fn weekly_report_html(repo: &ch_storage::Repository) -> Result<String, String> {
    let s = repo.ops_weekly_summary().map_err(|e| storage_err(e))?;
    let mut html = String::new();
    use std::fmt::Write;
    writeln!(html, "<!doctype html><html lang='zh-CN'><head><meta charset='utf-8'><title>Threadock 周报</title>").expect("write to String");
    writeln!(html, "<style>body{{font-family:-apple-system,'PingFang SC',sans-serif;margin:40px;background:#f7f8fa;color:#1a1e2e;}}").expect("write to String");
    writeln!(
        html,
        "h1{{font-size:20px;}} .meta{{color:#666;font-size:13px;margin-bottom:24px;}}"
    )
    .expect("write to String");
    writeln!(html, ".grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:12px;margin-bottom:24px;}}").expect("write to String");
    writeln!(
        html,
        ".card{{background:#fff;border:1px solid #e5e7eb;border-radius:10px;padding:16px;}}"
    )
    .expect("write to String");
    writeln!(
        html,
        ".card b{{display:block;font-size:24px;margin-bottom:4px;}}"
    )
    .expect("write to String");
    writeln!(html, "table{{width:100%;border-collapse:collapse;background:#fff;border-radius:10px;font-size:13px;}}").expect("write to String");
    writeln!(
        html,
        "th,td{{padding:10px 14px;border-bottom:1px solid #f0f0f0;text-align:left;}}"
    )
    .expect("write to String");
    writeln!(
        html,
        "th{{background:#f9fafb;font-size:11px;color:#6b7280;text-transform:uppercase;}}"
    )
    .expect("write to String");
    writeln!(
        html,
        ".good{{color:#059669;}} .warn{{color:#d97706;}} .bad{{color:#dc2626;}}"
    )
    .expect("write to String");
    writeln!(html, "</style></head><body>").expect("write to String");
    writeln!(html, "<h1>📊 Threadock 治理周报</h1>").expect("write to String");
    writeln!(
        html,
        "<div class='meta'>{} · 覆盖最近 7 天 · {} 个 Agent</div>",
        ch_domain::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        s.benchmark.len()
    )
    .expect("write to String");

    // KPI
    writeln!(html, "<div class='grid'>").expect("write to String");
    writeln!(
        html,
        "<div class='card'><b>{}</b>模型请求</div>",
        s.overview.total_requests
    )
    .expect("write to String");
    writeln!(
        html,
        "<div class='card'><b>{}</b>总 Tokens</div>",
        if s.overview.total_tokens >= 1_000_000_000 {
            format!("{:.2}B", s.overview.total_tokens as f64 / 1e9)
        } else if s.overview.total_tokens >= 1_000_000 {
            format!("{:.2}M", s.overview.total_tokens as f64 / 1e6)
        } else {
            s.overview.total_tokens.to_string()
        }
    )
    .expect("write to String");
    writeln!(
        html,
        "<div class='card'><b>${:.2}</b>估算成本</div>",
        s.overview.cost_usd
    )
    .expect("write to String");
    writeln!(
        html,
        "<div class='card'><b>{}</b>危险操作</div>",
        s.overview.destructive_calls
    )
    .expect("write to String");
    writeln!(
        html,
        "<div class='card'><b>{}</b>浪费会话</div>",
        s.waste_sessions
    )
    .expect("write to String");
    writeln!(html, "</div>").expect("write to String");

    // Agent 对比
    writeln!(html, "<h2>Agent 横向对比</h2><table><tr><th>Agent</th><th>请求</th><th>Tokens</th><th>成本</th><th>成功率</th><th>缓存命中</th><th>会话</th></tr>").expect("write to String");
    for b in &s.benchmark {
        writeln!(html, "<tr><td><b>{}</b></td><td>{}</td><td>{}</td><td>${:.2}</td><td class='{}'>{:.1}%</td><td>{:.1}%</td><td>{}</td></tr>",
            b.provider, b.total_requests,
            if b.total_tokens >= 1_000_000_000 { format!("{:.2}B", b.total_tokens as f64 / 1e9) }
            else if b.total_tokens >= 1_000_000 { format!("{:.2}M", b.total_tokens as f64 / 1e6) }
            else { b.total_tokens.to_string() },
            b.cost_usd,
            if b.success_rate > 95.0 {"good"} else if b.success_rate > 80.0 {"warn"} else {"bad"},
            b.success_rate, b.cache_hit_rate, b.sessions).expect("write to String");
    }
    writeln!(html, "</table>").expect("write to String");

    // 健康度
    if !s.health.is_empty() {
        writeln!(html, "<h2 style='margin-top:24px;'>Agent 健康度</h2><table><tr><th>Agent</th><th>请求</th><th>错误</th><th>重试</th><th>稳定性</th></tr>").expect("write to String");
        for h in &s.health {
            writeln!(html, "<tr><td>{}</td><td>{}</td><td class='{}'>{}</td><td>{}</td><td class='{}'>{:.0}</td></tr>",
                h.provider, h.total_requests, h.errors,
                if h.errors == 0 {"good"} else {"warn"},
                h.retries,
                if h.stability_score > 80.0 {"good"} else if h.stability_score > 50.0 {"warn"} else {"bad"},
                h.stability_score).expect("write to String");
        }
        writeln!(html, "</table>").expect("write to String");
    }

    writeln!(html, "<p style='margin-top:24px;color:#aaa;font-size:11px;'>由 Conversation Hub 自动生成 · 数据口径: input + output + reasoning (cache 不计费)</p>").expect("write to String");
    writeln!(html, "</body></html>").expect("write to String");
    Ok(html)
}

/// 默认定价（$/M tokens，可被 app_data/pricing.json 覆盖）。
/// "zcode"/"cursor" 为 provider 兜底价（模型未细分时使用）。
pub(crate) const DEFAULT_PRICING: &str = r#"{
  "GLM-5.2": {"input_per_mtok": 0.5, "output_per_mtok": 2.0},
  "GLM-5.3": {"input_per_mtok": 0.5, "output_per_mtok": 2.0},
  "MiniMax-M3": {"input_per_mtok": 0.3, "output_per_mtok": 1.2},
  "codex": {"input_per_mtok": 2.0, "output_per_mtok": 8.0},
  "gpt-5": {"input_per_mtok": 1.25, "output_per_mtok": 10.0},
  "claude": {"input_per_mtok": 3.0, "output_per_mtok": 15.0},
  "zcode": {"input_per_mtok": 0.5, "output_per_mtok": 2.0},
  "cursor": {"input_per_mtok": 0.5, "output_per_mtok": 2.0}
}"#;

/// provider_id → 兜底定价键（模型未细分/unknown 时按来源计价）。
pub(crate) const PROVIDER_PRICING_FALLBACK: &[(&str, &str)] = &[
    ("prov_zcode", "zcode"),
    ("prov_minimax-code", "MiniMax-M3"),
    ("prov_claude-code", "claude"),
    ("prov_codex", "codex"),
    ("prov_cursor", "cursor"),
];

pub(crate) fn pricing_path(state: &tauri::State<DaemonState>) -> std::path::PathBuf {
    state.data_dir.join("pricing.json")
}

/// 读取定价表（不存在时写入默认值；旧文件缺新键时内存合并默认键，不回写）。
pub(crate) fn ops_pricing_get_inner(state: &DaemonState) -> Result<serde_json::Value, String> {
    let path = state.data_dir.join("pricing.json");
    if !path.exists() {
        std::fs::write(&path, DEFAULT_PRICING).map_err(|e| io_err(e))?;
    }
    let content = std::fs::read_to_string(&path).map_err(|e| io_err(e))?;
    let mut pricing: serde_json::Value = serde_json::from_str(&content).map_err(|e| io_err(e))?;
    // 旧文件缺省键合并（如后来新增的 zcode/cursor 兜底价）
    if let (Some(dst), Ok(defs)) = (
        pricing.as_object_mut(),
        serde_json::from_str::<serde_json::Value>(DEFAULT_PRICING),
    ) {
        if let Some(def_map) = defs.as_object() {
            for (k, v) in def_map {
                dst.entry(k.clone()).or_insert(v.clone());
            }
        }
    }
    Ok(pricing)
}

#[tauri::command]
pub(crate) async fn ops_pricing_get(
    state: tauri::State<'_, DaemonState>,
) -> Result<serde_json::Value, String> {
    ops_pricing_get_inner(&state)
}

/// 保存定价表（前端编辑后写回）。
#[tauri::command]
pub(crate) async fn ops_pricing_set(
    state: tauri::State<'_, DaemonState>,
    pricing: serde_json::Value,
) -> Result<(), String> {
    let path = pricing_path(&state);
    let content = serde_json::to_string_pretty(&pricing).map_err(|e| storage_err(e))?;
    std::fs::write(&path, content).map_err(|e| io_err(e))
}

/// 定价应用核心：模型名匹配（前缀/包含）→ 命中；未命中走 provider 兜底价。
/// 返回 (更新模型数, 总成本)。ops_sync 自动调用 + 手动重算共用。
pub(crate) fn apply_pricing(
    repo: &ch_storage::Repository,
    pricing: &serde_json::Value,
) -> (i64, f64) {
    let mut table: Vec<(String, f64, f64)> = Vec::new();
    if let Some(map) = pricing.as_object() {
        for (model, v) in map {
            let pin = v
                .get("input_per_mtok")
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            let pout = v
                .get("output_per_mtok")
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            table.push((model.to_lowercase(), pin, pout));
        }
    }
    let Ok(models) = repo.ops_model_token_totals() else {
        return (0, 0.0);
    };
    let mut updated = 0i64;
    let mut total_cost = 0f64;
    for (model, provider_id, in_tok, out_tok) in models {
        let m = model.to_lowercase();
        // 1) 模型名匹配
        let hit = table
            .iter()
            .find(|(k, _, _)| {
                m.starts_with(k.as_str()) || k.starts_with(m.as_str()) || m.contains(k.as_str())
            })
            .map(|(k, _, _)| k.clone())
            // 2) provider 兜底（模型未细分，如 zcode turn_usage 无模型字段）
            .or_else(|| {
                PROVIDER_PRICING_FALLBACK
                    .iter()
                    .find(|(p, _)| *p == provider_id)
                    .map(|(_, k)| k.to_string())
            });
        if let Some(key) = hit {
            if let Some((_, pin, pout)) = table.iter().find(|(k, _, _)| *k == key) {
                // 按行内 tokens 单价写入：SUM(行成本) 与聚合口径一致
                if repo
                    .update_model_pricing(&model, &provider_id, *pin, *pout)
                    .is_ok()
                {
                    updated += 1;
                    total_cost += (in_tok as f64 / 1e6) * pin + (out_tok as f64 / 1e6) * pout;
                }
            }
        }
    }
    (updated, total_cost)
}

/// 按定价重算 cost_usd（改 pricing.json 后调用立即生效）。
#[tauri::command]
pub(crate) async fn ops_cost_recalc(
    state: tauri::State<'_, DaemonState>,
) -> Result<serde_json::Value, String> {
    let pricing = ops_pricing_get_inner(&state)?;
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let (updated, total_cost) = apply_pricing(&repo, &pricing);
    Ok(serde_json::json!({
        "models_updated": updated,
        "total_cost_usd": total_cost,
    }))
}

/// 当月用量 + 日均外推月底预测（预算告警/预算条）。
#[tauri::command]
pub(crate) async fn ops_month_projection(
    state: tauri::State<'_, DaemonState>,
) -> Result<ch_storage::MonthProjection, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.ops_month_projection().map_err(|e| storage_err(e))
}

/// 每日缓存命中趋势（概览缓存卡片的趋势图）。
#[tauri::command]
pub(crate) async fn ops_cache_trend(
    state: tauri::State<'_, DaemonState>,
    days: Option<i64>,
) -> Result<Vec<ch_storage::CacheTrendRow>, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.ops_cache_trend(days).map_err(|e| storage_err(e))
}

/// 用量汇总（本月 / 本年 / 全部）：成本页顶部总览卡。
#[tauri::command]
pub(crate) async fn ops_usage_summary(
    state: tauri::State<'_, DaemonState>,
) -> Result<ch_storage::UsageSummary, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.ops_usage_summary().map_err(|e| storage_err(e))
}
