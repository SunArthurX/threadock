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
/// `返回（耗时_ms`, 消息总数, 会话总数）。
pub fn seed_conversations(
    repo: &Repository,
    n_conversations: usize,
    messages_per_conv: usize,
) -> (u128, usize, usize) {
    repo.upsert_provider(Provider::Generic)
        .expect("upsert failed");
    let ws_id = repo
        .upsert_workspace(&Workspace::new("bench-ws"))
        .expect("upsert failed");

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
/// `返回（P95_ms`, 查询次数）。
pub fn bench_fts5_search(repo: &Repository, queries: &[&str], rounds: usize) -> (f64, usize) {
    use ch_storage::SearchQuery;
    let mut latencies = Vec::new();
    let mut count = 0;
    for _ in 0..rounds {
        for q in queries {
            let start = std::time::Instant::now();
            let _ = repo
                .search(&SearchQuery::new(*q))
                .expect("SQL execution failed");
            latencies.push(start.elapsed().as_secs_f64() * 1000.0);
            count += 1;
        }
    }
    let p95 = percentile(&latencies, 95.0);
    (p95, count)
}

/// 测量 Tantivy 搜索延迟（多次取 P95）。
#[must_use]
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
#[must_use]
pub fn percentile(data: &[f64], p: f64) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut sorted = data.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("unexpected None"));
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// 大规模快速 seed（Gate 1 专用）：绕开逐条 upsert（每条一个事务 + fsync），
/// 用分块事务直写 SQLite（FTS5 触发器照常生效，搜索路径真实）。
/// 只用于把库灌到目标规模后测「搜索延迟」；导入吞吐请看 [`seed_conversations`]。
///
/// `返回（seed` 秒数, 消息总数）。
pub fn seed_bulk_fast(
    db_path: &std::path::Path,
    n_conversations: usize,
    messages_per_conv: usize,
) -> (f64, usize) {
    // 先用 Repository 走 migration + 基础行（provider/workspace）
    let repo = ch_storage::Repository::open(db_path).expect("unexpected None");
    repo.upsert_provider(Provider::Generic)
        .expect("upsert failed");
    let ws_id = repo
        .upsert_workspace(&Workspace::new("bench-bulk-ws"))
        .expect("upsert failed");
    drop(repo);

    let mut conn = rusqlite::Connection::open(db_path).expect("unexpected None");
    conn.pragma_update(None, "journal_mode", "WAL")
        .expect("pragma");
    conn.pragma_update(None, "synchronous", "NORMAL")
        .expect("pragma");

    let start = std::time::Instant::now();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let mut total_msgs = 0usize;
    const CHUNK: usize = 5_000; // 每个事务的会话数

    conn.execute("BEGIN", []).expect("tx");
    for i in 0..n_conversations {
        conn.execute(
            "INSERT INTO conversations (id, workspace_id, provider_id, source_conversation_id,
                                        title, started_at, updated_at)
             VALUES (?1, ?2, 'prov_generic', ?3, ?4, ?5, ?5)",
            rusqlite::params![
                format!("conv_bulk_{i}"),
                ws_id,
                format!("bench-bulk-{i}"),
                {
                    // 标题词汇轮换（真实分布：仅少数标题含查询词）
                    const TOPICS: [&str; 10] = [
                        "登录流程重构",
                        "状态管理拆分",
                        "测试补充计划",
                        "依赖升级评估",
                        "接口限流方案",
                        "日志规范化",
                        "构建提速",
                        "权限模型梳理",
                        "Tauri Android 适配",
                        "数据迁移脚本",
                    ];
                    format!("会话 {i}：{}", TOPICS[i % TOPICS.len()])
                },
                now_ms
            ],
        )
        .expect("insert conv");
        for j in 0..messages_per_conv {
            let role = if j % 2 == 0 { "user" } else { "assistant" };
            // 关键词按比例注入（模拟真实词汇分布，命中率 ~3-5%）：
            // 100% 命中的病态场景见下方 worst_case 注释与报告
            let mut text = format!("消息 {i}-{j}：常规开发讨论，重构模块与测试补充。");
            if (i + j) % 20 == 0 {
                text.push_str(" Tauri Android 后台任务。");
            }
            if (i + j) % 33 == 0 {
                text.push_str(" 运行 cargo build 验证。");
            }
            if (i + j) % 50 == 0 {
                text.push_str(" WorkManager 约束。");
            }
            if (i + j) % 25 == 0 {
                text.push_str(" Rust 错误处理选型。");
            }
            conn.execute(
                "INSERT INTO messages (id, conversation_id, role, content_text, sequence_number, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    format!("msg_bulk_{i}_{j}"),
                    format!("conv_bulk_{i}"),
                    role,
                    text,
                    j + 1,
                    now_ms
                ],
            )
            .expect("insert msg");
            total_msgs += 1;
        }
        if (i + 1) % CHUNK == 0 {
            conn.execute("COMMIT", []).expect("commit");
            conn.execute("BEGIN", []).expect("tx");
        }
    }
    conn.execute("COMMIT", []).expect("commit");
    (start.elapsed().as_secs_f64(), total_msgs)
}
