//! 类型生成器：生成 TypeScript 绑定 + API 文档。
//! cargo run --example typegen
fn main() {
    // API 文档
    let cmds: Vec<(&str, &str, &str)> = vec![
        ("list_workspaces", "-", "WorkspaceDto[]"),
        ("list_conversations", "workspaceId?, provider?", "ConversationDto[]"),
        ("list_child_conversations", "parentSourceId, provider", "ConversationDto[]"),
        ("list_messages", "conversationId", "MessageDto[]"),
        ("list_events", "conversationId", "EventDto[]"),
        ("get_conversation_detail", "conversationId", "ConversationDetailDto"),
        ("get_conversation_by_source", "provider, sourceConversationId", "ConversationDto?"),
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
        ("export_conversation", "conversationId, format", "ExportOutput"),
        ("save_text_file", "path, content", "void"),
    ];
    let mut md = String::from("# Conversation Hub API\n\n> Auto-generated. Do not edit.\n\n## Commands\n\n");
    for (n, p, r) in &cmds {
        md.push_str(&format!("### `{n}`\n\n| 参数 | 返回 |\n|------|------|\n| {p} | `{r}` |\n\n"));
    }
    std::fs::create_dir_all("../../../docs").expect("mkdir docs");
    std::fs::write("../../../docs/api.md", &md).expect("write api.md");
    println!("✓ docs/api.md ({} bytes, {} commands)", md.len(), cmds.len());
}
