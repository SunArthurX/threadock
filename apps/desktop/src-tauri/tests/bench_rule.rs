//! 临时基准（不进 CI）：最大会话的规则提取耗时。
//! cargo test --lib bench_rule_extract -- --ignored --nocapture
use ch_daemon::{DaemonState, DaemonStateConfig};

#[test]
#[ignore = "读本机真实库副本"]
fn bench_rule_extract() {
    let app = std::path::PathBuf::from(std::env::var("HOME").expect("no HOME"))
        .join("Library/Application Support/com.threadock.desktop");
    assert!(app.join("threadock.db").exists(), "无真实数据");
    let dir = tempfile::TempDir::new().expect("tempdir");
    std::fs::copy(app.join("threadock.db"), dir.path().join("threadock.db")).expect("copy");
    let state = DaemonState::open(DaemonStateConfig {
        data_dir: dir.path().to_path_buf(),
        ..Default::default()
    })
    .expect("open");
    let repo = state.repo.lock().expect("mutex");
    let convs = repo.list_conversations(None).expect("convs");
    let mut worst: Option<(usize, String)> = None;
    for c in &convs {
        let n = repo.list_messages(&c.id).map(|m| m.len()).unwrap_or(0);
        if n > worst.as_ref().map_or(0, |(bn, _)| *bn) {
            worst = Some((n, c.id.clone()));
        }
    }
    // 全库逐会话提取，报最坏 top5（弹窗「无存档现场提取」路径的真实上界）
    let mut times: Vec<(std::time::Duration, String)> = Vec::new();
    for c in &convs {
        let msgs = repo.list_messages(&c.id).expect("msgs");
        let events = repo.list_events(&c.id).unwrap_or_default();
        let title = c.effective_title().to_string();
        let input = ch_knowledge::ExtractionInput {
            title: Some(title.clone()),
            messages: msgs,
            events,
        };
        let t = std::time::Instant::now();
        let _ = ch_knowledge::RuleExtractor::new().extract(&input);
        times.push((t.elapsed(), title));
    }
    times.sort_by_key(|(d, _)| std::cmp::Reverse(*d));
    let total: std::time::Duration = times.iter().map(|(d, _)| *d).sum();
    println!("全库 {} 会话总耗时 {:?}，最坏 top5：", times.len(), total);
    for (d, title) in times.iter().take(5) {
        println!("  {:?} · {}", d, title);
    }
}
