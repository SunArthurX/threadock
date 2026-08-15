//! Daemon 状态：双连接架构（WAL 模式下读写分离，互不阻塞）。
//!
//! `write_repo：写连接（auto_sync` / 导入 / 重置）
//! `read_repo`： 读连接（UI 查询 / 概览 / 会话详情）
//!
//! `SQLite` WAL 模式支持 N 个读者 + 1 个写者并发，
//! 两个连接各自持有独立 Mutex，读写路径完全解耦 →
//! 增量导入时 UI 查询零等待。

use ch_raw_store::RawStore;
use ch_search::SearchIndex;
use ch_storage::Repository;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct DaemonStateConfig {
    pub data_dir: PathBuf,
}

/// Daemon 全局状态。双连接 + 搜索索引 + Raw Store。
pub struct DaemonState {
    /// 写连接：同步/导入/重置（唯一写者）
    pub repo: Mutex<Repository>,
    /// 读连接：UI 查询（与写连接互不阻塞，WAL 并发读）
    pub read_repo: Mutex<Repository>,
    pub search_index: Mutex<SearchIndex>,
    pub raw_store: Mutex<RawStore>,
    pub data_dir: PathBuf,
}

impl DaemonState {
    /// 在 `data_dir` 下打开/创建双连接 + `SearchIndex` + `RawStore`。
    pub fn open(config: DaemonStateConfig) -> Result<Self, DaemonStateError> {
        std::fs::create_dir_all(&config.data_dir)?;
        let db_path = config.data_dir.join("threadock.db");
        let repo = Repository::open(&db_path)?;
        // 第二个连接：同一 DB 文件，独立 Mutex（WAL 读写并发）
        let read_repo = Repository::open(&db_path)?;
        let search_index = SearchIndex::open(config.data_dir.join("index"))?;
        let raw_store = RawStore::new(&config.data_dir)?;
        Ok(Self {
            repo: Mutex::new(repo),
            read_repo: Mutex::new(read_repo),
            search_index: Mutex::new(search_index),
            raw_store: Mutex::new(raw_store),
            data_dir: config.data_dir,
        })
    }

    /// 内存模式（测试用）。
    pub fn open_in_memory() -> Result<Self, DaemonStateError> {
        let dir = tempfile::TempDir::new().map_err(DaemonStateError::Io)?;
        let db_path = dir.path().join("threadock.db");
        let repo = Repository::open(&db_path)?;
        let read_repo = Repository::open(&db_path)?;
        let search_index = SearchIndex::open_in_memory()?;
        let raw_store = RawStore::new(dir.path())?;
        Ok(Self {
            repo: Mutex::new(repo),
            read_repo: Mutex::new(read_repo),
            search_index: Mutex::new(search_index),
            raw_store: Mutex::new(raw_store),
            data_dir: dir.path().to_path_buf(),
        })
    }

    /// 清空所有数据。保留 schema 和用户自定义脱敏规则。
    pub fn wipe_all(&self) -> Result<(), DaemonStateError> {
        self.repo.lock().expect("mutex poisoned").clear_all()?;
        self.search_index
            .lock()
            .expect("mutex poisoned")
            .clear_all()?;
        self.raw_store.lock().expect("mutex poisoned").clear()?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DaemonStateError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("storage: {0}")]
    Storage(#[from] ch_storage::StorageError),
    #[error("search: {0}")]
    Search(#[from] ch_search::SearchError),
    #[error("raw store: {0}")]
    Raw(#[from] ch_raw_store::RawStoreError),
}
