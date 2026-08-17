//! 洞察页面数据：活动节律 / 项目中心 / 知识库 / 提示词库 / 报告中心。
//!
//! 全部为对既有数据的聚合读取（新页面不引入新采集）。

use super::*;
use ch_daemon::DaemonState;
use tauri::Emitter;

// ── 活动节律 ────────────────────────────────────────────────────────────

/// 活动节律统计（热力图 / 时段分布 / 工具演化）。
#[tauri::command]
pub(crate) async fn activity_stats(
    state: tauri::State<'_, DaemonState>,
    days: Option<i64>,
) -> Result<ch_storage::ActivityStats, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    repo.activity_stats(days.unwrap_or(365))
        .map_err(|e| storage_err(e))
}

// ── 项目中心 ────────────────────────────────────────────────────────────

/// 项目卡片（usage.source_dir 口径，与成本页一致）。
#[tauri::command]
pub(crate) async fn projects_overview(
    state: tauri::State<'_, DaemonState>,
) -> Result<serde_json::Value, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let rows = repo.projects_overview().map_err(|e| storage_err(e))?;
    Ok(serde_json::json!({ "projects": rows }))
}

// ── 提示词库 ────────────────────────────────────────────────────────────

/// 最近用户提问（提示词库语料）。
#[tauri::command]
pub(crate) async fn recent_user_prompts(
    state: tauri::State<'_, DaemonState>,
    limit: Option<i64>,
) -> Result<serde_json::Value, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let rows = repo
        .recent_user_prompts(limit.unwrap_or(100))
        .map_err(|e| storage_err(e))?;
    Ok(serde_json::json!({ "prompts": rows }))
}

// ── 报告中心 ────────────────────────────────────────────────────────────

/// reports/ 目录下的历史周报列表。
#[tauri::command]
pub(crate) async fn list_reports(
    state: tauri::State<'_, DaemonState>,
) -> Result<Vec<serde_json::Value>, String> {
    let dir = state.data_dir.join("reports");
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&dir).map_err(|e| io_err(e))?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".html") {
            continue;
        }
        let meta = entry.metadata().map_err(|e| io_err(e))?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        out.push(serde_json::json!({
            "name": name,
            "size": meta.len(),
            "mtime_ms": mtime,
        }));
    }
    out.sort_by(|a, b| {
        b.get("mtime_ms")
            .and_then(|v| v.as_i64())
            .cmp(&a.get("mtime_ms").and_then(|v| v.as_i64()))
    });
    Ok(out)
}

/// 读取一份历史周报内容（文件名白名单校验，防路径穿越）。
#[tauri::command]
pub(crate) async fn read_report(
    state: tauri::State<'_, DaemonState>,
    name: String,
) -> Result<String, String> {
    if name.contains('/') || name.contains('\\') || name.contains("..") || !name.ends_with(".html")
    {
        return Err("非法报告文件名".into());
    }
    let path = state.data_dir.join("reports").join(&name);
    std::fs::read_to_string(&path).map_err(|e| io_err(e))
}

// ── 知识库 ──────────────────────────────────────────────────────────────

/// 全库批量知识提取（幂等：已提取的会话跳过，force 重提）。
/// 重活：逐会话正则提取，经 sync_progress 事件上报进度。
#[tauri::command]
pub(crate) async fn knowledge_extract_all(
    state: tauri::State<'_, DaemonState>,
    app: tauri::AppHandle,
    force: Option<bool>,
) -> Result<serde_json::Value, String> {
    let force = force.unwrap_or(false);
    let app_done = app.clone();
    let r = run_blocking(move || {
        let convs = {
            let repo = state.repo.lock().map_err(|e| storage_err(e))?;
            repo.list_conversations(None).map_err(|e| storage_err(e))?
        };
        let total = convs.len() as u64;
        let mut done: u64 = 0;
        let mut extracted = 0u64;
        let mut skipped = 0u64;
        for c in &convs {
            if !force {
                let has = {
                    let repo = state.repo.lock().map_err(|e| storage_err(e))?;
                    repo.get_knowledge(&c.id)
                        .map(|k| k.is_some())
                        .unwrap_or(false)
                };
                if has {
                    skipped += 1;
                    done += 1;
                    continue;
                }
            }
            let extracted_json = {
                let repo = state.repo.lock().map_err(|e| storage_err(e))?;
                let msgs = match repo.list_messages(&c.id) {
                    Ok(m) => m,
                    Err(_) => {
                        done += 1;
                        continue;
                    }
                };
                let events = repo.list_events(&c.id).unwrap_or_default();
                let input = ch_knowledge::ExtractionInput {
                    title: Some(c.effective_title().to_string()),
                    messages: msgs,
                    events,
                };
                let result = ch_knowledge::RuleExtractor::new().extract(&input);
                serde_json::to_string(&result).map_err(|e| import_err(e))?
            };
            {
                let repo = state.repo.lock().map_err(|e| storage_err(e))?;
                let _ = repo.save_knowledge(&c.id, "rule-v1", &extracted_json);
            }
            extracted += 1;
            done += 1;
            if done.is_multiple_of(5) || done == total {
                let _ = app.emit(
                    "sync_progress",
                    serde_json::json!({
                        "current": done,
                        "total": total,
                        "detail": "知识提取",
                        "finished": done == total,
                    }),
                );
            }
        }
        Ok::<serde_json::Value, String>(serde_json::json!({
            "conversations": total,
            "extracted": extracted,
            "skipped": skipped,
        }))
    })?;
    // 完成回报（与同步进度条协议一致）
    let _ = app_done.emit(
        "sync_progress",
        serde_json::json!({ "current": 1, "total": 1, "detail": "done", "finished": true }),
    );
    Ok(r)
}

/// 知识库全局聚合：TODO / 决策 / 常用命令 / 文件热度（可跳转原会话）。
#[tauri::command]
pub(crate) async fn knowledge_base_list(
    state: tauri::State<'_, DaemonState>,
) -> Result<serde_json::Value, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let convs = repo.list_conversations(None).map_err(|e| storage_err(e))?;
    let title_of = |id: &str| -> String {
        convs
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.effective_title().to_string())
            .unwrap_or_default()
    };

    let mut todos: Vec<serde_json::Value> = Vec::new();
    let mut decisions: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();
    let mut summaries: Vec<serde_json::Value> = Vec::new();
    // (count, last_conversation_id) —— 记最近一次出现该 cmd/file 的会话以便跳转
    let mut commands: std::collections::HashMap<String, (i64, String)> = Default::default();
    let mut files: std::collections::HashMap<String, (i64, String)> = Default::default();
    let mut extracted = 0u64;

    for c in &convs {
        let Some(rec) = repo.get_knowledge(&c.id).map_err(|e| storage_err(e))? else {
            continue;
        };
        extracted += 1;
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&rec.result_json) else {
            continue;
        };
        // 摘要（P1-B3 新增 6 类之一）
        if summaries.len() < 200 {
            if let Some(s) = v.get("summary").and_then(|x| x.as_str()) {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    summaries.push(serde_json::json!({
                        "summary": trimmed,
                        "conversation_id": c.id,
                        "title": title_of(&c.id),
                        "message_id": "",
                    }));
                }
            }
        }
        for d in v
            .get("decisions")
            .and_then(|x| x.as_array())
            .map(|a| a.as_slice())
            .unwrap_or_default()
        {
            if decisions.len() < 200 {
                let mid = d
                    .get("source_message_ids")
                    .and_then(|x| x.as_array())
                    .and_then(|a| a.first())
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                decisions.push(serde_json::json!({
                    "text": d.get("decision").and_then(|x| x.as_str()).unwrap_or(""),
                    "conversation_id": c.id,
                    "title": title_of(&c.id),
                    "message_id": mid,
                }));
            }
        }
        for t in v
            .get("todos")
            .and_then(|x| x.as_array())
            .map(|a| a.as_slice())
            .unwrap_or_default()
        {
            if todos.len() < 300 {
                let mid = t
                    .get("source_message_ids")
                    .and_then(|x| x.as_array())
                    .and_then(|a| a.first())
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                todos.push(serde_json::json!({
                    "text": t.get("text").and_then(|x| x.as_str()).unwrap_or(""),
                    "conversation_id": c.id,
                    "title": title_of(&c.id),
                    "message_id": mid,
                }));
            }
        }
        for e in v
            .get("errors")
            .and_then(|x| x.as_array())
            .map(|a| a.as_slice())
            .unwrap_or_default()
        {
            if errors.len() < 200 {
                let mid = e
                    .get("source_message_ids")
                    .and_then(|x| x.as_array())
                    .and_then(|a| a.first())
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                errors.push(serde_json::json!({
                    "error": e.get("error").and_then(|x| x.as_str()).unwrap_or(""),
                    "conversation_id": c.id,
                    "title": title_of(&c.id),
                    "message_id": mid,
                }));
            }
        }
        for cmd in v
            .get("commands")
            .and_then(|x| x.as_array())
            .map(|a| a.as_slice())
            .unwrap_or_default()
        {
            if let Some(s) = cmd.as_str() {
                let entry = commands.entry(s.to_string()).or_insert((0, String::new()));
                entry.0 += 1;
                entry.1 = c.id.clone();
            }
        }
        for f in v
            .get("files")
            .and_then(|x| x.as_array())
            .map(|a| a.as_slice())
            .unwrap_or_default()
        {
            if let Some(s) = f.get("path").and_then(|x| x.as_str()) {
                let entry = files.entry(s.to_string()).or_insert((0, String::new()));
                entry.0 += 1;
                entry.1 = c.id.clone();
            }
        }
    }

    let mut top_commands: Vec<serde_json::Value> = commands
        .into_iter()
        .map(|(cmd, (n, cid))| {
            serde_json::json!({
                "cmd": cmd,
                "count": n,
                "last_conversation_id": cid,
            })
        })
        .collect();
    top_commands.sort_by(|a, b| {
        b.get("count")
            .and_then(|v| v.as_i64())
            .cmp(&a.get("count").and_then(|v| v.as_i64()))
    });
    top_commands.truncate(20);

    let mut top_files: Vec<serde_json::Value> = files
        .into_iter()
        .map(|(path, (n, cid))| {
            serde_json::json!({
                "path": path,
                "count": n,
                "last_conversation_id": cid,
            })
        })
        .collect();
    top_files.sort_by(|a, b| {
        b.get("count")
            .and_then(|v| v.as_i64())
            .cmp(&a.get("count").and_then(|v| v.as_i64()))
    });
    top_files.truncate(20);

    let last_ms = repo
        .get_setting("last_knowledge_extract_ms")
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    // P1-E5：pending = 未提取的会话；versions = 每会话当前版本号
    let mut pending: Vec<serde_json::Value> = Vec::new();
    let mut versions: Vec<serde_json::Value> = Vec::new();
    for c in &convs {
        if repo
            .get_knowledge(&c.id)
            .map_err(|e| storage_err(e))?
            .is_none()
        {
            let updated = ch_storage::timestamp::to_millis(c.updated_at).unwrap_or(0);
            let provider_str = serde_json::to_value(&c.provider)
                .ok()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| format!("{:?}", c.provider));
            pending.push(serde_json::json!({
                "id": c.id,
                "title": c.effective_title(),
                "updated_at_ms": updated,
                "provider": provider_str,
            }));
        } else if let Ok(Some(rec)) = repo.get_knowledge(&c.id) {
            versions.push(serde_json::json!({
                "conversation_id": c.id,
                "title": c.effective_title(),
                "version": rec.version,
                "extractor": rec.extractor,
                "extracted_at": ch_storage::timestamp::to_millis(Some(rec.updated_at)).unwrap_or(0),
            }));
        }
    }
    pending.sort_by(|a, b| {
        b.get("updated_at_ms")
            .and_then(|v| v.as_i64())
            .cmp(&a.get("updated_at_ms").and_then(|v| v.as_i64()))
    });
    pending.truncate(50);
    versions.sort_by(|a, b| {
        b.get("extracted_at")
            .and_then(|v| v.as_i64())
            .cmp(&a.get("extracted_at").and_then(|v| v.as_i64()))
    });
    versions.truncate(50);

    Ok(serde_json::json!({
        "extracted": extracted,
        "total_conversations": convs.len(),
        "last_extract_ms": last_ms,
        "todos": todos,
        "decisions": decisions,
        "errors": errors,
        "summaries": summaries,
        "top_commands": top_commands,
        "top_files": top_files,
        "pending": pending,
        "versions": versions,
    }))
}

#[cfg(test)]
mod activity_tests {
    use ch_daemon::{DaemonState, DaemonStateConfig};

    /// 真实库副本验证 activity_stats 三块都有数据（页面空数据排查）。
    /// cargo test --lib activity_real -- --ignored --nocapture
    #[test]
    #[ignore = "依赖本机真实 app 数据副本"]
    fn activity_stats_on_real_copy() {
        let app = std::path::PathBuf::from(std::env::var("HOME").expect("unexpected None"))
            .join("Library/Application Support/com.threadock.desktop");
        if !app.join("threadock.db").exists() {
            panic!("本机无真实 app 数据");
        }
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        std::fs::copy(app.join("threadock.db"), dir.path().join("threadock.db"))
            .expect("file I/O failed");
        let state = DaemonState::open(DaemonStateConfig {
            data_dir: dir.path().to_path_buf(),
        })
        .expect("state open");
        let repo = state.repo.lock().expect("mutex poisoned");
        let stats = repo.activity_stats(365).expect("activity");
        println!(
            "heat={} hours={} tools={}",
            stats.heatmap.len(),
            stats.hourly.len(),
            stats.tools_trend.len()
        );
        println!("heat sample: {:?}", stats.heatmap.first());
        println!("hours sample: {:?}", stats.hourly.first());
        assert!(!stats.heatmap.is_empty(), "热力图必须有数据");
        assert!(!stats.hourly.is_empty(), "时段分布必须有数据");
        assert!(!stats.tools_trend.is_empty(), "工具趋势必须有数据");
    }
}
