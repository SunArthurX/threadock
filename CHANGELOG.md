# Changelog

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
