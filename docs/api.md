# Conversation Hub API

> Auto-generated. Do not edit.

## Commands

### `list_workspaces`

| 参数 | 返回 |
|------|------|
| - | `WorkspaceDto[]` |

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

