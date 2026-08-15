//! Tantivy 全文检索，对应 plan §9.5「Tantivy 中英文全文检索、BM25 和高亮」与 §13。
//!
//! ## 设计
//!
//! - 独立于 `SQLite` FTS5，作为 plan §9.5 的「主检索」实现。
//! - `索引字段：message_id` / `conversation_id` / provider / `workspace_id` / role / title / body。
//! - 中文分词：N-gram（plan §13「字符 N-gram 兜底」），2-gram 兼顾中文召回率与索引体积。
//! - BM25 排序 + 命中片段高亮。
//! - 索引完全可重建（plan §3 原则：Rebuildable index）——损坏后可从主数据全量重建。
//!
//! ## 与 FTS5 的关系
//!
//! FTS5（在 storage crate）是 MVP 降级方案。本 crate 是增强方案，二者并存：
//! 未来可按性能/质量选择，或用 Tantivy 完全替代。`SearchQuery`/`SearchResult` 契约
//! 与 `storage::search` 对齐，便于上层切换。

pub mod error;
pub mod index;

pub use error::{SearchError, SearchResult};
pub use index::{SearchHit, SearchIndex, SearchQuery};
