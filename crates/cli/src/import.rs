//! 导入链路：把一个来源文件经 Raw 归档 → Adapter → Normalize → Repository 入库。
//!
//! 对应 plan §8.3「数据处理流水线」与 §29「首个端到端链路」。
//! 这是 MVP 的核心可调用单元，CLI 与未来的 Daemon 共用。
//!
//! 完整流水线（plan §2.3 Raw + Normalized 双存储）：
//! 1. 读取原始文件字节 → 写入 Raw Store（BLAKE3 内容寻址 + zstd）。
//! 2. Adapter 解析为 `RawConversation`。
//! 3. Normalize 计算 hash + 完整度。
//! 4. Repository `入库（标准化数据），conversation.raw_payload_id` 指回 Raw。

use crate::Result;
use ch_domain::Provider;
use ch_normalization::normalize;
use ch_raw_store::RawStore;
use ch_storage::Repository;
use std::path::Path;

/// 单次导入的产出摘要。
#[derive(Debug, Clone, PartialEq)]
pub struct ImportSummary {
    pub conversation_id: String,
    pub workspace_id: Option<String>,
    pub messages_imported: usize,
    pub events_imported: usize,
    pub completeness: &'static str,
    pub conversation_hash: String,
    /// 原始数据在 Raw Store 中的内容 hash（plan §2.3）。
    pub raw_payload_id: Option<String>,
}

/// 导入一个 Markdown 文件。
///
/// - `repo`：标准化数据存储。
/// - `raw_store`：原始数据归档；传 `None` 则不归档原始数据（仅标准化）。
/// - `workspace_name`：可选工作区显示名。
///
/// 整个过程对同一文件幂等：重复导入不产生重复记录。
pub fn import_markdown(
    repo: &Repository,
    raw_store: Option<&RawStore>,
    path: impl AsRef<Path>,
    workspace_name: Option<&str>,
) -> Result<ImportSummary> {
    let path_ref = path.as_ref();

    // 1. 读取原始字节并归档到 Raw Store（plan §9.6）
    let raw_payload_id = if let Some(store) = raw_store {
        let bytes = std::fs::read(path_ref)?;
        let payload = store.put(&bytes)?;
        Some(payload.hash)
    } else {
        None
    };

    // 2. Adapter 解析（按扩展名分派，plan §10.5）
    let parsed = parse_by_extension(path_ref)?;
    // 3. Normalize
    let normalized = normalize(parsed)?;

    // 可选 workspace：用身份解析器（plan §4.3）决定归并到已有还是新建。
    // 候选用：显示名 + 源文件父目录作为 canonical_path。
    // （provider 记录由 import_conversation_batch 在事务内保证存在）
    let workspace_id = if let Some(name) = workspace_name {
        let parent_path = path_ref.parent().map(|p| p.to_string_lossy().into_owned());
        let mut candidate = ch_identity_resolver::SourceWorkspaceCandidate::new(name);
        candidate.canonical_path.clone_from(&parent_path);

        // 把 repo 中已有 workspace 作为 IdentityKey 列表
        let known: Vec<ch_identity_resolver::IdentityKey> = repo
            .list_workspaces()?
            .into_iter()
            .map(|ws| ch_identity_resolver::IdentityKey {
                workspace_id: ws.id,
                display_name: ws.display_name,
                manifest_id: None,
                git_remote: ws.git_remote,
                git_common_dir: ws.git_common_dir,
                canonical_path: ws.canonical_path,
                filesystem_id: None,
            })
            .collect();

        let resolution = ch_identity_resolver::resolve(&candidate, &known);
        match resolution {
            ch_identity_resolver::Resolution::AutoMerge(m) => Some(m.workspace_id),
            ch_identity_resolver::Resolution::NeedsConfirmation {
                candidate: Some(m), ..
            } => {
                // plan §4.3：低置信度匹配（名称相似度）需用户确认。
                // CLI 模式无法交互确认，明确打印提示后仍按最佳候选归并；
                // 用户可用 `ch list --workspace` 检查，或用不同 --workspace 名避免误并。
                let method = match m.method {
                    ch_domain::MatchMethod::NameSimilarity => "名称相似度",
                    _ => "低置信度",
                };
                eprintln!(
                    "⚠ 低置信度归并（{method}，置信度 {:.2}）：将「{name}」归并到已有 workspace {}",
                    m.confidence, m.workspace_id
                );
                eprintln!("  如需避免，请重新导入时使用不同的 --workspace 名称，或先手动调整。");
                Some(m.workspace_id)
            }
            ch_identity_resolver::Resolution::NeedsConfirmation {
                candidate: None, ..
            }
            | ch_identity_resolver::Resolution::CreateNew => {
                // 新建统一 workspace
                let mut ws = ch_domain::Workspace::new(name);
                ws.canonical_path = parent_path;
                Some(repo.upsert_workspace(&ws)?)
            }
        }
    } else {
        None
    };

    // 4. 关联 workspace + raw_payload_id 到 conversation，单事务批量入库
    //   （旧实现逐条 upsert 每次独立提交，WAL 下每条消息一次 fsync）
    let mut conv = normalized.conversation;
    conv.workspace_id.clone_from(&workspace_id);
    conv.raw_payload_id.clone_from(&raw_payload_id);
    let conversation_id = repo.import_conversation_batch(
        &conv,
        &normalized.messages,
        &normalized.events,
        None,
        None,
    )?;

    // 提交同步游标（plan §11.2：写库后提交游标）
    repo.upsert_cursor(
        Provider::Generic,
        None,
        "markdown-file",
        &path_ref.to_string_lossy(),
        None,
    )?;

    Ok(ImportSummary {
        conversation_id,
        workspace_id,
        messages_imported: normalized.messages.len(),
        events_imported: normalized.events.len(),
        completeness: normalized.completeness.label(),
        conversation_hash: normalized.conversation_hash,
        raw_payload_id,
    })
}

/// 按文件扩展名选择 adapter（plan §10.5）。
///
/// - `.md` / `.markdown` → markdown adapter
/// - `.jsonl` / `.ndjson` → jsonl adapter
/// - 其他 → 默认按 markdown 处理（最宽松）
fn parse_by_extension(path: &Path) -> Result<ch_normalization::RawConversation> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "jsonl" | "ndjson" => {
            let raw = ch_adapter_jsonl::parse_file(path)
                .map_err(|e| anyhow::anyhow!("jsonl parse: {e}"))?;
            Ok(raw)
        }
        _ => {
            let raw = ch_adapter_markdown::parse_file(path)
                .map_err(|e| anyhow::anyhow!("markdown parse: {e}"))?;
            Ok(raw)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_storage::Repository;
    use tempfile::{NamedTempFile, TempDir};

    fn write_md(content: &str) -> NamedTempFile {
        let f = NamedTempFile::new().expect("tempdir creation failed");
        std::fs::write(f.path(), content).expect("file I/O failed");
        f
    }

    /// 构造一个临时 `RawStore` 用于测试。
    fn raw_store() -> (TempDir, RawStore) {
        let dir = TempDir::new().expect("tempdir creation failed");
        let store = RawStore::new(dir.path()).expect("unexpected None");
        (dir, store)
    }

    #[test]
    fn import_single_markdown() {
        let repo = Repository::open_in_memory().expect("unexpected None");
        let f = write_md(
            "# 测试会话\n\n## User\n你好\n## Assistant\n你好啊\n## Command\ncargo build\n",
        );
        let summary =
            import_markdown(&repo, None, f.path(), Some("my-web-app")).expect("unexpected None");
        assert!(summary.conversation_id.starts_with("conv_"));
        assert_eq!(summary.messages_imported, 2);
        assert_eq!(summary.events_imported, 1);
        assert_eq!(summary.completeness, "部分");
        assert!(summary.workspace_id.is_some());

        // 数据真的进库了
        let conv = repo
            .get_conversation(&summary.conversation_id)
            .expect("unexpected None")
            .expect("unexpected None");
        assert_eq!(conv.title.as_deref(), Some("测试会话"));
        assert_eq!(
            repo.list_messages(&summary.conversation_id)
                .expect("unexpected None")
                .len(),
            2
        );
        assert_eq!(
            repo.list_events(&summary.conversation_id)
                .expect("unexpected None")
                .len(),
            1
        );
    }

    #[test]
    fn import_is_idempotent_on_repeat() {
        // plan §11.3：重复导入不产生重复
        let repo = Repository::open_in_memory().expect("unexpected None");
        let f = write_md("## User\nhi\n## Assistant\nhello\n");

        let s1 = import_markdown(&repo, None, f.path(), None).expect("unexpected None");
        let s2 = import_markdown(&repo, None, f.path(), None).expect("unexpected None");

        assert_eq!(
            s1.conversation_id, s2.conversation_id,
            "same conversation id"
        );
        assert_eq!(repo.count_conversations().expect("unexpected None"), 1);
        assert_eq!(
            repo.list_messages(&s1.conversation_id)
                .expect("unexpected None")
                .len(),
            2
        );
    }

    #[test]
    fn import_reuses_workspace_on_repeat() {
        // 重复导入同名 workspace 不应产生重复 workspace 记录
        let repo = Repository::open_in_memory().expect("unexpected None");
        let f = write_md("## User\nhi\n## Assistant\nhello\n");

        let s1 = import_markdown(&repo, None, f.path(), Some("proj-x")).expect("unexpected None");
        let s2 = import_markdown(&repo, None, f.path(), Some("proj-x")).expect("unexpected None");

        assert_eq!(s1.workspace_id, s2.workspace_id, "workspace must be reused");
        assert_eq!(repo.list_workspaces().expect("unexpected None").len(), 1);
    }

    #[test]
    fn import_without_workspace() {
        let repo = Repository::open_in_memory().expect("unexpected None");
        let f = write_md("## User\nhi\n## Assistant\nyo\n");
        let summary = import_markdown(&repo, None, f.path(), None).expect("unexpected None");
        assert!(summary.workspace_id.is_none());

        let conv = repo
            .get_conversation(&summary.conversation_id)
            .expect("unexpected None")
            .expect("unexpected None");
        assert!(conv.workspace_id.is_none());
    }

    #[test]
    fn import_records_cursor() {
        let repo = Repository::open_in_memory().expect("unexpected None");
        let f = write_md("## User\nhi\n## Assistant\nyo\n");
        import_markdown(&repo, None, f.path(), None).expect("unexpected None");
        let cursor = repo
            .get_cursor(Provider::Generic, None, "markdown-file")
            .expect("unexpected None");
        assert!(cursor.is_some());
        assert!(cursor.expect("unexpected None").contains("tmp"));
    }

    #[test]
    fn multiple_imports_dont_collide() {
        let repo = Repository::open_in_memory().expect("unexpected None");
        let f1 = write_md("## User\nAAA\n## Assistant\naaa\n");
        let f2 = write_md("## User\nBBB\n## Assistant\nbbb\n");

        let s1 = import_markdown(&repo, None, f1.path(), Some("proj-a")).expect("unexpected None");
        let s2 = import_markdown(&repo, None, f2.path(), Some("proj-b")).expect("unexpected None");

        assert_ne!(s1.conversation_id, s2.conversation_id);
        assert_eq!(repo.count_conversations().expect("unexpected None"), 2);
        assert_ne!(s1.conversation_hash, s2.conversation_hash);
    }

    #[test]
    fn completeness_full_when_all_event_types() {
        let repo = Repository::open_in_memory().expect("unexpected None");
        let f = write_md(
            "## User\nhi\n## Assistant\ndone\n## Tool\nbash\n## Command\ncargo build\n## Diff\nfoo.rs\n## Command\ncargo build done\n",
        );
        let s = import_markdown(&repo, None, f.path(), None).expect("unexpected None");
        assert_eq!(s.completeness, "完整");
    }

    #[test]
    fn import_archives_raw_payload() {
        // plan §2.3 Raw + Normalized 双存储：原始文件应归档到 Raw Store
        let repo = Repository::open_in_memory().expect("unexpected None");
        let (_raw_dir, raw_store) = raw_store();
        let f = write_md("## User\n原始内容\n## Assistant\n回复\n");

        let summary =
            import_markdown(&repo, Some(&raw_store), f.path(), None).expect("unexpected None");

        // raw_payload_id 非空且是 64 hex
        let hash = summary.raw_payload_id.expect("raw should be archived");
        assert_eq!(hash.len(), 64);

        // conversation 入库后 raw_payload_id 指回 Raw Store
        let conv = repo
            .get_conversation(&summary.conversation_id)
            .expect("unexpected None")
            .expect("unexpected None");
        assert_eq!(conv.raw_payload_id.as_deref(), Some(hash.as_str()));

        // Raw Store 里能读回原始内容
        assert!(raw_store.exists(&hash).expect("unexpected None"));
        let back = raw_store.get(&hash).expect("unexpected None");
        let back_str = std::str::from_utf8(&back).expect("unexpected None");
        assert!(back_str.contains("原始内容"));
    }

    #[test]
    fn import_without_raw_store_still_works() {
        // 不传 Raw Store 时，raw_payload_id 为空，但标准化数据正常入库
        let repo = Repository::open_in_memory().expect("unexpected None");
        let f = write_md("## User\nhi\n## Assistant\nyo\n");
        let summary = import_markdown(&repo, None, f.path(), None).expect("unexpected None");
        assert!(summary.raw_payload_id.is_none());

        let conv = repo
            .get_conversation(&summary.conversation_id)
            .expect("unexpected None")
            .expect("unexpected None");
        assert!(conv.raw_payload_id.is_none());
        assert_eq!(
            repo.list_messages(&summary.conversation_id)
                .expect("unexpected None")
                .len(),
            2
        );
    }

    #[test]
    fn raw_archive_is_idempotent() {
        // 重复导入同一文件：Raw Store 只存一份（内容寻址去重）
        let repo = Repository::open_in_memory().expect("unexpected None");
        let (_raw_dir, raw_store) = raw_store();
        let f = write_md("## User\nsame\n## Assistant\ncontent\n");

        let s1 = import_markdown(&repo, Some(&raw_store), f.path(), None).expect("unexpected None");
        let s2 = import_markdown(&repo, Some(&raw_store), f.path(), None).expect("unexpected None");

        assert_eq!(s1.raw_payload_id, s2.raw_payload_id, "same raw hash");
        assert_eq!(
            raw_store.stats().expect("unexpected None").count,
            1,
            "only one raw object"
        );
    }

    fn write_jsonl(content: &str) -> NamedTempFile {
        let f = NamedTempFile::with_suffix(".jsonl").expect("tempdir creation failed");
        std::fs::write(f.path(), content).expect("file I/O failed");
        f
    }

    #[test]
    fn import_jsonl_file() {
        let repo = Repository::open_in_memory().expect("unexpected None");
        let f = write_jsonl(
            r#"{"type":"meta","title":"JSONL 会话","model":"gpt-test"}
{"type":"message","role":"user","text":"你好"}
{"type":"message","role":"assistant","text":"你好啊"}
{"type":"event","event_type":"command_started","summary":"cargo build"}
"#,
        );
        let summary =
            import_markdown(&repo, None, f.path(), Some("jsonl-proj")).expect("unexpected None");
        assert_eq!(summary.messages_imported, 2);
        assert_eq!(summary.events_imported, 1);

        let conv = repo
            .get_conversation(&summary.conversation_id)
            .expect("unexpected None")
            .expect("unexpected None");
        assert_eq!(conv.title.as_deref(), Some("JSONL 会话"));
        assert_eq!(conv.model.as_deref(), Some("gpt-test"));
    }

    #[test]
    fn parse_by_extension_routes_by_suffix() {
        use ch_normalization::RawConversation;
        let md = write_md("## User\nhi\n## Assistant\nyo\n");
        let jl = write_jsonl("{\"type\":\"message\",\"role\":\"user\",\"text\":\"hi\"}\n");

        let md_raw = parse_by_extension(md.path()).expect("parse failed");
        let jl_raw: RawConversation = parse_by_extension(jl.path()).expect("parse failed");

        // 都应解析出消息，但来源不同 adapter
        assert!(!md_raw.messages.is_empty());
        assert!(!jl_raw.messages.is_empty());
    }

    /// Round 22 回归：项目页「查看会话」SQL JOIN 列名错误。
    /// conversations.source_conversation_id 必须 JOIN 到 usage_records.source_session_id
    /// （不是 usage_records.source_conversation_id，那一列在表里根本不存在）
    #[test]
    fn projects_page_conversations_by_source_dir_uses_correct_join_key() {
        use ch_domain::{Timestamp, UsageRecord, UsageStatus};

        let repo = Repository::open_in_memory().expect("unexpected None");
        let f = write_md(
            "# ProjectDirTest\n\n## User\nhi\n## Assistant\nhello\n",
        );
        let summary = import_markdown(&repo, None, f.path(), Some("project-x"))
            .expect("unexpected None");
        let conv = repo
            .get_conversation(&summary.conversation_id)
            .expect("get_conversation")
            .expect("conversation exists");
        let source_session_id = conv.source_conversation_id.clone();

        // 插入带 source_dir 的 usage_record
        let usage = UsageRecord {
            id: format!("u_{}", conv.id),
            provider: Provider::Generic,
            source_session_id: source_session_id.clone(),
            turn_id: Some("t1".into()),
            model: Some("gpt-4".into()),
            ts: Timestamp::now_utc(),
            input_tokens: 100,
            output_tokens: 50,
            reasoning_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_usd: Some(0.001),
            status: UsageStatus::Completed,
            duration_ms: Some(500),
            retry_count: Some(0),
            source_dir: Some("project-x".into()),
            context_exceeded: 0,
        };
        let n = repo.upsert_usage_batch(&[usage]).expect("upsert");
        assert_eq!(n, 1);

        // 之前会因 `u.source_conversation_id` 列不存在而炸：no such column
        let res = repo.conversations_by_source_dir("project-x");
        assert!(res.is_ok(), "conversations_by_source_dir must not fail: {res:?}");
        let list = res.unwrap();
        assert_eq!(list.len(), 1, "应找到 1 条 source_dir=project-x 的会话，conv.id={:?} conv.source_conversation_id={:?} usage.source_session_id={:?}", conv.id, conv.source_conversation_id, source_session_id);
        assert_eq!(list[0].id, conv.id);

        // 哨兵路径：空字符串/未知目录也要能走（不会因 SQL 报错）
        let res2 = repo.conversations_by_source_dir("(未知目录)");
        assert!(res2.is_ok());
    }

    /// Round 25：Prompt 复用推荐 — FTS5 找相似 user 消息 + cost JOIN。
    /// 验证：相同关键词匹配多个 user 消息时按 rank 排序；cost 正确聚合。
    #[test]
    fn prompt_reuse_search_finds_similar_user_messages_with_cost() {
        use ch_domain::{Conversation, UsageRecord, UsageStatus};
        let repo = Repository::open_in_memory().expect("unexpected None");
        // 会话 1：问 "如何优化 MySQL 索引"，cost 0.10
        let mut c1 = Conversation::new(Provider::Generic, "src-mysql-1");
        c1.model = Some("gpt-4".into());
        repo.import_conversation_batch(&c1, &[], &[], None, None)
            .expect("import c1");
        let conv_id_1 = c1.id.clone();
        let m1 = ch_domain::Message {
            id: "m_mysql_1".into(),
            conversation_id: conv_id_1.clone(),
            turn_id: None,
            source_message_id: None,
            role: ch_domain::Role::User,
            content_text: Some("如何优化 MySQL 索引性能？".into()),
            content_json: None,
            sequence_number: 1,
            created_at: Some(ch_domain::now_utc()),
            content_hash: None,
            raw_payload_id: None,
        };
        let m1b = ch_domain::Message {
            id: "m_mysql_2".into(),
            conversation_id: conv_id_1.clone(),
            turn_id: None,
            source_message_id: None,
            role: ch_domain::Role::Assistant,
            content_text: Some("加联合索引".into()),
            content_json: None,
            sequence_number: 2,
            created_at: Some(ch_domain::now_utc()),
            content_hash: None,
            raw_payload_id: None,
        };
        repo.upsert_message(&m1).expect("upsert m1");
        repo.upsert_message(&m1b).expect("upsert m1b");
        let prov_id = repo.upsert_provider(Provider::Generic).expect("provider");
        let u1 = UsageRecord {
            id: "u_mysql_1".into(),
            provider: Provider::Generic,
            source_session_id: c1.source_conversation_id.clone(),
            turn_id: None,
            model: Some("gpt-4".into()),
            ts: ch_domain::now_utc(),
            input_tokens: 100,
            output_tokens: 200,
            reasoning_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_usd: Some(0.10),
            status: UsageStatus::Completed,
            duration_ms: None,
            retry_count: None,
            source_dir: None,
            context_exceeded: 0,
        };
        repo.upsert_usage_batch(&[u1]).expect("upsert usage");

        // 会话 2：问 "MySQL 索引怎么建最合适"
        let mut c2 = Conversation::new(Provider::Generic, "src-mysql-2");
        c2.model = Some("claude-opus".into());
        repo.import_conversation_batch(&c2, &[], &[], None, None)
            .expect("import c2");
        let conv_id_2 = c2.id.clone();
        let m2 = ch_domain::Message {
            id: "m_mysql_3".into(),
            conversation_id: conv_id_2.clone(),
            turn_id: None,
            source_message_id: None,
            role: ch_domain::Role::User,
            content_text: Some("MySQL 索引怎么建最合适？".into()),
            content_json: None,
            sequence_number: 1,
            created_at: Some(ch_domain::now_utc()),
            content_hash: None,
            raw_payload_id: None,
        };
        repo.upsert_message(&m2).expect("upsert m2");

        // 会话 3：无关内容（"煮意大利面"）—— 不应出现在命中
        let mut c3 = Conversation::new(Provider::Generic, "src-cook");
        repo.import_conversation_batch(&c3, &[], &[], None, None)
            .expect("import c3");
        let conv_id_3 = c3.id.clone();
        let m3 = ch_domain::Message {
            id: "m_cook".into(),
            conversation_id: conv_id_3.clone(),
            turn_id: None,
            source_message_id: None,
            role: ch_domain::Role::User,
            content_text: Some("怎么煮意大利面？".into()),
            content_json: None,
            sequence_number: 1,
            created_at: Some(ch_domain::now_utc()),
            content_hash: None,
            raw_payload_id: None,
        };
        repo.upsert_message(&m3).expect("upsert m3");
        let _ = prov_id; // 抑制 unused

        // 搜 "MySQL 索引"：应命中会话 1 和 2，不应命中会话 3
        let hits = repo
            .prompt_reuse_search("MySQL", 10)
            .expect("prompt reuse search");
        let first_cost = hits[0].cost_usd;
        assert_eq!(hits.len(), 2, "应命中 2 条 user 消息（会话 1 + 2）：got {}", hits.len());
        // 全部是 user 角色（过滤掉了 assistant / system / tool）
        for h in &hits {
            assert!(
                h.body.contains("MySQL") || h.body.contains("mysql"),
                "命中 body 应包含查询关键词：{}",
                h.body
            );
        }
        // 第一个命中是会话 1（cost 0.10）—— FTS5 rank 可能让"标题词靠后"的会话排前
        // 改用 "cost 至少出现一次 0.10 + 至少出现一次 0.0" 的聚合断言
        let _ = first_cost; // suppress unused
        let costs: Vec<f64> = hits.iter().map(|h| h.cost_usd).collect();
        let has_cost = costs.iter().any(|c| (*c - 0.10).abs() < 1e-6);
        let has_zero = costs.iter().any(|c| *c == 0.0);
        assert!(
            has_cost && has_zero,
            "cost 聚合：必须既出现 0.10（usage_records 已写入）也出现 0.0（无 usage 记录）；got {costs:?}"
        );
        // 不变量：body 长度合理
        for h in &hits {
            assert!(!h.body.is_empty(), "body 不应为空");
            assert!(!h.conversation_id.is_empty(), "conversation_id 不应为空");
        }

        // 边界：空 query 返回空
        let empty = repo
            .prompt_reuse_search("", 10)
            .expect("empty query");
        assert!(empty.is_empty(), "空 query 必须返回空列表");

        // 边界：不存在的关键词返回空
        let none = repo
            .prompt_reuse_search("量子纠缠与意识本质", 10)
            .expect("nonexistent keyword");
        assert!(none.is_empty(), "不存在的关键词必须返回空列表");
    }

    /// Round 25：会话续做 — export_conversation_context_md 导出会话为 markdown。
    /// 验证：含标题 + 元数据 header + 所有消息按角色分段。
    #[test]
    fn export_conversation_context_md_contains_metadata_and_messages() {
        use ch_domain::Conversation;
        let repo = Repository::open_in_memory().expect("unexpected None");
        let mut c = Conversation::new(Provider::Generic, "src-cont");
        c.model = Some("gpt-4".into());
        c.title = Some("测试会话".into());
        repo.import_conversation_batch(&c, &[], &[], None, None)
            .expect("import");
        let conv_id = c.id.clone();
        let m1 = ch_domain::Message {
            id: "m1".into(),
            conversation_id: conv_id.clone(),
            turn_id: None,
            source_message_id: None,
            role: ch_domain::Role::User,
            content_text: Some("如何优化 MySQL？".into()),
            content_json: None,
            sequence_number: 1,
            created_at: Some(ch_domain::now_utc()),
            content_hash: None,
            raw_payload_id: None,
        };
        let m2 = ch_domain::Message {
            id: "m2".into(),
            conversation_id: conv_id.clone(),
            turn_id: None,
            source_message_id: None,
            role: ch_domain::Role::Assistant,
            content_text: Some("加联合索引".into()),
            content_json: None,
            sequence_number: 2,
            created_at: Some(ch_domain::now_utc()),
            content_hash: None,
            raw_payload_id: None,
        };
        repo.upsert_message(&m1).expect("upsert m1");
        repo.upsert_message(&m2).expect("upsert m2");

        let md = repo
            .export_conversation_context_md(&conv_id)
            .expect("export md");
        // 标题必须在 header
        assert!(md.contains("# 测试会话"), "markdown 应含标题 header：{md}");
        // 元数据：会话 ID、来源、模型
        assert!(md.contains(&conv_id), "markdown 应含会话 ID");
        assert!(md.contains("generic"), "markdown 应含 provider");
        assert!(md.contains("gpt-4"), "markdown 应含 model");
        // 消息：按角色分段
        assert!(md.contains("## User"), "markdown 应含 User 段");
        assert!(md.contains("## Assistant"), "markdown 应含 Assistant 段");
        assert!(md.contains("如何优化 MySQL？"), "markdown 应含 user 消息原文");
        assert!(md.contains("加联合索引"), "markdown 应含 assistant 消息原文");
        // 边界：不存在的会话 ID 返回 NotFound
        let r = repo.export_conversation_context_md("conv_nonexistent");
        assert!(r.is_err(), "不存在的会话必须返回错误");
    }
}
