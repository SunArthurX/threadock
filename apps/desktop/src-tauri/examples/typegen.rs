//! 类型生成器：生成 TypeScript 绑定 + API 文档。
//! cargo run --example typegen
fn main() {
    // API 文档
    let cmds: Vec<(&str, &str, &str)> = vec![
        ("list_workspaces", "-", "WorkspaceDto[]"),
        ("app_setting_get", "key", "string?"),
        ("app_setting_set", "key, value", "void"),
        (
            "sources_new_count",
            "-",
            "{zcode, claude_code, cursor, minimax, codex, total}",
        ),
        ("ops_month_projection", "-", "MonthProjection"),
        ("ops_cache_trend", "days?", "CacheTrendRow[]"),
        (
            "audit_scan_conversation",
            "conversationId",
            "AuditFinding[]",
        ),
        (
            "audit_finding_set_state",
            "fingerprint, status, note?",
            "void",
        ),
        ("audit_finding_restore", "fingerprint", "void"),
        ("audit_finding_states", "-", "AuditFindingState[]"),
        ("governance_log_list", "limit?", "GovernanceLogRow[]"),
        (
            "storage_stats",
            "-",
            "{db_bytes, raw_count, raw_bytes, index_bytes}",
        ),
        ("gc_raw_store", "-", "{scanned, deleted, freed_bytes}"),
        ("retention_apply", "days", "{archived}"),
        ("weekly_report_auto", "-", "{generated, path}"),
        ("set_archived", "id, archived", "void"),
        ("delete_conversation", "id", "void"),
        ("restore_conversation", "id", "void"),
        ("hard_delete_conversation", "id", "void"),
        ("rebuild_search_index", "-", "{messages}"),
        (
            "activity_stats",
            "days?",
            "{heatmap, hourly, hourly_weekday, hourly_weekend, tools_trend, tool_daily}",
        ),
        ("projects_overview", "-", "ProjectRow[]"),
        ("recent_user_prompts", "limit?", "PromptRow[]"),
        ("list_reports", "-", "ReportFile[]"),
        ("read_report", "name", "string"),
        (
            "knowledge_extract_all",
            "force?",
            "{conversations, extracted, skipped}",
        ),
        (
            "knowledge_base_list",
            "-",
            "{todos, decisions, top_commands, top_files, ...}",
        ),
        (
            "backup_create",
            "path, password",
            "{db_size, raw_count, raw_bytes}",
        ),
        (
            "backup_restore",
            "path, password, targetDir",
            "{db_size, raw_count}",
        ),
        ("reset_range", "startMs", "{conversations, messages}"),
        (
            "reset_range_preview",
            "startMs",
            "{conversations, messages, usage_records}",
        ),
        (
            "list_conversations",
            "workspaceId?, provider?",
            "ConversationDto[]",
        ),
        (
            "list_conversations_by_date",
            "fromMs?, toMs?",
            "ConversationDto[]",
        ),
        ("list_conversations_by_dir", "dir", "ConversationDto[]"),
        (
            "list_child_conversations",
            "parentSourceId, provider",
            "ConversationDto[]",
        ),
        ("list_messages", "conversationId", "MessageDto[]"),
        ("list_events", "conversationId", "EventDto[]"),
        (
            "get_conversation_detail",
            "conversationId",
            "ConversationDetailDto",
        ),
        (
            "get_conversation_by_source",
            "provider, sourceConversationId",
            "ConversationDto?",
        ),
        ("extract_knowledge", "conversationId", "ExtractionResult"),
        ("search", "query", "SearchResultDto[]"),
        ("import_file", "path, workspaceName?", "ImportResultDto"),
        ("auto_sync", "limit?", "{zcode_imported, ...}"),
        ("cancel_sync", "-", "void"),
        ("reset_all_data", "-", "void"),
        ("ops_sync", "force?", "{usage_written, tools_written}"),
        ("ops_overview", "days?", "OpsOverview"),
        ("ops_by_provider", "days?", "ProviderUsage[]"),
        ("ops_by_model", "days?", "ModelUsage[]"),
        ("ops_timeseries", "days?", "DailyUsage[]"),
        ("ops_tool_toplist", "days?, n?", "ToolUsageRow[]"),
        ("ops_risky_calls", "days?, n?", "RiskyCallDto[]"),
        ("ops_agent_health", "days?", "AgentHealth[]"),
        ("ops_latency_stats", "days?", "LatencyStat[]"),
        ("ops_token_waste", "days?, n?", "TokenWaste[]"),
        ("ops_agent_benchmark", "days?", "AgentBenchmark[]"),
        ("ops_weekly_report", "-", "string"),
        ("ops_cost_by_dir", "days?, n?", "DirCost[]"),
        ("ops_cache_stats", "days?", "CacheStat[]"),
        ("ops_anomalies", "days?", "AnomalyRow[]"),
        ("ops_month_usage", "-", "{tokens, cost_usd}"),
        ("audit_scan", "-", "AuditReport"),
        ("audit_export_html", "-", "string"),
        ("assets_sync", "force?", "{written}"),
        ("assets_list", "-", "AssetRow[]"),
        ("automations_sync", "force?", "{written}"),
        ("automations_list", "-", "AutomationRow[]"),
        (
            "export_conversation",
            "conversationId, format",
            "ExportOutput",
        ),
        ("save_text_file", "path, content", "void"),
        ("saved_search_list", "-", "SavedSearchRecord[]"),
        ("saved_search_upsert", "name, query", "id"),
        ("saved_search_delete", "id", "void"),
        ("workspace_merge", "sourceId, targetId", "moved count"),
        (
            "workspace_split",
            "conversationIds, newName",
            "new workspace id",
        ),
        ("workspace_rename", "id, newName", "void"),
        ("workspace_source_links", "-", "SourceWorkspaceLink[]"),
        ("conversation_raw", "conversationId", "string?（原始归档）"),
        ("open_source_app", "provider", "说明文字"),
        (
            "resume_command",
            "conversationId",
            "string?（CLI 来源恢复命令）",
        ),
    ];
    let mut md =
        String::from("# Conversation Hub API\n\n> Auto-generated. Do not edit.\n\n## Commands\n\n");
    for (n, p, r) in &cmds {
        md.push_str(&format!(
            "### `{n}`\n\n| 参数 | 返回 |\n|------|------|\n| {p} | `{r}` |\n\n"
        ));
    }
    std::fs::create_dir_all("../../../docs").expect("mkdir docs");
    std::fs::write("../../../docs/api.md", &md).expect("write api.md");
    println!(
        "✓ docs/api.md ({} bytes, {} commands)",
        md.len(),
        cmds.len()
    );
}
