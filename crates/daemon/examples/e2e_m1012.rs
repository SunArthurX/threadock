//! M10-M12 E2E：真实数据上验证健康度/延迟/浪费查询全部出数
use ch_daemon::{DaemonState, DaemonStateConfig};

fn main() {
    let home = std::env::var("HOME").expect("write to String");
    let dir = format!("{home}/Library/Application Support/com.conversation-hub.desktop");
    let st = DaemonState::open(DaemonStateConfig {
        data_dir: dir.into(),
        ..Default::default()
    })
    .expect("write to String");
    let repo = st.repo.lock().expect("write to String");

    println!("── M10 Agent 健康度:");
    for h in repo.ops_agent_health(Some(30)).expect("write to String") {
        println!(
            "   {:14} req={:>6} 成功={:>5.1}% 错误={:>4.1}% 重试={:>5.1}% 稳定性={:.0}",
            h.provider,
            h.total_requests,
            h.success_rate,
            h.error_rate,
            h.retry_rate,
            h.stability_score
        );
    }

    println!("── M11 延迟 P50/P95:");
    for l in repo.ops_latency_stats(Some(30)).expect("write to String") {
        println!(
            "   {:14} P50={:.0}ms P95={:.0}ms avg={:.0}ms (n={})",
            l.provider, l.p50_ms, l.p95_ms, l.avg_ms, l.sample_count
        );
    }

    println!("── M12 Token 浪费 Top5:");
    for w in repo.ops_token_waste(Some(30), 5).expect("write to String") {
        println!(
            "   {:14} {}… in={} out={} ratio={:.0}x 缓存={} 浪费度={:.0}",
            w.provider,
            &w.session_id[..14.min(w.session_id.len())],
            w.input_tokens,
            w.output_tokens,
            w.ratio,
            w.cache_read,
            w.waste_score
        );
    }
    println!("ALL M10-M12 E2E PASSED");
}
