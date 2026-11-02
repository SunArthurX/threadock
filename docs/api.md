# Conversation Hub API

> Auto-generated. Do not edit.

## Tauri Commands

### `list_workspaces`

| 参数 | 返回 |
|------|------|
| - | `WorkspaceDto[]` |

### `app_setting_get`

| 参数 | 返回 |
|------|------|
| key | `string?` |

### `app_setting_set`

| 参数 | 返回 |
|------|------|
| key, value | `void` |

### `sources_new_count`

| 参数 | 返回 |
|------|------|
| - | `{zcode, claude_code, cursor, minimax, codex, total}` |

### `ops_month_projection`

| 参数 | 返回 |
|------|------|
| - | `MonthProjection` |

### `ops_cache_trend`

| 参数 | 返回 |
|------|------|
| days? | `CacheTrendRow[]` |

### `audit_scan_conversation`

| 参数 | 返回 |
|------|------|
| conversationId | `AuditFinding[]` |

### `audit_finding_set_state`

| 参数 | 返回 |
|------|------|
| fingerprint, status, note? | `void` |

### `audit_finding_restore`

| 参数 | 返回 |
|------|------|
| fingerprint | `void` |

### `audit_finding_states`

| 参数 | 返回 |
|------|------|
| - | `AuditFindingState[]` |

### `governance_log_list`

| 参数 | 返回 |
|------|------|
| limit? | `GovernanceLogRow[]` |

### `storage_stats`

| 参数 | 返回 |
|------|------|
| - | `{db_bytes, raw_count, raw_bytes, index_bytes}` |

### `gc_raw_store`

| 参数 | 返回 |
|------|------|
| - | `{scanned, deleted, freed_bytes}` |

### `retention_apply`

| 参数 | 返回 |
|------|------|
| days | `{archived}` |

### `weekly_report_auto`

| 参数 | 返回 |
|------|------|
| - | `{generated, path}` |

### `set_archived`

| 参数 | 返回 |
|------|------|
| id, archived | `void` |

### `delete_conversation`

| 参数 | 返回 |
|------|------|
| id | `void` |

### `restore_conversation`

| 参数 | 返回 |
|------|------|
| id | `void` |

### `hard_delete_conversation`

| 参数 | 返回 |
|------|------|
| id | `void` |

### `rebuild_search_index`

| 参数 | 返回 |
|------|------|
| - | `{messages}` |

### `activity_stats`

| 参数 | 返回 |
|------|------|
| days? | `{heatmap, hourly, hourly_weekday, hourly_weekend, tools_trend, tool_daily}` |

### `projects_overview`

| 参数 | 返回 |
|------|------|
| - | `ProjectRow[]` |

### `recent_user_prompts`

| 参数 | 返回 |
|------|------|
| limit? | `PromptRow[]` |

### `list_reports`

| 参数 | 返回 |
|------|------|
| - | `ReportFile[]` |

### `read_report`

| 参数 | 返回 |
|------|------|
| name | `string` |

### `knowledge_extract_all`

| 参数 | 返回 |
|------|------|
| force? | `{conversations, extracted, skipped}` |

### `knowledge_base_list`

| 参数 | 返回 |
|------|------|
| - | `{todos, decisions, top_commands, top_files, ...}` |

### `backup_create`

| 参数 | 返回 |
|------|------|
| path, password | `{db_size, raw_count, raw_bytes}` |

### `backup_restore`

| 参数 | 返回 |
|------|------|
| path, password, targetDir | `{db_size, raw_count}` |

### `reset_range`

| 参数 | 返回 |
|------|------|
| startMs | `{conversations, messages}` |

### `reset_range_preview`

| 参数 | 返回 |
|------|------|
| startMs | `{conversations, messages, usage_records}` |

### `list_conversations`

| 参数 | 返回 |
|------|------|
| workspaceId?, provider? | `ConversationDto[]` |

### `list_conversations_by_date`

| 参数 | 返回 |
|------|------|
| fromMs?, toMs? | `ConversationDto[]` |

### `list_conversations_by_dir`

| 参数 | 返回 |
|------|------|
| dir | `ConversationDto[]` |

### `list_child_conversations`

| 参数 | 返回 |
|------|------|
| parentSourceId, provider | `ConversationDto[]` |

### `list_messages`

| 参数 | 返回 |
|------|------|
| conversationId | `MessageDto[]` |

### `list_events`

| 参数 | 返回 |
|------|------|
| conversationId | `EventDto[]` |

### `event_payload_full`

| 参数 | 返回 |
|------|------|
| eventId | `string?（事件完整 payload JSON，不截断）` |

### `conversation_context_usage`

| 参数 | 返回 |
|------|------|
| conversationId | `ContextUsageSummary?（上下文窗口用量）` |

### `conversation_digest`

| 参数 | 返回 |
|------|------|
| conversationId | `ConversationDigest?（会话结构化摘要）` |

### `get_conversation_detail`

| 参数 | 返回 |
|------|------|
| conversationId | `ConversationDetailDto` |

### `get_conversation_by_source`

| 参数 | 返回 |
|------|------|
| provider, sourceConversationId | `ConversationDto?` |

### `extract_knowledge`

| 参数 | 返回 |
|------|------|
| conversationId | `ExtractionResult` |

### `search`

| 参数 | 返回 |
|------|------|
| query | `SearchResultDto[]` |

### `import_file`

| 参数 | 返回 |
|------|------|
| path, workspaceName? | `ImportResultDto` |

### `auto_sync`

| 参数 | 返回 |
|------|------|
| limit? | `{zcode_imported, ...}` |

### `cancel_sync`

| 参数 | 返回 |
|------|------|
| - | `void` |

### `reset_all_data`

| 参数 | 返回 |
|------|------|
| - | `void` |

### `ops_sync`

| 参数 | 返回 |
|------|------|
| force? | `{usage_written, tools_written}` |

### `ops_overview`

| 参数 | 返回 |
|------|------|
| days? | `OpsOverview` |

### `ops_by_provider`

| 参数 | 返回 |
|------|------|
| days? | `ProviderUsage[]` |

### `ops_by_model`

| 参数 | 返回 |
|------|------|
| days? | `ModelUsage[]` |

### `ops_timeseries`

| 参数 | 返回 |
|------|------|
| days? | `DailyUsage[]` |

### `ops_tool_toplist`

| 参数 | 返回 |
|------|------|
| days?, n? | `ToolUsageRow[]` |

### `ops_risky_calls`

| 参数 | 返回 |
|------|------|
| days?, n? | `RiskyCallDto[]` |

### `ops_agent_health`

| 参数 | 返回 |
|------|------|
| days? | `AgentHealth[]` |

### `ops_latency_stats`

| 参数 | 返回 |
|------|------|
| days? | `LatencyStat[]` |

### `ops_token_waste`

| 参数 | 返回 |
|------|------|
| days?, n? | `TokenWaste[]` |

### `ops_agent_benchmark`

| 参数 | 返回 |
|------|------|
| days? | `AgentBenchmark[]` |

### `ops_weekly_report`

| 参数 | 返回 |
|------|------|
| - | `string` |

### `ops_cost_by_dir`

| 参数 | 返回 |
|------|------|
| days?, n? | `DirCost[]` |

### `ops_cache_stats`

| 参数 | 返回 |
|------|------|
| days? | `CacheStat[]` |

### `ops_anomalies`

| 参数 | 返回 |
|------|------|
| days? | `AnomalyRow[]` |

### `ops_month_usage`

| 参数 | 返回 |
|------|------|
| - | `{tokens, cost_usd}` |

### `audit_scan`

| 参数 | 返回 |
|------|------|
| - | `AuditReport` |

### `audit_export_html`

| 参数 | 返回 |
|------|------|
| - | `string` |

### `assets_sync`

| 参数 | 返回 |
|------|------|
| force? | `{written}` |

### `assets_list`

| 参数 | 返回 |
|------|------|
| - | `AssetRow[]` |

### `automations_sync`

| 参数 | 返回 |
|------|------|
| force? | `{written}` |

### `automations_list`

| 参数 | 返回 |
|------|------|
| - | `AutomationRow[]` |

### `export_conversation`

| 参数 | 返回 |
|------|------|
| conversationId, format | `ExportOutput` |

### `save_text_file`

| 参数 | 返回 |
|------|------|
| path, content | `void` |

### `handoff_targets`

| 参数 | 返回 |
|------|------|
| - | `HandoffTargetInfo[]` |

### `handoff_conversations`

| 参数 | 返回 |
|------|------|
| conversationIds, target, all?, dryRun?, includeContext?, cwd? | `HandoffSummary` |

### `saved_search_list`

| 参数 | 返回 |
|------|------|
| - | `SavedSearchRecord[]` |

### `saved_search_upsert`

| 参数 | 返回 |
|------|------|
| name, query | `id` |

### `saved_search_delete`

| 参数 | 返回 |
|------|------|
| id | `void` |

### `workspace_merge`

| 参数 | 返回 |
|------|------|
| sourceId, targetId | `moved count` |

### `workspace_split`

| 参数 | 返回 |
|------|------|
| conversationIds, newName | `new workspace id` |

### `workspace_rename`

| 参数 | 返回 |
|------|------|
| id, newName | `void` |

### `workspace_source_links`

| 参数 | 返回 |
|------|------|
| - | `SourceWorkspaceLink[]` |

### `conversation_raw`

| 参数 | 返回 |
|------|------|
| conversationId | `string?（原始归档）` |

### `open_source_app`

| 参数 | 返回 |
|------|------|
| provider | `说明文字` |

### `resume_command`

| 参数 | 返回 |
|------|------|
| conversationId | `string?（CLI 来源恢复命令）` |

### `add_tag`

| 参数 | 返回 |
|------|------|
| id, tag | `void` |

### `available_providers`

| 参数 | 返回 |
|------|------|
|  | `string[]` |

### `budget_get`

| 参数 | 返回 |
|------|------|
|  | `BudgetSettings` |

### `budget_set`

| 参数 | 返回 |
|------|------|
| settings | `void` |

### `get_conversation_note`

| 参数 | 返回 |
|------|------|
| id | `NoteDto?` |

### `import_from_claude_code`

| 参数 | 返回 |
|------|------|
| sessionId | `ImportResultDto` |

### `import_from_codex`

| 参数 | 返回 |
|------|------|
| sessionId | `ImportResultDto` |

### `import_from_cursor`

| 参数 | 返回 |
|------|------|
| sessionId | `ImportResultDto` |

### `live_tasks`

| 参数 | 返回 |
|------|------|
| - | `LiveTasksDto` |

### `live_transcript`

| 参数 | 返回 |
|------|------|
| source, sessionId, limit? | `LiveActivity[]` |

### `import_from_deepseek`

| 参数 | 返回 |
|------|------|
| sessionId | `ImportResultDto` |

### `import_from_minimax`

| 参数 | 返回 |
|------|------|
| sessionId | `ImportResultDto` |

### `import_from_zcode`

| 参数 | 返回 |
|------|------|
| sessionId | `ImportResultDto` |

### `knowledge_get_stored`

| 参数 | 返回 |
|------|------|
| conversationId | `StoredKnowledge?` |

### `knowledge_xref`

| 参数 | 返回 |
|------|------|
| conversationId, keywords | `XrefEntry[]` |

### `list_all_tags`

| 参数 | 返回 |
|------|------|
| limit? | `TagCountDto[]` |

### `list_claude_code_sessions`

| 参数 | 返回 |
|------|------|
|  | `SourceSessionDto[]` |

### `list_codex_sessions`

| 参数 | 返回 |
|------|------|
|  | `SourceSessionDto[]` |

### `list_cursor_sessions`

| 参数 | 返回 |
|------|------|
|  | `SourceSessionDto[]` |

### `list_deepseek_sessions`

| 参数 | 返回 |
|------|------|
|  | `SourceSessionDto[]` |

### `list_minimax_sessions`

| 参数 | 返回 |
|------|------|
|  | `SourceSessionDto[]` |

### `list_tags`

| 参数 | 返回 |
|------|------|
| id | `string[]` |

### `list_zcode_sessions`

| 参数 | 返回 |
|------|------|
|  | `SourceSessionDto[]` |

### `llm_config_get`

| 参数 | 返回 |
|------|------|
|  | `LlmConfigView` |

### `llm_config_set`

| 参数 | 返回 |
|------|------|
| input | `LlmConfigView` |

### `llm_extract_status`

| 参数 | 返回 |
|------|------|
| conversationId | `LlmExtractStatus` |

### `llm_test_connection`

| 参数 | 返回 |
|------|------|
|  | `{ok, latency_ms, model}` |

### `ops_cost_recalc`

| 参数 | 返回 |
|------|------|
|  | `{models_updated, total_cost_usd}` |

### `ops_pricing_get`

| 参数 | 返回 |
|------|------|
|  | `{模型名: {input_per_mtok, output_per_mtok}, ...}` |

### `ops_pricing_set`

| 参数 | 返回 |
|------|------|
| pricing | `void` |

### `ops_usage_summary`

| 参数 | 返回 |
|------|------|
|  | `UsageSummary` |

### `policy_delete`

| 参数 | 返回 |
|------|------|
| name | `void` |

### `policy_list`

| 参数 | 返回 |
|------|------|
|  | `PolicyRuleRecord[]` |

### `policy_upsert`

| 参数 | 返回 |
|------|------|
| rule | `void` |

### `prompt_reuse_search`

| 参数 | 返回 |
|------|------|
| query, limit? | `PromptReuseHit[]` |

### `read_image_file`

| 参数 | 返回 |
|------|------|
| path | `ImageData?` |

### `read_text_file`

| 参数 | 返回 |
|------|------|
| path | `string` |

### `reset_range_bounds`

| 参数 | 返回 |
|------|------|
|  | `{earliest_ms, latest_ms}` |

### `resume_in_terminal`

| 参数 | 返回 |
|------|------|
| conversationId | `string?` |

### `search_grouped`

| 参数 | 返回 |
|------|------|
| query, role? | `SearchHitGroupDto[]` |

### `search_tree_hits`

| 参数 | 返回 |
|------|------|
| query, rootConversationId, role? | `SearchResultDto[]` |

### `set_conversation_note`

| 参数 | 返回 |
|------|------|
| id, note? | `number` |

### `set_favorite`

| 参数 | 返回 |
|------|------|
| id, favorite | `void` |

### `set_user_title`

| 参数 | 返回 |
|------|------|
| id, title? | `void` |

### `terminal_kill`

| 参数 | 返回 |
|------|------|
|  | `void` |

### `terminal_resize`

| 参数 | 返回 |
|------|------|
| cols, rows | `void` |

### `terminal_spawn`

| 参数 | 返回 |
|------|------|
| cols, rows | `void` |

### `terminal_write`

| 参数 | 返回 |
|------|------|
| data | `void` |

### `write_clipboard`

| 参数 | 返回 |
|------|------|
| text | `void` |

## Daemon JSON-RPC

### `system.getInfo`

| 参数 | 返回 |
|------|------|
| - | `{protocol_version, data_dir, conversation_count}` |

### `workspace.list`

| 参数 | 返回 |
|------|------|
| - | `Workspace[]` |

### `conversation.list`

| 参数 | 返回 |
|------|------|
| workspace_id? | `Conversation[]` |

### `conversation.get`

| 参数 | 返回 |
|------|------|
| id | `Conversation` |

### `conversation.handoff`

| 参数 | 返回 |
|------|------|
| target, ids?, all?, provider?, dry_run?, include_context?, cwd?, dest? | `{target, dry_run, results}` |

### `message.list`

| 参数 | 返回 |
|------|------|
| conversation_id | `Message[]` |

### `event.list`

| 参数 | 返回 |
|------|------|
| conversation_id | `Event[]` |

### `conversation.delete`

| 参数 | 返回 |
|------|------|
| id, hard? | `{id, deleted, hard}` |

### `conversation.restore`

| 参数 | 返回 |
|------|------|
| id, hard? | `{id, restored}` |

### `conversation.similar`

| 参数 | 返回 |
|------|------|
| conversation_id, limit? | `Conversation[]` |

### `knowledge.extract`

| 参数 | 返回 |
|------|------|
| conversation_id | `ExtractionResult` |

### `knowledge.save`

| 参数 | 返回 |
|------|------|
| conversation_id, result | `{id, saved}` |

### `knowledge.get`

| 参数 | 返回 |
|------|------|
| conversation_id | `{id, version, extractor, result}` |

### `search.query`

| 参数 | 返回 |
|------|------|
| query, engine? | `SearchResult[]` |

### `provider.sync`

| 参数 | 返回 |
|------|------|
| path, workspace_name? | `{conversation_id, workspace_id, messages, events, raw_payload_id}` |

