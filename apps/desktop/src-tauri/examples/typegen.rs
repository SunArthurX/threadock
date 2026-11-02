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
            "event_payload_full",
            "eventId",
            "string?（事件完整 payload JSON，不截断）",
        ),
        (
            "conversation_context_usage",
            "conversationId",
            "ContextUsageSummary?（上下文窗口用量）",
        ),
        (
            "conversation_digest",
            "conversationId",
            "ConversationDigest?（会话结构化摘要）",
        ),
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
        ("handoff_targets", "-", "HandoffTargetInfo[]"),
        (
            "handoff_conversations",
            "conversationIds, target, all?, dryRun?, includeContext?, cwd?",
            "HandoffSummary",
        ),
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
        // ── 以下为此前漏收的命令（参数/返回类型按 Rust 签名逐条核对）──
        // 清单与 lib.rs 的 generate_handler! 一一对应；新增命令时两边都要改。
        ("add_tag", "id, tag", "void"),
        ("available_providers", "", "string[]"),
        ("budget_get", "", "BudgetSettings"),
        ("budget_set", "settings", "void"),
        ("get_conversation_note", "id", "NoteDto?"),
        ("import_from_claude_code", "sessionId", "ImportResultDto"),
        ("import_from_codex", "sessionId", "ImportResultDto"),
        ("import_from_cursor", "sessionId", "ImportResultDto"),
        ("import_from_deepseek", "sessionId", "ImportResultDto"),
        ("import_from_minimax", "sessionId", "ImportResultDto"),
        ("import_from_zcode", "sessionId", "ImportResultDto"),
        ("knowledge_get_stored", "conversationId", "StoredKnowledge?"),
        ("knowledge_xref", "conversationId, keywords", "XrefEntry[]"),
        ("list_all_tags", "limit?", "TagCountDto[]"),
        ("list_claude_code_sessions", "", "SourceSessionDto[]"),
        ("list_codex_sessions", "", "SourceSessionDto[]"),
        ("list_cursor_sessions", "", "SourceSessionDto[]"),
        ("list_deepseek_sessions", "", "SourceSessionDto[]"),
        ("list_minimax_sessions", "", "SourceSessionDto[]"),
        ("list_tags", "id", "string[]"),
        ("list_zcode_sessions", "", "SourceSessionDto[]"),
        ("llm_config_get", "", "LlmConfigView"),
        ("llm_config_set", "input", "LlmConfigView"),
        ("llm_extract_status", "conversationId", "LlmExtractStatus"),
        ("llm_test_connection", "", "{ok, latency_ms, model}"),
        ("ops_cost_recalc", "", "{models_updated, total_cost_usd}"),
        (
            "ops_pricing_get",
            "",
            "{模型名: {input_per_mtok, output_per_mtok}, ...}",
        ),
        ("ops_pricing_set", "pricing", "void"),
        ("ops_usage_summary", "", "UsageSummary"),
        ("policy_delete", "name", "void"),
        ("policy_list", "", "PolicyRuleRecord[]"),
        ("policy_upsert", "rule", "void"),
        ("prompt_reuse_search", "query, limit?", "PromptReuseHit[]"),
        ("read_image_file", "path", "ImageData?"),
        ("read_text_file", "path", "string"),
        ("reset_range_bounds", "", "{earliest_ms, latest_ms}"),
        ("resume_in_terminal", "conversationId", "string?"),
        ("search_grouped", "query, role?", "SearchHitGroupDto[]"),
        (
            "search_tree_hits",
            "query, rootConversationId, role?",
            "SearchResultDto[]",
        ),
        ("set_conversation_note", "id, note?", "number"),
        ("set_favorite", "id, favorite", "void"),
        ("set_user_title", "id, title?", "void"),
        ("terminal_kill", "", "void"),
        ("terminal_resize", "cols, rows", "void"),
        ("terminal_spawn", "cols, rows", "void"),
        ("terminal_write", "data", "void"),
        ("write_clipboard", "text", "void"),
    ];

    // Daemon JSON-RPC 方法：对应 crates/daemon/src/server.rs 的 dispatch 表。
    // 参数名取各 handler 的 serde Deserialize 参数结构体（snake_case 即实参名）；
    // 返回按各 handler 实际序列化的结构描述。
    let daemon_cmds: Vec<(&str, &str, &str)> = vec![
        (
            "system.getInfo",
            "-",
            "{protocol_version, data_dir, conversation_count}",
        ),
        ("workspace.list", "-", "Workspace[]"),
        ("conversation.list", "workspace_id?", "Conversation[]"),
        ("conversation.get", "id", "Conversation"),
        (
            "conversation.handoff",
            "target, ids?, all?, provider?, dry_run?, include_context?, cwd?, dest?",
            "{target, dry_run, results}",
        ),
        ("message.list", "conversation_id", "Message[]"),
        ("event.list", "conversation_id", "Event[]"),
        ("conversation.delete", "id, hard?", "{id, deleted, hard}"),
        ("conversation.restore", "id, hard?", "{id, restored}"),
        (
            "conversation.similar",
            "conversation_id, limit?",
            "Conversation[]",
        ),
        ("knowledge.extract", "conversation_id", "ExtractionResult"),
        ("knowledge.save", "conversation_id, result", "{id, saved}"),
        (
            "knowledge.get",
            "conversation_id",
            "{id, version, extractor, result}",
        ),
        ("search.query", "query, engine?", "SearchResult[]"),
        (
            "provider.sync",
            "path, workspace_name?",
            "{conversation_id, workspace_id, messages, events, raw_payload_id}",
        ),
    ];

    let mut md = String::from(
        "# Conversation Hub API\n\n> Auto-generated. Do not edit.\n\n## Tauri Commands\n\n",
    );
    for (n, p, r) in &cmds {
        md.push_str(&format!(
            "### `{n}`\n\n| 参数 | 返回 |\n|------|------|\n| {p} | `{r}` |\n\n"
        ));
    }
    md.push_str("## Daemon JSON-RPC\n\n");
    for (n, p, r) in &daemon_cmds {
        md.push_str(&format!(
            "### `{n}`\n\n| 参数 | 返回 |\n|------|------|\n| {p} | `{r}` |\n\n"
        ));
    }
    std::fs::create_dir_all("../../../docs").expect("mkdir docs");
    std::fs::write("../../../docs/api.md", &md).expect("write api.md");
    println!(
        "✓ docs/api.md ({} bytes, {} commands, {} daemon methods)",
        md.len(),
        cmds.len(),
        daemon_cmds.len()
    );
}
