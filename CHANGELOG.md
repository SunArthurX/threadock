# Changelog

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
