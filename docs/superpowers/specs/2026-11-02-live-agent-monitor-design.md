# 设计：多 Agent 实时任务监控（Live Board）

日期：2026-11-02 · 分支：`feature/live-agent-monitor` · 状态：已定稿，随本分支实现

## 1. 需求

在一个窗口看到本机所有**正在运行**的 coding agent 会话与实时进度，覆盖：
Claude Code / Codex Desktop / Codex CLI / ZCode / MiniMax Code。

参考 [pingdotgg/t3code](https://github.com/pingdotgg/t3code)（「agent harness control surface」：
经 ACP provider 体系**控制** agent，事件日志为源真相，per-thread 订阅）。

## 2. 调研结论（2026-11-02 本机实测）

| 源 | 存储 | 运行态信号 | 进度信号 |
|---|---|---|---|
| Claude Code | `~/.claude/projects/<编码路径>/*.jsonl` append-only | 文件 mtime（秒级） | 尾部行 type（assistant/tool_use/…）；行可能带 `atis-latch` 等噪声需跳过 |
| Codex CLI + Desktop（共用） | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | 行 mtime；首行 `session_meta.originator`（实测 `codex_work_desktop`）可区分桌面/CLI | `event_msg` 的 `item_started/item_completed` + `turn_id` |
| ZCode | `~/.zcode/cli/db/db.sqlite`（WAL，2.3 GB / session 表 1093 行） | `turn_usage.status ∈ running/completed/error/cancelled`（**精确运行态**）；`session.time_updated` | `todo` 表（per-session done/total）；part `state.status=running` |
| MiniMax Code | `~/.minimax/v2/sqlite/runtime-state.sqlite`（WAL，1.6 GB / 363 行） | `local_runtime_sessions.status`（实测取值 idle/error/aborted/interrupted/…）+ `updated_at_ms`（有索引） | `local_runtime_token_usage`（per-turn token）；`local_runtime_message_rows.data_json` 尾部消息 |

**查询成本实测**：两库活跃会话查询均 ~6-9 ms，3 秒轮询成立。
**结论：被动只读监控可行，零侵入**，与 threadock「只读快照」原则（plan §10.1）一致。

### 方案对比

- **A. 被动监控（选定）**：直接扫源文件/源库。零侵入、 Desktop GUI agent（Codex Desktop / MiniMax 桌面）也能覆盖——它们的数据同样落在本机存储。
- B. ACP 主动控制（t3code 路线）：需接管 agent 启动，改变产品形态；桌面 GUI agent 不可控；工作量大。列为远期方向。
- C. 混合（实际采用）：A 为主 + 已导入会话深链回 threadock 主界面。

## 3. 架构

```
crates/live-monitor（新，仅依赖 adapter-sdk/domain/serde/time/thiserror/tempfile(dev)）
├── types.rs      LiveSource/LiveState/LiveTask/LiveActivity/LiveProgress
├── tail.rs       JSONL 尾部读取（最后 ~256KB，逐行 JSON，跳过噪声行）
└── probe/
    ├── mod.rs    LiveProbe trait + scan_all()（probe 级错误互不影响）
    ├── claude.rs   mtime 扫描 + 尾行解析（跳过 atis-latch 等非对话行）
    ├── codex.rs    session_meta(originator/cwd) + 尾部 event_msg
    ├── zcode.rs    ro SQLite：活跃 session + turn_usage(running) + todo 进度 + 尾消息
    └── minimax.rs  ro SQLite：活跃 session(status) + token_usage + 尾消息行

apps/desktop/src-tauri/src/commands/live.rs（新）
├── live_tasks()                  全源扫描 + 1s 结果缓存防抖；回填 conversation_id（按 provider+source_conversation_id 匹配已导入会话）
└── live_transcript(source, session_id, limit)   直接读源返回最近 N 条活动（抽屉用）

apps/desktop/src/LiveBoard.tsx（新页面「实时」，导航 ops 组，⌘9）
└── 卡片列表：源徽标 + 状态灯 + 标题/工作区 + 最近活动摘要 + 进度行 + 已用时长
    + 3s 轮询（页面可见时）+ 点击开抽屉（2s 轮询 live_transcript）
    + 「在 Threadock 打开」深链（conversation_id 命中时）
```

### 状态判定（v1 启发式，按源可覆盖）

- `Running`：最近活动 ≤ 45s；或 zcode 存在 `running` turn。
- `Waiting`：尾部出现审批/权限请求（codex approval 事件、claude 控制请求）。
- `Idle`：45s–10min 无写入（可能等用户输入/长工具）。
- `Recent`：10min–24h；`Stale`：更久。**不猜 Finished**（无源信号能证明）。
- MiniMax 显式 `status` 列优先于启发式。

## 4. 错误处理

- probe 级隔离：单源失败只计入返回的 `errors[]`，不拖垮整板。
- 源目录/库不存在 → 该源返回空（未装该 agent 是正常态）。
- SQLite 只读打开（`adapter-sdk::open_readonly`）+ 查询全部带 `LIMIT`；主同步 BusyGuard 不受影响（live 命令独立、不排全局忙锁）。

## 5. 测试与门禁

- `ch-live-monitor` 单测：fixture JSONL（claude/codex 各含噪声行）+ 临时 sqlite 造 zcode/minimax 最小 schema（复用 adapter 测试建表法）。
- 新命令收录 `examples/typegen.rs` 清单并重新生成 `docs/api.md`（`api_doc_coverage` 测试强制）。
- 预检钩子 6 项全过（eslint/typecheck/rust fmt+clippy/test/api.md 比对等）。

## 6. 非目标（v1）

不做控制面（发消息/审批代理）；不引 notify 文件监听（轮询够用、零新依赖）；
不做 Cursor / DeepSeek live 探测（需求未点名，`LiveSource` 枚举留扩展位）；
不做进程存活检测（mtime + 源侧运行态字段已够 v1 判定）。
