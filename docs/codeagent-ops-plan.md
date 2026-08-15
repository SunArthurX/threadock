# CodeAgentOps 执行计划

> Conversation Hub 的第二增长曲线：从「对话归档」升级为「CodeAgent 治理平台」。
>
> 版本：v1.0 · 2026-08 · 基于 4 个真实数据源逆向验证

---

## 0. 定位与设计原则

**做什么**：对 ZCode / Claude Code / Cursor / MiniMax Code 等 CodeAgent 做
用量统计、成本核算、效率分析、安全审计、策略治理 —— 一个本地优先的 AgentOps 控制台。

**不做什么**：
- 不干预 Agent 运行（纯只读采集，绝不写 agent 数据库）
- 不上云（数据全部本地，备份加密可选）
- 不做实时监控（T+批量导入，复用现有 10 分钟同步）

**架构原则**：复用现有管线 `adapter → normalize → storage`，ops 数据走平行的
`ops-adapter → metrics → storage(V6)` 管线，不污染对话主链路。

---

## 1. 数据资产盘点（2026-08 实测验证）

| 数据维度 | ZCode | MiniMax | Claude Code | Cursor |
|---------|-------|---------|-------------|--------|
| **Token 用量** | ✅ `turn_usage`(839行) + `model_usage`(27,804行)：input/output/reasoning/cache 全口径 | ✅ `local_runtime_token_usage`：per-turn 全口径 + **cost_usd** | ✅ JSONL 每条 assistant 消息 `usage` | ⚠️ bubble `tokenCount`（本机库已清空，机会性提取） |
| **模型明细** | ✅ model_id/provider/variant/status/latency/retry/`context_exceeded`/error_type | ✅ model (MiniMax-M3) | ⚠️ 可从消息推断 | ❌ |
| **工具调用** | ✅ `tool_usage`(32,610行)：tool_name/**read_only**/**destructive**(82条)/approval_status/exit_code/duration | ⚠️ 需从消息解析 | ⚠️ tool_use 事件（已有） | ⚠️ capabilityType |
| **性能指标** | ✅ duration_ms / **TTFT** / retry_count | ✅ ts | ⚠️ 时间戳推算 | ⚠️ |
| **治理对象实测** | GLM-5.2: 27,667 次请求 / **43 亿 input tokens**；工具 Top: Bash 12,345 / Read 7,599 / Edit 7,046 | MiniMax-M3 | — | — |

> 结论：**数据层完全成立**。ZCode 覆盖度最高（原生 ops 表），MiniMax 次之（token+成本），
> Claude Code 够用，Cursor 保底支持。

---

## 2. 总体架构

```
┌──────────────────────────────────────────────────────────┐
│  Tauri 前端                                               │
│  ┌──────────────┐  ┌──────────────────────────────────┐  │
│  │ 💬 对话视图    │  │ 📊 治理视图 (新增)                │  │
│  │ (现有三栏)    │  │ KPI卡 / 趋势图 / 分布图 / 榜单    │  │
│  └──────────────┘  │ / 审计报告 / 策略配置              │  │
│                    └──────────────────────────────────┘  │
├──────────────────────────────────────────────────────────┤
│  Tauri commands                                          │
│  ops_overview / ops_timeseries / ops_by_provider         │
│  ops_by_model / ops_tool_toplist / ops_risky_calls       │
│  audit_scan / policy_rules / budget_status               │
├──────────────────────────────────────────────────────────┤
│  新 crate: ch-ops-metrics                                │
│  采集: ZCodeOpsAdapter / MiniMaxOpsAdapter /             │
│        ClaudeCodeOpsAdapter / CursorOpsAdapter           │
│  统一模型: UsageRecord / ToolCallRecord                   │
├──────────────────────────────────────────────────────────┤
│  storage V6 (2 张新表)                                   │
│  usage_records / tool_call_records + 聚合索引            │
├──────────────────────────────────────────────────────────┤
│  数据源（只读）                                           │
│  ~/.zcode/...db  ~/.minimax/...sqlite  ~/.claude/...jsonl│
└──────────────────────────────────────────────────────────┘
```

---

## 3. 统一数据模型

### 3.1 领域模型（ch-domain 扩展）

```rust
/// 一次模型调用的用量记录（turn 级或 request 级）。
pub struct UsageRecord {
    pub id: String,
    pub provider: Provider,
    pub source_session_id: String,   // 关联 source_conversation_id
    pub turn_id: Option<String>,
    pub model: Option<String>,
    pub ts: Timestamp,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub cost_usd: Option<f64>,       // 来源已算好（MiniMax）或本地定价补算
    pub status: UsageStatus,         // running/completed/error/cancelled
    pub duration_ms: Option<i64>,
    pub retry_count: Option<i64>,
}

/// 一次工具调用（治理核心对象）。
pub struct ToolCallRecord {
    pub id: String,
    pub provider: Provider,
    pub source_session_id: String,
    pub tool_name: String,
    pub ts: Timestamp,
    pub read_only: Option<bool>,
    pub destructive: Option<bool>,   // ZCode 原生；其他来源靠规则推断
    pub approval_status: Option<String>,
    pub exit_code: Option<i64>,
    pub duration_ms: Option<i64>,
    pub status: ToolCallStatus,
    pub command_text: Option<String>, // Bash 类工具保留命令文本（审计用）
}
```

### 3.2 Schema V6

```sql
CREATE TABLE IF NOT EXISTS usage_records (
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    source_session_id TEXT NOT NULL,
    turn_id TEXT,
    model TEXT,
    ts INTEGER NOT NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL,
    status TEXT NOT NULL DEFAULT 'completed',
    duration_ms INTEGER,
    retry_count INTEGER,
    UNIQUE(provider_id, source_session_id, turn_id, ts)
);
CREATE INDEX idx_usage_ts ON usage_records(ts);
CREATE INDEX idx_usage_provider_ts ON usage_records(provider_id, ts);

CREATE TABLE IF NOT EXISTS tool_call_records (
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    source_session_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    ts INTEGER NOT NULL,
    read_only INTEGER,
    destructive INTEGER,
    approval_status TEXT,
    exit_code INTEGER,
    duration_ms INTEGER,
    status TEXT NOT NULL DEFAULT 'completed',
    command_text TEXT,
    UNIQUE(provider_id, source_session_id, tool_name, ts)
);
CREATE INDEX idx_tool_ts ON tool_call_records(ts);
CREATE INDEX idx_tool_destructive ON tool_call_records(destructive) WHERE destructive = 1;
```

> 幂等键含 ts，重放导入天然去重；conversation 级汇总在查询时 `SUM()` 而非落列，避免双写不一致。

---

## 4. 里程碑

### M1 — 采集层：ops-adapter（后端，1 个新 crate）

**范围**
- 新建 `crates/ops-metrics`（名 `ch-ops-metrics`）
- `ZCodeOpsAdapter`：`turn_usage` + `model_usage` + `tool_usage` 三表 → 统一模型
- `MiniMaxOpsAdapter`：`local_runtime_token_usage` → UsageRecord（含 cost_usd）
- `ClaudeCodeOpsAdapter`：JSONL `usage` 字段聚合 → 每 assistant 消息一条 UsageRecord
- `CursorOpsAdapter`：bubble `tokenCount`（机会性，库缺失时静默跳过）
- destructive 推断规则（非 ZCode 来源）：`Bash+命令黑名单` / `Write/Edit 大范围` → `destructive=1`

**改动文件**：`crates/ops-metrics/{Cargo.toml, src/lib.rs, src/zcode.rs, src/minimax.rs, src/claude_code.rs, src/cursor.rs}`

**验收**
- 4 个 adapter 各带真实 fixture 单测（从本机库导出脱敏样本）
- `discover_usage(db) -> Vec<UsageRecord>` 可枚举 ZCode 43 亿 tokens 不 OOM（流式/分页）

### M2 — 存储与聚合（V6 + repository + commands）

**范围**
- storage V6 migration（上文 2 表）
- `Repository`：`upsert_usage_batch`（事务批量）、`upsert_tool_call_batch`
- 聚合查询（全 SQL，走索引）：
  - `ops_overview(range)` → {sessions, turns, tokens_in/out, cost, error_rate, destructive_ops, avg_ttft}
  - `ops_by_provider(range)` → GROUP BY provider
  - `ops_by_model(range)` → GROUP BY model
  - `ops_timeseries_daily(range)` → GROUP BY strftime(ts)
  - `ops_tool_toplist(range, n)` → 工具调用频次/错误率/耗时
  - `ops_risky_calls(range)` → WHERE destructive=1 OR exit_code!=0 OR approval='requested'
- Tauri commands：上列 6 个 `ops_*` 命令 + `auto_sync` 扩展（导入对话后顺带导 metrics）
- CLI：`hub ops sync` / `hub ops overview`

**验收**
- `cargo test`：聚合 SQL 黄金样本测试（seed 已知数据 → 断言聚合值）
- 端到端：ZCode 真实库 sync 后 `ops_overview` 返回 27,667 次请求 / 43 亿 tokens 量级正确

### M3 — 治理 Dashboard UI

**范围**
- 顶栏视图切换：`💬 对话 | 📊 治理`（localStorage 记忆）
- 治理页布局：
  ```
  ┌ KPI: 会话数 │ 总Tokens │ 估算成本 │ 危险操作 ┐
  ├──────────────┬───────────────────────────┤
  │ Agent 分布    │  每日 Tokens 趋势 (bar)    │
  │ (donut)      │                           │
  ├──────────────┴───────────────────────────┤
  │ 模型明细表 (model/请求数/tokens/错误率)     │
  ├──────────────────────────────────────────┤
  │ 工具 Top10 (含 destructive 标红)           │
  │ 危险操作列表 (点击跳回对话视图高亮)          │
  └──────────────────────────────────────────┘
  ```
- 图表：**自写轻量 SVG 组件**（`DonutChart` / `BarChart` / `Sparkline`），零第三方依赖，复用设计 token
- 时间范围选择器：7d / 30d / 90d / all
- 危险操作行点击 → 切对话视图 → 定位会话（复用现有 jumpToSearchResult 机制）

**验收**
- 深浅两主题截图审查通过
- 43 亿 tokens 量级下图表渲染 < 100ms（数据已聚合，纯前端绘制）

### M4 — 安全审计引擎

**范围**
- `ch-audit`（或并入 ops-metrics）：两类扫描
  - **敏感信息**：复用现有 7 条 redaction 规则 + 自定义规则全库扫描 messages
  - **危险命令**：内置规则集匹配 `tool_call_records.command_text` / events：
    `rm -rf`, `git push --force`, `curl…| sh`, `chmod 777`, `sudo`, `dd of=`, `mkfs`, fork 炸弹等
- 审计报告：JSON + HTML 导出（命中位置、规则、严重级）
- UI：审计页（规则列表 / 扫描按钮 / 结果表 / 一键导出）
- 治理动作（只读）：命中项 → 跳转会话查看上下文

**验收**
- 用故意植入密钥的样本会话验证扫描命中与导出
- ZCode 82 条 destructive 记录全部入报告

### M5 — 策略、成本与预算

**范围**
- 定价模型：`assets/pricing.json`（本地可编辑 `{model: {input_per_mtok, output_per_mtok}}`）
  - cost = Σ usage × 单价（MiniMax 自带 cost_usd 优先）
- 预算：设置月度 token / 成本阈值；同步后检查 → `tauri-plugin-notification` 本地通知
- 策略规则（存 `redaction_rules` 同款表或新 `policy_rules`）：命令黑名单自定义、通知开关
- 治理页新增「预算」卡：本月用量 / 阈值进度条

**验收**
- 改 pricing.json 后成本即时重算
- 超阈值触发系统通知（macOS 通知中心可截图）

---

## 5. 排期与依赖

```
M1 采集层 ──► M2 存储+聚合 ──► M3 Dashboard ──► M5 策略预算
                     └────────► M4 安全审计 ────┘
（M4 只依赖 M2 的 tool_call_records，可与 M3 并行）
```

| 里程碑 | 规模 | 关键风险 |
|-------|------|---------|
| M1 | ~1.2k 行 | ZCode model_usage 2.7 万行导入性能 → 批量事务 + 分页 |
| M2 | ~800 行 | 聚合口径统一（computed_total vs input+output）→ 定死 input+output+reasoning，cache 单列不计费 |
| M3 | ~1.5k 行（含 SVG 图表） | 无（纯前端） |
| M4 | ~700 行 | 误报率 → 规则分级（高危/可疑）+ 白名单 |
| M5 | ~500 行 | 定价数据维护成本 → 默认表 + 用户覆盖 |

---

## 6. 与现有代码的接缝

| 现有资产 | 复用方式 |
|---------|---------|
| 4 个对话 adapter | ops-adapter 与其共享数据源路径常量与只读打开逻辑 |
| auto_sync 防重入 (IS_BUSY) | metrics 导入并入同一同步周期，同一把锁 |
| events 表 (tool_call_started 等) | M2 前的临时工具数据源；V6 后以 tool_call_records 为准 |
| redaction 规则引擎 | M4 敏感信息扫描直接复用 |
| 导出框架 (Markdown/JSON) | 审计报告复用其序列化骨架 |
| 设计系统 (双主题 token) | 治理页完全复用；图表色取 provider 专属色 |

---

## 7. 明确不做（v1 边界）

- ❌ 实时流式监控（需 agent 侧 hook，违背只读原则）
- ❌ 团队/多用户（本地单用户；备份加密已覆盖数据外带）
- ❌ 自动阻断危险命令（只告警审计，不干预执行）
- ❌ 云端控制台 / 远程策略下发
