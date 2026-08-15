//! 统一配置：管理各 Agent 数据源路径、同步频率等可调参数。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub sources: SourcesConfig,
    pub sync: SyncConfig,
    pub search: SearchConfig,
    pub audit: AuditConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcesConfig {
    pub zcode_db: PathBuf,
    pub claude_home: PathBuf,
    pub cursor_db: PathBuf,
    pub minimax_db: PathBuf,
    pub codex_home: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    pub conversation_interval_secs: u64,
    pub ops_throttle_secs: u64,
    pub max_sessions_per_sync: usize,
    pub batch_chunk_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    pub max_line_bytes: usize,
    pub tantivy_heap_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    pub scan_sensitive: bool,
    pub scan_dangerous_commands: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        Self {
            sources: SourcesConfig {
                zcode_db: PathBuf::from(&home).join(".zcode/cli/db/db.sqlite"),
                claude_home: PathBuf::from(&home).join(".claude"),
                cursor_db: PathBuf::from(&home)
                    .join("Library/Application Support/Cursor/User/globalStorage/state.vscdb"),
                minimax_db: PathBuf::from(&home).join(".minimax/v2/sqlite/runtime-state.sqlite"),
                codex_home: PathBuf::from(&home).join(".codex"),
            },
            sync: SyncConfig {
                conversation_interval_secs: 600,
                ops_throttle_secs: 1800,
                max_sessions_per_sync: 500,
                batch_chunk_size: 2000,
            },
            search: SearchConfig {
                max_line_bytes: 2 * 1024 * 1024,
                tantivy_heap_bytes: 15_000_000,
            },
            audit: AuditConfig {
                scan_sensitive: true,
                scan_dangerous_commands: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let c = AgentConfig::default();
        assert!(c.sources.zcode_db.to_string_lossy().contains(".zcode"));
        assert_eq!(c.sync.conversation_interval_secs, 600);
        assert_eq!(c.search.max_line_bytes, 2 * 1024 * 1024);
    }
}
