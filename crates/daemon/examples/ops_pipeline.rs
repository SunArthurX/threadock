//! 治理管线端到端验证：真实数据目录上跑 采集→分块入库→定价→聚合。
use ch_daemon::{DaemonState, DaemonStateConfig};
use std::time::Instant;

#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
fn main() {
    let home = std::env::var("HOME").expect("write to String");
    let dir = format!("{home}/Library/Application Support/com.conversation-hub.desktop");
    let t0 = Instant::now();
    let st = DaemonState::open(DaemonStateConfig {
        data_dir: dir.into(),
    })
    .expect("write to String");
    println!("DaemonState::open: {:?}", t0.elapsed());

    // 1. 采集（4 源）
    let t = Instant::now();
    let (mut u_all, mut t_all) =
        ch_ops_metrics::collect_zcode(format!("{home}/.zcode/cli/db/db.sqlite"))
            .expect("write to String");
    println!("zcode: usage={} tools={}", u_all.len(), t_all.len());
    let u =
        ch_ops_metrics::collect_minimax(format!("{home}/.minimax/v2/sqlite/runtime-state.sqlite"))
            .expect("write to String");
    println!("minimax: usage={}", u.len());
    u_all.extend(u);
    let (u, tc) =
        ch_ops_metrics::collect_claude_code(format!("{home}/.claude")).expect("write to String");
    println!("claude: usage={} tools={}", u.len(), tc.len());
    u_all.extend(u);
    t_all.extend(tc);
    let (u, tc) = ch_ops_metrics::collect_codex(format!("{home}/.codex")).expect("write to String");
    println!("codex: usage={} tools={}", u.len(), tc.len());
    u_all.extend(u);
    t_all.extend(tc);
    println!(
        "── 采集总计: {:?}  usage={} tools={}",
        t.elapsed(),
        u_all.len(),
        t_all.len()
    );

    // 2. 分块入库
    let t = Instant::now();
    let repo = st.repo.lock().expect("write to String");
    let zc_usage: Vec<_> = u_all
        .iter()
        .filter(|r| r.provider == ch_domain::Provider::ZCode)
        .cloned()
        .collect();
    let others: Vec<_> = u_all
        .iter()
        .filter(|r| r.provider != ch_domain::Provider::ZCode)
        .cloned()
        .collect();
    let nz = repo
        .replace_provider_usage("prov_zcode", &zc_usage)
        .expect("write to String");
    let n1 = repo.upsert_usage_batch(&others).expect("write to String") + nz;
    let n2 = repo
        .upsert_tool_call_batch(&t_all)
        .expect("write to String");
    println!(
        "── 入库: {:?}  usage 新增 {n1} tools 新增 {n2}",
        t.elapsed()
    );

    // 3. 定价（模型匹配 + provider 兜底，与 tauri apply_pricing 同逻辑）
    let pricing: serde_json::Value = serde_json::json!({
        "glm-5.2": {"input_per_mtok": 0.5, "output_per_mtok": 2.0},
        "minimax-m3": {"input_per_mtok": 0.3, "output_per_mtok": 1.2},
        "codex": {"input_per_mtok": 2.0, "output_per_mtok": 8.0},
        "claude": {"input_per_mtok": 3.0, "output_per_mtok": 15.0},
        "zcode": {"input_per_mtok": 0.5, "output_per_mtok": 2.0}
    });
    let fallback = [
        ("prov_zcode", "zcode"),
        ("prov_minimax-code", "minimax-m3"),
        ("prov_claude-code", "claude"),
        ("prov_codex", "codex"),
    ];
    let mut cost_total = 0f64;
    for (model, pid, i, o) in repo.ops_model_token_totals().expect("write to String") {
        let m = model.to_lowercase();
        let key = ["glm-5.2", "minimax-m3", "codex", "claude", "zcode"]
            .iter()
            .find(|k| m.contains(**k))
            .map(|k| (*k).to_string())
            .or_else(|| {
                fallback
                    .iter()
                    .find(|(p, _)| *p == pid)
                    .map(|(_, k)| (*k).to_string())
            });
        if let Some(k) = key {
            let v = &pricing[&k];
            let pin = v["input_per_mtok"].as_f64().expect("write to String");
            let pout = v["output_per_mtok"].as_f64().expect("write to String");
            let _ = repo.update_model_pricing(&model, &pid, pin, pout);
            let c = (i as f64 / 1e6) * pin + (o as f64 / 1e6) * pout;
            cost_total += c;
            println!("   {model:24} {pid:20} in={i:>12} out={o:>10} cost=${c:.2}");
        }
    }
    println!("── 总成本: ${cost_total:.2}");

    // 4. 聚合（治理页口径）
    let ov = repo.ops_overview(None).expect("write to String");
    println!(
        "── Overview: 请求={} tokens={} 成本=${:.2} 危险={} 工具={}",
        ov.total_requests, ov.total_tokens, ov.cost_usd, ov.destructive_calls, ov.total_tool_calls
    );
    for p in repo.ops_by_provider(None).expect("write to String") {
        println!(
            "   {:14} req={:>6} tokens={:>14} err={}",
            p.provider, p.requests, p.total_tokens, p.errors
        );
    }
    let ts = repo.ops_timeseries_daily(Some(7)).expect("write to String");
    println!("── 近7天趋势: {} 天", ts.len());
    for d in &ts {
        println!("   {} tokens={}", d.day, d.total_tokens);
    }
    drop(repo);
    println!("── 全管线总耗时: {:?}", t0.elapsed());
}
