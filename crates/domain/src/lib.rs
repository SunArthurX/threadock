//! Conversation Hub 统一领域模型（v0.1）
//!
//! 对应 plan §4「术语和统一领域模型」与 §12「数据模型」。
//! 本 crate 只定义类型，不涉及任何存储或解析逻辑。

pub mod config;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

pub mod error;
pub mod id;

pub use error::{DomainError, Result};

/// 统一时间戳类型：所有领域对象使用 UTC 时间。
pub type Timestamp = OffsetDateTime;

/// 生成新的对象 ID（ULID 风格的带前缀 UUID v4）。
///
/// 前缀让人一眼看出对象类型，便于日志和调试。
pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

// ────────────────────────────────────────────────────────────────────────────
// 枚举：来源产品、消息角色、事件类型、状态
// ────────────────────────────────────────────────────────────────────────────

/// 来源产品（Provider），对应 plan §4.1。
///
/// 注意：`Unknown` 兜底用于未来新增来源；新增成员必须追加到末尾以保证序列化稳定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Provider {
    Codex,
    Cursor,
    #[serde(rename = "claude-code")]
    ClaudeCode,
    ZCode,
    #[serde(rename = "minimax-code")]
    MinimaxCode,
    #[serde(rename = "opencode")]
    OpenCode,
    /// Markdown / JSONL / ZIP 等通用导入
    Generic,
    Unknown,
}

impl Provider {
    /// 稳定的字符串标识，用于数据库存储和幂等键。
    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::Codex => "codex",
            Provider::Cursor => "cursor",
            Provider::ClaudeCode => "claude-code",
            Provider::ZCode => "zcode",
            Provider::MinimaxCode => "minimax-code",
            Provider::OpenCode => "opencode",
            Provider::Generic => "generic",
            Provider::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Provider {
    type Err = DomainError;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "codex" => Ok(Provider::Codex),
            "cursor" => Ok(Provider::Cursor),
            "claude-code" => Ok(Provider::ClaudeCode),
            "zcode" => Ok(Provider::ZCode),
            "minimax-code" => Ok(Provider::MinimaxCode),
            "opencode" => Ok(Provider::OpenCode),
            "generic" => Ok(Provider::Generic),
            "unknown" => Ok(Provider::Unknown),
            other => Err(DomainError::UnknownProvider(other.to_string())),
        }
    }
}

/// 消息角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => "tool",
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 统一事件类型，对应 plan §12.2。
///
/// 覆盖 Tool Call、Command、File、Diff、Approval、Browser、MCP、Subagent、Plan、Status、Error、Artifact。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    ToolCallStarted,
    ToolCallCompleted,
    CommandStarted,
    CommandCompleted,
    FileRead,
    FileCreated,
    FileUpdated,
    FileDeleted,
    DiffGenerated,
    ApprovalRequested,
    ApprovalGranted,
    ApprovalDenied,
    BrowserAction,
    McpCall,
    SubagentStarted,
    SubagentCompleted,
    PlanCreated,
    StatusChanged,
    Error,
    ArtifactCreated,
}

impl EventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventType::ToolCallStarted => "tool_call_started",
            EventType::ToolCallCompleted => "tool_call_completed",
            EventType::CommandStarted => "command_started",
            EventType::CommandCompleted => "command_completed",
            EventType::FileRead => "file_read",
            EventType::FileCreated => "file_created",
            EventType::FileUpdated => "file_updated",
            EventType::FileDeleted => "file_deleted",
            EventType::DiffGenerated => "diff_generated",
            EventType::ApprovalRequested => "approval_requested",
            EventType::ApprovalGranted => "approval_granted",
            EventType::ApprovalDenied => "approval_denied",
            EventType::BrowserAction => "browser_action",
            EventType::McpCall => "mcp_call",
            EventType::SubagentStarted => "subagent_started",
            EventType::SubagentCompleted => "subagent_completed",
            EventType::PlanCreated => "plan_created",
            EventType::StatusChanged => "status_changed",
            EventType::Error => "error",
            EventType::ArtifactCreated => "artifact_created",
        }
    }
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 对象生命周期状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Active,
    Completed,
    Failed,
    Cancelled,
    Archived,
    Deleted,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Active => "active",
            Status::Completed => "completed",
            Status::Failed => "failed",
            Status::Cancelled => "cancelled",
            Status::Archived => "archived",
            Status::Deleted => "deleted",
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 来源安装与映射
// ────────────────────────────────────────────────────────────────────────────

/// 某台设备上的一个来源应用安装实例，对应 plan §12.1 installations。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installation {
    pub id: String,
    pub provider: Provider,
    pub device_id: String,
    pub app_version: Option<String>,
    pub executable_path: Option<String>,
    pub data_path: Option<String>,
    pub schema_fingerprint: Option<String>,
    pub status: Status,
    pub last_seen_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Installation {
    pub fn new(provider: Provider, device_id: impl Into<String>) -> Self {
        let now = now_utc();
        Self {
            id: new_id("inst"),
            provider,
            device_id: device_id.into(),
            app_version: None,
            executable_path: None,
            data_path: None,
            schema_fingerprint: None,
            status: Status::Active,
            last_seen_at: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// 来源 Workspace（未合并的原始 workspace），对应 plan §12.1 source_workspaces。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceWorkspace {
    pub provider: Provider,
    pub installation_id: String,
    /// 来源侧的 workspace 标识（路径或 ID）。
    pub source_workspace_id: String,
    /// 映射到的统一 Workspace。
    pub workspace_id: String,
    pub raw_name: Option<String>,
    pub raw_path: Option<String>,
    /// 合并方式，见 plan §4.3（manual / git_remote / path / ...）。
    pub match_method: Option<MatchMethod>,
    pub match_confidence: Option<f64>,
    pub source_payload_id: Option<String>,
}

/// Workspace 合并方式，对应 plan §4.3 的 7 级优先级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchMethod {
    /// 用户手动绑定（最高优先级）
    Manual,
    /// 统一 Project Manifest ID
    ManifestId,
    /// 规范化 Git Remote URL
    GitRemote,
    /// Git Common Directory
    GitCommonDir,
    /// 规范化绝对路径
    CanonicalPath,
    /// 文件系统对象 ID（inode）
    FilesystemId,
    /// 名称相似度（最低，低置信度候选）
    NameSimilarity,
}

impl MatchMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            MatchMethod::Manual => "manual",
            MatchMethod::ManifestId => "manifest_id",
            MatchMethod::GitRemote => "git_remote",
            MatchMethod::GitCommonDir => "git_common_dir",
            MatchMethod::CanonicalPath => "canonical_path",
            MatchMethod::FilesystemId => "filesystem_id",
            MatchMethod::NameSimilarity => "name_similarity",
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 核心领域对象：Workspace / Conversation / Turn / Message / Event
// ────────────────────────────────────────────────────────────────────────────

/// 统一 Workspace（合并后的项目），对应 plan §12.1 workspaces。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub display_name: String,
    pub user_title: Option<String>,
    pub canonical_path: Option<String>,
    pub git_remote: Option<String>,
    pub git_common_dir: Option<String>,
    pub status: Status,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Workspace {
    /// 用户可见名称：用户自定义标题优先，否则用展示名。
    pub fn effective_title(&self) -> &str {
        self.user_title.as_deref().unwrap_or(&self.display_name)
    }

    pub fn new(display_name: impl Into<String>) -> Self {
        let now = now_utc();
        Self {
            id: new_id("ws"),
            display_name: display_name.into(),
            user_title: None,
            canonical_path: None,
            git_remote: None,
            git_common_dir: None,
            status: Status::Active,
            created_at: now,
            updated_at: now,
        }
    }
}

/// 一条对话或 Agent 任务，对应 plan §12.1 conversations。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub workspace_id: Option<String>,
    pub provider: Provider,
    pub installation_id: Option<String>,
    /// 来源侧的会话 ID，参与幂等键。
    pub source_conversation_id: String,
    pub title: Option<String>,
    pub user_title: Option<String>,
    pub status: Option<Status>,
    pub model: Option<String>,
    pub started_at: Option<Timestamp>,
    pub updated_at: Option<Timestamp>,
    pub completed_at: Option<Timestamp>,
    pub source_status: Status,
    pub source_url: Option<String>,
    /// 导入完整度分数 0.0~1.0，见 plan §17.3。
    pub completeness_score: Option<f64>,
    pub content_hash: Option<String>,
    pub raw_payload_id: Option<String>,
    /// 来源侧的父会话 ID（主子任务链路，如 MiniMax 的 parentSessionId）。
    /// 为 None 表示顶层主任务；为 Some(pid) 表示 pid 是父会话的 source_conversation_id。
    pub source_parent_id: Option<String>,
}

impl Conversation {
    pub fn new(provider: Provider, source_conversation_id: impl Into<String>) -> Self {
        Self {
            id: new_id("conv"),
            workspace_id: None,
            provider,
            installation_id: None,
            source_conversation_id: source_conversation_id.into(),
            title: None,
            user_title: None,
            status: None,
            model: None,
            started_at: None,
            updated_at: None,
            completed_at: None,
            source_status: Status::Active,
            source_url: None,
            completeness_score: None,
            content_hash: None,
            raw_payload_id: None,
            source_parent_id: None,
        }
    }

    /// 用户可见标题。
    pub fn effective_title(&self) -> &str {
        self.user_title
            .as_deref()
            .or(self.title.as_deref())
            .unwrap_or("(untitled)")
    }
}

/// 一次用户输入及其引发的完整执行，对应 plan §12.1 turns。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub id: String,
    pub conversation_id: String,
    pub source_turn_id: Option<String>,
    pub sequence_number: i64,
    pub status: Option<Status>,
    pub started_at: Option<Timestamp>,
    pub completed_at: Option<Timestamp>,
    pub duration_ms: Option<i64>,
}

impl Turn {
    pub fn new(conversation_id: impl Into<String>, sequence_number: i64) -> Self {
        Self {
            id: new_id("turn"),
            conversation_id: conversation_id.into(),
            source_turn_id: None,
            sequence_number,
            status: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
        }
    }
}

/// 一条消息，对应 plan §12.1 messages。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub turn_id: Option<String>,
    pub source_message_id: Option<String>,
    pub role: Role,
    /// 纯文本内容（可空，工具消息可能只有 json）。
    pub content_text: Option<String>,
    /// 结构化内容（工具调用、多模态等）。
    pub content_json: Option<serde_json::Value>,
    pub sequence_number: i64,
    pub created_at: Option<Timestamp>,
    pub content_hash: Option<String>,
    pub raw_payload_id: Option<String>,
}

impl Message {
    pub fn new(conversation_id: impl Into<String>, role: Role, sequence_number: i64) -> Self {
        Self {
            id: new_id("msg"),
            conversation_id: conversation_id.into(),
            turn_id: None,
            source_message_id: None,
            role,
            content_text: None,
            content_json: None,
            sequence_number,
            created_at: None,
            content_hash: None,
            raw_payload_id: None,
        }
    }
}

/// 执行事件（Tool Call / Command / Diff / Approval 等），对应 plan §12.1 events。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub conversation_id: String,
    pub turn_id: Option<String>,
    pub source_event_id: Option<String>,
    pub event_type: EventType,
    pub status: Option<Status>,
    pub summary: Option<String>,
    pub payload_json: Option<serde_json::Value>,
    pub sequence_number: i64,
    pub created_at: Option<Timestamp>,
    pub completed_at: Option<Timestamp>,
    pub raw_payload_id: Option<String>,
}

impl Event {
    pub fn new(
        conversation_id: impl Into<String>,
        event_type: EventType,
        sequence_number: i64,
    ) -> Self {
        Self {
            id: new_id("evt"),
            conversation_id: conversation_id.into(),
            turn_id: None,
            source_event_id: None,
            event_type,
            status: None,
            summary: None,
            payload_json: None,
            sequence_number,
            created_at: None,
            completed_at: None,
            raw_payload_id: None,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// CodeAgentOps：用量与工具调用指标（plan codeagent-ops §3）
// ────────────────────────────────────────────────────────────────────────────

/// 一次模型调用的用量记录（turn 级或 request 级）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageRecord {
    pub id: String,
    pub provider: Provider,
    /// 来源侧会话 ID（对应 Conversation.source_conversation_id）。
    pub source_session_id: String,
    pub turn_id: Option<String>,
    pub model: Option<String>,
    pub ts: Timestamp,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    /// 来源侧已算好的成本（MiniMax 自带）；None 表示需本地定价补算。
    pub cost_usd: Option<f64>,
    pub status: UsageStatus,
    pub duration_ms: Option<i64>,
    pub retry_count: Option<i64>,
    /// 来源侧工作目录（成本按项目归因，M7）。
    pub source_dir: Option<String>,
    /// context 超限次数（ZCode 原生，M9）。
    pub context_exceeded: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageStatus {
    Running,
    Completed,
    Error,
    Cancelled,
}

impl UsageStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            UsageStatus::Running => "running",
            UsageStatus::Completed => "completed",
            UsageStatus::Error => "error",
            UsageStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> UsageStatus {
        match s {
            "running" => UsageStatus::Running,
            "error" => UsageStatus::Error,
            "cancelled" => UsageStatus::Cancelled,
            _ => UsageStatus::Completed,
        }
    }
}

/// 一次工具调用（治理核心对象）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub id: String,
    pub provider: Provider,
    pub source_session_id: String,
    pub tool_name: String,
    pub ts: Timestamp,
    pub read_only: Option<bool>,
    /// 是否破坏性操作（ZCode 原生标记；其他来源靠规则推断）。
    pub destructive: Option<bool>,
    pub approval_status: Option<String>,
    pub exit_code: Option<i64>,
    pub duration_ms: Option<i64>,
    pub status: UsageStatus,
    /// Bash 类工具保留命令文本（审计用）。
    pub command_text: Option<String>,
}

/// 一个 Agent 资产（skill/plugin/mcp），M6 资产清单。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetRecord {
    pub id: String,
    pub provider: Provider,
    /// skill / plugin / mcp / builtin_skill
    pub kind: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    /// 描述/正文命中危险模式的次数（安全扫描）。
    pub risky_hits: i64,
    pub installed_at: Option<String>,
    pub path: Option<String>,
}

/// 一个自动化/定时任务，M8 治理。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationRecord {
    pub id: String,
    pub provider: Provider,
    pub name: String,
    pub kind: String,
    pub schedule: Option<String>,
    pub status: Option<String>,
    pub detail: Option<String>,
}

impl UsageRecord {
    /// 计费口径：input + output + reasoning（cache 不计费，单列）。
    pub fn billable_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.reasoning_tokens
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 工具函数
// ────────────────────────────────────────────────────────────────────────────

/// 当前 UTC 时间。集中一处便于测试替换。
pub fn now_utc() -> Timestamp {
    OffsetDateTime::now_utc()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_roundtrip() {
        for p in [
            Provider::Codex,
            Provider::Cursor,
            Provider::ClaudeCode,
            Provider::ZCode,
            Provider::MinimaxCode,
            Provider::OpenCode,
            Provider::Generic,
            Provider::Unknown,
        ] {
            let s = p.as_str();
            let back: Provider = s.parse().unwrap();
            assert_eq!(p, back, "provider {s} should roundtrip");
        }
    }

    #[test]
    fn provider_unknown_string_rejected() {
        assert!("not-a-provider".parse::<Provider>().is_err());
    }

    #[test]
    fn event_type_as_str_matches_plan() {
        // 对应 plan §12.2 的 19 种事件类型
        assert_eq!(EventType::ToolCallStarted.as_str(), "tool_call_started");
        assert_eq!(EventType::CommandCompleted.as_str(), "command_completed");
        assert_eq!(EventType::DiffGenerated.as_str(), "diff_generated");
        assert_eq!(EventType::ApprovalGranted.as_str(), "approval_granted");
        assert_eq!(EventType::ArtifactCreated.as_str(), "artifact_created");
    }

    #[test]
    fn workspace_effective_title_prefers_user_title() {
        let mut ws = Workspace::new("display-name");
        assert_eq!(ws.effective_title(), "display-name");
        ws.user_title = Some("custom".into());
        assert_eq!(ws.effective_title(), "custom");
    }

    #[test]
    fn conversation_effective_title_fallbacks() {
        let mut c = Conversation::new(Provider::Codex, "src-1");
        assert_eq!(c.effective_title(), "(untitled)");
        c.title = Some("from source".into());
        assert_eq!(c.effective_title(), "from source");
        c.user_title = Some("user override".into());
        assert_eq!(c.effective_title(), "user override");
    }

    #[test]
    fn ids_have_correct_prefix() {
        assert!(new_id("ws").starts_with("ws_"));
        assert!(new_id("conv").starts_with("conv_"));
        assert!(new_id("msg").starts_with("msg_"));
    }

    #[test]
    fn match_method_priority_strings() {
        // plan §4.3 优先级：manual > manifest_id > git_remote > git_common_dir > canonical_path > filesystem_id > name_similarity
        assert_eq!(MatchMethod::Manual.as_str(), "manual");
        assert_eq!(MatchMethod::NameSimilarity.as_str(), "name_similarity");
    }

    #[test]
    fn serde_provider_lowercase() {
        let json = serde_json::to_string(&Provider::ClaudeCode).unwrap();
        assert_eq!(json, "\"claude-code\"");
        let back: Provider = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Provider::ClaudeCode);
    }
}
