//! 数据维护域：存储看板、孤儿 GC、保留策略、周报自动生成。
//!
//! 全部为本地维护动作，敏感操作（GC/保留归档）写入治理操作流水。

use super::*;
use ch_daemon::DaemonState;
use tauri::Emitter;

/// 存储占用统计。
#[derive(serde::Serialize)]
pub(crate) struct StorageStats {
    pub db_bytes: u64,
    pub raw_count: u64,
    pub raw_bytes: u64,
    pub index_bytes: u64,
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for e in entries.flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => {
                    total += dir_size(&e.path());
                }
                Ok(_) => {
                    total += std::fs::metadata(e.path()).map(|m| m.len()).unwrap_or(0);
                }
                Err(_) => {}
            }
        }
    }
    total
}

/// 三块存储（DB / raw blob / 搜索索引）的占用看板。
#[tauri::command]
pub(crate) async fn storage_stats(
    state: tauri::State<'_, DaemonState>,
) -> Result<StorageStats, String> {
    let data_dir = &state.data_dir;
    let raw = state
        .raw_store
        .lock()
        .map_err(|e| storage_err(e))?
        .stats()
        .map_err(|e| io_err(e))?;
    Ok(StorageStats {
        db_bytes: std::fs::metadata(data_dir.join("threadock.db"))
            .map(|m| m.len())
            .unwrap_or(0),
        raw_count: raw.count,
        raw_bytes: raw.bytes,
        index_bytes: dir_size(&data_dir.join("index")),
    })
}

/// GC 核心：删除不被任何会话引用、且落盘超过 1 小时（防与导入竞态）的 blob。
/// 返回 (扫描数, 删除数, 释放字节)。
fn gc_raw_inner(state: &DaemonState) -> Result<(usize, usize, u64), String> {
    let refs = {
        let repo = state.repo.lock().map_err(|e| storage_err(e))?;
        repo.list_raw_payload_refs().map_err(|e| storage_err(e))?
    };
    let raw_store = state.raw_store.lock().map_err(|e| storage_err(e))?;
    let blobs = raw_store.list_blobs().map_err(|e| io_err(e))?;
    let mut deleted = 0usize;
    let mut freed = 0u64;
    for (hash, size) in &blobs {
        if refs.contains(hash) {
            continue;
        }
        if let Ok(p) = raw_store.path_of(hash) {
            if let Ok(meta) = std::fs::metadata(&p) {
                if let Ok(mtime) = meta.modified() {
                    if mtime.elapsed().map(|d| d.as_secs() < 3600).unwrap_or(true) {
                        continue;
                    }
                }
            }
        }
        if raw_store.delete(hash).is_ok() {
            deleted += 1;
            freed += size;
        }
    }
    drop(raw_store);
    if deleted > 0 {
        if let Ok(repo) = state.repo.lock() {
            let _ = repo.log_governance_action(
                "gc_raw_store",
                None,
                None,
                "ok",
                Some(&format!(r#"{{"deleted": {deleted}, "freed": {freed}}}"#)),
            );
        }
    }
    Ok((blobs.len(), deleted, freed))
}

/// 孤儿 blob GC（设置-数据分区触发）。
#[tauri::command]
pub(crate) async fn gc_raw_store(
    state: tauri::State<'_, DaemonState>,
) -> Result<serde_json::Value, String> {
    let (scanned, deleted, freed) = run_blocking(|| gc_raw_inner(&state))?;
    Ok(serde_json::json!({ "scanned": scanned, "deleted": deleted, "freed_bytes": freed }))
}

/// 立即执行保留策略：归档 N 天前未归档会话，返回归档数量。
#[tauri::command]
pub(crate) async fn retention_apply(
    state: tauri::State<'_, DaemonState>,
    days: i64,
) -> Result<serde_json::Value, String> {
    if days <= 0 {
        return Err("保留天数必须为正（0 = 关闭策略，不需要执行）".into());
    }
    let archived = {
        let repo = state.repo.lock().map_err(|e| storage_err(e))?;
        let n = repo
            .archive_conversations_older_than(days)
            .map_err(|e| storage_err(e))?;
        let _ = repo.log_governance_action(
            "retention_archive",
            None,
            None,
            "ok",
            Some(&format!(r#"{{"days": {days}, "archived": {n}}}"#)),
        );
        n
    };
    Ok(serde_json::json!({ "archived": archived }))
}

/// 周报自动生成：距上次生成超过 7 天则落盘一份到 app_data/reports/。
///
/// 返回 {generated, path}；未到期时 generated=false。
#[tauri::command]
pub(crate) async fn weekly_report_auto(
    state: tauri::State<'_, DaemonState>,
) -> Result<serde_json::Value, String> {
    let last_ms: i64 = {
        let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
        repo.get_setting("last_weekly_ms")
            .map_err(|e| storage_err(e))?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    let now_ms =
        (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
    let week_ms: i64 = 7 * 24 * 3600 * 1000;
    if last_ms > 0 && now_ms - last_ms < week_ms {
        return Ok(serde_json::json!({ "generated": false, "path": null }));
    }

    let html = {
        let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
        super::ops::weekly_report_html(&repo)?
    };
    let dir = state.data_dir.join("reports");
    std::fs::create_dir_all(&dir).map_err(|e| io_err(e))?;
    let date = ch_domain::now_utc().date();
    let path = dir.join(format!("weekly-{}.html", date));
    std::fs::write(&path, html.as_bytes()).map_err(|e| io_err(e))?;

    {
        let repo = state.repo.lock().map_err(|e| storage_err(e))?;
        repo.set_setting("last_weekly_ms", &now_ms.to_string())
            .map_err(|e| storage_err(e))?;
    }
    Ok(serde_json::json!({
        "generated": true,
        "path": path.to_string_lossy(),
    }))
}

/// 全量重建搜索索引（tokenizer 修复后的存量迁移 + 索引孤儿清理）。
#[tauri::command]
pub(crate) async fn rebuild_search_index(
    state: tauri::State<'_, DaemonState>,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    let count = run_blocking(|| {
        let app = app.clone();
        let total = state
            .repo
            .lock()
            .map_err(|e| storage_err(e))?
            .count_conversations()
            .map_err(|e| storage_err(e))? as u64;
        let n = rebuild_index_inner(&state, &mut |done: u64| {
            let _ = app.emit(
                "sync_progress",
                serde_json::json!({ "current": done, "total": total, "detail": "重建索引", "finished": false }),
            );
        })?;
        let _ = app.emit(
            "sync_progress",
            serde_json::json!({ "current": total, "total": total, "detail": "done", "finished": true }),
        );
        Ok::<usize, String>(n)
    })?;
    let _ = {
        let repo = state.repo.lock().map_err(|e| storage_err(e))?;
        repo.log_governance_action(
            "rebuild_search_index",
            None,
            None,
            "ok",
            Some(&format!(r#"{{"messages": {count}}}"#)),
        )
    };
    Ok(serde_json::json!({ "messages": count }))
}

/// 重建核心：清空索引 → 全量重灌所有会话消息。
fn rebuild_index_inner(
    state: &DaemonState,
    progress: &mut dyn FnMut(u64),
) -> Result<usize, String> {
    let convs: Vec<ch_domain::Conversation> = {
        let repo = state.repo.lock().map_err(|e| storage_err(e))?;
        repo.list_conversations(None).map_err(|e| storage_err(e))?
    };
    let mut docs: Vec<ch_search::index::IndexableMessage> = Vec::new();
    {
        let repo = state.repo.lock().map_err(|e| storage_err(e))?;
        let mut done: u64 = 0;
        for c in &convs {
            let msgs = repo.list_messages(&c.id).map_err(|e| storage_err(e))?;
            let title = c.effective_title().to_string();
            for m in &msgs {
                docs.push(ch_search::index::IndexableMessage {
                    message_id: m.id.clone(),
                    conversation_id: c.id.clone(),
                    provider: c.provider,
                    workspace_id: c.workspace_id.clone(),
                    role: m.role,
                    title: Some(title.clone()),
                    body: m.content_text.clone(),
                });
            }
            done += 1;
            progress(done);
        }
    }
    let n = docs.len();
    {
        let idx = state.search_index.lock().map_err(|e| storage_err(e))?;
        let mut writer = idx
            .writer(ch_search::index::DEFAULT_WRITER_HEAP)
            .map_err(|e| search_err(e))?;
        idx.rebuild(&mut writer, |w| -> Result<usize, ch_search::SearchError> {
            for d in &docs {
                idx.index_message(w, d)?;
            }
            Ok(n)
        })
        .map_err(|e| search_err(e))?;
        idx.commit(writer).map_err(|e| search_err(e))?;
    }
    Ok(n)
}

/// 范围重置预览：[start, now] 内将删除的会话/消息/指标条数。
#[tauri::command]
pub(crate) async fn reset_range_preview(
    state: tauri::State<'_, DaemonState>,
    start_ms: i64,
) -> Result<serde_json::Value, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let (convs, msgs, usage) = repo
        .reset_range_stats(start_ms)
        .map_err(|e| storage_err(e))?;
    Ok(serde_json::json!({
        "conversations": convs,
        "messages": msgs,
        "usage_records": usage,
    }))
}

/// 范围重置边界：返回库中最早数据时间戳（用于 UI 限制 `min`）。
/// 空库返回 0（前端 fallback 到今天）。
#[tauri::command]
pub(crate) async fn reset_range_bounds(
    state: tauri::State<'_, DaemonState>,
) -> Result<serde_json::Value, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    let earliest_ms = repo.reset_range_min_ts().map_err(|e| storage_err(e))?.unwrap_or(0);
    let now_ms =
        (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
    Ok(serde_json::json!({
        "earliest_ms": earliest_ms,
        "latest_ms": now_ms,
    }))
}

/// 写剪贴板（round 25c 兜底）：直接调 arboard crate，绕开 Tauri plugin 系统的复杂性。
/// macOS WKWebView 拦截 navigator.clipboard.writeText + execCommand 时唯一稳妥路径。
#[tauri::command]
pub(crate) async fn write_clipboard(text: String) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("arboard init failed: {e}"))?;
    clipboard
        .set_text(text)
        .map_err(|e| format!("arboard set_text failed: {e}"))?;
    Ok(())
}

/// 按时间范围重置：删除开始时间之后的数据。
/// 不限制最早开始时间（库中有 1 年前数据也允许重置整段）。
/// 会话删除同步清理搜索索引文档 + 清 import_state（让 autoSync 能重导入）；
/// 全部动作记入治理流水。
#[tauri::command]
pub(crate) async fn reset_range(
    state: tauri::State<'_, DaemonState>,
    app: tauri::AppHandle,
    start_ms: i64,
) -> Result<serde_json::Value, String> {
    let now_ms =
        (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
    if start_ms > now_ms {
        return Err("开始时间晚于当前时间：参数错误".into());
    }
    let app2 = app.clone();
    let r = run_blocking(move || {
        let (convs, msg_ids) = {
            let repo = state.repo.lock().map_err(|e| storage_err(e))?;
            repo.reset_range(start_ms).map_err(|e| storage_err(e))?
        };
        // 搜索索引：删除范围内消息文档
        if !msg_ids.is_empty() {
            let idx = state.search_index.lock().map_err(|e| storage_err(e))?;
            let mut writer = idx
                .writer(ch_search::index::DEFAULT_WRITER_HEAP)
                .map_err(|e| search_err(e))?;
            for mid in &msg_ids {
                let _ = idx.delete_message(&mut writer, mid);
            }
            idx.commit(writer).map_err(|e| search_err(e))?;
        }
        {
            let repo = state.repo.lock().map_err(|e| storage_err(e))?;
            let _ = repo.log_governance_action(
                "reset_range",
                None,
                None,
                "ok",
                Some(&format!(
                    r#"{{"start_ms": {start_ms}, "conversations": {convs}, "messages": {}}}"#,
                    msg_ids.len()
                )),
            );
        }
        Ok::<serde_json::Value, String>(serde_json::json!({
            "conversations": convs,
            "messages": msg_ids.len(),
        }))
    })?;
    let _ = app2.emit(
        "sync_progress",
        serde_json::json!({ "current": 1, "total": 1, "detail": "done", "finished": true }),
    );
    Ok(r)
}

#[cfg(test)]
mod tests {
    use ch_daemon::{DaemonState, DaemonStateConfig};

    #[test]
    fn gc_deletes_only_orphans() {
        // 一个被引用 + 一个孤儿 blob：GC 只删孤儿
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let state = DaemonState::open(DaemonStateConfig {
            data_dir: dir.path().to_path_buf(),
        })
        .expect("state open");
        let raw_store = state.raw_store.lock().expect("mutex poisoned");
        let referenced = raw_store
            .put_json(&serde_json::json!({"a": 1}))
            .expect("file I/O failed");
        let orphan = raw_store
            .put_json(&serde_json::json!({"orphan": true}))
            .expect("file I/O failed");
        assert!(raw_store.exists(&referenced.hash).expect("file I/O failed"));
        drop(raw_store);

        // 会话引用 referenced
        {
            let repo = state.repo.lock().expect("mutex poisoned");
            let conv = ch_domain::Conversation::new(ch_domain::Provider::Generic, "src-gc");
            let mut c = conv;
            c.raw_payload_id = Some(referenced.hash.clone());
            repo.import_conversation_batch(&c, &[], &[], None, None)
                .expect("SQL execution failed");
        }

        // 孤儿文件 mtime 是刚刚 → 1 小时保护期内，先验证保护期跳过
        let (_, deleted, _) = gc(&state);
        assert_eq!(deleted, 0, "新写入孤儿在保护期内不删");
        // 把孤儿 mtime 拨回 2 小时前
        {
            let raw_store = state.raw_store.lock().expect("mutex poisoned");
            let p = raw_store.path_of(&orphan.hash).expect("file I/O failed");
            let old = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
            // 拨回 mtime：用 utime 语义（截断重开 + set_modified）
            let f = std::fs::OpenOptions::new()
                .write(true)
                .open(&p)
                .expect("file I/O failed");
            f.set_modified(old).expect("file I/O failed");
        }
        let (_, deleted, freed) = gc(&state);
        assert_eq!(deleted, 1, "过期孤儿应被删除");
        assert_eq!(freed, orphan.stored_size);
        let raw_store = state.raw_store.lock().expect("mutex poisoned");
        assert!(
            raw_store.exists(&referenced.hash).expect("file I/O failed"),
            "被引用的必须保留"
        );
        assert!(
            !raw_store.exists(&orphan.hash).expect("file I/O failed"),
            "孤儿必须已删"
        );
    }

    fn gc(state: &DaemonState) -> (usize, usize, u64) {
        super::gc_raw_inner(state).expect("gc inner")
    }

    #[test]
    fn hard_delete_cascades_index_and_raw() {
        use ch_normalization::RawConversation;
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let state = DaemonState::open(DaemonStateConfig {
            data_dir: dir.path().to_path_buf(),
        })
        .expect("state open");
        // 导入一条会话（走完整流水线：raw + DB + 索引）
        let raw = RawConversation {
            provider: ch_domain::Provider::Generic,
            source_conversation_id: "src-hd".into(),
            title: Some("级联删除测试".into()),
            model: None,
            started_at: None,
            messages: vec![ch_normalization::RawMessage {
                role: ch_domain::Role::User,
                text: Some("cascade unique keyword".into()),
                content_json: None,
                source_message_id: None,
                created_at: None,
            }],
            events: vec![],
            source_parent_id: None,
        };
        let dto = super::super::import::import_raw_to_state(&state, raw, None, None)
            .expect("import failed");
        let cid = dto.conversation_id;
        let raw_hash = {
            let repo = state.repo.lock().expect("mutex poisoned");
            repo.get_conversation(&cid)
                .expect("SQL execution failed")
                .expect("unexpected None")
                .raw_payload_id
        }
        .expect("raw hash");

        // 索引能搜到
        {
            let idx = state.search_index.lock().expect("mutex poisoned");
            let hits = idx
                .search(&ch_search::SearchQuery::new("cascade"))
                .expect("unexpected None");
            assert!(!hits.is_empty());
        }
        // raw 存在
        {
            let rs = state.raw_store.lock().expect("mutex poisoned");
            assert!(rs.exists(&raw_hash).expect("file I/O failed"));
        }

        // 硬删（与 hard_delete_conversation 命令同一套级联逻辑）
        let (hash_opt, msg_ids): (Option<String>, Vec<String>) = {
            let repo = state.repo.lock().expect("mutex poisoned");
            let ids = repo
                .list_messages(&cid)
                .expect("SQL execution failed")
                .into_iter()
                .map(|m| m.id)
                .collect();
            (Some(raw_hash.clone()), ids)
        };
        {
            let repo = state.repo.lock().expect("mutex poisoned");
            repo.hard_delete_conversation(&cid)
                .expect("SQL execution failed");
        }
        {
            let idx = state.search_index.lock().expect("mutex poisoned");
            let mut writer = idx
                .writer(ch_search::index::DEFAULT_WRITER_HEAP)
                .expect("file I/O failed");
            for mid in &msg_ids {
                let _ = idx.delete_message(&mut writer, mid);
            }
            idx.commit(writer).expect("file I/O failed");
        }
        if let Some(h) = hash_opt {
            let rs = state.raw_store.lock().expect("mutex poisoned");
            rs.delete(&h).expect("file I/O failed");
        }

        // 三处全清
        {
            let repo = state.repo.lock().expect("mutex poisoned");
            assert!(repo
                .get_conversation(&cid)
                .expect("SQL execution failed")
                .is_none());
        }
        {
            let idx = state.search_index.lock().expect("mutex poisoned");
            let hits = idx
                .search(&ch_search::SearchQuery::new("cascade"))
                .expect("unexpected None");
            assert!(hits.is_empty(), "索引文档必须删除");
        }
        {
            let rs = state.raw_store.lock().expect("mutex poisoned");
            assert!(
                !rs.exists(&raw_hash).expect("file I/O failed"),
                "raw blob 必须删除"
            );
        }
    }
}

#[cfg(test)]
mod reset_timing_tests {
    use ch_daemon::{DaemonState, DaemonStateConfig};
    use std::time::Instant;

    /// 真实环境计时（手动跑）：分步定位 wipe_all 慢点。
    /// cargo test --lib reset_timing -- --ignored --nocapture
    #[test]
    #[ignore = "依赖本机真实 app 数据副本，CI 不跑"]
    fn wipe_all_step_timing_on_real_copy() {
        let app = std::path::PathBuf::from(std::env::var("HOME").expect("unexpected None"))
            .join("Library/Application Support/com.threadock.desktop");
        if !app.join("threadock.db").exists() {
            panic!("本机无真实 app 数据");
        }
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        // 拷完整数据目录（db + index + raw），跑新版物理重建 wipe_all 全程计时
        for entry in std::fs::read_dir(&app).expect("file I/O failed").flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("threadock.db") || name == "index" || name == "raw" {
                let dst = dir.path().join(entry.file_name());
                if entry.path().is_dir() {
                    let mut opts = fs_extra::dir::CopyOptions::new();
                    opts.copy_inside = true;
                    fs_extra::dir::copy(entry.path(), &dst, &opts).expect("file I/O failed");
                } else {
                    std::fs::copy(entry.path(), &dst).expect("file I/O failed");
                }
            }
        }
        let state = DaemonState::open(DaemonStateConfig {
            data_dir: dir.path().to_path_buf(),
        })
        .expect("state open");

        let t0 = Instant::now();
        state.wipe_all().expect("wipe");
        println!("wipe_all(new physical rebuild): {:?}", t0.elapsed());

        // 重置后状态断言：库可查、会话为 0、索引/raw 目录为空索引可用
        let count = state
            .repo
            .lock()
            .expect("mutex poisoned")
            .count_conversations()
            .expect("SQL execution failed");
        assert_eq!(count, 0, "重置后会话必须为 0");
    }
}

#[cfg(test)]
mod reset_range_tests {
    use super::*;
    use ch_daemon::{DaemonState, DaemonStateConfig};
    use ch_domain::{Conversation, Provider, Timestamp, UsageRecord, UsageStatus};

    fn make_state() -> (tempfile::TempDir, DaemonState) {
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let state = DaemonState::open(DaemonStateConfig {
            data_dir: dir.path().to_path_buf(),
        })
        .expect("state open");
        (dir, state)
    }

    fn ms_to_ts(ms: i64) -> Timestamp {
        ch_storage::timestamp::from_millis(Some(ms)).expect("valid ms")
    }

    /// 库为空时 min_ts 返回 None（前端 fallback 到今天）。
    #[test]
    fn reset_range_min_ts_empty_db_returns_none() {
        let (_d, state) = make_state();
        let v = state
            .repo
            .lock()
            .expect("mutex poisoned")
            .reset_range_min_ts()
            .expect("min_ts");
        assert!(v.is_none(), "空库必须返回 None");
    }

    /// 库有数据时 min_ts 返回最早 ts（取三表最小）。
    #[test]
    fn reset_range_min_ts_returns_earliest_across_tables() {
        let (_d, state) = make_state();
        let now_ms = (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
        let old_conv_ms = now_ms - 30 * 86_400_000;
        let new_conv_ms = now_ms - 86_400_000;
        let usage_ms = now_ms - 60 * 86_400_000; // 应是最小

        let repo = state.repo.lock().expect("mutex poisoned");
        let mut c1 = Conversation::new(Provider::Generic, "src-old");
        c1.started_at = Some(ms_to_ts(old_conv_ms));
        c1.updated_at = Some(ms_to_ts(old_conv_ms));
        repo.import_conversation_batch(&c1, &[], &[], None, Some(old_conv_ms))
            .expect("import c1");
        let mut c2 = Conversation::new(Provider::Generic, "src-new");
        c2.started_at = Some(ms_to_ts(new_conv_ms));
        c2.updated_at = Some(ms_to_ts(new_conv_ms));
        repo.import_conversation_batch(&c2, &[], &[], None, Some(new_conv_ms))
            .expect("import c2");

        let u = UsageRecord {
            id: "u1".into(),
            provider: Provider::Generic,
            source_session_id: "src-old".into(),
            turn_id: None,
            model: Some("gpt-4".into()),
            ts: ms_to_ts(usage_ms),
            input_tokens: 1,
            output_tokens: 1,
            reasoning_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_usd: Some(0.01),
            status: UsageStatus::Completed,
            duration_ms: None,
            retry_count: None,
            source_dir: None,
            context_exceeded: 0,
        };
        repo.upsert_usage_batch(&[u]).expect("upsert usage");
        drop(repo);

        let v = state
            .repo
            .lock()
            .expect("mutex poisoned")
            .reset_range_min_ts()
            .expect("min_ts")
            .expect("must be Some");
        assert_eq!(v, usage_ms, "min_ts 必须取三表最小（usage.ts 60 天前最老）");
    }

    /// 移除 31 天限制：任意历史日期都可重置，不再被拒。
    #[test]
    fn reset_range_allows_any_past_start_no_31d_cap() {
        let (_d, state) = make_state();
        let now_ms = (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
        let r = reset_range_inner(&state, now_ms - 365 * 86_400_000);
        assert!(r.is_ok(), "1 年前的开始时间必须可重置（无 31 天下限）：{r:?}");
    }

    /// 未来时间必须被拒（防误传）。
    #[test]
    fn reset_range_rejects_future_start() {
        let (_d, state) = make_state();
        let now_ms = (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
        let r = reset_range_inner(&state, now_ms + 86_400_000);
        assert!(r.is_err(), "未来时间必须被拒");
    }

    /// 重置后 import_state 必须被同步清掉，否则 autoSync 增量会全 skip 导致"数据丢失"。
    #[test]
    fn reset_range_clears_import_state_for_deleted_source_ids() {
        let (_d, state) = make_state();
        let now_ms = (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
        let recent_ms = now_ms - 3 * 86_400_000;
        let recent_ts = ms_to_ts(recent_ms);

        let repo = state.repo.lock().expect("mutex poisoned");
        let mut c = Conversation::new(Provider::Generic, "src-clear-me");
        c.started_at = Some(recent_ts);
        c.updated_at = Some(recent_ts);
        repo.import_conversation_batch(&c, &[], &[], None, Some(recent_ms))
            .expect("import");
        // 写 import_state（模拟上次同步留下的记录）
        let prov_id = Provider::Generic.as_str();
        repo.record_import_states(prov_id, &[("src-clear-me".into(), Some(recent_ms))])
            .expect("record state");
        let m = repo
            .import_state_map(prov_id)
            .expect("state map");
        assert!(m.contains_key("src-clear-me"), "import_state 必须有记录才能验证清理");
        drop(repo);

        // 重置 7 天前到现在：会命中该会话
        let r = reset_range_inner(&state, now_ms - 7 * 86_400_000).expect("reset");
        assert_eq!(r.0, 1, "应删 1 个会话");

        // 验证 import_state 已被同步清掉
        let repo = state.repo.lock().expect("mutex poisoned");
        let m = repo
            .import_state_map(prov_id)
            .expect("state map");
        assert!(
            !m.contains_key("src-clear-me"),
            "重置后 import_state 必须清掉对应 source_id，否则 autoSync 增量会全 skip 导致数据丢失"
        );
    }

    /// clear_all（DELETE 路径）也必须清 import_state（防御性；wipe_all 物理重建已自动清）。
    /// 复盘：用户报告"全部重置后 zcode 少了 142 条，重新点导入才解决"——
    /// 走的是 reset_all_data → wipe_all 物理重建路径，import_state 自动空。
    /// 但若有人写脚本直接调 clear_all（DELETE 路径），同样需要清 import_state。
    #[test]
    fn clear_all_clears_import_state() {
        let (_d, state) = make_state();
        let now_ms = (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;
        let recent_ms = now_ms - 3 * 86_400_000;
        let repo = state.repo.lock().expect("mutex poisoned");
        // 写一些 import_state 记录（模拟历史导入状态）
        repo.record_import_states(
            Provider::Generic.as_str(),
            &[
                ("zcode-sess-1".into(), Some(recent_ms)),
                ("zcode-sess-2".into(), Some(recent_ms)),
                ("claude-sess-1".into(), Some(recent_ms)),
            ],
        )
        .expect("record states");
        let m = repo.import_state_map(Provider::Generic.as_str()).expect("state map");
        assert_eq!(m.len(), 3, "前置条件：3 条 import_state 记录");
        drop(repo);

        // 调 clear_all（DELETE 路径）
        state
            .repo
            .lock()
            .expect("mutex poisoned")
            .clear_all()
            .expect("clear_all");

        // 验证 import_state 已空
        let repo = state.repo.lock().expect("mutex poisoned");
        let m = repo.import_state_map(Provider::Generic.as_str()).expect("state map");
        assert!(m.is_empty(), "clear_all 必须清空 import_state，否则 autoSync 会跳过来源全 skip");
    }

    #[test]
    #[ignore = "依赖本机真实 app 数据副本"]
    fn reset_range_keeps_old_data_on_real_copy() {
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
        let now_ms =
            (ch_domain::now_utc() - time::OffsetDateTime::UNIX_EPOCH).whole_milliseconds() as i64;

        let before = state
            .repo
            .lock()
            .expect("mutex poisoned")
            .count_conversations()
            .expect("SQL execution failed");
        let r = reset_range_inner(&state, now_ms - 7 * 86_400_000).expect("range reset");
        let after = state
            .repo
            .lock()
            .expect("mutex poisoned")
            .count_conversations()
            .expect("SQL execution failed");
        println!("before={before} deleted={} after={after}", r.0);
        assert!(after < before, "范围内会话必须被删除");
        assert!(
            after > 0,
            "30+ 天前的老数据必须保留（真实库 >7 天会话不可能全删）"
        );
        assert_eq!(before - after, r.0 as i64, "删除数与返回一致");
    }

    fn reset_range_inner(state: &DaemonState, start_ms: i64) -> Result<(i64, Vec<String>), String> {
        let repo = state.repo.lock().map_err(|e| storage_err(e))?;
        repo.reset_range(start_ms).map_err(|e| storage_err(e))
    }
}
