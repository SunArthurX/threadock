//! 存储层错误类型。

use thiserror::Error;

pub type StorageResult<T> = std::result::Result<T, StorageError>;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("database connection closed")]
    ConnectionClosed,

    #[error("migration failed at version {version}: {reason}")]
    Migration { version: u32, reason: String },

    #[error("integrity check failed: {0}")]
    Integrity(String),

    #[error("not found: {entity} {id}")]
    NotFound { entity: &'static str, id: String },

    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },

    #[error("serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
}
