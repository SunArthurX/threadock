//! M13-M15 E2E：横向对比 + 周报 + 时间线数据验证
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

    println!("── M13 Agent 横向对比:");
    for b in repo.ops_agent_benchmark(Some(30)).expect("write to String") {
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
    let s = repo.ops_weekly_summary().expect("write to String");
    println!(
        "   7天: 请求={} tokens={} 成本=${:.2} 浪费会话={}",
        s.overview.total_requests, s.overview.total_tokens, s.overview.cost_usd, s.waste_sessions
    );
    println!(
        "   Agent 数: {} 健康数据: {} 条",
        s.benchmark.len(),
        s.health.len()
    );

    let convs = repo.list_conversations(None).expect("write to String");
    if let Some(conv) = convs.first() {
        let msgs = repo.list_messages(&conv.id).expect("write to String");
        let evts = repo.list_events(&conv.id).expect("write to String");
        println!(
            "── M15 时间线: 「{}」消息={} 事件={}",
            conv.effective_title().chars().take(30).collect::<String>(),
            msgs.len(),
            evts.len()
        );
    }
    println!("ALL M13-M15 E2E PASSED");
}
