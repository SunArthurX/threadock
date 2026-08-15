# Conversation Hub API

> Auto-generated. Do not edit.

## Commands

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
| days? | `{heatmap, hourly, tools_trend}` |

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

