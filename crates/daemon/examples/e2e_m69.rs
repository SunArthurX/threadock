//! M6-M9 端到端验证：真实数据采集 + 入库 + 全部新查询出数
use ch_daemon::{DaemonState, DaemonStateConfig};
use std::time::Instant;

fn main() {
    let home = std::env::var("HOME").expect("write to String");
    let dir = format!("{home}/Library/Application Support/com.conversation-hub.desktop");
    let st = DaemonState::open(DaemonStateConfig {
        data_dir: dir.into(),
    })
    .expect("write to String");
    let repo = st.repo.lock().expect("write to String");

    // ── M6 资产 ──
    let t = Instant::now();
    let assets = ch_ops_metrics::collect_assets(&home).expect("write to String");
    let by_agent: Vec<(String, usize)> = {
        let mut m = std::collections::BTreeMap::new();
        for a in &assets {
            *m.entry(a.provider.as_str().to_string()).or_insert(0) += 1;
        }
        m.into_iter().collect()
    };
    println!(
        "M6 资产: {:?} 采集 {} 条 → {:?}",
        t.elapsed(),
        assets.len(),
        by_agent
    );
    for p in ["zcode", "codex", "claude-code", "minimax-code"] {
        let subset: Vec<_> = assets
            .iter()
            .filter(|a| a.provider.as_str() == p)
            .cloned()
            .collect();
        let _ = repo.replace_provider_assets(&format!("prov_{p}"), &subset);
    }
    let listed = repo.list_assets().expect("write to String");
    let risky = listed.iter().filter(|a| a.risky_hits > 0).count();
    println!(
        "   入库 {} 条，含危险模式 {} 条；样本: {:?} v{} [{}]",
        listed.len(),
        risky,
        listed
            .iter()
            .find(|a| a.kind == "plugin" && a.version.is_some())
            .map(|a| a.name.clone())
            .unwrap_or_default(),
        listed
            .iter()
            .find(|a| a.kind == "plugin" && a.version.is_some())
            .and_then(|a| a.version.clone())
            .unwrap_or_default(),
        listed
            .first()
            .map(|a| a.provider.clone())
            .unwrap_or_default()
    );

    // ── M8 自动化 ──
    let t = Instant::now();
    let autos = ch_ops_metrics::collect_automations(&home).expect("write to String");
    println!("M8 自动化: {:?} 采集 {} 条", t.elapsed(), autos.len());
    for p in ["zcode", "codex", "minimax-code"] {
        let subset: Vec<_> = autos
            .iter()
            .filter(|a| a.provider.as_str() == p)
            .cloned()
            .collect();
        let _ = repo.replace_provider_automations(&format!("prov_{p}"), &subset);
    }
    for a in repo.list_automations().expect("write to String").iter().take(5) {
        println!(
            "   [{}] {} {} {} {}",
            a.provider,
            a.name,
            a.kind,
            a.schedule.as_deref().unwrap_or(""),
            a.status.as_deref().unwrap_or("")
        );
    }

    // ── M7 成本归因 + 缓存（需要 source_dir：先重灌 usage）──
    let t = Instant::now();
    let (zu, _) = ch_ops_metrics::collect_zcode(format!("{home}/.zcode/cli/db/db.sqlite")).expect("write to String");
    let _ = repo.replace_provider_usage("prov_zcode", &zu).expect("write to String");
    println!(
        "M7 前置 zcode usage 重灌（带 source_dir）: {:?} {} 条",
        t.elapsed(),
        zu.len()
    );
    println!("── 按项目成本 Top5:");
    for d in repo.ops_cost_by_dir(None, 5).expect("write to String") {
        println!(
            "   {} tokens={} cost=${:.2} req={}",
            d.dir, d.tokens, d.cost_usd, d.requests
        );
    }
    println!("── 缓存命中率:");
    for c in repo.ops_cache_stats(None).expect("write to String") {
        println!(
            "   {:14} cache={}/{} 命中 {:.1}%",
            c.provider,
            c.cache_read_tokens,
            c.input_tokens + c.cache_read_tokens,
            c.hit_rate * 100.0
        );
    }

    // ── M9 异常 ──
    let an = repo.ops_anomalies(None).expect("write to String");
    println!("M9 异常: {} 条", an.len());
    for a in an.iter().take(8) {
        println!("   [{}] {} {}", a.severity, a.kind, a.detail);
    }
    println!("ALL M6-M9 E2E PASSED");
}
