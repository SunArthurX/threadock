//! Claude Code 实时探测：`~/.claude/projects/<编码路径>/<uuid>.jsonl` 追加写入，
//! 文件 mtime 即活跃时间；尾部行解析出最近活动 / 标题 / token 用量。

use crate::tail::tail_lines;
use crate::types::{LiveActivity, LiveProgress, LiveSource, LiveTask};
use crate::{classify, home_dir, snippet, system_time_ms, BOARD_WINDOW_MS};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 扫描（`claude_home` 通常为 `~/.claude`）。
pub fn scan(now_ms: i64) -> Result<Vec<LiveTask>, String> {
    scan_home(&home_dir()?.join(".claude"), now_ms)
}

/// 指定根目录扫描（测试注入用）。
pub fn scan_home(claude_home: &Path, now_ms: i64) -> Result<Vec<LiveTask>, String> {
    let projects = claude_home.join("projects");
    if !projects.is_dir() {
        return Ok(vec![]);
    }
    let cutoff = now_ms - BOARD_WINDOW_MS;
    let mut tasks = Vec::new();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&projects)
        .map_err(|e| format!("read_dir {}: {e}", projects.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        for f in files.filter_map(|e| e.ok().map(|e| e.path())) {
            if f.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = std::fs::metadata(&f) else {
                continue;
            };
            let Some(mtime_ms) =
                system_time_ms(meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH))
            else {
                continue;
            };
            if mtime_ms < cutoff {
                continue;
            }
            let session_id = f
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            if session_id.is_empty() {
                continue;
            }
            let parsed = parse_tail(&f);
            let encoded_dir = dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let workspace = parsed
                .cwd
                .clone()
                .or_else(|| decode_project_dir(&encoded_dir));
            let title = parsed
                .summary
                .or(parsed.first_user_text)
                .or_else(|| workspace_fallback_label(workspace.as_deref(), &encoded_dir));
            tasks.push(LiveTask {
                source: LiveSource::ClaudeCode,
                session_id,
                title,
                workspace,
                originator: None,
                started_at_ms: meta.created().ok().and_then(system_time_ms),
                last_activity_ms: mtime_ms,
                state: classify(now_ms, mtime_ms),
                activity: parsed.activities.last().cloned(),
                progress: Some(parsed.progress),
            });
        }
    }
    Ok(tasks)
}

fn workspace_fallback_label(workspace: Option<&str>, encoded: &str) -> Option<String> {
    workspace.map(String::from).or_else(|| {
        let seg = encoded.rsplit('-').next().unwrap_or(encoded);
        (!seg.is_empty()).then(|| seg.to_string())
    })
}

/// 单会话活动条数上限（防御超大 tail 爆列表）。
const MAX_ACTIVITIES: usize = 400;

struct ParsedTail {
    /// 窗口内全部可展示活动，时间升序（旧→新）：面板摘要取末条，transcript 取尾部 N 条。
    activities: Vec<LiveActivity>,
    /// `{"type":"summary","summary":...}` 压缩标题
    summary: Option<String>,
    /// 窗口内最早一条真实用户输入（标题兜底）
    first_user_text: Option<String>,
    cwd: Option<String>,
    progress: LiveProgress,
}

/// 尾部解析：收集活动（倒序遍历后翻转成升序）+ 标题 / token / todo 进度 / cwd。
#[allow(clippy::too_many_lines)] // 解析主函数：行类型分支多，拆了反而散
fn parse_tail(path: &Path) -> ParsedTail {
    let mut out = ParsedTail {
        activities: Vec::new(),
        summary: None,
        first_user_text: None,
        cwd: None,
        progress: LiveProgress::default(),
    };
    let Ok((full, lines)) = tail_lines(path, 256 * 1024) else {
        return out;
    };
    // 标题取窗口内最早的用户输入：倒序遍历不断覆盖即得最早
    let mut earliest_user: Option<String> = None;
    for line in lines.iter().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue; // atis-latch 等非 JSON / 噪声行
        };
        match v.get("type").and_then(Value::as_str).unwrap_or("") {
            "assistant" => {
                if out.cwd.is_none() {
                    out.cwd = v.get("cwd").and_then(Value::as_str).map(String::from);
                }
                let msg = v.get("message").unwrap_or(&Value::Null);
                if let Some(usage) = msg.get("usage") {
                    let sum = |k: &str| usage.get(k).and_then(Value::as_i64).unwrap_or(0);
                    let total = sum("input_tokens")
                        + sum("output_tokens")
                        + sum("cache_read_input_tokens")
                        + sum("cache_creation_input_tokens");
                    if total > 0 {
                        out.progress.total_tokens = Some(total);
                    }
                }
                if let Some(act) = assistant_activity(msg) {
                    push_capped(&mut out.activities, act);
                }
                // 最新一条 TodoWrite 即当前任务清单
                if out.progress.todos_total.is_none() {
                    if let Some((done, total)) = todo_progress(msg) {
                        out.progress.todos_done = Some(done);
                        out.progress.todos_total = Some(total);
                    }
                }
            }
            "user" => {
                if out.cwd.is_none() {
                    out.cwd = v.get("cwd").and_then(Value::as_str).map(String::from);
                }
                if let Some(text) = user_text(v.get("message").unwrap_or(&Value::Null)) {
                    if !text.is_empty() {
                        earliest_user = Some(snippet(&text, 80));
                        push_capped(
                            &mut out.activities,
                            LiveActivity {
                                kind: "message".into(),
                                role: Some("user".into()),
                                snippet: snippet(&text, 160),
                                at_ms: entry_ts_ms(&v),
                            },
                        );
                    }
                }
            }
            "summary" => {
                if out.summary.is_none() {
                    out.summary = v
                        .get("summary")
                        .and_then(Value::as_str)
                        .map(|s| snippet(s, 80));
                }
            }
            "system" => {
                if let Some(text) = v
                    .get("content")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                {
                    push_capped(
                        &mut out.activities,
                        LiveActivity {
                            kind: "system".into(),
                            role: None,
                            snippet: snippet(text, 160),
                            at_ms: entry_ts_ms(&v),
                        },
                    );
                }
            }
            _ => {}
        }
    }
    // 全文件可读时（<256KB），头部的 summary 标题最可靠；窗口 earliest_user 兜底
    if full && out.summary.is_none() {
        for line in &lines {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if v.get("type").and_then(Value::as_str) == Some("summary") {
                out.summary = v
                    .get("summary")
                    .and_then(Value::as_str)
                    .map(|s| snippet(s, 80));
                break;
            }
        }
    }
    // 倒序入列 → 翻转成时间升序（旧 → 新）
    out.activities.reverse();
    out.first_user_text = earliest_user;
    out
}

/// 活动列表上限保护（倒序遍历时最新活动排前，超限直接丢弃更旧的）。
fn push_capped(list: &mut Vec<LiveActivity>, act: LiveActivity) {
    if list.len() < MAX_ACTIVITIES {
        list.push(act);
    }
}

/// 抽屉 transcript：窗口内最近 `limit` 条活动（时间升序）。
#[must_use]
pub fn transcript(path: &Path, limit: usize) -> Vec<LiveActivity> {
    let parsed = parse_tail(path);
    let start = parsed.activities.len().saturating_sub(limit);
    parsed.activities[start..].to_vec()
}

fn entry_ts_ms(v: &Value) -> i64 {
    v.get("timestamp")
        .and_then(Value::as_str)
        .and_then(crate::iso_to_ms)
        .unwrap_or(0)
}

/// assistant 消息 → 活动：优先取末尾 tool_use，其次 text。
fn assistant_activity(msg: &Value) -> Option<LiveActivity> {
    let content = msg.get("content")?.as_array()?;
    for item in content.iter().rev() {
        match item.get("type").and_then(Value::as_str).unwrap_or("") {
            "tool_use" => {
                let name = item.get("name").and_then(Value::as_str).unwrap_or("tool");
                return Some(LiveActivity {
                    kind: "tool".into(),
                    role: Some("assistant".into()),
                    snippet: snippet(&tool_summary(name, item.get("input")), 160),
                    at_ms: 0,
                });
            }
            "text" => {
                if let Some(t) = item.get("text").and_then(Value::as_str) {
                    let t = t.trim();
                    if !t.is_empty() {
                        return Some(LiveActivity {
                            kind: "message".into(),
                            role: Some("assistant".into()),
                            snippet: snippet(t, 160),
                            at_ms: 0,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// 最新 TodoWrite 的进度（done, total）。
fn todo_progress(msg: &Value) -> Option<(u32, u32)> {
    let content = msg.get("content")?.as_array()?;
    for item in content.iter().rev() {
        if item.get("type").and_then(Value::as_str) == Some("tool_use")
            && item.get("name").and_then(Value::as_str) == Some("TodoWrite")
        {
            let todos = item
                .get("input")
                .and_then(|i| i.get("todos"))
                .and_then(Value::as_array)?;
            let total = todos.len() as u32;
            let done = todos
                .iter()
                .filter(|t| t.get("status").and_then(Value::as_str) == Some("completed"))
                .count() as u32;
            return Some((done, total));
        }
    }
    None
}

/// user 消息正文：string 或 [{type:text}]；跳过 `<` 开头的命令/注入包装与 tool_result。
fn user_text(msg: &Value) -> Option<String> {
    match msg.get("content") {
        Some(Value::String(s)) => {
            if s.trim_start().starts_with('<') {
                None
            } else {
                Some(s.clone())
            }
        }
        Some(Value::Array(items)) => {
            for item in items.iter().rev() {
                if item.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(t) = item.get("text").and_then(Value::as_str) {
                        let t = t.trim();
                        if !t.is_empty() && !t.starts_with('<') {
                            return Some(t.to_string());
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// 工具摘要（与 adapter-claude-code 的 summarize 同语义）。
fn tool_summary(name: &str, input: Option<&Value>) -> String {
    let arg = |k: &str| {
        input
            .and_then(|i| i.get(k))
            .and_then(Value::as_str)
            .unwrap_or("")
    };
    match name {
        "Bash" => {
            let cmd = arg("command");
            if cmd.is_empty() {
                "Bash".into()
            } else {
                cmd.lines().next().unwrap_or("").trim().to_string()
            }
        }
        "Read" => format!("读取 {}", basename(arg("file_path"))),
        "Write" => format!("写入 {}", basename(arg("file_path"))),
        "Edit" | "MultiEdit" | "NotebookEdit" => format!("修改 {}", basename(arg("file_path"))),
        "TodoWrite" => "更新 TODO".into(),
        _ => format!("工具 {name}"),
    }
}

fn basename(p: &str) -> String {
    p.rsplit('/').next().unwrap_or(p).to_string()
}

/// `-Users-sunqingguang-Downloads-06.code-threadock` → 还原路径（存在才用）。
fn decode_project_dir(encoded: &str) -> Option<String> {
    let stripped = encoded.strip_prefix('-').unwrap_or(encoded);
    if stripped.is_empty() {
        return None;
    }
    let candidate = stripped.replace('-', "/");
    if Path::new(&candidate).is_dir() {
        Some(candidate)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LiveState;
    use std::io::Write;

    fn write_session(dir: &Path, project: &str, name: &str, lines: &[String]) -> PathBuf {
        let pd = dir.join("projects").join(project);
        std::fs::create_dir_all(&pd).expect("mkdir");
        let p = pd.join(name);
        let mut f = std::fs::File::create(&p).expect("create");
        for l in lines {
            writeln!(f, "{l}").expect("write");
        }
        p
    }

    fn now_ms() -> i64 {
        system_time_ms(std::time::SystemTime::now()).expect("clock")
    }

    const A: &str = r#"{"type":"assistant","timestamp":"2026-11-02T10:00:01.000Z","cwd":"/tmp/proj","message":{"role":"assistant","content":[{"type":"tool_use","name":"TodoWrite","input":{"todos":[{"content":"a","status":"completed"},{"content":"b","status":"in_progress"}]}},{"type":"tool_use","name":"Bash","input":{"command":"cargo test\nnpm test"}},{"type":"text","text":"开始跑测试"}],"usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":10,"cache_creation_input_tokens":5}}}"#;
    const U: &str = r#"{"type":"user","timestamp":"2026-11-02T09:59:00.000Z","cwd":"/tmp/proj","message":{"role":"user","content":"帮我跑一下测试"}}"#;
    const NOISE: &str = r#"{"type":"atis-latch","atis":"","sessionId":"x"}"#;

    #[test]
    fn parses_activity_title_progress() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_session(
            dir.path(),
            "-tmp-proj",
            "s1.jsonl",
            &[U.to_string(), A.to_string(), NOISE.to_string()],
        );
        let tasks = scan_home(dir.path(), now_ms()).expect("scan");
        assert_eq!(tasks.len(), 1);
        let t = &tasks[0];
        assert_eq!(t.session_id, "s1");
        assert_eq!(t.state, LiveState::Running);
        assert_eq!(t.workspace.as_deref(), Some("/tmp/proj"));
        // 最近活动是 assistant 的 text（tool_use 在 text 前，倒序取 text）
        let act = t.activity.as_ref().expect("activity");
        assert_eq!(act.kind, "message");
        assert_eq!(act.role.as_deref(), Some("assistant"));
        assert!(act.snippet.contains("开始跑测试"));
        // 标题：最早用户输入
        assert_eq!(t.title.as_deref(), Some("帮我跑一下测试"));
        // 进度：todo 1/2 + token 165
        let p = t.progress.as_ref().expect("progress");
        assert_eq!(p.todos_done, Some(1));
        assert_eq!(p.todos_total, Some(2));
        assert_eq!(p.total_tokens, Some(165));
    }

    #[test]
    fn stale_files_are_skipped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = write_session(dir.path(), "-tmp-proj", "old.jsonl", &[U.to_string()]);
        // mtime 拨回 2 小时前
        let past = now_ms() - 2 * 60 * 60 * 1000;
        filetime::set_file_times(
            &p,
            filetime::FileTime::from_unix_time(past / 1000, ((past % 1000) * 1_000_000) as u32),
            filetime::FileTime::from_unix_time(past / 1000, ((past % 1000) * 1_000_000) as u32),
        )
        .expect("touch");
        let tasks = scan_home(dir.path(), now_ms()).expect("scan");
        assert!(tasks.is_empty(), "30 分钟窗口外的会话不应上板");
    }

    #[test]
    fn iso_to_ms_roundtrip() {
        assert_eq!(crate::iso_to_ms("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(
            crate::iso_to_ms("2026-11-02T00:00:00Z"),
            Some(1_793_577_600_000)
        );
        assert_eq!(crate::iso_to_ms("garbage"), None);
    }
}
