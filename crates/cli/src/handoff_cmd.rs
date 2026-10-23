//! `ch handoff` 子命令：把归档会话交接到目标 Agent 的原生会话存储。
//!
//! ```text
//! ch handoff <claude-code|codex|zcode|minimax-code|cursor|all> \
//!    (--all | --ids <id,id,...> | --provider <p> | --workspace <id>) \
//!    [--cwd <path>] [--dest <path>] [--dry-run] [--no-context]
//! ```

use anyhow::{bail, Context, Result};
use ch_handoff::{loader, HandoffOptions, HandoffTarget};
use ch_storage::{ConversationFilter, Repository};
use std::path::PathBuf;

/// 会话选择方式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// 全部（排除软删除与子任务）。
    All,
    /// 按内部 id 点名。
    Ids(Vec<String>),
    /// 按来源 provider。
    Provider(String),
    /// 按 workspace id。
    Workspace(String),
}

/// 解析后的命令行参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffCliArgs {
    pub targets: Vec<HandoffTarget>,
    pub selection: Selection,
    pub cwd: Option<PathBuf>,
    pub dest: Option<PathBuf>,
    pub dry_run: bool,
    pub include_context: bool,
}

/// 解析 `handoff` 之后的参数。
pub fn parse_args(args: &[&str]) -> Result<HandoffCliArgs> {
    let Some((target_str, rest)) = args.split_first() else {
        bail!(
            "handoff: missing target (claude-code | codex | zcode | minimax-code | cursor | all)"
        );
    };
    let targets = if *target_str == "all" {
        HandoffTarget::all()
    } else {
        vec![target_str
            .parse::<HandoffTarget>()
            .map_err(|e| anyhow::anyhow!("{e}"))?]
    };

    let mut selection: Option<Selection> = None;
    let mut cwd = None;
    let mut dest = None;
    let mut dry_run = false;
    let mut include_context = true;

    let mut i = 0;
    while i < rest.len() {
        let take_value = |name: &str| -> Result<&str> {
            rest.get(i + 1)
                .copied()
                .with_context(|| format!("handoff: {name} requires a value"))
        };
        match rest[i] {
            "--all" => {
                selection = Some(Selection::All);
                i += 1;
            }
            "--ids" => {
                let v = take_value("--ids")?;
                let ids: Vec<String> = v
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
                if ids.is_empty() {
                    bail!("handoff: --ids got an empty list");
                }
                selection = Some(Selection::Ids(ids));
                i += 2;
            }
            "--provider" => {
                selection = Some(Selection::Provider(take_value("--provider")?.to_string()));
                i += 2;
            }
            "--workspace" => {
                selection = Some(Selection::Workspace(take_value("--workspace")?.to_string()));
                i += 2;
            }
            "--cwd" => {
                cwd = Some(PathBuf::from(take_value("--cwd")?));
                i += 2;
            }
            "--dest" => {
                dest = Some(PathBuf::from(take_value("--dest")?));
                i += 2;
            }
            "--dry-run" => {
                dry_run = true;
                i += 1;
            }
            "--no-context" => {
                include_context = false;
                i += 1;
            }
            other => bail!("handoff: unknown argument {other}"),
        }
    }

    let Some(selection) = selection else {
        bail!("handoff: choose conversations with --all | --ids <id,...> | --provider <p> | --workspace <id>");
    };
    if dest.is_some() && targets.len() > 1 {
        bail!("handoff: --dest only applies to a single target (not `all`)");
    }

    Ok(HandoffCliArgs {
        targets,
        selection,
        cwd,
        dest,
        dry_run,
        include_context,
    })
}

/// 执行交接并打印结果；任一目标有失败条目时返回错误（exit code 非零）。
pub fn run(repo: &Repository, args: &HandoffCliArgs) -> Result<()> {
    let (owned, missing) = match &args.selection {
        Selection::All => (
            loader::load_filtered(repo, &ConversationFilter::new())?,
            Vec::new(),
        ),
        Selection::Ids(ids) => loader::load_by_ids(repo, ids)?,
        Selection::Provider(p) => {
            let provider: ch_domain::Provider = p
                .parse()
                .map_err(|e| anyhow::anyhow!("handoff: bad --provider: {e}"))?;
            (
                loader::load_filtered(repo, &ConversationFilter::new().with_provider(provider))?,
                Vec::new(),
            )
        }
        Selection::Workspace(ws) => (
            loader::load_filtered(repo, &ConversationFilter::new().with_workspace(ws.clone()))?,
            Vec::new(),
        ),
    };
    for id in &missing {
        eprintln!("warn: conversation not found: {id}");
    }
    if owned.is_empty() {
        println!("nothing to hand off (0 conversations matched)");
        return Ok(());
    }

    let mut opts = HandoffOptions::new(ch_handoff::user_home());
    if let Some(cwd) = &args.cwd {
        opts.fallback_cwd.clone_from(cwd);
    }
    opts.dest_override.clone_from(&args.dest);
    opts.dry_run = args.dry_run;
    opts.include_context = args.include_context;
    // 与导出同策略：用户配置了自定义脱敏规则时，交接内容先脱敏再写入目标库。
    opts.redact_rules = loader::load_redaction_rules(repo);

    let inputs = loader::as_inputs(&owned);
    let mut total_failed = 0usize;
    for target in &args.targets {
        let result = ch_handoff::handoff_batch(&inputs, *target, &opts);
        let tag = if args.dry_run { " (dry-run)" } else { "" };
        println!(
            "== {target}{tag}: {} ok, {} failed ==",
            result.ok.len(),
            result.failed.len()
        );
        for o in &result.ok {
            println!(
                "  ✓ {}  →  {}  [{} items] {}",
                o.title, o.session_id, o.items_written, o.destination
            );
        }
        for f in &result.failed {
            println!("  ✗ {} ({}): {}", f.title, f.conversation_id, f.error);
        }
        total_failed += result.failed.len();
    }

    if total_failed > 0 {
        bail!("handoff finished with {total_failed} failure(s)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_domain::{Conversation, Message, Provider, Role};

    #[test]
    fn parse_single_target_with_ids() {
        let a = parse_args(&["claude-code", "--ids", "a,b", "--dry-run"]).expect("parse failed");
        assert_eq!(a.targets, vec![HandoffTarget::ClaudeCode]);
        assert_eq!(a.selection, Selection::Ids(vec!["a".into(), "b".into()]));
        assert!(a.dry_run);
        assert!(a.include_context);
    }

    #[test]
    fn parse_all_targets_all_conversations() {
        let a =
            parse_args(&["all", "--all", "--no-context", "--cwd", "/tmp/p"]).expect("parse failed");
        assert_eq!(a.targets.len(), 5);
        assert_eq!(a.selection, Selection::All);
        assert!(!a.include_context);
        assert_eq!(a.cwd, Some(PathBuf::from("/tmp/p")));
    }

    #[test]
    fn parse_rejects_missing_selection_and_bad_target() {
        assert!(parse_args(&["codex"]).is_err());
        assert!(parse_args(&["nope", "--all"]).is_err());
        assert!(parse_args(&["all", "--all", "--dest", "/x"]).is_err());
        assert!(parse_args(&["codex", "--ids"]).is_err());
    }

    #[test]
    fn run_hands_off_to_claude_dest() {
        let repo = Repository::open_in_memory().expect("repo failed");
        repo.upsert_provider(Provider::Codex)
            .expect("upsert failed");
        let mut c = Conversation::new(Provider::Codex, "src-1");
        c.title = Some("交接测试".into());
        let cid = repo.upsert_conversation(&c).expect("upsert failed");
        let mut m = Message::new(&cid, Role::User, 1);
        m.content_text = Some("继续做登录功能".into());
        repo.upsert_message(&m).expect("upsert failed");

        let dest = tempfile::tempdir().expect("tempdir creation failed");
        let args = HandoffCliArgs {
            targets: vec![HandoffTarget::ClaudeCode],
            selection: Selection::All,
            cwd: Some(PathBuf::from("/tmp/proj")),
            dest: Some(dest.path().to_path_buf()),
            dry_run: false,
            include_context: true,
        };
        run(&repo, &args).expect("run failed");
        let files: Vec<_> = walk_jsonl(dest.path());
        assert_eq!(files.len(), 1, "应生成 1 个 Claude 会话文件");
        let text = std::fs::read_to_string(&files[0]).expect("file I/O failed");
        assert!(text.contains("继续做登录功能"));
        assert!(text.contains("\"cwd\":\"/tmp/proj\""));
    }

    #[test]
    fn run_with_no_match_is_ok() {
        let repo = Repository::open_in_memory().expect("repo failed");
        let args = HandoffCliArgs {
            targets: vec![HandoffTarget::Codex],
            selection: Selection::All,
            cwd: None,
            dest: None,
            dry_run: true,
            include_context: true,
        };
        run(&repo, &args).expect("empty run should succeed");
    }

    fn walk_jsonl(dir: &std::path::Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    out.extend(walk_jsonl(&p));
                } else if p.extension().and_then(|x| x.to_str()) == Some("jsonl") {
                    out.push(p);
                }
            }
        }
        out
    }
}
