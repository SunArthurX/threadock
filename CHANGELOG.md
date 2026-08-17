# Changelog

## [1.0.1] - 2026-08-17

v1.0.0 后的全功能测试轮（CLI 真人 E2E 59 项 + Tauri 命令层 3 旅程 + 浏览器 GUI 真人模拟）发现并修复：

### Fixed
- **daemon 忽略 `--db` 文件名**：`ch --db hub.db daemon` 实际打开的是 `<dir>/threadock.db`
  （空库）——CLI E2E 发现；`DaemonStateConfig` 新增 `db_path` 透传，默认行为不变
- **`import-from zcode` 在 `~/.zcode` 缺失时**报 sqlite 原始错误：list/import 加存在性
  守卫，对齐 claude-code 的友好提示行为
- **前端对后端 null 返回零容错**：`ops_by_provider` 等返回 null 时概览/成本页直接崩
  （ErrorBoundary 兜底但视图报废）——OpsView 18 处 + ActivityView 3 处 + KnowledgeView
  2 处归一化 `?? []`；恶劣后端（全部命令返回 null）下 8 视图全部正常渲染空态
- **GUI 版本号漂移**：前端 `APP_VERSION="0.1.0"` / `CORE_VERSION="0.2.0"` 硬编码从未
  随版本更新（更新日志弹窗显示 v0.1.0）——改为构建期从 `package.json` /
  workspace `Cargo.toml` 派生，新增 round5 契约测试锁定「三者一致」；补 1.0.0 更新
  日志条目
- **知识库页在 IPC 异常时永远「加载中」**：null 归一为空 KB 走空态

### Tests
- 新增 `scripts/e2e_cli.sh`：13 组场景 × 59 断言（真实二进制 + 临时库；导入幂等/
  双引擎搜索语法/知识/脱敏/导出/备份恢复/硬删级联/daemon 14 方法含错误路径）
- 新增 Tauri 命令层 E2E（`src/e2e_journeys.rs`）：mock app + 真实后端 3 条用户旅程
  （会话生命周期/Workspace 治理/保存搜索），直接调 GUI 实际 invoke 的命令函数
- 浏览器 GUI 真人模拟：IPC mock 注入驱动真实前端——8 视图巡检、搜索语法、保存搜索
  下拉、原始视图切换、恢复命令复制、Workspace 重命名/置信度徽标、恶劣后端压力

## [1.0.0] - 2026-08-17

首个正式版本。以执行计划 Phase 2（MVP，Gate 1）为验收基线，P0 发布工程 +
P1 MVP 功能缺口 + P2 验收证据全部闭环。

### Added
- **搜索查询语法**（plan §13.2）：`provider:` `workspace:` `type:` `role:` `status:`
  `file:` `model:` `after:` `before:` 前缀，FTS5 全量 SQL 过滤 + Tantivy 索引内过滤
  （含纯过滤 AllQuery、多 workspace OR），双引擎三集成层（GUI/daemon/CLI）一致生效
- **保存搜索**：V14 `saved_searches` 表，搜索框 ☆ 保存 → 下拉一键执行/删除
- **Workspace 治理**（plan §4.3/P2-2）：手动合并（并入 + 治理审计 + 映射升 manual/1.0）、
  拆分（会话多选 → 移到新 Workspace）、重命名、来源映射置信度列表；
  设置页新增「Workspace 管理」分区（<0.8 低置信度警示徽标）
- **原始视图 ↔ 统一视图**（plan P2-3）：会话详情一键查看 Raw Store 未标准化归档
- **一键打开来源应用 / 恢复命令**（plan P2-3）：GUI 来源拉起应用；
  claude-code / codex 生成 `claude --resume` / `codex resume` 命令并复制
- **jieba 可插拔中文分词器**（plan §13.1）：`--features jieba` 启用（自实现
  tantivy TokenStream 适配，词典全局单例），默认 N-gram 兜底；CI 独立 job 验证
- **Release 流水线**：推 v* tag → 三平台 Tauri 安装包 + CLI 四平台二进制 + SHA256SUMS
- **用户文档**：`docs/user-guide.md`（11 章用户指南）+ `docs/privacy.md`（隐私声明）
- **Golden Fixture Kit**（plan §20.2）：`fixtures/` 脱敏样本集 + adapter golden tests
- **Gate 1 大规模基准**：10 万会话 / 50 万消息 FTS5 P95 = **50.9ms**（红线 300ms），
  报告留档 `docs/benchmark-report-v1.0.0.md`

### Fixed
- **resolver 误并缺陷**：候选与已知项都带结构化标识（remote/path/fsid）却全未命中时，
  纯名称相同不再静默 AutoMerge，降级 NeedsConfirmation（合并准确率基准化时发现）
- 版本三处不一致（workspace/Tauri/CHANGELOG）自 0.4.0 起统一

### Tests
- workspace 421（+42：query_syntax 18 / 语法集成 8 / saved-search 与 workspace 治理 6 /
  golden 4 / 大规模基准 1 / jieba 1 等）+ Tauri 13 + 前端 344，全绿
- ESLint 0 error / 0 warning（57 个历史 warning 清零：40+ 真修复 + 10 处带理由豁免）
- 合并准确率：11 例标注样本 100%（错误 AutoMerge = 0）

### 已知限制（发布说明）
- 安装包**未签名**（macOS 右键打开 / Windows SmartScreen「仍要运行」；SHA256 校验）
- 自动更新（Tauri Updater）未配置，规划 1.1
- OpenCode Adapter、Daemon UDS IPC、Adapter 进程配额移至 1.1（v1.0 范围裁剪决策）

## [0.4.0] - 2026-08-17

> 版本治理：此前 workspace（0.2.0）/ Tauri（0.1.0）/ CHANGELOG（0.3.0）三处版本不一致，自本版起统一。

### Added（0.3.0 后 26 轮产品迭代补记）
- **Command Palette**：全局命令面板 + 快捷键
- **首次启动引导**（OnboardingTour）+ 启动更新日志（ChangelogModal）
- **活动页大改版**：GitHub 风格热力图（7×N 布局 + hover tooltip + 自适应）、4 张统计卡、工具 Top 10、24h 时间分布
- **会话详情增强**：页内搜索、多选批量操作、新鲜度标识、Toast undo、右键菜单、代码块语法高亮
- **知识库增强**：跨会话知识引用、知识筛选导出、周对比视图
- **私人笔记**：会话级 user-only markdown 笔记（V13 conversation_notes 表，不参与搜索/导出/统计）
- **配置导入导出** + 批量加标签 + 自定义标题（空 = 恢复原始）
- **Prompt 复用 / 会话续做**：write_clipboard 自定义 Tauri command（绕开 macOS WKWebView 剪贴板限制）
- **UI 体系**：自定义 ScrollArea 组件（WKWebView 滚动条兼容）、可拖拽 resizer、暗色/亮色主题协调、全页 max-width 布局
- **底部状态栏** + 排序置顶 + 用户偏好持久化

### Fixed
- 项目页「查看会话」SQL JOIN 列名错（sqlite no such column）
- 重置时间范围改为用户自选 + 修 import_state 残留导致「重置 = 数据丢失」
- ScrollArea 滚轮失灵、热力图 cell 变形/黑屏回归、activity_stats tuple 序列化
- 第 26 轮 4 区并发体检：32 findings → 0

### Engineering
- CI：修复三类历史失败 + clippy 1.97+ 新增 lint + rustfmt 版本要求
- 安全：启用 Dependabot + CodeQL；修 supply-chain advisory 3 个（2 完成 + 1 个上游 blocked：RUSTSEC-2026-0253 lru，等 tantivy 升级，CI 已容忍）
- 测试增长：workspace 379 + Tauri 13 + 前端 344（全绿）；ESLint 0 error

## [0.3.0] - 2026-08-15

### Governance Closed-Loop (治理闭环，三批次)
- **Budget alerting**: notify_on_exceed now enforced (settings toggle + topbar global budget bar + over-limit toast with per-month dedup); month-end cost/token projection (daily-run-rate extrapolation)
- **Audit disposition**: findings persisted fingerprints (V11 audit_finding_states), ignore / false-positive whitelist, per-conversation rescan, policy rule enable/disable switches (was delete-only)
- **Notification system**: in-app toasts (budget/audit/weekly/retention) — none existed before
- **Conversation governance**: favorite star + tag editing (backend APIs existed, UI was zero), archive, soft-delete + recycle-bin view with restore, hard delete with typed「删除」confirmation
- **Hard delete now cascades**: raw blob + tantivy documents + governance log (previously leaked both)
- **Timeline fixed (M15)**: messages+events merged by timestamp (was unsorted concat), event timestamps shown (EventDto.created_at_ms added), cap 100 → 2000
- **Data lifecycle**: storage dashboard (db/raw/index), orphan-blob GC with 1h race protection, retention policy (auto-archive >N days at startup), search-index rebuild
- **Weekly report automation**: auto-generated when >7 days stale into app_data/reports/ + manual button
- **Governance audit trail**: audit_logs table enabled — every sensitive action (reset/GC/delete/policy/disposition) recorded and shown in Settings
- **Dashboard**: per-card visibility toggles (persisted), daily cache-hit trend chart, automation watch-list (read-only principle preserved)

### Fixed
- **Search delete was silently broken**: "raw" tokenizer was actually SimpleTokenizer — underscore IDs were tokenized so delete_term never matched; switched to RawTokenizer + realistic-ID regression test + rebuild-index migration command for existing indexes
- MSRV job/toolchain: lockfile requires 1.88 (declared 1.75 was fictional)

### Tests
- 371 workspace (was 361) + 7 tauri (was 4... now 7) + 34 frontend (was 24)

## [0.2.1] - 2026-08-15

### Security
- **Search snippet XSS fixed (stored)**: Tantivy & FTS5 highlight paths both HTML-escape output; CSP enabled in tauri.conf.json (was null)
- **Backup KDF upgraded to Argon2id**: random 16-byte salt per backup (v2 format); v1 backups still restorable; zip-slip path validation + length-field bounds (no panic on crafted files)
- **Redaction coverage extended**: content_json now redacted in JSON exports; custom rules applied on GUI & batch export paths (was CLI-single-only); new patterns: sk-proj-/sk-ant- (hyphenated keys), glpat-, xox[baprs]-, AIza, bare JWT, PEM private key blocks
- **Daemon hardening**: 16MiB per-line request cap (capped reader), panic recovery via catch_unwind (handler crash no longer kills daemon)
- **Audit HTML report**: rule names now escaped (was raw injection point)

### Performance
- **Import throughput 1575 → 3903 msg/s (2.48x)**: synchronous=FULL → NORMAL (WAL-recommended), all 4 legacy per-message upsert paths unified onto single-transaction import_conversation_batch (Tauri import_file, daemon provider.sync, CLI import/import_raw)
- **N+1 eliminated on conversation list**: one GROUP BY for all child counts (was 1 query per conversation on every UI refresh)
- **RawStore dedup short-circuit**: existing content-hash objects skip zstd-9 recompression + rewrite + fsync
- **Audit scan O(n²) → O(n)**: OFFSET pagination replaced with keyset (WHERE m.id > last)
- **conversation.similar bounded**: 100 candidates x 20 messages cap (was: entire DB in memory)
- **Heavy commands off the async runtime**: auto_sync/ops_sync/audit/import wrapped in block_in_place; ops_sync collects (lock-free) before taking the write lock
- **daemon read path on read connection**: all read handlers use read_repo (was: write lock for everything)
- **Tantivy writer heap centralized** as ch_search::DEFAULT_WRITER_HEAP

### Fixed
- import_conversation_batch no longer overwrites conv.workspace_id with NULL when workspace_name is None
- IS_BUSY panic-safety: RAII BusyGuard (a panic no longer permanently blocks sync/reset)
- 7 silent error swallows now logged (import-state persistence failures caused invisible duplicate imports)
- MSRV violation: NonZero::get fn-ref (stable 1.79) replaced with 1.75-compatible call

### Changed
- Error codes actually classified now: 139 internal_err sites split into storage/search/import/io (was: everything Internal)
- open_db deduplicated into ch_adapter_sdk::open_readonly (3 copies removed)
- README updated: Threadock branding, V10 schema/22 tables, current crate tree, corrected doc paths
- Clippy clean under rust 1.95 pedantic (37 new-lint fixes across workspace)

### Refactored
- **storage/repository.rs split (3373 → 2082 lines)**: ops metrics into `repository/ops_queries.rs` (25 methods), audit/policy/budget into `repository/audit_repo.rs`, knowledge persistence into `repository/knowledge_repo.rs`, settings/redaction rules into `repository/settings.rs` — public re-export paths unchanged
- **auto_sync_inner table-driven**: 5 copy-pasted provider blocks replaced by `source_table()` descriptor table + one generic loop; JSON output keys byte-identical (frontend contract)
- **src-tauri/lib.rs split (1913 → 174 lines)**: commands organized into `commands/{conversations,import,ops,audit,export_cmd,dto}` with shared helpers in `commands/mod.rs`

### Engineering
- **Frontend testing from zero**: vitest + Testing Library (14 tests incl. snippet XSS regression), ESLint (typescript-eslint + react-hooks, 0 errors)
- **CI extended**: workspace tests on ubuntu/macos/windows matrix, MSRV job (1.88 — declared 1.75 was fictional, lockfile requires 1.88; verified locally), cargo-audit supply-chain job, frontend lint+test steps
- **Tauri command layer tests from zero**: source_table/auto_sync roundtrip (output-key contract), import_file_inner roundtrip + idempotency

### Tests
- 361 workspace tests (was 342) + 3 Tauri + 14 frontend: +XSS escape (tantivy/fts5), +Argon2 salt/zip-slip/truncated-backup rejection, +content_json redaction, +capped-line reader, +batch workspace preservation, +bulk child counts, +RawStore idempotency


## [0.2.0] - 2026-08-15

### Added
- **M1-M15 CodeAgentOps**: Full observability platform for AI coding agents
  - M1-M2: Metrics collection (4 agents, 44K+ request-level usage records)
  - M3: Governance dashboard with animated charts
  - M4: Security audit engine (sensitive info + dangerous commands)
  - M5: Policy, budget, and pricing model
  - M6: Asset inventory (skills/plugins across all agents)
  - M7: Cost attribution by project + cache hit rate analysis
  - M8: Automation/cron task governance
  - M9: Anomaly detection (error spikes, retry storms, context exceeded)
  - M10: Agent health scoring (stability 0-100)
  - M11: Latency P50/P95 percentile tracking
  - M12: Token waste detection (context accumulation patterns)
  - M13: Agent side-by-side benchmark comparison
  - M14: Weekly HTML report generation
  - M15: Conversation timeline view
- **5 AI IDE adapters**: ZCode, Claude Code, Cursor, MiniMax Code, Codex
- **Dual-connection SQLite**: WAL mode read/write separation for zero-lock UI
- **All 56 Tauri commands async**: Non-blocking IPC on tokio runtime
- **Collapsible left sidebar**: 5-tab navigation with icon-only collapse mode
- **Incremental import**: Stale detection via source update timestamps
- **10-minute auto-sync**: Background data refresh with cancel support
- **Dark/light theme**: CSS variable design system with persistence

### Performance
- WAL dual-connection: reads never blocked by writes (6ms concurrent queries)
- Chunked batch transactions: 2000 rows per commit
- Single tantivy commit per sync cycle
- Fire-and-forget ops_sync: zero blocking on page render
- Parallel codex collection: 8 threads, 8s → 1.8s

### Fixed
- providers.adapter_id NOT NULL constraint crash (silent import failures)
- MiniMax hidden session filtering (visibility vs archived distinction)
- ZCode subagent_child parent linkage (424 orphan children repaired)
- Audit UTF-8 char boundary panic (multibyte context slicing)
- Invalid Date in risk calls (OffsetDateTime serialization → ms)
- ops_overview parameter count mismatch (10 placeholders, 1 param)
- Import freshness via V10 import_state table

## [0.1.0] - 2026-08-02

### Initial Release
- Core conversation archival and search platform
- 4 AI IDE adapters with read-only snapshot principle
- Tantivy full-text search with N-gram Chinese tokenization
- BLAKE3 content-addressed raw storage with zstd compression
- Encrypted backup (XChaCha20-Poly1305)
- Redaction engine (7 builtin + custom regex rules)
