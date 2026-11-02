//! Codex（CLI + Desktop 共用）实时探测：`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`。
//! 首行 `session_meta` 带 `originator`（`codex_work_desktop` / `codex_cli_rs` …），
//! 可区分桌面端与 CLI；尾部 `event_msg` 是进度主信号。

use crate::tail::tail_lines;
use crate::types::{LiveActivity, LiveProgress, LiveSource, LiveState, LiveTask};
use crate::{classify, home_dir, iso_to_ms, snippet, system_time_ms, BOARD_WINDOW_MS};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 尾部窗口（与 claude 一致：256KB 足够覆盖活跃段）。
const TAIL_BYTES: u64 = 256 * 1024;

/// 注入型正文前缀（与 adapter-codex 的 INJECTED_TAGS 同源，防污染标题/摘要）。
const INJECTED_TAGS: &[&str] = &[
    "<environment_context>",
    "<user_instructions>",
    "<turn_context>",
    "<turn_aborted>",
    "<IDE_INFORMATION>",
    "<system-reminder>",
    "<ENVIRONMENT",
    "<codex_internal_context>",
    "<subagent_notification>",
    "<goal_context>",
    "<send_user_message_question_reply>",
    "<heartbeat>",
    "<external_codex_",
];

pub fn scan(now_ms: i64) -> Result<Vec<LiveTask>, String> {
    scan_home(&home_dir()?.join(".codex"), now_ms)
}

/// 指定根目录扫描（测试注入用）。
pub fn scan_home(codex_home: &Path, now_ms: i64) -> Result<Vec<LiveTask>, String> {
    let sessions = codex_home.join("sessions");
    if !sessions.is_dir() {
        return Ok(vec![]);
    }
    let cutoff = now_ms - BOARD_WINDOW_MS;
    let mut files = Vec::new();
    collect_rollouts(&sessions, &mut files);
    let mut tasks = Vec::new();
    for f in files {
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
        let meta_line = first_session_meta(&f);
        let session_id = meta_line
            .as_ref()
            .and_then(|m| {
                m.get("id")
                    .or_else(|| m.get("session_id"))
                    .and_then(Value::as_str)
            })
            .map_or_else(
                || {
                    // rollout-2026-10-27T10-35-18-<uuid>.jsonl → uuid 段兜底
                    f.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default()
                        .rsplit('-')
                        .take(5)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>()
                        .join("-")
                },
                String::from,
            );
        if session_id.is_empty() {
            continue;
        }
        let parsed = parse_tail(&f);
        let cwd = meta_line
            .as_ref()
            .and_then(|m| m.get("cwd").and_then(Value::as_str))
            .map(String::from)
            .or(parsed.cwd);
        let originator = meta_line
            .as_ref()
            .and_then(|m| m.get("originator").and_then(Value::as_str))
            .map(String::from);
        let title = parsed.first_user_text.or_else(|| {
            cwd.as_ref()
                .and_then(|c| Path::new(c).file_name())
                .map(|n| n.to_string_lossy().to_string())
        });
        let state = if parsed.waiting && classify(now_ms, mtime_ms) == LiveState::Running {
            LiveState::Waiting
        } else {
            classify(now_ms, mtime_ms)
        };
        tasks.push(LiveTask {
            source: LiveSource::Codex,
            session_id,
            title,
            workspace: cwd,
            originator,
            started_at_ms: meta.created().ok().and_then(system_time_ms),
            last_activity_ms: mtime_ms,
            state,
            activity: parsed.activities.last().cloned(),
            progress: Some(parsed.progress),
        });
    }
    Ok(tasks)
}

fn collect_rollouts(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.filter_map(std::result::Result::ok) {
        let p = e.path();
        if p.is_dir() {
            collect_rollouts(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(p);
        }
    }
}

/// 读首行拿 session_meta payload（失败/缺失返回 None，由尾部兜底）。
#[must_use]
fn first_session_meta(path: &Path) -> Option<Value> {
    let f = std::fs::File::open(path).ok()?;
    let mut head = vec![0u8; 8 * 1024];
    let mut f = f;
    let n = std::io::Read::read(&mut f, &mut head).ok()?;
    let buf = String::from_utf8_lossy(&head[..n]);
    let line = buf.lines().next()?;
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(Value::as_str) == Some("session_meta") {
        v.get("payload").cloned()
    } else {
        None
    }
}

/// 单会话活动条数上限。
const MAX_ACTIVITIES: usize = 400;

struct ParsedTail {
    /// 窗口内全部可展示活动，时间升序（旧→新）。
    activities: Vec<LiveActivity>,
    waiting: bool,
    cwd: Option<String>,
    first_user_text: Option<String>,
    progress: LiveProgress,
}

#[allow(clippy::too_many_lines)] // 解析主函数：行类型分支多，拆了反而散
fn parse_tail(path: &Path) -> ParsedTail {
    let mut out = ParsedTail {
        activities: Vec::new(),
        waiting: false,
        cwd: None,
        first_user_text: None,
        progress: LiveProgress::default(),
    };
    let Ok((_full, lines)) = tail_lines(path, TAIL_BYTES) else {
        return out;
    };
    let mut earliest_user: Option<String> = None;
    for line in lines.iter().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let ts_ms = v
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(iso_to_ms)
            .unwrap_or(0);
        match v.get("type").and_then(Value::as_str).unwrap_or("") {
            "event_msg" => {
                let payload = v.get("payload").unwrap_or(&Value::Null);
                let ptype = payload.get("type").and_then(Value::as_str).unwrap_or("");
                if ptype.contains("approval") {
                    out.waiting = true;
                }
                match ptype {
                    "agent_message" => {
                        if let Some(t) = payload
                            .get("message")
                            .or_else(|| payload.get("text"))
                            .and_then(Value::as_str)
                            .filter(|t| !t.trim().is_empty())
                        {
                            push_capped(
                                &mut out.activities,
                                LiveActivity {
                                    kind: "message".into(),
                                    role: Some("assistant".into()),
                                    snippet: snippet(t, 160),
                                    at_ms: ts_ms,
                                },
                            );
                        }
                    }
                    "agent_reasoning" | "reasoning" => {
                        if let Some(t) = payload
                            .get("text")
                            .and_then(Value::as_str)
                            .filter(|t| !t.trim().is_empty())
                        {
                            push_capped(
                                &mut out.activities,
                                LiveActivity {
                                    kind: "thinking".into(),
                                    role: Some("assistant".into()),
                                    snippet: snippet(t, 160),
                                    at_ms: ts_ms,
                                },
                            );
                        }
                    }
                    "item_started" | "item_completed" => {
                        let item = payload.get("item").unwrap_or(&Value::Null);
                        if let Some(act) = item_activity(item, ptype == "item_started", ts_ms) {
                            push_capped(&mut out.activities, act);
                        }
                    }
                    "token_count" => {
                        if out.progress.total_tokens.is_none() {
                            let total = payload
                                .get("info")
                                .and_then(|i| i.get("total_token_usage"))
                                .and_then(|u| u.get("total_tokens"))
                                .and_then(Value::as_i64);
                            if let Some(t) = total {
                                out.progress.total_tokens = Some(t);
                            }
                        }
                    }
                    "user_message" => {
                        if let Some(t) = payload.get("message").and_then(Value::as_str) {
                            if !t.trim().is_empty() {
                                earliest_user = Some(snippet(t, 80));
                                push_capped(
                                    &mut out.activities,
                                    LiveActivity {
                                        kind: "message".into(),
                                        role: Some("user".into()),
                                        snippet: snippet(t, 160),
                                        at_ms: ts_ms,
                                    },
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
            "response_item" => {
                let payload = v.get("payload").unwrap_or(&Value::Null);
                if payload.get("type").and_then(Value::as_str) == Some("message") {
                    let role = payload.get("role").and_then(Value::as_str).unwrap_or("");
                    let text = message_text(payload);
                    if text.is_empty() || INJECTED_TAGS.iter().any(|tag| text.starts_with(tag)) {
                        continue;
                    }
                    if role == "user" {
                        earliest_user = Some(snippet(&text, 80));
                    }
                    push_capped(
                        &mut out.activities,
                        LiveActivity {
                            kind: "message".into(),
                            role: Some(role).filter(|r| !r.is_empty()).map(String::from),
                            snippet: snippet(&text, 160),
                            at_ms: ts_ms,
                        },
                    );
                }
            }
            "session_meta" => {
                let payload = v.get("payload").unwrap_or(&Value::Null);
                if out.cwd.is_none() {
                    out.cwd = payload.get("cwd").and_then(Value::as_str).map(String::from);
                }
            }
            _ => {}
        }
    }
    // 倒序入列 → 翻转成时间升序；同一内容会同时出现在 response_item 与
    // event_msg（Codex 双写），按 kind+snippet 相邻去重
    out.activities.reverse();
    out.activities
        .dedup_by(|a, b| a.kind == b.kind && a.snippet == b.snippet);
    out.first_user_text = earliest_user;
    out
}

/// 活动列表上限保护。
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

/// item_started/completed 的 item → 活动（CommandExecution / McpToolCall / FileChange / Reasoning）。
fn item_activity(item: &Value, started: bool, at_ms: i64) -> Option<LiveActivity> {
    let itype = item.get("type").and_then(Value::as_str)?;
    let verb = |zh: &str| {
        if started {
            format!("▶ {zh}")
        } else {
            zh.to_string()
        }
    };
    let (kind, snippet_text) = match itype {
        "CommandExecution" => {
            let cmd = item
                .get("command")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            let first = cmd.lines().next().unwrap_or("命令").to_string();
            ("tool".to_string(), verb(&first))
        }
        "McpToolCall" => {
            let server = item.get("server").and_then(Value::as_str).unwrap_or("mcp");
            let tool = item.get("tool").and_then(Value::as_str).unwrap_or("tool");
            ("tool".to_string(), format!("{server}·{tool}"))
        }
        "FileChange" => {
            let files = item.get("changes").and_then(Value::as_array).map(|a| {
                a.iter()
                    .filter_map(|c| c.get("path").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(", ")
            });
            (
                "tool".to_string(),
                verb(&files.map_or_else(|| "改文件".to_string(), |f| format!("改文件 {f}"))),
            )
        }
        "Reasoning" => ("thinking".to_string(), String::new()),
        _ => return None,
    };
    if snippet_text.is_empty() && kind == "thinking" {
        if let Some(t) = item.get("text").and_then(Value::as_str) {
            return Some(LiveActivity {
                kind,
                role: Some("assistant".into()),
                snippet: snippet(t, 160),
                at_ms,
            });
        }
        return None;
    }
    Some(LiveActivity {
        kind,
        role: Some("assistant".into()),
        snippet: snippet_text,
        at_ms,
    })
}

/// response_item message 的 content[].text 拼接。
fn message_text(payload: &Value) -> String {
    payload
        .get("content")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|c| c.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn now_ms() -> i64 {
        system_time_ms(std::time::SystemTime::now()).expect("clock")
    }

    fn write_rollout(dir: &Path, name: &str, lines: &[String]) -> PathBuf {
        let dd = dir.join("sessions").join("2026").join("11").join("02");
        std::fs::create_dir_all(&dd).expect("mkdir");
        let p = dd.join(name);
        let mut f = std::fs::File::create(&p).expect("create");
        for l in lines {
            writeln!(f, "{l}").expect("write");
        }
        p
    }

    const META: &str = r#"{"timestamp":"2026-11-02T10:00:00.000Z","ordinal":0,"type":"session_meta","payload":{"id":"codex-1","cwd":"/tmp/proj","originator":"codex_work_desktop"}}"#;
    const USER_ITEM: &str = r#"{"timestamp":"2026-11-02T10:00:05.000Z","ordinal":1,"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"修复这个 bug"}]}}"#;
    const INJECTED: &str = r#"{"timestamp":"2026-11-02T10:00:06.000Z","ordinal":2,"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<turn_context>noise</turn_context>"}]}}"#;
    const REASONING: &str = r#"{"timestamp":"2026-11-02T10:00:07.000Z","ordinal":3,"type":"event_msg","payload":{"type":"agent_reasoning","text":"正在分析"}}"#;
    const EXEC: &str = r#"{"timestamp":"2026-11-02T10:00:08.000Z","ordinal":4,"type":"event_msg","payload":{"type":"item_started","item":{"type":"CommandExecution","command":["cargo","test"]}}}"#;
    const APPROVAL: &str = r#"{"timestamp":"2026-11-02T10:00:09.000Z","ordinal":5,"type":"event_msg","payload":{"type":"exec_approval_request","thread_id":"codex-1"}}"#;
    const TOKENS: &str = r#"{"timestamp":"2026-11-02T10:00:10.000Z","ordinal":6,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":4321}}}}"#;

    #[test]
    fn parses_desktop_session_with_waiting_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_rollout(
            dir.path(),
            "rollout-x.jsonl",
            &[
                META.to_string(),
                USER_ITEM.to_string(),
                INJECTED.to_string(),
                REASONING.to_string(),
                EXEC.to_string(),
                APPROVAL.to_string(),
                TOKENS.to_string(),
            ],
        );
        let tasks = scan_home(dir.path(), now_ms()).expect("scan");
        assert_eq!(tasks.len(), 1);
        let t = &tasks[0];
        assert_eq!(t.source, LiveSource::Codex);
        assert_eq!(t.session_id, "codex-1");
        assert_eq!(t.originator.as_deref(), Some("codex_work_desktop"));
        assert_eq!(t.workspace.as_deref(), Some("/tmp/proj"));
        assert_eq!(t.state, LiveState::Waiting, "审批请求 → Waiting");
        assert_eq!(t.title.as_deref(), Some("修复这个 bug"));
        let act = t.activity.as_ref().expect("activity");
        assert_eq!(act.kind, "tool");
        assert!(act.snippet.contains("cargo test"), "{:?}", act.snippet);
        assert_eq!(
            t.progress.as_ref().expect("progress").total_tokens,
            Some(4321)
        );
    }

    #[test]
    fn stale_and_zst_skipped() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_rollout(dir.path(), "rollout-z.jsonl.zst", &[META.to_string()]);
        let stale = now_ms() - 60 * 60 * 1000;
        let p = write_rollout(dir.path(), "rollout-old.jsonl", &[META.to_string()]);
        let ft = filetime::FileTime::from_unix_time(stale / 1000, 0);
        filetime::set_file_times(&p, ft, ft).expect("touch");
        let tasks = scan_home(dir.path(), now_ms()).expect("scan");
        assert!(tasks.is_empty());
    }

    #[test]
    fn filename_uuid_fallback_when_no_meta() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_rollout(
            dir.path(),
            "rollout-2026-11-02T10-00-00-abcde123-4567-8901-2345-6789abcdef01.jsonl",
            &[USER_ITEM.to_string()],
        );
        let tasks = scan_home(dir.path(), now_ms()).expect("scan");
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].session_id, "abcde123-4567-8901-2345-6789abcdef01",
            "无 session_meta 时从文件名取 uuid"
        );
    }
}
