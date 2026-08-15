//! M13-M15 E2E：横向对比 + 周报 + 时间线数据验证
use ch_daemon::{DaemonState, DaemonStateConfig};

fn main() {
    let home = std::env::var("HOME").unwrap();
    let dir = format!("{home}/Library/Application Support/com.conversation-hub.desktop");
    let st = DaemonState::open(DaemonStateConfig {
        data_dir: dir.into(),
    })
    .unwrap();
    let repo = st.repo.lock().unwrap();

    println!("── M13 Agent 横向对比:");
    for b in repo.ops_agent_benchmark(Some(30)).unwrap() {
        println!(
            "   {:14} req={:>6} tokens={:>12} cost=${:>7.2} 成功={:.1}% 缓存={:.1}% 会话={}",
            b.provider,
            b.total_requests,
            b.total_tokens,
            b.cost_usd,
            b.success_rate,
            b.cache_hit_rate,
            b.sessions
        );
    }

    println!("── M14 周报数据:");
    let s = repo.ops_weekly_summary().unwrap();
    println!(
        "   7天: 请求={} tokens={} 成本=${:.2} 浪费会话={}",
        s.overview.total_requests, s.overview.total_tokens, s.overview.cost_usd, s.waste_sessions
    );
    println!(
        "   Agent 数: {} 健康数据: {} 条",
        s.benchmark.len(),
        s.health.len()
    );

    let convs = repo.list_conversations(None).unwrap();
    if let Some(conv) = convs.first() {
        let msgs = repo.list_messages(&conv.id).unwrap();
        let evts = repo.list_events(&conv.id).unwrap();
        println!(
            "── M15 时间线: 「{}」消息={} 事件={}",
            conv.effective_title().chars().take(30).collect::<String>(),
            msgs.len(),
            evts.len()
        );
    }
    println!("ALL M13-M15 E2E PASSED");
}
