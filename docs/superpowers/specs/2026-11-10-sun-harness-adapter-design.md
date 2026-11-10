# sun-harness 接入设计（对标 deepseek-harness 全链路）

日期：2026-11-10 · 状态：已获用户批准 · 发版：v1.8.0（含）

## 背景与目标

sun-harness 是用户本地的个人 agent harness（`~/Downloads/06.code/sun-harness`），会话以
append-only JSONL 存于 `~/.sun/sessions/`。目标：threadock 以**只读被动导入**方式接入
sun-harness，能力与 deepseek-harness（dsh）适配器完全同级：会话列表、手动导入、启动
auto_sync、ops 统计、搜索、ContextView 全保真上下文。随后按惯例发 v1.8.0。

**范围外**：live-monitor 实时探测、handoff 写回、export——均不改动。

## 数据源事实（本机实测，2026-11-10）

- 根目录：`SUN_HOME` 环境变量优先，否则 `~/.sun`（sun 自身 `paths.ts sunHome()` 同规则）；
  会话目录 `<root>/sessions/`，本机 435 个会话
- 文件形态两种：平铺 `<id>.jsonl`（id 形如 `20261107-01b4eed9`，本机全部为此形态）、
  目录 `<id>/session.jsonl`（sun 的 `publishImport` 导入发布形态）
- 无 zstd 压缩、无独立 thinking/reasoning 块、无独立 tool/call 事件（工具调用挂在
  assistant 消息的 `toolCalls[]` 上）
- 事件类型全集（按行数）：`assistant/message`、`tool/result`、`checkpoint`、`run/status`、
  `user/message`、`request/header`、`session/meta`、`verify/result`、`job/started|finished`、
  `todo/update`、`session/title`、`approval/request|decision`、`model/switch`

## §1 发现层

- `discover_sessions(root)`：只列顶层会话（导入面板用）；`discover_all_sessions(root)`：
  全部会话含 subagent（auto_sync / 手动导入定位用，parent 链）
- 两种文件形态都识别；id 取文件名（平铺）或目录名（目录形态）
- **跳过 `session/meta.importedThrough` 非空的会话**：sun 的 export/import 会把外部工具
  会话镜像进 sessions 目录，本体由各自 adapter 导入，避免重复入库（同 dsh 排除
  `ext-` 镜像的原则）。目录形态但非 importedThrough 的正常收录
- subagent：`session/meta.parent` 存在 → `source_parent_id` 主子链路
- 目录不存在 / `SUN_HOME` 指向缺失路径 → 返回空列表（非错误）

## §2 事件映射

| sun 事件 | 映射 |
|---|---|
| `session/meta` | 会话头：title 初值 / cwd / model / createdAt；`parent`→父子链；`importedThrough` 非空→整会话跳过；`approval` 等其余字段不消费 |
| `user/message` | `source=='user'`→用户消息正文；其余 source（`hook/system/schedule/job/verify/memory/trigger`）→系统注入事件：保留为领域事件（全保真），不计入对话正文 |
| `assistant/message` | `text` 非空→助手消息正文；`toolCalls[]` 逐个→ToolCallStarted 领域事件（`bash/shell`→命令、`write/edit/apply_patch` 类→变更、`read` 类→读文件、其余保留原始名泛化为工具调用）；`usage.{inputTokens,outputTokens}`→token 用量；`model`→模型名 |
| `tool/result` | 按 `callId` 与调用配对，`output`/`isError` 并入事件 payload；命令类升格 CommandCompleted；无配对调用的孤立 result 降级保留原始事件 |
| `session/title` | 标题覆盖，后发优先（`source` 字段不区分对待） |
| `model/switch` | `to` 作模型名兜底 |
| `request/header` | `systemPrompt`→系统块事件（全保真保留） |
| 其余（`run/status`/`checkpoint`/`todo/update`/`approval/*`/`job/*`/`verify/result`） | 忽略——dsh 降级原则：未知事件不崩溃、不猜字段 |

- 坏行（crash 造成的 torn final line）：跳行继续，sun 自身同此容忍
- 空 `text` 且无 `toolCalls` 的 assistant 消息不产生空正文消息

## §3 threadock 接线清单（对标 dsh，14 处）

1. **新 crate** `crates/adapter-sun-harness`：单文件 lib.rs，三入口
   `discover_sessions` / `discover_all_sessions` / `parse_session`，错误类型
   `SunHarnessError`，内嵌单测；加入根 workspace members
2. `crates/domain/src/lib.rs`：`Provider::SunHarness`，字符串 `"sun-harness"`，
   `from_str`/`as_str` 双向映射，全量 Provider 列表（≈line 659）加入
3. `apps/desktop/src-tauri/src/commands/import.rs`：
   - `sun_home()`：读 `SUN_HOME`，缺省 `~/.sun`
   - `list_sun_sessions` / `import_from_sun` 两条 tauri 命令（对标 dsh 同款）
   - `source_table` 增 sun 段：`provider_id: "prov_sun-harness"`、`stat_key: "sun"`、
     `workspace: "Sun Harness"`，auto_sync 计数 `sun_imported`；全保真回填经
     source_table 自动覆盖（新源无存量，无需专门回填迁移）
   - 单测：临时 home 写 sun 会话 → 导入计数、幂等、importedThrough 排除
4. `apps/desktop/src-tauri/src/lib.rs`：注册两条新命令
5. `apps/desktop/src-tauri/examples/typegen.rs`：如命令清单需同步则更新
6. `apps/desktop/src-tauri/src/adapter_journeys.rs`：sun journey（discover→parse→import
   端到端）
7. 前端 7 文件：`types.ts`（Provider/来源枚举加 `sun-harness`）、`ImportMenu.tsx`
   （导入菜单加 Sun Harness 入口）、`ConversationList.tsx`（来源徽章/过滤）、`App.tsx`、
   `SettingsView.tsx`、`OverviewCards.tsx`、`ops-types.ts`
8. `crates/storage/src/repository.rs` + `repository/ops_queries.rs`：provider 接入
   统计/查询（对标 dsh 的挂点）
9. `apps/desktop/src-tauri`（独立 workspace）`Cargo.toml` 加 `ch_adapter_sun_harness` 依赖；
   两份 Cargo.lock 随发版刷新

**不改**：live-monitor、handoff、export、llm（`client.rs` 的 deepseek 命中是 LLM
供应商命名，与本适配器无关）。

## §4 错误处理与降级

- 会话目录不存在 → 空列表；单文件解析失败 → 跳过该会话，不打断整轮（回填/auto_sync
  已有逐条 warn 留痕惯例）
- 未知事件类型 / 未知工具名 / 未知 content 形态 → 不崩溃、不猜字段，按上表降级
- auto_sync 与既有源共用幂等机制（mtime/内容 hash），无新增逻辑

## §5 测试与验收

- adapter 单测：真实事件快照 fixture 覆盖——消息正文、工具配对（含孤立 result）、
  注入过滤、标题覆盖、parent 链、importedThrough 跳过、目录形态、坏行容忍
- import.rs 测试：auto_sync `sun_imported` 计数、重复同步幂等、镜像排除
- journey 端到端测试（对标 dsh journey）
- 全量 `cargo test` + precheck 通过（注意 CI stable clippy 新 lint 漂移老坑）
- 真机冒烟（tauri dev）：435 个 sun 会话可发现、抽样导入、列表/详情/搜索/ContextView
  可见、ops 卡片含 Sun Harness

## §6 发版（v1.8.0）

- 四处版本号对齐：`package.json`、`package-lock.json`（×2 处）、
  `src-tauri/tauri.conf.json`、根 `Cargo.toml [workspace.package]`（CORE_VERSION 源）
- 刷新**两份独立 Cargo.lock**（根 + src-tauri）
- 提交并推送 master（HTTPS remote，必要时走本机代理）；推 `v1.8.0` tag 触发
  release.yml 生成 GitHub Release
- CI 偶发 flaky（npm test）：重推即过；以 ls-remote SHA == 本地 rev-parse 为准
