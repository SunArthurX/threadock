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

    // 确保 provider 记录存在（conversations.provider_id 有外键约束）
    repo.upsert_provider(normalized.conversation.provider)?;

    // 可选 workspace：用身份解析器（plan §4.3）决定归并到已有还是新建。
    // 候选用：显示名 + 源文件父目录作为 canonical_path。
    let workspace_id = if let Some(name) = workspace_name {
        let parent_path = path_ref.parent().map(|p| p.to_string_lossy().into_owned());
        let mut candidate = ch_identity_resolver::SourceWorkspaceCandidate::new(name);
        candidate.canonical_path = parent_path.clone();

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

    // 4. 关联 workspace + raw_payload_id 到 conversation
    let mut conv = normalized.conversation;
    conv.workspace_id = workspace_id.clone();
    conv.raw_payload_id = raw_payload_id.clone();
    let conversation_id = repo.upsert_conversation(&conv)?;

    // 消息
    for m in &normalized.messages {
        let mut m = m.clone();
        m.conversation_id = conversation_id.clone();
        repo.upsert_message(&m)?;
    }
    // 事件
    for e in &normalized.events {
        let mut e = e.clone();
        e.conversation_id = conversation_id.clone();
        repo.upsert_event(&e)?;
    }

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
        let summary = import_markdown(&repo, None, f.path(), Some("my-web-app")).expect("unexpected None");
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
            repo.list_messages(&summary.conversation_id).expect("unexpected None").len(),
            2
        );
        assert_eq!(repo.list_events(&summary.conversation_id).expect("unexpected None").len(), 1);
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
        assert_eq!(repo.list_messages(&s1.conversation_id).expect("unexpected None").len(), 2);
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

        let summary = import_markdown(&repo, Some(&raw_store), f.path(), None).expect("unexpected None");

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
            repo.list_messages(&summary.conversation_id).expect("unexpected None").len(),
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
        assert_eq!(raw_store.stats().expect("unexpected None").count, 1, "only one raw object");
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
        let summary = import_markdown(&repo, None, f.path(), Some("jsonl-proj")).expect("unexpected None");
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
}
