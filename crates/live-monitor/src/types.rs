//! 实时任务监控数据模型（serde camelCase，直接供 Tauri 命令返回前端）。

use serde::Serialize;

/// 数据源（与 `ch_domain::Provider::as_str` 保持同一套字符串标识）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LiveSource {
    ClaudeCode,
    Codex,
    ZCode,
    MinimaxCode,
}

impl LiveSource {
    /// 数据库存的 provider 字符串（深链匹配已导入会话用）。
    #[must_use]
    pub fn provider_str(self) -> &'static str {
        match self {
            LiveSource::ClaudeCode => "claude-code",
            LiveSource::Codex => "codex",
            LiveSource::ZCode => "zcode",
            LiveSource::MinimaxCode => "minimax-code",
        }
    }

    /// 展示名。
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            LiveSource::ClaudeCode => "Claude Code",
            LiveSource::Codex => "Codex",
            LiveSource::ZCode => "ZCode",
            LiveSource::MinimaxCode => "MiniMax Code",
        }
    }
}

/// 运行状态（v1 启发式：写入新鲜度 + 源侧显式状态字段，见设计文档）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LiveState {
    /// 活跃写入中（近 45s 有写入，或源侧显式 running）。
    Running,
    /// 等待用户动作（审批 / 提问）。
    Waiting,
    /// 45s ~ 10min 无写入（可能在等用户输入或长工具）。
    Idle,
    /// 10min ~ 30min 前有过活动。
    Recent,
}

/// 最近一条活动摘要。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveActivity {
    /// message / tool / thinking / system / status
    pub kind: String,
    /// user / assistant（kind = message 时）。
    pub role: Option<String>,
    /// 单行摘要（已截断，中文安全）。
    pub snippet: String,
    pub at_ms: i64,
}

/// 进度信号（能取到多少给多少，各源能力不同）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveProgress {
    pub todos_done: Option<u32>,
    pub todos_total: Option<u32>,
    pub message_count: Option<i64>,
    /// 最近一次上报的 token 总量（近似上下文规模）。
    pub total_tokens: Option<i64>,
}

/// 一个正在（或刚刚）运行的 agent 会话。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTask {
    pub source: LiveSource,
    /// 源侧会话 ID（jsonl 文件名 / SQLite 主键）。
    pub session_id: String,
    pub title: Option<String>,
    pub workspace: Option<String>,
    /// Codex 专用：`codex_work_desktop`（桌面）/ `codex_cli_rs`（CLI）等。
    pub originator: Option<String>,
    pub started_at_ms: Option<i64>,
    pub last_activity_ms: i64,
    pub state: LiveState,
    pub activity: Option<LiveActivity>,
    pub progress: Option<LiveProgress>,
}

/// 单源扫描失败（互不影响其他源）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceError {
    pub source: LiveSource,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_str_matches_storage() {
        assert_eq!(LiveSource::ClaudeCode.provider_str(), "claude-code");
        assert_eq!(LiveSource::Codex.provider_str(), "codex");
        assert_eq!(LiveSource::ZCode.provider_str(), "zcode");
        assert_eq!(LiveSource::MinimaxCode.provider_str(), "minimax-code");
    }

    #[test]
    fn serializes_camel_case() {
        let json = serde_json::to_string(&LiveSource::MinimaxCode).expect("serialize");
        assert_eq!(json, r#""minimaxCode""#);
        let task = LiveTask {
            source: LiveSource::ZCode,
            session_id: "s1".into(),
            title: None,
            workspace: None,
            originator: None,
            started_at_ms: None,
            last_activity_ms: 1,
            state: LiveState::Running,
            activity: None,
            progress: None,
        };
        let json = serde_json::to_string(&task).expect("serialize");
        assert!(json.contains("sessionId"), "{json}");
        assert!(json.contains("lastActivityMs"), "{json}");
    }
}
