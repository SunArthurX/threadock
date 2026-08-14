//! Claude Code ops 采集：JSONL 每条 assistant 消息的 `usage` 字段 → UsageRecord。

use crate::{infer_destructive, OpsResult};
use ch_domain::{Provider, ToolCallRecord, UsageRecord, UsageStatus};
use std::path::Path;

/// 采集一个 Claude Code 会话文件的用量。
/// 返回 (usage, tool_calls)。文件不存在返回空。
pub fn collect_claude_code_session(
    file_path: impl AsRef<Path>,
    session_id: &str,
) -> OpsResult<(Vec<UsageRecord>, Vec<ToolCallRecord>)> {
    let path = file_path.as_ref();
    if !path.exists() {
        return Ok((Vec::new(), Vec::new()));
    }
    let file = std::fs::File::open(path)?;
    let mut usage = Vec::new();
    let mut tools = Vec::new();
    let mut seq: i64 = 0;

    // 限行流式读取：跳过单行 >2MB 的负载（防内存尖峰）
    let mut reader = std::io::BufReader::new(file);
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    loop {
        let lr = crate::read_line_capped(&mut reader, &mut buf, crate::MAX_JSONL_LINE)?;
        if !lr.complete && buf.is_empty() {
            break;
        }
        if lr.oversized {
            continue;
        }
        let t = std::str::from_utf8(&buf).unwrap_or("").trim();
        if t.is_empty() {
            continue;
        }
        let Ok(rec) = serde_json::from_str::<serde_json::Value>(t) else {
            continue;
        };
        seq += 1;
        let ts = rec
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(|s| {
                time::OffsetDateTime::parse(
                    s,
                    &time::format_description::well_known::Rfc3339,
                )
                .ok()
            });

        // assistant 消息的 usage
        if rec.get("type").and_then(|v| v.as_str()) == Some("assistant") {
            let u = rec.pointer("/message/usage");
            if let Some(u) = u {
                let input = u.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                let output = u.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                let cache_read = u
                    .get("cache_read_input_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let cache_write = u
                    .get("cache_creation_input_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                if input + output + cache_read + cache_write > 0 {
                    usage.push(UsageRecord {
                        id: format!("cu_{session_id}_{seq}"),
                        provider: Provider::ClaudeCode,
                        source_session_id: session_id.to_string(),
                        turn_id: None,
                        model: rec
                            .pointer("/message/model")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        ts: ts.unwrap_or_else(ch_domain::now_utc),
                        input_tokens: input,
                        output_tokens: output,
                        reasoning_tokens: 0,
                        cache_read_tokens: cache_read,
                        cache_write_tokens: cache_write,
                        cost_usd: None,
                        status: UsageStatus::Completed,
                        duration_ms: None,
                        retry_count: None,
                    });
                }
            }
        }

        // tool_use → ToolCallRecord（command 从 input 提取）
        let content = rec.get("message").and_then(|m| m.get("content"));
        if let Some(arr) = content.and_then(|c| c.as_array()) {
            for item in arr {
                if item.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
                    let cmd = item
                        .pointer("/input/command")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    tools.push(ToolCallRecord {
                        id: format!("ct_{session_id}_{seq}_{name}"),
                        provider: Provider::ClaudeCode,
                        source_session_id: session_id.to_string(),
                        tool_name: name.to_string(),
                        ts: ts.unwrap_or_else(ch_domain::now_utc),
                        read_only: None,
                        destructive: cmd.as_deref().map(infer_destructive),
                        approval_status: None,
                        exit_code: None,
                        duration_ms: None,
                        status: UsageStatus::Completed,
                        command_text: cmd,
                    });
                }
            }
        }
    }
    Ok((usage, tools))
}

/// 采集整个 ~/.claude/projects 下所有会话。
pub fn collect_claude_code(claude_home: impl AsRef<Path>) -> OpsResult<(Vec<UsageRecord>, Vec<ToolCallRecord>)> {
    let projects = claude_home.as_ref().join("projects");
    if !projects.exists() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut all_usage = Vec::new();
    let mut all_tools = Vec::new();
    let mut files = Vec::new();
    scan_jsonl(&projects, &mut files);
    for f in files {
        let session_id = f
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if session_id.is_empty() {
            continue;
        }
        match collect_claude_code_session(&f, &session_id) {
            Ok((mut u, mut t)) => {
                all_usage.append(&mut u);
                all_tools.append(&mut t);
            }
            Err(e) => tracing::warn!(file = %f.display(), error = %e, "cc ops collect failed"),
        }
    }
    Ok((all_usage, all_tools))
}

fn scan_jsonl(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            scan_jsonl(&p, out);
        } else if p.extension().and_then(|x| x.to_str()) == Some("jsonl") {
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_usage_and_tool_calls() {
        let dir = tempfile::TempDir::new().unwrap();
        let f = dir.path().join("abc.jsonl");
        std::fs::write(
            &f,
            concat!(
                r#"{"timestamp":"2026-08-01T10:00:00.000Z","type":"user","message":{"role":"user","content":"hi"}}"#,
                "\n",
                r#"{"timestamp":"2026-08-01T10:00:05.000Z","type":"assistant","message":{"role":"assistant","model":"claude-x","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":200,"cache_creation_input_tokens":10}}}"#,
                "\n",
                r#"{"timestamp":"2026-08-01T10:00:06.000Z","type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Bash","input":{"command":"rm -rf /tmp/x"}}]}}"#,
            ),
        )
        .unwrap();

        let (usage, tools) = collect_claude_code_session(&f, "abc").unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].input_tokens, 100);
        assert_eq!(usage[0].cache_read_tokens, 200);
        assert_eq!(usage[0].model.as_deref(), Some("claude-x"));
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool_name, "Bash");
        assert_eq!(tools[0].destructive, Some(true));
    }

    #[test]
    fn missing_file_empty() {
        let (u, t) = collect_claude_code_session("/nonexistent/x.jsonl", "x").unwrap();
        assert!(u.is_empty());
        assert!(t.is_empty());
    }
}
