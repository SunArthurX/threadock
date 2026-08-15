//! 性能基准数据生成器与测量工具，对应 plan §7.2「性能目标」。
//!
//! 用法：`cargo test --release -p ch-benchmarks --test perf -- --nocapture --ignored`
//!
//! plan §7.2 目标：
//! - 100k Conversation 搜索 P95 < 300ms
//! - 本地导入吞吐 > 500 Message/s
//! - 冷启动 P95 < 2.5s（不含 Tauri UI）

use ch_domain::{Conversation, Message, Provider, Role, Workspace};
use ch_storage::Repository;

/// 生成 N 条会话，每条含 M 条消息，写入指定 Repository。
///
/// 返回（耗时_ms, 消息总数, 会话总数）。
pub fn seed_conversations(
    repo: &Repository,
    n_conversations: usize,
    messages_per_conv: usize,
) -> (u128, usize, usize) {
    repo.upsert_provider(Provider::Generic).expect("upsert failed");
    let ws_id = repo.upsert_workspace(&Workspace::new("bench-ws")).expect("upsert failed");

    let start = std::time::Instant::now();
    let mut total_messages = 0;
    for i in 0..n_conversations {
        let mut conv = Conversation::new(Provider::Generic, format!("bench-conv-{i}"));
        conv.workspace_id = Some(ws_id.clone());
        conv.title = Some(format!("会话 {i}：Rust Tauri Android 性能测试"));
        let cid = repo.upsert_conversation(&conv).expect("upsert failed");

        for j in 0..messages_per_conv {
            let role = if j % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            };
            let mut m = Message::new(&cid, role, (j + 1) as i64);
            m.content_text = Some(format!(
                "消息 {i}-{j}：这是一段用于性能测试的内容，讨论 Tauri Android 后台任务和 Rust 错误处理。关键词：WorkManager、thiserror、cargo build。"
            ));
            repo.upsert_message(&m).expect("upsert failed");
            total_messages += 1;
        }
    }
    let elapsed = start.elapsed().as_millis();
    (elapsed, total_messages, n_conversations)
}

/// 测量 FTS5 搜索延迟（多次取 P95）。
///
/// 返回（P95_ms, 查询次数）。
pub fn bench_fts5_search(repo: &Repository, queries: &[&str], rounds: usize) -> (f64, usize) {
    use ch_storage::SearchQuery;
    let mut latencies = Vec::new();
    let mut count = 0;
    for _ in 0..rounds {
        for q in queries {
            let start = std::time::Instant::now();
            let _ = repo.search(&SearchQuery::new(*q)).expect("SQL execution failed");
            latencies.push(start.elapsed().as_secs_f64() * 1000.0);
            count += 1;
        }
    }
    let p95 = percentile(&latencies, 95.0);
    (p95, count)
}

/// 测量 Tantivy 搜索延迟（多次取 P95）。
pub fn bench_tantivy_search(
    index: &ch_search::SearchIndex,
    queries: &[&str],
    rounds: usize,
) -> (f64, usize) {
    let mut latencies = Vec::new();
    let mut count = 0;
    for _ in 0..rounds {
        for q in queries {
            let start = std::time::Instant::now();
            let query = ch_search::SearchQuery::new(*q);
            let _ = index.search(&query);
            latencies.push(start.elapsed().as_secs_f64() * 1000.0);
            count += 1;
        }
    }
    let p95 = percentile(&latencies, 95.0);
    (p95, count)
}

/// 计算百分位数。
pub fn percentile(data: &[f64], p: f64) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut sorted = data.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("unexpected None"));
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
