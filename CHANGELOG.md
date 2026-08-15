# Changelog

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
