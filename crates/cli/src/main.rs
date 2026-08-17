//! Conversation Hub CLI（MVP）。
//!
//! 子命令：
//! - `ch import <md-file> [--workspace <name>]` 导入一个 Markdown 会话
//! - `ch list [--workspace <id>]` 列出会话
//! - `ch show <conversation-id>` 展示会话详情
//! - `ch integrity` 数据库完整性检查
//!
//! 数据库位置：默认 `./conversation-hub.db`，可用 `--db` 覆盖。

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::ExitCode;

mod export_cmd;
mod import;

use export_cmd::ExportFormat;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

#[allow(clippy::too_many_lines)] // CLI 子命令分派主入口
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        return Ok(());
    }

    // 简易参数解析（MVP 阶段不引入 clap，保持零额外依赖）
    let (db_path, sub) = extract_db(&args);
    let repo = ch_storage::Repository::open(&db_path)
        .with_context(|| format!("open db at {}", db_path.display()))?;

    // Raw Store：与数据库同目录（plan §9.6 布局：<dir>/raw/ab/cd/<hash>.json.zst）
    let data_dir = db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map_or_else(
            || std::path::PathBuf::from("."),
            std::path::Path::to_path_buf,
        );
    let raw_store = ch_raw_store::RawStore::new(&data_dir).context("open raw store")?;

    // Tantivy 索引：与数据库同目录下的 index/（plan §9.5/§13）
    let search_index =
        ch_search::SearchIndex::open(data_dir.join("index")).context("open search index")?;

    // 把 String slice 转成 &str 方便字面量匹配
    let sub_str: Vec<&str> = sub.iter().map(std::string::String::as_str).collect();
    match sub_str.as_slice() {
        ["import", file] => {
            let summary = import::import_markdown(&repo, Some(&raw_store), file, None)
                .context("import markdown")?;
            index_imported(&search_index, &repo, &summary.conversation_id)?;
            print_summary(&summary);
        }
        ["import", file, "--workspace", name] => {
            let summary = import::import_markdown(&repo, Some(&raw_store), file, Some(name))
                .context("import markdown")?;
            index_imported(&search_index, &repo, &summary.conversation_id)?;
            print_summary(&summary);
        }
        ["import-from", "claude-code", "list"] => {
            list_claude_code_sessions()?;
        }
        ["import-from", "claude-code", session_id] => {
            import_from_claude_code(&repo, &search_index, &raw_store, session_id)?;
        }
        ["import-from", "zcode", "list"] => {
            list_zcode_sessions()?;
        }
        ["import-from", "zcode", session_id] => {
            import_from_zcode(&repo, &search_index, &raw_store, session_id)?;
        }
        ["list", args @ ..] => {
            list_with_filter(&repo, args)?;
        }
        ["show", id] => {
            show_conversation(&repo, id)?;
        }
        ["search", args @ ..] => {
            run_search(&repo, args)?;
        }
        ["search-tantivy", args @ ..] => {
            run_tantivy_search(&repo, &search_index, args)?;
        }
        ["export", "markdown", id, out] => {
            export_cmd::export_conversation(&repo, id, out, ExportFormat::Markdown)?;
        }
        ["export", "json", id, out] => {
            export_cmd::export_conversation(&repo, id, out, ExportFormat::Json)?;
        }
        ["export", "workspace", id, out_dir] => {
            let summary = export_cmd::export_workspace(&repo, id, out_dir, ExportFormat::Markdown)
                .context("export workspace")?;
            println!(
                "exported {} conversation(s) from workspace {} → {}",
                summary.conversations_exported, summary.workspace_id, summary.output_dir
            );
            println!("files written: {}", summary.files_written);
        }
        ["backup", out_path] => {
            let password = read_backup_password()?;
            let raw_root = data_dir.join("raw");
            let meta = ch_backup::create_backup(
                &ch_backup::BackupSource {
                    db_path: db_path.clone(),
                    raw_root: Some(raw_root),
                },
                &password,
                std::path::Path::new(out_path),
            )
            .context("create backup")?;
            println!("backup created: {out_path}");
            println!("  db size:   {} bytes", meta.db_size);
            println!("  raw files: {}", meta.raw_count);
            println!("  raw bytes: {}", meta.raw_bytes);
        }
        ["restore", backup_path, target_dir] => {
            let password = read_backup_password()?;
            let meta = ch_backup::restore_backup(
                std::path::Path::new(backup_path),
                &password,
                std::path::Path::new(target_dir),
            )
            .context("restore backup")?;
            println!("restored to: {target_dir}");
            println!("  db size:   {} bytes", meta.db_size);
            println!("  raw files: {}", meta.raw_count);
        }
        ["favorite", id] => {
            repo.set_favorite(id, true).context("set favorite")?;
            println!("favorited: {id}");
        }
        ["unfavorite", id] => {
            repo.set_favorite(id, false).context("unset favorite")?;
            println!("unfavorited: {id}");
        }
        ["archive", id] => {
            repo.set_archived(id, true).context("archive")?;
            println!("archived: {id}");
        }
        ["delete", id] => {
            // 默认软删除（plan §11.4）
            repo.soft_delete_conversation(id).context("soft delete")?;
            println!("soft deleted: {id}（用 `ch undelete {id}` 恢复）");
        }
        ["delete", id, "--hard"] => {
            repo.hard_delete_conversation(id).context("hard delete")?;
            println!("hard deleted: {id}（已永久移除）");
        }
        ["undelete", id] => {
            repo.restore_conversation(id).context("restore")?;
            println!("restored: {id}");
        }
        ["unarchive", id] => {
            repo.set_archived(id, false).context("unarchive")?;
            println!("unarchived: {id}");
        }
        ["tag", id, tag] => {
            repo.add_tag(id, tag).context("add tag")?;
            println!("tagged {id}: {tag}");
        }
        ["untag", id, tag] => {
            repo.remove_tag(id, tag).context("remove tag")?;
            println!("untagged {id}: {tag}");
        }
        ["tags", id] => {
            let tags = repo.list_tags(id).context("list tags")?;
            if tags.is_empty() {
                println!("(no tags)");
            } else {
                for t in tags {
                    println!("{t}");
                }
            }
        }
        ["favorites"] => {
            let ids = repo
                .list_favorite_conversation_ids()
                .context("list favorites")?;
            if ids.is_empty() {
                println!("(no favorites)");
            } else {
                for id in ids {
                    println!("{id}");
                }
            }
        }
        ["knowledge", id] => {
            extract_knowledge(&repo, id, false)?;
        }
        ["knowledge", id, "--save"] => {
            extract_knowledge(&repo, id, true)?;
        }
        ["knowledge", id, "--show"] => {
            show_saved_knowledge(&repo, id)?;
        }
        ["similar", id] => {
            find_similar_cli(&repo, id)?;
        }
        ["redaction-rule", "add", name, pattern] => {
            repo.add_redaction_rule(name, pattern)
                .context("add redaction rule")?;
            println!("added rule: {name} = {pattern}");
        }
        ["redaction-rule", "list"] => {
            let rules = repo.list_redaction_rules().context("list rules")?;
            if rules.is_empty() {
                println!("(no custom rules)");
            } else {
                for r in rules {
                    println!(
                        "  {} = {} ({})",
                        r.name,
                        r.pattern,
                        if r.enabled { "on" } else { "off" }
                    );
                }
            }
        }
        ["redaction-rule", "remove", name] => {
            repo.remove_redaction_rule(name).context("remove rule")?;
            println!("removed rule: {name}");
        }
        ["integrity"] => {
            let ok = repo.integrity_check().context("integrity check")?;
            println!("integrity: {}", if ok { "ok" } else { "FAILED" });
        }
        ["daemon"] => {
            // 在当前进程内启动 stdio JSON-RPC 服务（plan §8.2）
            let state = ch_daemon::DaemonState::open(ch_daemon::DaemonStateConfig {
                data_dir: data_dir.clone(),
            })
            .context("open daemon state")?;
            eprintln!(
                "Conversation Hub daemon listening on stdio (data: {})",
                data_dir.display()
            );
            ch_daemon::serve_stdio(
                &state,
                std::io::stdin().lock(),
                &mut std::io::BufWriter::new(std::io::stdout().lock()),
            );
        }
        ["help" | "--help" | "-h"] => print_usage(),
        _ => {
            print_usage();
            anyhow::bail!("unknown subcommand: {sub:?}");
        }
    }
    Ok(())
}

/// 解析 list 子命令的过滤参数（plan §6.4）。
fn list_with_filter(repo: &ch_storage::Repository, args: &[&str]) -> Result<()> {
    let mut filter = ch_storage::ConversationFilter::new();
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--favorite" | "--favorites" => {
                filter = filter.favorites_only();
            }
            "--archived" => {
                filter = filter.archived_only();
            }
            "--unarchived" => {
                filter = filter.unarchived_only();
            }
            "--workspace" => {
                if let Some(v) = args.get(i + 1) {
                    filter = filter.with_workspace(*v);
                    i += 2;
                    continue;
                }
            }
            "--provider" => {
                if let Some(v) = args.get(i + 1) {
                    if let Ok(p) = v.parse::<ch_domain::Provider>() {
                        filter = filter.with_provider(p);
                    }
                    i += 2;
                    continue;
                }
            }
            other => {
                anyhow::bail!("unknown list option: {other}");
            }
        }
        i += 1;
    }
    let convs = repo
        .list_conversations_filtered(&filter)
        .context("list conversations (filtered)")?;
    print_conversation_list(&convs);
    Ok(())
}

fn print_conversation_list(convs: &[ch_domain::Conversation]) {
    if convs.is_empty() {
        println!("(no conversations)");
        return;
    }
    println!("{:<34} {:<12} {:<8} TITLE", "ID", "PROVIDER", "STATUS");
    for c in convs {
        println!(
            "{:<34} {:<12} {:<8} {}",
            c.id,
            c.provider,
            c.status.map_or("", |s| s.as_str()),
            c.effective_title(),
        );
    }
}

fn show_conversation(repo: &ch_storage::Repository, id: &str) -> Result<()> {
    let conv = repo
        .get_conversation(id)
        .context("get conversation")?
        .ok_or_else(|| anyhow::anyhow!("conversation not found: {id}"))?;
    println!("ID:        {}", conv.id);
    println!("Provider:  {}", conv.provider);
    println!("Title:     {}", conv.effective_title());
    println!(
        "Status:    {}",
        conv.status.map_or("unknown", |s| s.as_str())
    );
    if let Some(score) = conv.completeness_score {
        println!("Complete:  {score:.2}");
    }
    if let Some(h) = &conv.content_hash {
        println!("Hash:      {h}");
    }
    println!("\n--- Messages ---");
    for m in repo.list_messages(id).context("list messages")? {
        println!(
            "[{}] {}",
            m.role,
            m.content_text.as_deref().unwrap_or("(empty)")
        );
    }
    let events = repo.list_events(id).context("list events")?;
    if !events.is_empty() {
        println!("\n--- Events ---");
        for e in events {
            println!("[{}] {}", e.event_type, e.summary.as_deref().unwrap_or(""));
        }
    }
    Ok(())
}

/// 列出 Claude Code 会话。
fn list_claude_code_sessions() -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "~".into());
    let claude_home = format!("{home}/.claude");
    let sessions = ch_adapter_claude_code::discover_sessions(&claude_home)
        .context("discover claude code sessions")?;
    if sessions.is_empty() {
        println!("（未找到 Claude Code 会话，检查 ~/.claude/projects/ 是否存在）");
        return Ok(());
    }
    println!("找到 {} 个 Claude Code 会话：", sessions.len());
    println!("{:<40} {:<10} PROJECT", "SESSION_ID", "SIZE");
    for s in sessions.iter().take(50) {
        println!(
            "{:<40} {:>8} KB  {}",
            s.session_id,
            s.size_bytes / 1024,
            s.project_dir,
        );
    }
    if sessions.len() > 50 {
        println!("...（仅显示前 50，共 {} 个）", sessions.len());
    }
    Ok(())
}

/// 从 Claude Code 导入一条会话。
fn import_from_claude_code(
    repo: &ch_storage::Repository,
    search_index: &ch_search::SearchIndex,
    raw_store: &ch_raw_store::RawStore,
    session_id: &str,
) -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "~".into());
    let claude_home = format!("{home}/.claude");
    // 找到对应文件
    let sessions =
        ch_adapter_claude_code::discover_sessions(&claude_home).context("discover sessions")?;
    let session = sessions
        .into_iter()
        .find(|s| s.session_id == session_id)
        .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;

    let raw = ch_adapter_claude_code::parse_session(&session.file_path)
        .context("parse claude code session")?;
    let summary = import_raw(
        repo,
        search_index,
        raw_store,
        raw,
        Some(&session.project_dir),
    )?;
    println!("✓ 从 Claude Code 导入成功");
    print_summary(&summary);
    Ok(())
}

/// 列出 `ZCode` 会话。
fn list_zcode_sessions() -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "~".into());
    let db_path = format!("{home}/.zcode/cli/db/db.sqlite");
    let sessions =
        ch_adapter_zcode::discover_sessions(&db_path).context("discover zcode sessions")?;
    if sessions.is_empty() {
        println!("（未找到 ZCode 会话，检查 ~/.zcode/cli/db/db.sqlite 是否存在）");
        return Ok(());
    }
    println!("找到 {} 个 ZCode 会话：", sessions.len());
    println!("{:<42} {:>6} TITLE", "SESSION_ID", "MSGS");
    for s in sessions.iter().take(50) {
        println!("{:<42} {:>6} {}", s.session_id, s.message_count, s.title);
    }
    if sessions.len() > 50 {
        println!("...（仅显示前 50，共 {} 个）", sessions.len());
    }
    Ok(())
}

/// 从 `ZCode` 导入一条会话。
fn import_from_zcode(
    repo: &ch_storage::Repository,
    search_index: &ch_search::SearchIndex,
    raw_store: &ch_raw_store::RawStore,
    session_id: &str,
) -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "~".into());
    let db_path = format!("{home}/.zcode/cli/db/db.sqlite");
    let raw =
        ch_adapter_zcode::parse_session(&db_path, session_id).context("parse zcode session")?;
    let summary = import_raw(repo, search_index, raw_store, raw, None)?;
    println!("✓ 从 ZCode 导入成功");
    print_summary(&summary);
    Ok(())
}

/// 通用：把 `RawConversation` 导入到 repo + `search_index` + `raw_store`。
fn import_raw(
    repo: &ch_storage::Repository,
    search_index: &ch_search::SearchIndex,
    raw_store: &ch_raw_store::RawStore,
    raw: ch_normalization::RawConversation,
    workspace_name: Option<&str>,
) -> Result<import::ImportSummary> {
    // 序列化原始数据到 Raw Store
    let raw_bytes = serde_json::to_vec(&raw)?;
    let raw_payload = raw_store.put(&raw_bytes)?;

    let normalized = ch_normalization::normalize(raw)?;

    // workspace（名称精确查找/新建，事务内完成）
    // 单事务批量入库：provider + workspace + 会话 + 消息 + 事件一次提交
    let workspace_id = workspace_name.map(|name| {
        if let Ok(Some(existing)) = repo.find_workspace_by_name(name) {
            existing.id
        } else {
            let ws = ch_domain::Workspace::new(name);
            repo.upsert_workspace(&ws).unwrap_or_default()
        }
    });

    let mut conv = normalized.conversation;
    conv.workspace_id.clone_from(&workspace_id);
    conv.raw_payload_id = Some(raw_payload.hash.clone());
    let conversation_id = repo
        .import_conversation_batch(&conv, &normalized.messages, &normalized.events, None, None)
        .context("import conversation batch")?;

    // 索引
    let mut writer = search_index.writer(ch_search::index::DEFAULT_WRITER_HEAP)?;
    let conv_title = conv.effective_title().to_string();
    for m in &normalized.messages {
        let im = ch_search::index::IndexableMessage {
            message_id: m.id.clone(),
            conversation_id: conversation_id.clone(),
            provider: conv.provider,
            workspace_id: workspace_id.clone(),
            role: m.role,
            title: Some(conv_title.clone()),
            body: m.content_text.clone(),
        };
        search_index.index_message(&mut writer, &im)?;
    }
    search_index.commit(writer)?;

    Ok(import::ImportSummary {
        conversation_id,
        workspace_id,
        messages_imported: normalized.messages.len(),
        events_imported: normalized.events.len(),
        completeness: normalized.completeness.label(),
        conversation_hash: normalized.conversation_hash,
        raw_payload_id: Some(raw_payload.hash),
    })
}

/// import 后把会话消息同步写入 Tantivy 索引（plan §9.5）。
fn index_imported(
    index: &ch_search::SearchIndex,
    repo: &ch_storage::Repository,
    conversation_id: &str,
) -> Result<()> {
    let conv = repo
        .get_conversation(conversation_id)?
        .ok_or_else(|| anyhow::anyhow!("conversation not found after import: {conversation_id}"))?;
    let messages = repo.list_messages(conversation_id)?;
    let mut writer = index.writer(ch_search::index::DEFAULT_WRITER_HEAP)?;
    for m in &messages {
        let im = ch_search::index::IndexableMessage {
            message_id: m.id.clone(),
            conversation_id: conversation_id.to_string(),
            provider: conv.provider,
            workspace_id: conv.workspace_id.clone(),
            role: m.role,
            title: Some(conv.effective_title().to_string()),
            body: m.content_text.clone(),
        };
        index.index_message(&mut writer, &im)?;
    }
    index.commit(writer)?;
    Ok(())
}

/// 用 Tantivy 执行搜索（plan §13 增强检索）。
fn run_tantivy_search(
    repo: &ch_storage::Repository,
    index: &ch_search::SearchIndex,
    args: &[&str],
) -> Result<()> {
    let mut keywords: Vec<String> = Vec::new();
    let mut provider: Option<ch_domain::Provider> = None;
    let mut workspace_id: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--provider" => {
                if let Some(v) = args.get(i + 1) {
                    provider = v.parse().ok();
                    i += 2;
                    continue;
                }
            }
            "--workspace" => {
                if let Some(v) = args.get(i + 1) {
                    workspace_id = Some((*v).to_string());
                    i += 2;
                    continue;
                }
            }
            other => keywords.push(other.to_string()),
        }
        i += 1;
    }
    if keywords.is_empty() {
        anyhow::bail!("search-tantivy requires at least one keyword");
    }
    let query_str = keywords.join(" ");

    // 查询语法（plan §13.2）：workspace: 名字解析 + DB 级后过滤
    let parsed = ch_domain::query_syntax::parse(&query_str);
    let ws_ids = match &parsed.workspace {
        Some(w) => repo.workspace_ids_by_name_or_id(w)?,
        None => Vec::new(),
    };
    let db_filter = repo.search_filter_conversation_ids(&parsed)?;
    if let Some(ws) = &parsed.workspace {
        if ws_ids.is_empty() {
            println!("(no workspace matches '{ws}')");
            return Ok(());
        }
    }

    let mut q = ch_search::SearchQuery::new(&query_str).with_workspace_ids(ws_ids);
    if let Some(p) = provider {
        q = q.with_provider(p);
    }
    if let Some(w) = workspace_id {
        q = q.with_workspace(w);
    }
    if db_filter.is_some() {
        q = q.with_limit(200);
    }

    let hits = index.search(&q).context("tantivy search")?;
    let hits = match &db_filter {
        Some(set) => hits
            .into_iter()
            .filter(|h| set.contains(&h.conversation_id))
            .collect::<Vec<_>>(),
        None => hits,
    };
    if hits.is_empty() {
        println!("(no matches via Tantivy)");
        return Ok(());
    }
    println!(
        "Tantivy found {} match(es) for {:?}:",
        hits.len(),
        query_str
    );
    println!("{:-<80}", "");
    for (idx, h) in hits.iter().enumerate() {
        println!(
            "{}. [{}] {} | {} (score {:.3})",
            idx + 1,
            h.provider,
            h.title.as_deref().unwrap_or("(untitled)"),
            h.role,
            h.score
        );
        println!("   …{}…", h.snippet);
        println!("   conversation: {}", h.conversation_id);
    }
    Ok(())
}

fn run_search(repo: &ch_storage::Repository, args: &[&str]) -> Result<()> {
    // 解析：关键词 token + 可选 --provider <p> / --workspace <id>
    let mut keywords: Vec<String> = Vec::new();
    let mut provider: Option<ch_domain::Provider> = None;
    let mut workspace_id: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--provider" => {
                if let Some(v) = args.get(i + 1) {
                    provider = v.parse().ok();
                    i += 2;
                    continue;
                }
            }
            "--workspace" => {
                if let Some(v) = args.get(i + 1) {
                    workspace_id = Some((*v).to_string());
                    i += 2;
                    continue;
                }
            }
            other => keywords.push(other.to_string()),
        }
        i += 1;
    }

    if keywords.is_empty() {
        anyhow::bail!("search requires at least one keyword");
    }
    let query_str = keywords.join(" ");

    let mut q = ch_storage::SearchQuery::new(&query_str);
    if let Some(p) = provider {
        q = q.with_provider(p);
    }
    if let Some(w) = workspace_id {
        q = q.with_workspace(w);
    }

    let results = repo.search(&q).context("search")?;
    if results.is_empty() {
        println!("(no matches)");
        return Ok(());
    }
    println!("Found {} match(es) for {:?}:", results.len(), query_str);
    println!("{:-<80}", "");
    for (idx, sr) in results.iter().enumerate() {
        println!(
            "{}. [{}] {} | {}",
            idx + 1,
            sr.provider,
            sr.title.as_deref().unwrap_or("(untitled)"),
            sr.role
        );
        // 去掉 snippet 里的 HTML 高亮标签，终端用 >> << 替代便于阅读
        let clean_snip = sr.snippet.replace("<b>", "»").replace("</b>", "«");
        println!("   …{clean_snip}…");
        println!("   conversation: {}", sr.conversation_id);
    }
    Ok(())
}

fn print_summary(s: &import::ImportSummary) {
    println!("imported conversation: {}", s.conversation_id);
    if let Some(w) = &s.workspace_id {
        println!("workspace:             {w}");
    }
    println!("messages:              {}", s.messages_imported);
    println!("events:                {}", s.events_imported);
    println!("completeness:          {}", s.completeness);
    println!("content hash:          {}", s.conversation_hash);
    if let Some(h) = &s.raw_payload_id {
        println!("raw payload:           {h}");
    }
}

/// 对一条会话执行知识提取（plan §13.5）。save=true 时持久化结果。
fn extract_knowledge(
    repo: &ch_storage::Repository,
    conversation_id: &str,
    save: bool,
) -> Result<()> {
    let conv = repo
        .get_conversation(conversation_id)?
        .ok_or_else(|| anyhow::anyhow!("conversation not found: {conversation_id}"))?;
    let messages = repo.list_messages(conversation_id)?;
    let events = repo.list_events(conversation_id)?;

    let input = ch_knowledge::ExtractionInput {
        title: Some(conv.effective_title().to_string()),
        messages,
        events,
    };
    let result = ch_knowledge::RuleExtractor::new().extract(&input);

    if save {
        let json = serde_json::to_string(&result)?;
        let kid = repo.save_knowledge(conversation_id, &result.extractor, &json)?;
        println!("✓ 已保存知识提取结果（id: {kid}）\n");
    }

    print_extraction_result(&result);
    Ok(())
}

/// 找出与指定会话相似的其他会话（plan §6.7）。
fn find_similar_cli(repo: &ch_storage::Repository, conversation_id: &str) -> Result<()> {
    let target_conv = repo
        .get_conversation(conversation_id)?
        .ok_or_else(|| anyhow::anyhow!("conversation not found: {conversation_id}"))?;
    let target_msgs = repo.list_messages(conversation_id)?;
    let target = ch_knowledge::conversation_text(
        conversation_id,
        Some(target_conv.effective_title()),
        &target_msgs,
    );

    // 收集所有其他会话作为候选
    let all_convs = repo.list_conversations(None)?;
    let mut candidates = Vec::new();
    for c in &all_convs {
        if c.id == conversation_id {
            continue;
        }
        let msgs = repo.list_messages(&c.id)?;
        candidates.push(ch_knowledge::conversation_text(
            &c.id,
            Some(c.effective_title()),
            &msgs,
        ));
    }

    let hits = ch_knowledge::find_similar(&target, &candidates, 10);
    if hits.is_empty() {
        println!("（未找到相似会话）");
        return Ok(());
    }
    println!("与「{}」相似的会话：", target_conv.effective_title());
    for (i, h) in hits.iter().enumerate() {
        // 找到对应会话标题
        let title = all_convs
            .iter()
            .find(|c| c.id == h.conversation_id)
            .map(|c| c.effective_title().to_string())
            .unwrap_or_default();
        println!(
            "  {}. [{:.0}%] {} — {}",
            i + 1,
            h.score * 100.0,
            h.conversation_id,
            title
        );
    }
    Ok(())
}

/// 显示已保存的知识提取结果（plan §13.5 读取持久化版本）。
fn show_saved_knowledge(repo: &ch_storage::Repository, conversation_id: &str) -> Result<()> {
    let rec = repo.get_knowledge(conversation_id)?.ok_or_else(|| {
        anyhow::anyhow!("未找到已保存的知识提取，先运行 `ch knowledge <id> --save`")
    })?;
    let result: ch_knowledge::ExtractionResult = serde_json::from_str(&rec.result_json)?;
    println!(
        "=== 已保存的知识提取（版本 {}，提取器 {}）===",
        rec.version, rec.extractor
    );
    println!();
    print_extraction_result(&result);
    Ok(())
}

/// 打印提取结果（CLI 与 --show 共用）。
fn print_extraction_result(result: &ch_knowledge::ExtractionResult) {
    println!("=== 知识提取（{}）===", result.extractor);
    println!();
    if !result.summary.is_empty() {
        println!("📖 摘要：{}", result.summary);
    }
    if !result.decisions.is_empty() {
        println!("\n🎯 决策（{}）：", result.decisions.len());
        for d in &result.decisions {
            println!("  • {}", d.decision);
        }
    }
    if !result.todos.is_empty() {
        println!("\n📋 TODO（{}）：", result.todos.len());
        for t in &result.todos {
            println!("  • {}", t.text);
        }
    }
    if !result.errors.is_empty() {
        println!("\n❌ 错误（{}）：", result.errors.len());
        for e in &result.errors {
            println!("  • {}", e.error);
        }
    }
    if !result.commands.is_empty() {
        println!("\n⚙️ 命令（{}）：", result.commands.len());
        for c in &result.commands {
            println!("  • {c}");
        }
    }
    if !result.files.is_empty() {
        println!("\n📄 涉及文件（{}）：", result.files.len());
        for f in &result.files {
            println!("  • {}", f.path);
        }
    }
    if result.summary.is_empty()
        && result.decisions.is_empty()
        && result.todos.is_empty()
        && result.errors.is_empty()
        && result.commands.is_empty()
        && result.files.is_empty()
    {
        println!("（未提取到知识结构）");
    }
}

/// 从环境变量读取备份密码。避免命令行明文（plan §14 安全）。
fn read_backup_password() -> Result<String> {
    match std::env::var("CH_BACKUP_PASSWORD") {
        Ok(p) if !p.is_empty() => Ok(p),
        _ => anyhow::bail!(
            "backup password required: set CH_BACKUP_PASSWORD environment variable (min 8 chars)"
        ),
    }
}

fn print_usage() {
    eprintln!("Conversation Hub CLI (MVP)");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  ch [--db <path>] <subcommand>");
    eprintln!();
    eprintln!("SUBCOMMANDS:");
    eprintln!("  import <md-file> [--workspace <name>]   Import a Markdown conversation");
    eprintln!("  import-from claude-code <list|session-id>  Import from ~/.claude (real data)");
    eprintln!("  import-from zcode <list|session-id>       Import from ~/.zcode (real data)");
    eprintln!("  list   [--workspace <id>] [--favorite] [--archived] [--provider <p>]  List/filter conversations");
    eprintln!("  show   <conversation-id>                Show conversation detail");
    eprintln!("  search <keyword...> [--provider <p>] [--workspace <id>]  Full-text search (FTS5)");
    eprintln!(
        "  search-tantivy <keyword...> [--provider <p>]            Full-text search (Tantivy)"
    );
    eprintln!("  export markdown <id> <out.md>           Export conversation to Markdown");
    eprintln!("  export json <id> <out.json>             Export conversation to JSON");
    eprintln!("  export workspace <id> <out-dir>         Export all conversations in a workspace");
    eprintln!("  backup <out.chbak>                      Encrypted backup (password via $CH_BACKUP_PASSWORD)");
    eprintln!("  restore <in.chbak> <target-dir>         Restore backup (password via $CH_BACKUP_PASSWORD)");
    eprintln!("  favorite <id>                           Mark conversation as favorite");
    eprintln!("  unfavorite <id>                         Remove favorite");
    eprintln!("  tag <id> <tag>                          Add a tag");
    eprintln!("  untag <id> <tag>                        Remove a tag");
    eprintln!("  tags <id>                               List tags");
    eprintln!("  favorites                               List favorite conversation ids");
    eprintln!("  archive <id>                            Archive conversation");
    eprintln!(
        "  delete <id> [--hard]                    Soft/hard delete conversation (plan §11.4)"
    );
    eprintln!("  undelete <id>                           Restore soft-deleted conversation");
    eprintln!("  integrity                               Run SQLite integrity check");
    eprintln!("  knowledge <id> [--save|--show]          Extract/save/show knowledge (plan §13.5)");
    eprintln!("  similar <id>                            Find similar conversations (plan §6.7)");
    eprintln!("  redaction-rule add <name> <regex>       Add custom redaction rule (plan §14.6)");
    eprintln!("  redaction-rule list                     List custom rules");
    eprintln!("  redaction-rule remove <name>            Remove a rule");
    eprintln!("  daemon                                  Start stdio JSON-RPC server (plan §8.2)");
    eprintln!("  help                                    Show this help");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("  --db <path>   Database file (default: ./conversation-hub.db)");
}

/// 从参数里提取 `--db <path>`（默认 `./conversation-hub.db`），返回剩余子命令。
fn extract_db(args: &[String]) -> (PathBuf, Vec<String>) {
    let mut db = PathBuf::from("./conversation-hub.db");
    let mut rest = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--db" {
            if let Some(v) = args.get(i + 1) {
                db = PathBuf::from(v);
                i += 2;
                continue;
            }
        }
        rest.push(args[i].clone());
        i += 1;
    }
    (db, rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_db_default() {
        let (db, rest) = extract_db(&["list".to_string()]);
        assert_eq!(db, PathBuf::from("./conversation-hub.db"));
        assert_eq!(rest, vec!["list"]);
    }

    #[test]
    fn extract_db_custom() {
        let (db, rest) = extract_db(&[
            "--db".to_string(),
            "/tmp/x.db".to_string(),
            "import".to_string(),
            "f.md".to_string(),
        ]);
        assert_eq!(db, PathBuf::from("/tmp/x.db"));
        assert_eq!(rest, vec!["import", "f.md"]);
    }

    #[test]
    fn extract_db_missing_value_falls_back() {
        // `--db` 后无值：保留 `--db` 在 rest，db 用默认
        let (db, rest) = extract_db(&["--db".to_string()]);
        assert_eq!(db, PathBuf::from("./conversation-hub.db"));
        assert_eq!(rest, vec!["--db"]);
    }
}
