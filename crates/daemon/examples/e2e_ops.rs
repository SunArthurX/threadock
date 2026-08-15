//! 治理命令层等价自测：days=30 全聚合 + 真实库全量审计（防 panic/空数据回归）
use ch_daemon::{DaemonState, DaemonStateConfig};

fn main() {
    let home = std::env::var("HOME").expect("write to String");
    let dir = format!("{home}/Library/Application Support/com.conversation-hub.desktop");
    let st = DaemonState::open(DaemonStateConfig {
        data_dir: dir.into(),
    })
    .expect("write to String");
    let repo = st.repo.lock().expect("write to String");

    // ── 前端治理页同参（range=30 / 全部=60 …）──
    for days in [Some(30i64), Some(7), None] {
        let label = days.map(|d| d.to_string()).unwrap_or_else(|| "all".into());
        let ov = repo
            .ops_overview(days)
            .unwrap_or_else(|e| panic!("overview({label}) 失败: {e}"));
        let bp = repo.ops_by_provider(days).expect("write to String");
        let bm = repo.ops_by_model(days).expect("write to String");
        let ts = repo.ops_timeseries_daily(days).expect("write to String");
        let tt = repo.ops_tool_toplist(days, 10).expect("write to String");
        let rc = repo.ops_risky_calls(days, 50).expect("write to String");
        println!("days={label}: 请求={} tokens={} 成本=${:.2} | providers={} models={} 天数={} top工具={} 风险={}",
            ov.total_requests, ov.total_tokens, ov.cost_usd, bp.len(), bm.len(), ts.len(), tt.len(), rc.len());
        assert!(ov.total_requests > 0, "overview({label}) 不应为空");
        assert!(
            !bp.is_empty() && !bm.is_empty() && !tt.is_empty(),
            "聚合({label}) 不应为空"
        );
    }
    println!("── 模型明细样本:");
    for m in repo.ops_by_model(Some(30)).expect("write to String").iter().take(6) {
        println!(
            "   {} [{}] in={} out={} err={}",
            m.model, m.provider_id, m.input_tokens, m.output_tokens, m.errors
        );
    }
    println!("── 工具 Top3:");
    for t in repo.ops_tool_toplist(Some(30), 3).expect("write to String") {
        println!(
            "   {} calls={} destructive={}",
            t.tool_name, t.calls, t.destructive
        );
    }
    drop(repo);

    // ── 真实库全量审计（多字节消息回归 + 完整跑通）──
    let repo = st.repo.lock().expect("write to String");
    let t = std::time::Instant::now();
    let report = ch_audit::run_audit(&repo).expect("审计不应 panic");
    println!(
        "── 审计: {:?} 扫描 {} 消息 / {} 命令 → 高危 {} 中危 {} 低危 {}（共 {} 条发现）",
        t.elapsed(),
        report.scanned_messages,
        report.scanned_tool_calls,
        report.high,
        report.medium,
        report.low,
        report.findings.len()
    );
    let html = ch_audit::render_html(&report);
    assert!(html.len() > 1000);
    println!("── HTML 报告 {} 字节 ✓", html.len());
    println!("ALL E2E OPS CHECKS PASSED");
}
