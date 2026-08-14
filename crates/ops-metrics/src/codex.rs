//! Codex ops 采集：JSONL `token_count.info.total_token_usage`（累计快照）
//! → 每会话一条 UsageRecord（取最后一条快照）；`function_call` → ToolCallRecord。

use crate::{infer_destructive, OpsResult};
use ch_domain::{Provider, ToolCallRecord, UsageRecord, UsageStatus};
use std::path::Path;

/// 采集一个 Codex 会话文件：返回该会话的（0 或 1 条）用量快照 + 工具调用。
pub fn collect_codex_session(
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
    // token_count 是累计值：记最后一条快照（时间 + 数值）
    let mut last_snapshot: Option<(time::OffsetDateTime, i64, i64, i64, i64)> = None;
    let mut source_dir: Option<String> = None;
    let mut seq: i64 = 0;

    // 限行流式读取：跳过单行 >2MB 的二进制/图片负载（防内存尖峰卡死）
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
                time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
            });

        // session_meta.cwd（归因）
        if rec.get("type").and_then(|v| v.as_str()) == Some("session_meta")
            && source_dir.is_none()
        {
            source_dir = rec
                .pointer("/payload/cwd")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
        if rec.get("type").and_then(|v| v.as_str()) == Some("event_msg") {
            let p_type = rec.pointer("/payload/type").and_then(|v| v.as_str());
            if p_type == Some("token_count") {
                if let Some(info) = rec.pointer("/payload/info") {
                    if let Some(u) = info.pointer("/total_token_usage") {
                        last_snapshot = Some((
                            ts.unwrap_or_else(ch_domain::now_utc),
                            u.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                            u.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                            u
                                .get("reasoning_output_tokens")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0),
                            u.get("cached_input_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                        ));
                    }
                }
            }
        } else if rec.get("type").and_then(|v| v.as_str()) == Some("response_item") {
            let p_type = rec.pointer("/payload/type").and_then(|v| v.as_str());
            if p_type == Some("function_call") || p_type == Some("custom_tool_call") {
                let name = rec
                    .pointer("/payload/name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("tool");
                let args = rec
                    .pointer("/payload/arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let cmd = extract_cmd_from_args(name, args);
                tools.push(ToolCallRecord {
                    id: format!("xt_{session_id}_{seq}"),
                    provider: Provider::Codex,
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

    if let Some((ts, input, output, reasoning, cached)) = last_snapshot {
        if input + output > 0 {
            usage.push(UsageRecord {
                id: format!("xu_{session_id}"),
                provider: Provider::Codex,
                source_session_id: session_id.to_string(),
                turn_id: None,
                model: Some("codex".into()),
                ts,
                input_tokens: input,
                output_tokens: output,
                reasoning_tokens: reasoning,
                cache_read_tokens: cached,
                cache_write_tokens: 0,
                cost_usd: None,
                status: UsageStatus::Completed,
                duration_ms: None,
                retry_count: None,
                source_dir: source_dir.clone(),
                context_exceeded: 0,
            });
        }
    }
    Ok((usage, tools))
}

/// 从 exec_command 参数中提取 cmd 文本。
fn extract_cmd_from_args(name: &str, args: &str) -> Option<String> {
    if name == "exec_command" || name == "shell" {
        let v: serde_json::Value = serde_json::from_str(args).unwrap_or_default();
        return v
            .get("cmd")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());
    }
    None
}

/// 采集整个 ~/.codex（sessions/ + archived_sessions/）。
/// 文件级并行（按 CPU 数分片，封顶 8 线程）：114 个文件从串行 ~6.5s 降至 ~1s。
pub fn collect_codex(codex_home: impl AsRef<Path>) -> OpsResult<(Vec<UsageRecord>, Vec<ToolCallRecord>)> {
    let home = codex_home.as_ref();
    let mut files = Vec::new();
    scan_jsonl(&home.join("sessions"), &mut files);
    scan_jsonl(&home.join("archived_sessions"), &mut files);

    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8);
    // 按大小交替分片让负载均衡（大文件分散到不同线程）
    files.sort_by_key(|f| std::cmp::Reverse(std::fs::metadata(f).map(|m| m.len()).unwrap_or(0)));
    let mut shards: Vec<Vec<std::path::PathBuf>> = vec![Vec::new(); n_threads];
    for (i, f) in files.into_iter().enumerate() {
        shards[i % n_threads].push(f);
    }

    let all_usage = std::sync::Mutex::new(Vec::new());
    let all_tools = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        let usage_ref = &all_usage;
        let tools_ref = &all_tools;
        for shard in &shards {
            s.spawn(move || {
                for f in shard {
                    let session_id = f
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if session_id.is_empty() {
                        continue;
                    }
                    match collect_codex_session(f, &session_id) {
                        Ok((u, t)) => {
                            usage_ref.lock().unwrap().extend(u);
                            tools_ref.lock().unwrap().extend(t);
                        }
                        Err(e) => tracing::warn!(file = %f.display(), error = %e, "codex ops collect failed"),
                    }
                }
            });
        }
    });
    Ok((all_usage.into_inner().unwrap(), all_tools.into_inner().unwrap()))
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
    fn collects_snapshot_and_tools() {
        let dir = tempfile::TempDir::new().unwrap();
        let f = dir.path().join("rollout-x.jsonl");
        std::fs::write(
            &f,
            concat!(
                r#"{"timestamp":"2026-08-01T10:00:00.000Z","type":"session_meta","payload":{"id":"x1"}}"#,
                "\n",
                r#"{"timestamp":"2026-08-01T10:00:01.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}}"#,
                "\n",
                r#"{"timestamp":"2026-08-01T10:00:02.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":50,"reasoning_output_tokens":10,"total_tokens":160}}}}"#,
                "\n",
                r#"{"timestamp":"2026-08-01T10:00:03.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":300,"cached_input_tokens":40,"output_tokens":80,"reasoning_output_tokens":20,"total_tokens":400}}}}"#,
                "\n",
                r#"{"timestamp":"2026-08-01T10:00:04.000Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"rm -rf /tmp/x\"}","call_id":"c1"}}"#,
            ),
        )
        .unwrap();

        let (usage, tools) = collect_codex_session(&f, "x1").unwrap();
        // 取最后一条快照（累计值）
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].input_tokens, 300);
        assert_eq!(usage[0].output_tokens, 80);
        assert_eq!(usage[0].reasoning_tokens, 20);
        assert_eq!(usage[0].cache_read_tokens, 40);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].destructive, Some(true));
        assert_eq!(tools[0].command_text.as_deref(), Some("rm -rf /tmp/x"));
    }

    #[test]
    fn missing_file_empty() {
        let (u, t) = collect_codex_session("/nonexistent/x.jsonl", "x").unwrap();
        assert!(u.is_empty());
        assert!(t.is_empty());
    }
}
