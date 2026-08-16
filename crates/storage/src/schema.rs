//! 数据库 schema，对应 plan §12.1 核心表。
//!
//! 设计要点：
//! - 所有主键为 TEXT（带前缀的 UUID），见 `domain::id`。
//! - 时间戳统一存 unix 毫秒（i64）。
//! - 枚举存为 TEXT（小写字符串），由领域层负责转换。
//! - `conversations` 上有 UNIQUE 约束保证幂等：见 plan §11.3 幂等键。
//! - 索引覆盖常用查询路径。

/// V1 建表脚本。每个语句用 `;` 分隔，由 migration 逐条执行。
pub const SCHEMA_V1: &str = r"
-- ── 元数据：schema 版本追踪 ────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS schema_version (
    version    INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
);

-- ── providers：来源产品（去重维度） ───────────────────────────────────
CREATE TABLE IF NOT EXISTS providers (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    adapter_id      TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

-- ── installations：设备上的来源应用实例 ───────────────────────────────
CREATE TABLE IF NOT EXISTS installations (
    id                  TEXT PRIMARY KEY,
    provider_id         TEXT NOT NULL,
    device_id           TEXT NOT NULL,
    app_version         TEXT,
    executable_path     TEXT,
    data_path           TEXT,
    schema_fingerprint  TEXT,
    status              TEXT NOT NULL,
    last_seen_at        INTEGER,
    created_at          INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL,
    FOREIGN KEY(provider_id) REFERENCES providers(id)
);
CREATE INDEX IF NOT EXISTS idx_installations_provider ON installations(provider_id);

-- ── workspaces：合并后的统一项目 ──────────────────────────────────────
CREATE TABLE IF NOT EXISTS workspaces (
    id              TEXT PRIMARY KEY,
    display_name    TEXT NOT NULL,
    user_title      TEXT,
    canonical_path  TEXT,
    git_remote      TEXT,
    git_common_dir  TEXT,
    status          TEXT NOT NULL DEFAULT 'active',
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

-- ── source_workspaces：来源侧 workspace 到统一 workspace 的映射 ────────
CREATE TABLE IF NOT EXISTS source_workspaces (
    provider_id         TEXT NOT NULL,
    installation_id     TEXT NOT NULL,
    source_workspace_id TEXT NOT NULL,
    workspace_id        TEXT NOT NULL,
    raw_name            TEXT,
    raw_path            TEXT,
    match_method        TEXT,
    match_confidence    REAL,
    source_payload_id   TEXT,
    PRIMARY KEY(provider_id, installation_id, source_workspace_id),
    FOREIGN KEY(workspace_id) REFERENCES workspaces(id)
);

-- ── conversations：对话/Agent 任务（幂等核心表） ──────────────────────
CREATE TABLE IF NOT EXISTS conversations (
    id                     TEXT PRIMARY KEY,
    workspace_id           TEXT,
    provider_id            TEXT NOT NULL,
    installation_id        TEXT,
    source_conversation_id TEXT NOT NULL,
    title                  TEXT,
    user_title             TEXT,
    status                 TEXT,
    model                  TEXT,
    started_at             INTEGER,
    updated_at             INTEGER,
    completed_at           INTEGER,
    source_status          TEXT NOT NULL DEFAULT 'active',
    source_url             TEXT,
    completeness_score     REAL,
    content_hash           TEXT,
    raw_payload_id         TEXT,
    UNIQUE(provider_id, installation_id, source_conversation_id)
);
CREATE INDEX IF NOT EXISTS idx_conversations_workspace ON conversations(workspace_id);
CREATE INDEX IF NOT EXISTS idx_conversations_provider ON conversations(provider_id);
CREATE INDEX IF NOT EXISTS idx_conversations_updated ON conversations(updated_at);

-- ── turns：一次输入引发的完整执行 ─────────────────────────────────────
CREATE TABLE IF NOT EXISTS turns (
    id              TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    source_turn_id  TEXT,
    sequence_number INTEGER NOT NULL,
    status          TEXT,
    started_at      INTEGER,
    completed_at    INTEGER,
    duration_ms     INTEGER,
    FOREIGN KEY(conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_turns_conversation ON turns(conversation_id);

-- ── messages：消息 ────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS messages (
    id                TEXT PRIMARY KEY,
    conversation_id   TEXT NOT NULL,
    turn_id           TEXT,
    source_message_id TEXT,
    role              TEXT NOT NULL,
    content_text      TEXT,
    content_json      TEXT,
    sequence_number   INTEGER NOT NULL,
    created_at        INTEGER,
    content_hash      TEXT,
    raw_payload_id    TEXT,
    FOREIGN KEY(conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(conversation_id);
CREATE INDEX IF NOT EXISTS idx_messages_hash ON messages(content_hash);

-- ── events：执行事件（tool call/command/diff/approval…） ───────────────
CREATE TABLE IF NOT EXISTS events (
    id               TEXT PRIMARY KEY,
    conversation_id  TEXT NOT NULL,
    turn_id          TEXT,
    source_event_id  TEXT,
    event_type       TEXT NOT NULL,
    status           TEXT,
    summary          TEXT,
    payload_json     TEXT,
    sequence_number  INTEGER NOT NULL,
    created_at       INTEGER,
    completed_at     INTEGER,
    raw_payload_id   TEXT,
    FOREIGN KEY(conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_events_conversation ON events(conversation_id);
CREATE INDEX IF NOT EXISTS idx_events_type ON events(event_type);

-- ── sync_cursors：增量同步游标，见 plan §11.2 ─────────────────────────
CREATE TABLE IF NOT EXISTS sync_cursors (
    provider_id        TEXT NOT NULL,
    installation_id    TEXT NOT NULL,
    cursor_type        TEXT NOT NULL,
    cursor_value       TEXT,
    schema_fingerprint TEXT,
    last_success_at    INTEGER,
    PRIMARY KEY(provider_id, installation_id, cursor_type)
);

-- ── audit_logs：审计日志 ──────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS audit_logs (
    id            TEXT PRIMARY KEY,
    actor_type    TEXT NOT NULL,
    actor_id      TEXT,
    action        TEXT NOT NULL,
    target_type   TEXT,
    target_id     TEXT,
    result        TEXT NOT NULL,
    metadata_json TEXT,
    created_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_logs(created_at);

-- ── FTS5：全文搜索索引（plan §9.5 MVP/降级方案，§13.1 索引字段） ───────
-- 把 message 文本与所属 conversation 标题、provider 一起建联合索引。
-- contentless='1' 让索引不存原文（原文在 messages 表），节省空间。
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    message_id UNINDEXED,
    conversation_id UNINDEXED,
    provider UNINDEXED,
    workspace_id UNINDEXED,
    role UNINDEXED,
    title,
    body,
    tokenize = 'unicode61 remove_diacritics 2'
);

-- 触发器：messages INSERT 后同步到 FTS（含 conversation 标题与 provider）
CREATE TRIGGER IF NOT EXISTS messages_ai_fts AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(message_id, conversation_id, provider, workspace_id, role, title, body)
    SELECT
        NEW.id,
        NEW.conversation_id,
        (SELECT p.name FROM conversations c JOIN providers p ON p.id = c.provider_id WHERE c.id = NEW.conversation_id),
        (SELECT c.workspace_id FROM conversations c WHERE c.id = NEW.conversation_id),
        NEW.role,
        (SELECT COALESCE(c.user_title, c.title) FROM conversations c WHERE c.id = NEW.conversation_id),
        COALESCE(NEW.content_text, '');
END;

-- 触发器：messages DELETE 后清理 FTS
CREATE TRIGGER IF NOT EXISTS messages_ad_fts AFTER DELETE ON messages BEGIN
    DELETE FROM messages_fts WHERE message_id = OLD.id;
END;

-- 触发器：messages UPDATE 后重建对应 FTS 行
CREATE TRIGGER IF NOT EXISTS messages_au_fts AFTER UPDATE ON messages BEGIN
    DELETE FROM messages_fts WHERE message_id = OLD.id;
    INSERT INTO messages_fts(message_id, conversation_id, provider, workspace_id, role, title, body)
    SELECT
        NEW.id,
        NEW.conversation_id,
        (SELECT p.name FROM conversations c JOIN providers p ON p.id = c.provider_id WHERE c.id = NEW.conversation_id),
        (SELECT c.workspace_id FROM conversations c WHERE c.id = NEW.conversation_id),
        NEW.role,
        (SELECT COALESCE(c.user_title, c.title) FROM conversations c WHERE c.id = NEW.conversation_id),
        COALESCE(NEW.content_text, '');
END;
";

/// V2：收藏、标签、归档（plan §6.3 Workspace 管理 / §6.4 浏览 / §6.5 标签）。
///
/// - `conversations` 新增 `favorite`(INT 0/1)、`is_archived`(INT 0/1) 列。
/// - 新增 `conversation_tags` 多对多关联表。
pub const SCHEMA_V2: &str = r"
-- 收藏与归档标记（向后兼容：默认 0）
ALTER TABLE conversations ADD COLUMN favorite INTEGER NOT NULL DEFAULT 0;
ALTER TABLE conversations ADD COLUMN is_archived INTEGER NOT NULL DEFAULT 0;

-- 标签：多对多
CREATE TABLE IF NOT EXISTS conversation_tags (
    conversation_id TEXT NOT NULL,
    tag             TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    PRIMARY KEY(conversation_id, tag),
    FOREIGN KEY(conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_tags_tag ON conversation_tags(tag);
";

/// V3：知识提取结果持久化（plan §13.5「人工编辑后保留版本，不覆盖原始对话」）。
///
/// 每次提取/编辑存一行，支持多版本（`version` 递增）。`is_current` 标记当前版本。
/// 提取结果以 JSON 整体存储（ExtractionResult 序列化）。
pub const SCHEMA_V3: &str = r"
CREATE TABLE IF NOT EXISTS knowledge_extractions (
    id              TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    version         INTEGER NOT NULL,
    is_current      INTEGER NOT NULL DEFAULT 1,
    extractor       TEXT NOT NULL,
    result_json     TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    FOREIGN KEY(conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_knowledge_conv ON knowledge_extractions(conversation_id);
CREATE INDEX IF NOT EXISTS idx_knowledge_current ON knowledge_extractions(conversation_id, is_current);
";

/// V4：自定义脱敏规则持久化（plan §14.6「用户可配置忽略正则规则」）。
pub const SCHEMA_V4: &str = r"
CREATE TABLE IF NOT EXISTS redaction_rules (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    pattern    TEXT NOT NULL,
    enabled    INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
";

/// `V5：会话主子任务关系（source_parent_id`）。
/// ALTER TABLE ADD COLUMN 不支持 IF NOT EXISTS，由迁移框架的版本号机制保证只执行一次。
pub const SCHEMA_V5: &str = r"
ALTER TABLE conversations ADD COLUMN source_parent_id TEXT;
CREATE INDEX IF NOT EXISTS idx_conversations_parent ON conversations(source_parent_id);
";

/// V6：CodeAgentOps 指标表（plan codeagent-ops §3.2）。
pub const SCHEMA_V6: &str = r"
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
CREATE INDEX IF NOT EXISTS idx_usage_ts ON usage_records(ts);
CREATE INDEX IF NOT EXISTS idx_usage_provider_ts ON usage_records(provider_id, ts);

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
CREATE INDEX IF NOT EXISTS idx_tool_ts ON tool_call_records(ts);
CREATE INDEX IF NOT EXISTS idx_tool_destructive ON tool_call_records(destructive) WHERE destructive = 1;
";

/// V7：审计策略规则 + 预算设置（plan codeagent-ops M4/M5）。
pub const SCHEMA_V7: &str = r"
CREATE TABLE IF NOT EXISTS policy_rules (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    pattern TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('dangerous_command', 'sensitive')),
    severity TEXT NOT NULL DEFAULT 'medium' CHECK(severity IN ('low', 'medium', 'high')),
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS budget_settings (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    monthly_token_limit INTEGER,
    monthly_cost_limit REAL,
    notify_on_exceed INTEGER NOT NULL DEFAULT 1,
    updated_at INTEGER NOT NULL
);
";
/// V8：通用键值设置（同步节流时间戳等跨进程持久状态）。
pub const SCHEMA_V8: &str = r"
CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

/// V9：CodeAgentOps M6-M9 —— 资产清单、自动化任务、用量归因扩展。
pub const SCHEMA_V9: &str = r"
ALTER TABLE usage_records ADD COLUMN source_dir TEXT;
ALTER TABLE usage_records ADD COLUMN context_exceeded INTEGER NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS asset_records (
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT,
    description TEXT,
    risky_hits INTEGER NOT NULL DEFAULT 0,
    installed_at TEXT,
    path TEXT,
    UNIQUE(provider_id, kind, name, version)
);

CREATE TABLE IF NOT EXISTS automation_records (
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'cron',
    schedule TEXT,
    status TEXT,
    detail TEXT,
    UNIQUE(provider_id, name)
);
";
/// V10：导入新鲜度状态（「已导入」判定：源更新时间 ≤ 导入时观察时间）。
pub const SCHEMA_V10: &str = r"
CREATE TABLE IF NOT EXISTS import_state (
    source_pk TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    observed_ms INTEGER,
    imported_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_import_state_provider ON import_state(provider_id);
";

/// FTS5 行触发器（范围重置临时禁用后重建用）。
pub const SCHEMA_FTS_TRIGGERS: &str = r"
CREATE TRIGGER IF NOT EXISTS messages_ai_fts AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(message_id, conversation_id, provider, workspace_id, role, title, body)
    SELECT
        NEW.id,
        NEW.conversation_id,
        (SELECT p.name FROM conversations c JOIN providers p ON p.id = c.provider_id WHERE c.id = NEW.conversation_id),
        (SELECT c.workspace_id FROM conversations c WHERE c.id = NEW.conversation_id),
        NEW.role,
        (SELECT COALESCE(c.user_title, c.title) FROM conversations c WHERE c.id = NEW.conversation_id),
        COALESCE(NEW.content_text, '');
END;

-- 触发器：messages DELETE 后清理 FTS
CREATE TRIGGER IF NOT EXISTS messages_ad_fts AFTER DELETE ON messages BEGIN
    DELETE FROM messages_fts WHERE message_id = OLD.id;
END;

-- 触发器：messages UPDATE 后重建对应 FTS 行
CREATE TRIGGER IF NOT EXISTS messages_au_fts AFTER UPDATE ON messages BEGIN
    DELETE FROM messages_fts WHERE message_id = OLD.id;
    INSERT INTO messages_fts(message_id, conversation_id, provider, workspace_id, role, title, body)
    SELECT
        NEW.id,
        NEW.conversation_id,
        (SELECT p.name FROM conversations c JOIN providers p ON p.id = c.provider_id WHERE c.id = NEW.conversation_id),
        (SELECT c.workspace_id FROM conversations c WHERE c.id = NEW.conversation_id),
        NEW.role,
        (SELECT COALESCE(c.user_title, c.title) FROM conversations c WHERE c.id = NEW.conversation_id),
        COALESCE(NEW.content_text, '');
END;
";

/// V12：范围重置/按时间检索走索引（一个月以上数据长存场景）。
pub const SCHEMA_V12: &str = r"
CREATE INDEX IF NOT EXISTS idx_conversations_updated ON conversations(updated_at);
";

/// V13：会话私有笔记（user-only markdown，不参与搜索/导出/统计）。
/// 用独立表而非 conversations 列，零侵入现有 SELECT 列表。
pub const SCHEMA_V13: &str = r"
CREATE TABLE IF NOT EXISTS conversation_notes (
    conversation_id TEXT PRIMARY KEY,
    note            TEXT NOT NULL,
    updated_at      INTEGER NOT NULL,
    FOREIGN KEY(conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_conv_notes_updated ON conversation_notes(updated_at);
";

/// V11：审计发现处置状态（忽略/误报白名单，M4 处置闭环）。
pub const SCHEMA_V11: &str = r"
CREATE TABLE IF NOT EXISTS audit_finding_states (
    fingerprint TEXT PRIMARY KEY,
    status      TEXT NOT NULL,
    note        TEXT,
    created_at  INTEGER NOT NULL
);
";
