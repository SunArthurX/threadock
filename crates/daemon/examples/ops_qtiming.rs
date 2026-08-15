//! 治理页 6 个查询的 DEBUG 耗时实测（与应用同构建模式）
use ch_daemon::{DaemonState, DaemonStateConfig};
use std::time::Instant;

fn main() {
    let home = std::env::var("HOME").unwrap();
    let dir = format!("{home}/Library/Application Support/com.conversation-hub.desktop");
    let st = DaemonState::open(DaemonStateConfig {
        data_dir: dir.into(),
    })
    .unwrap();
    let repo = st.repo.lock().unwrap();

    let t = Instant::now();
    let _ = repo.ops_overview(Some(30)).unwrap();
    println!("ops_overview(30):    {:?}", t.elapsed());

    let t = Instant::now();
    let _ = repo.ops_by_provider(Some(30)).unwrap();
    println!("ops_by_provider:     {:?}", t.elapsed());

    let t = Instant::now();
    let _ = repo.ops_by_model(Some(30)).unwrap();
    println!("ops_by_model:        {:?}", t.elapsed());

    let t = Instant::now();
    let _ = repo.ops_timeseries_daily(Some(30)).unwrap();
    println!("ops_timeseries:      {:?}", t.elapsed());

    let t = Instant::now();
    let _ = repo.ops_tool_toplist(Some(30), 10).unwrap();
    println!("ops_tool_toplist:    {:?}", t.elapsed());

    let t = Instant::now();
    let _ = repo.ops_risky_calls(Some(30), 50).unwrap();
    println!("ops_risky_calls:     {:?}", t.elapsed());

    let t = Instant::now();
    let _ = repo.ops_month_usage_since(0).unwrap();
    println!("ops_month_usage:     {:?}", t.elapsed());
}
