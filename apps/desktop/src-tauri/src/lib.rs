//! Threadock 桌面应用后端。
//!
//! 通过嵌入 DaemonState（plan §8.2 单点写者）访问数据层；应用入口 [`run`]。
//! 具体命令按域拆分在 [`commands`]（会话浏览 / 导入 / 治理 / 审计 / 导出）。

#![allow(clippy::redundant_closure)]

mod commands;
use commands::*;

use ch_daemon::{DaemonState, DaemonStateConfig};
use tauri::Manager;

// ── Tauri commands ──────────────────────────────────────────────────────

// ── Cursor / MiniMax 真实来源导入 ──────────────────────────────────────

// ── CodeAgentOps：指标采集与聚合查询（plan codeagent-ops M2）──────────

// ── M5：预算设置 ───────────────────────────────────────────────────────

// ── M10-M12：健康度 / 延迟 / Token 浪费 ────────────────────────────────

// ── M5：定价模型 ───────────────────────────────────────────────────────

// ── 应用启动 ────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 日志：导入失败等 warn 输出到 stderr（此前静默，事故排查困难）
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::INFO)
                .with_target(false)
                .init();

            // 数据库放在 app data 目录（plan §9.6 布局）
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("app data dir should be available");
            std::fs::create_dir_all(&data_dir).expect("create data dir");
            // 嵌入 DaemonState（plan §8.2 单点写者），统一持有 repo + search_index + raw_store
            let daemon_state = DaemonState::open(DaemonStateConfig {
                data_dir: data_dir.clone(),
            })
            .expect("open daemon state");
            app.manage(daemon_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_workspaces,
            list_conversations,
            list_child_conversations,
            list_messages,
            list_events,
            get_conversation_detail,
            extract_knowledge,
            search,
            import_file,
            list_zcode_sessions,
            import_from_zcode,
            list_claude_code_sessions,
            import_from_claude_code,
            list_cursor_sessions,
            import_from_cursor,
            list_minimax_sessions,
            import_from_minimax,
            list_codex_sessions,
            import_from_codex,
            ops_sync,
            ops_overview,
            ops_by_provider,
            ops_by_model,
            ops_timeseries,
            ops_tool_toplist,
            ops_risky_calls,
            assets_sync,
            assets_list,
            automations_sync,
            automations_list,
            ops_cost_by_dir,
            ops_cache_stats,
            ops_anomalies,
            ops_agent_health,
            ops_latency_stats,
            ops_token_waste,
            ops_agent_benchmark,
            ops_weekly_report,
            get_conversation_by_source,
            audit_scan,
            audit_export_html,
            policy_list,
            policy_upsert,
            policy_delete,
            budget_get,
            budget_set,
            ops_month_usage,
            ops_pricing_get,
            ops_pricing_set,
            ops_cost_recalc,
            auto_sync,
            reset_all_data,
            cancel_sync,
            export_conversation,
            save_text_file,
            set_favorite,
            add_tag,
            list_tags,
            app_setting_get,
            app_setting_set,
            ops_month_projection,
            ops_cache_trend,
            audit_scan_conversation,
            audit_finding_set_state,
            audit_finding_restore,
            audit_finding_states,
            governance_log_list,
            storage_stats,
            gc_raw_store,
            retention_apply,
            weekly_report_auto,
            set_archived,
            delete_conversation,
            restore_conversation,
            hard_delete_conversation,
            rebuild_search_index,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Threadock");
}

// ── 命令层测试（此前为 0 覆盖）──────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_table_empty_home_is_empty() {
        // HOME 指向空目录：5 个来源都不存在 → 空描述表
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let home = dir.path().to_string_lossy().into_owned();
        assert!(source_table(&home).is_empty());
    }

    #[test]
    fn auto_sync_empty_home_reports_all_sources() {
        // 空环境完整跑一轮：全部来源 0 导入 0 跳过，JSON 键与旧版逐键一致（前端契约）
        let state = DaemonState::open_in_memory().expect("state open");
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        std::env::set_var("HOME", dir.path());
        let v = auto_sync_inner(&state, None).expect("auto sync");
        assert_eq!(v.get("cancelled"), Some(&serde_json::json!(false)));
        for k in ["zcode", "claude_code", "cursor", "minimax", "codex"] {
            assert_eq!(
                v.get(format!("{k}_imported")),
                Some(&serde_json::json!(0)),
                "{k}_imported"
            );
            assert_eq!(
                v.get(format!("{k}_skipped")),
                Some(&serde_json::json!(0)),
                "{k}_skipped"
            );
        }
    }

    #[test]
    fn import_file_inner_roundtrip() {
        // 手动导入命令的最小闭环：markdown → 单事务入库 → 可查询
        let state = DaemonState::open_in_memory().expect("state open");
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let md = dir.path().join("s.md");
        std::fs::write(
            &md,
            "# 会话标题\n\n## User\n\n你好\n\n## Assistant\n\n世界\n",
        )
        .expect("file I/O failed");
        let r = import_file_inner(
            &state,
            md.to_str().expect("unexpected None"),
            Some("测试ws"),
        )
        .expect("import");
        assert!(r.messages >= 2, "should import 2+ messages");
        let repo = state.read_repo.lock().expect("mutex poisoned");
        assert_eq!(repo.count_conversations().expect("SQL execution failed"), 1);
        // 导入幂等：同文件再导一次仍是 1 条会话（内容 hash 去重）
        drop(repo);
        import_file_inner(
            &state,
            md.to_str().expect("unexpected None"),
            Some("测试ws"),
        )
        .expect("re-import");
        let repo = state.read_repo.lock().expect("mutex poisoned");
        assert_eq!(repo.count_conversations().expect("SQL execution failed"), 1);
    }
}
