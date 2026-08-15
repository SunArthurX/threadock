//! Conversation Hub 存储层。
//!
//! 基于 SQLite（WAL 模式），对应 plan §9.4 与 §12.1。
//! 所有写操作由 `Repository` 单点负责（plan §9.4 原则），不暴露裸连接。

pub mod error;
pub mod filter;
pub mod migration;
pub mod repository;
pub mod schema;
pub mod search;
pub mod timestamp;

pub use error::{StorageError, StorageResult};
pub use filter::ConversationFilter;
pub use repository::{
    AgentBenchmark, AgentHealth, AnomalyRow, AssetRow, AuditFindingState, AuditMessageRow,
    AutomationRow, BudgetSettings, CacheStat, CacheTrendRow, DailyUsage, DirCost, GovernanceLogRow,
    KnowledgeRecord, LatencyStat, ModelUsage, MonthProjection, OpsOverview, PolicyRuleRecord,
    ProviderUsage, RedactionRuleRecord, Repository, TokenWaste, ToolUsageRow, WeeklySummary,
};
pub use search::{build_match_expr, SearchQuery, SearchResult};
