//! CLI export 子命令实现：把一条会话导出为 Markdown 或 JSON。

use crate::Result;
use ch_export::{to_json, to_markdown, ExportOptions};
use ch_storage::Repository;
use std::path::Path;

/// 批量导出结果。
#[derive(Debug, Clone)]
pub struct BatchExportSummary {
    pub workspace_id: String,
    pub output_dir: String,
    pub conversations_exported: usize,
    pub files_written: usize,
}

/// 导出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Markdown,
    Json,
}

/// 导出一条会话到文件。
///
/// 默认启用「全部内容 + 脱敏」（plan §6.6：导出前敏感信息扫描和脱敏）。
pub fn export_conversation(
    repo: &Repository,
    conversation_id: &str,
    out_path: &str,
    format: ExportFormat,
) -> Result<()> {
    // 读取会话
    let conv = repo
        .get_conversation(conversation_id)?
        .ok_or_else(|| anyhow::anyhow!("conversation not found: {conversation_id}"))?;
    let messages = repo.list_messages(conversation_id)?;
    let events = repo.list_events(conversation_id)?;

    // 可选 workspace
    let workspace = conv
        .workspace_id
        .as_deref()
        .and_then(|wid| repo.get_workspace(wid).ok().flatten());

    // 默认导出全部 + 脱敏（内置规则）
    let opts = ExportOptions::everything();
    let mut content = match format {
        ExportFormat::Markdown => to_markdown(&conv, &messages, &events, &opts),
        ExportFormat::Json => to_json(workspace.as_ref(), &conv, &messages, &events, &opts)?,
    };

    // 应用用户自定义脱敏规则（plan §14.6）
    let custom_rules: Vec<ch_export::CustomRule> = repo
        .list_redaction_rules()?
        .into_iter()
        .filter(|r| r.enabled)
        .map(|r| ch_export::CustomRule::new(r.name, r.pattern))
        .collect();
    let custom_count = custom_rules.len();
    if !custom_rules.is_empty() {
        let (redacted, _) = ch_export::redact_with(&content, &custom_rules);
        content = redacted;
    }

    let path = Path::new(out_path);
    std::fs::write(path, content.as_bytes())?;

    println!("exported {conversation_id} → {out_path}");
    if let Some(stats) = ch_export::serialize::build_export_data(
        workspace.as_ref(),
        &conv,
        &messages,
        &events,
        &opts,
    )
    .redaction
    {
        println!("redaction: {} item(s) sanitized (builtin)", stats.total());
    } else {
        println!("redaction: 0 item(s) sanitized (builtin)");
    }
    println!("custom rules applied: {custom_count}");
    Ok(())
}

/// 批量导出一个 workspace 下的所有会话（plan §6.6「Workspace 批量导出」）。
///
/// 每个会话写一个文件到 `output_dir`，文件名 `<conversation_id>.md` 或 `.json`。
/// 默认启用脱敏。
pub fn export_workspace(
    repo: &Repository,
    workspace_id: &str,
    output_dir: &str,
    format: ExportFormat,
) -> Result<BatchExportSummary> {
    // 校验 workspace 存在
    let ws = repo
        .get_workspace(workspace_id)?
        .ok_or_else(|| anyhow::anyhow!("workspace not found: {workspace_id}"))?;

    let convs = repo.list_conversations(Some(workspace_id))?;
    let outdir = Path::new(output_dir);
    std::fs::create_dir_all(outdir)?;

    let opts = ExportOptions::everything();
    let mut files_written = 0;
    for c in &convs {
        let messages = repo.list_messages(&c.id)?;
        let events = repo.list_events(&c.id)?;
        let content = match format {
            ExportFormat::Markdown => to_markdown(c, &messages, &events, &opts),
            ExportFormat::Json => to_json(Some(&ws), c, &messages, &events, &opts)?,
        };
        let ext = match format {
            ExportFormat::Markdown => "md",
            ExportFormat::Json => "json",
        };
        let file_path = outdir.join(format!("{}.{}", c.id, ext));
        std::fs::write(&file_path, content.as_bytes())?;
        files_written += 1;
    }

    Ok(BatchExportSummary {
        workspace_id: workspace_id.to_string(),
        output_dir: output_dir.to_string(),
        conversations_exported: convs.len(),
        files_written,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_domain::{Conversation, Message, Provider, Role};
    use ch_storage::Repository;
    use tempfile::TempDir;

    fn seeded_repo() -> (TempDir, Repository, String) {
        let dir = TempDir::new().expect("tempdir creation failed");
        let repo = Repository::open(dir.path().join("hub.db")).expect("unexpected None");
        repo.upsert_provider(Provider::Generic).expect("upsert failed");
        let mut c = Conversation::new(Provider::Generic, "src-export");
        c.title = Some("含 token=ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ1234567890 的会话".into());
        let cid = repo.upsert_conversation(&c).expect("upsert failed");
        let mut m = Message::new(&cid, Role::User, 1);
        m.content_text = Some("联系 alice@corp.com".into());
        repo.upsert_message(&m).expect("upsert failed");
        (dir, repo, cid)
    }

    #[test]
    fn export_markdown_writes_file_with_redaction() {
        let (_dir, repo, cid) = seeded_repo();
        let outdir = TempDir::new().expect("tempdir creation failed");
        let out = outdir.path().join("conv.md");
        export_conversation(&repo, &cid, out.to_str().expect("unexpected None"), ExportFormat::Markdown).expect("unexpected None");
        let content = std::fs::read_to_string(&out).expect("file I/O failed");
        assert!(content.contains("[REDACTED:github_token]"));
        assert!(content.contains("[REDACTED:email]"));
        assert!(!content.contains("ghp_aBcDeF"));
    }

    #[test]
    fn export_json_writes_valid_json_with_redaction() {
        let (_dir, repo, cid) = seeded_repo();
        let outdir = TempDir::new().expect("tempdir creation failed");
        let out = outdir.path().join("conv.json");
        export_conversation(&repo, &cid, out.to_str().expect("unexpected None"), ExportFormat::Json).expect("unexpected None");
        let content = std::fs::read_to_string(&out).expect("file I/O failed");
        let data: ch_export::ExportData = serde_json::from_str(&content).expect("parse failed");
        assert!(data
            .conversation
            .title
            .as_deref()
            .expect("unexpected None")
            .contains("[REDACTED:github_token]"));
    }

    #[test]
    fn export_nonexistent_conversation_errors() {
        let (_dir, repo, _cid) = seeded_repo();
        let outdir = TempDir::new().expect("tempdir creation failed");
        let out = outdir.path().join("x.md");
        let err = export_conversation(
            &repo,
            "conv_nope",
            out.to_str().expect("unexpected None"),
            ExportFormat::Markdown,
        );
        assert!(err.is_err());
    }

    // ── 批量导出 ──────────────────────────────────────────────────────────

    fn seeded_with_workspace() -> (TempDir, Repository, String, Vec<String>) {
        let dir = TempDir::new().expect("tempdir creation failed");
        let repo = Repository::open(dir.path().join("hub.db")).expect("unexpected None");
        repo.upsert_provider(ch_domain::Provider::Generic).expect("upsert failed");
        let ws_id = repo
            .upsert_workspace(&ch_domain::Workspace::new("batch-ws"))
            .expect("unexpected None");

        let mut conv_ids = Vec::new();
        for i in 0..3 {
            let mut c =
                ch_domain::Conversation::new(ch_domain::Provider::Generic, format!("src-{i}"));
            c.workspace_id = Some(ws_id.clone());
            c.title = Some(format!("会话 {i}"));
            let cid = repo.upsert_conversation(&c).expect("upsert failed");
            let mut m = ch_domain::Message::new(&cid, ch_domain::Role::User, 1);
            m.content_text = Some(format!("消息 {i}"));
            repo.upsert_message(&m).expect("upsert failed");
            conv_ids.push(cid);
        }
        // 另一个不在该 workspace 的会话
        let other = ch_domain::Conversation::new(ch_domain::Provider::Generic, "src-other");
        repo.upsert_conversation(&other).expect("upsert failed");

        (dir, repo, ws_id, conv_ids)
    }

    #[test]
    fn export_workspace_writes_all_conversations() {
        let (_dir, repo, ws_id, conv_ids) = seeded_with_workspace();
        let outdir = TempDir::new().expect("tempdir creation failed");

        let summary = export_workspace(
            &repo,
            &ws_id,
            outdir.path().to_str().expect("unexpected None"),
            ExportFormat::Markdown,
        )
        .expect("unexpected None");
        assert_eq!(summary.conversations_exported, 3);
        assert_eq!(summary.files_written, 3);

        // 目录下应有 3 个 .md 文件
        let entries: Vec<_> = std::fs::read_dir(outdir.path()).expect("file I/O failed").collect();
        let md_count = entries
            .iter()
            .filter(|e| {
                e.as_ref()
                    .expect("unexpected None")
                    .path()
                    .extension()
                    .and_then(|x| x.to_str())
                    == Some("md")
            })
            .count();
        assert_eq!(md_count, 3);

        // 内容应含脱敏占位或标题
        for cid in &conv_ids {
            let _ = cid; // 文件名基于 id，验证至少有内容
        }
        let first_file = entries[0].as_ref().expect("unexpected None").path();
        let content = std::fs::read_to_string(&first_file).expect("file I/O failed");
        assert!(content.contains("# ") || content.contains("会话"));
    }

    #[test]
    fn export_workspace_json_format() {
        let (_dir, repo, ws_id, _conv_ids) = seeded_with_workspace();
        let outdir = TempDir::new().expect("tempdir creation failed");
        let summary = export_workspace(
            &repo,
            &ws_id,
            outdir.path().to_str().expect("unexpected None"),
            ExportFormat::Json,
        )
        .expect("unexpected None");
        assert_eq!(summary.files_written, 3);
        let json_files: Vec<_> = std::fs::read_dir(outdir.path())
            .expect("unexpected None")
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
            .collect();
        assert_eq!(json_files.len(), 3);
    }

    #[test]
    fn export_workspace_nonexistent_errors() {
        let (_dir, repo, _ws_id, _conv_ids) = seeded_with_workspace();
        let outdir = TempDir::new().expect("tempdir creation failed");
        let err = export_workspace(
            &repo,
            "ws_nope",
            outdir.path().to_str().expect("unexpected None"),
            ExportFormat::Markdown,
        );
        assert!(err.is_err());
    }

    #[test]
    fn export_workspace_empty_workspace() {
        let dir = TempDir::new().expect("tempdir creation failed");
        let repo = Repository::open(dir.path().join("h.db")).expect("unexpected None");
        repo.upsert_provider(ch_domain::Provider::Generic).expect("upsert failed");
        let ws_id = repo
            .upsert_workspace(&ch_domain::Workspace::new("empty-ws"))
            .expect("unexpected None");
        let outdir = TempDir::new().expect("tempdir creation failed");
        let summary = export_workspace(
            &repo,
            &ws_id,
            outdir.path().to_str().expect("unexpected None"),
            ExportFormat::Markdown,
        )
        .expect("unexpected None");
        assert_eq!(summary.conversations_exported, 0);
        assert_eq!(summary.files_written, 0);
    }
}
