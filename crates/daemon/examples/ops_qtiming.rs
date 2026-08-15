//! 治理页 6 个查询的 DEBUG 耗时实测（与应用同构建模式）
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

    let t = Instant::now();
    let _ = repo.ops_overview(Some(30)).expect("write to String");
    println!("ops_overview(30):    {:?}", t.elapsed());

    let t = Instant::now();
    let _ = repo.ops_by_provider(Some(30)).expect("write to String");
    println!("ops_by_provider:     {:?}", t.elapsed());

    let t = Instant::now();
    let _ = repo.ops_by_model(Some(30)).expect("write to String");
    println!("ops_by_model:        {:?}", t.elapsed());

    let t = Instant::now();
    let _ = repo.ops_timeseries_daily(Some(30)).expect("write to String");
    println!("ops_timeseries:      {:?}", t.elapsed());

    let t = Instant::now();
    let _ = repo.ops_tool_toplist(Some(30), 10).expect("write to String");
    println!("ops_tool_toplist:    {:?}", t.elapsed());

    let t = Instant::now();
    let _ = repo.ops_risky_calls(Some(30), 50).expect("write to String");
    println!("ops_risky_calls:     {:?}", t.elapsed());

    let t = Instant::now();
    let _ = repo.ops_month_usage_since(0).expect("write to String");
    println!("ops_month_usage:     {:?}", t.elapsed());
}
