//! 性能基准测试，对应 plan §7.2。
//!
//! 运行：`cargo test --release -p ch-benchmarks --test perf -- --nocapture --ignored`
//!
//! 这些测试标记为 #[ignore]，因为它们需要较长时间和数据量。
//! 普通 `cargo test` 不会跑它们。

use ch_benchmarks::{bench_fts5_search, bench_tantivy_search, percentile, seed_conversations};
use ch_search::index::IndexableMessage;
use ch_storage::{Repository, SearchQuery};
use tempfile::TempDir;

/// plan §7.2：本地导入吞吐 > 500 Message/s
#[test]
#[ignore = "基准测试耗时较长，需手动 --ignored 运行"]
fn import_throughput() {
    let dir = TempDir::new().expect("tempdir creation failed");
    let repo = Repository::open(dir.path().join("bench.db")).expect("unexpected None");

    // 500 会话 × 10 消息 = 5000 消息
    let (elapsed_ms, total_msgs, n_conv) = seed_conversations(&repo, 500, 10);
    let throughput = total_msgs as f64 / (elapsed_ms as f64 / 1000.0);

    println!("┌──────────────────────────────────────────┐");
    println!("│ 导入吞吐基准（plan §7.2 目标 > 500 msg/s）│");
    println!("├──────────────────────────────────────────┤");
    println!("│ 会话数:   {n_conv:>10}                      │");
    println!("│ 消息数:   {total_msgs:>10}                      │");
    println!("│ 耗时:     {elapsed_ms:>7} ms                    │");
    println!("│ 吞吐:   {throughput:>10.0} msg/s                │");
    println!("│ 目标:       > 500 msg/s                │");
    println!(
        "│ 结果:     {}",
        if throughput > 500.0 {
            "✓ PASS"
        } else {
            "✗ FAIL"
        }
    );
    println!("└──────────────────────────────────────────┘");

    assert!(
        throughput > 500.0,
        "吞吐 {throughput:.0} msg/s 低于目标 500"
    );
}

/// plan §7.2：搜索 P95 < 300ms（FTS5）
#[test]
#[ignore = "基准测试耗时较长，需手动 --ignored 运行"]
fn fts5_search_latency() {
    let dir = TempDir::new().expect("tempdir creation failed");
    let repo = Repository::open(dir.path().join("bench.db")).expect("unexpected None");

    // 生成 1000 会话 × 5 消息 = 5000 消息
    seed_conversations(&repo, 1000, 5);

    let queries = ["tauri", "android", "WorkManager", "cargo build", "错误处理"];
    let (p95, count) = bench_fts5_search(&repo, &queries, 10);

    println!("┌──────────────────────────────────────────────┐");
    println!("│ FTS5 搜索延迟（plan §7.2 目标 P95 < 300ms）   │");
    println!("├──────────────────────────────────────────────┤");
    println!("│ 查询次数: {count:>10}                          │");
    println!("│ P95 延迟: {p95:>10.1} ms                       │");
    println!("│ 目标:        < 300 ms                         │");
    println!(
        "│ 结果:     {}",
        if p95 < 300.0 { "✓ PASS" } else { "✗ FAIL" }
    );
    println!("└──────────────────────────────────────────────┘");

    assert!(p95 < 300.0, "P95 {p95:.1}ms 超过目标 300ms");
}

/// Tantivy 搜索延迟基准（无硬性目标，记录用）
#[test]
#[ignore = "基准测试耗时较长，需手动 --ignored 运行"]
fn tantivy_search_latency() {
    let dir = TempDir::new().expect("tempdir creation failed");
    let repo = Repository::open(dir.path().join("bench.db")).expect("unexpected None");
    let index = ch_search::SearchIndex::open(dir.path().join("idx")).expect("unexpected None");

    // 生成数据 + 索引
    seed_conversations(&repo, 500, 5);
    let convs = repo.list_conversations(None).expect("unexpected None");
    let mut writer = index.writer(50_000_000).expect("file I/O failed");
    for c in &convs {
        let msgs = repo.list_messages(&c.id).expect("unexpected None");
        for m in &msgs {
            index
                .index_message(
                    &mut writer,
                    &IndexableMessage {
                        message_id: m.id.clone(),
                        conversation_id: c.id.clone(),
                        provider: c.provider,
                        workspace_id: c.workspace_id.clone(),
                        role: m.role,
                        title: c.title.clone(),
                        body: m.content_text.clone(),
                    },
                )
                .expect("unexpected None");
        }
    }
    index.commit(writer).expect("file I/O failed");

    let queries = ["tauri", "android", "WorkManager", "错误"];
    let (p95, count) = bench_tantivy_search(&index, &queries, 10);

    println!("┌──────────────────────────────────────────────┐");
    println!("│ Tantivy 搜索延迟（记录用，无硬性目标）         │");
    println!("├──────────────────────────────────────────────┤");
    println!("│ 查询次数: {count:>10}                          │");
    println!("│ P95 延迟: {p95:>10.1} ms                       │");
    println!("└──────────────────────────────────────────────┘");
}

/// plan §7.2：冷启动 < 2.5s（打开数据库 + migration）
#[test]
#[ignore = "基准测试耗时较长，需手动 --ignored 运行"]
fn cold_start() {
    let dir = TempDir::new().expect("tempdir creation failed");
    // 先建库
    let _ = Repository::open(dir.path().join("bench.db")).expect("unexpected None");

    // 测量重新打开的冷启动时间
    let mut latencies = Vec::new();
    for _ in 0..10 {
        let start = std::time::Instant::now();
        let _repo = Repository::open(dir.path().join("bench.db")).expect("unexpected None");
        latencies.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let p95 = percentile(&latencies, 95.0);

    println!("┌──────────────────────────────────────────────┐");
    println!("│ 冷启动（plan §7.2 目标 P95 < 2500ms）         │");
    println!("├──────────────────────────────────────────────┤");
    println!("│ P95 延迟: {p95:>10.1} ms                       │");
    println!("│ 目标:        < 2500 ms                        │");
    println!(
        "│ 结果:     {}",
        if p95 < 2500.0 { "✓ PASS" } else { "✗ FAIL" }
    );
    println!("└──────────────────────────────────────────────┘");

    assert!(p95 < 2500.0, "冷启动 P95 {p95:.1}ms 超过目标 2500ms");
}

/// 非忽略的快速冒烟测试：验证基准设施可用
#[test]
fn bench_smoke() {
    let dir = TempDir::new().expect("tempdir creation failed");
    let repo = Repository::open(dir.path().join("smoke.db")).expect("unexpected None");
    let (elapsed, msgs, convs) = seed_conversations(&repo, 5, 2);
    assert_eq!(convs, 5);
    assert_eq!(msgs, 10);
    assert!(elapsed < 5000, "小规模 seed 不应超过 5 秒");

    // FTS5 能搜到
    let results = repo
        .search(&SearchQuery::new("tauri"))
        .expect("SQL execution failed");
    assert!(!results.is_empty());

    let p = percentile(&[1.0, 2.0, 3.0, 4.0, 5.0], 50.0);
    assert!((p - 3.0).abs() < 0.01);
}

/// Gate 1 红线（plan §1.4/Phase 2 验收）：**10 万会话**规模下搜索 P95 < 300ms。
/// 与上面的小规模用例分开：seed 100k 会话耗时数分钟，仅在发布前跑。
/// 运行：`cargo test --release -p ch-benchmarks --test perf large_scale -- --ignored --nocapture`
#[test]
#[ignore = "10 万会话 seed 需数分钟，仅发布前验证"]
fn large_scale_search_gate1() {
    let dir = TempDir::new().expect("tempdir creation failed");
    let repo = Repository::open(dir.path().join("bench-large.db")).expect("unexpected None");

    // 20000 会话 × 5 消息 = 10 万消息；会话数按 Gate 1 的 100k 会话口径再乘 5 批
    // （用 20k×5 批循环写满 100k 会话，避免单次内存峰值）
    let start = std::time::Instant::now();
    let mut total_msgs = 0usize;
    const BATCHES: usize = 5;
    for _ in 0..BATCHES {
        let (_ms, msgs, _n) = seed_conversations(&repo, 20_000, 5);
        total_msgs += msgs;
    }
    let seed_secs = start.elapsed().as_secs_f64();
    let n_conv = repo
        .list_conversations(None)
        .expect("unexpected None")
        .len();

    let queries = [
        "tauri",
        "android",
        "WorkManager",
        "cargo build",
        "错误处理",
        "zzznomatch",
    ];
    let (p95, count) = bench_fts5_search(&repo, &queries, 10);

    println!("┌──────────────────────────────────────────────────┐");
    println!("│ Gate 1 大规模基准：100k 会话 FTS5 P95 < 300ms     │");
    println!("├──────────────────────────────────────────────────┤");
    println!("│ 会话数:   {n_conv:>10}                              ");
    println!("│ 消息数:   {total_msgs:>10}                              ");
    println!("│ seed 耗时: {seed_secs:>8.1} s                        ");
    println!("│ 查询次数: {count:>10}                                ");
    println!("│ P95 延迟: {p95:>10.1} ms                             ");
    println!(
        "│ 结果:     {}",
        if p95 < 300.0 { "✓ PASS" } else { "✗ FAIL" }
    );
    println!("└──────────────────────────────────────────────────┘");

    assert_eq!(n_conv, 100_000, "必须真达到 10 万会话规模");
    assert!(p95 < 300.0, "Gate 1：P95 {p95:.1}ms 超过 300ms");
}
