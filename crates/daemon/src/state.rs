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
    /// 清空所有数据。保留用户自定义脱敏规则与治理配置（策略/预算/定价/设置）。
    ///
    /// 实现：物理删除重建而非逐表 DELETE——实测 190MB 库（977 会话 / 4.7 万消息
    /// 加 FTS5 触发器级联）DELETE 路径需 12 分钟；删文件重跑 migration 仅毫秒级
    /// （2026-08-15 重置卡死事故）。
    pub fn wipe_all(&self) -> Result<(), DaemonStateError> {
        // 1. 快照需要保留的用户数据（脱敏规则；其余治理配置存于独立表，重建不丢）
        let redaction: Vec<ch_storage::RedactionRuleRecord> = {
            let repo = self.repo.lock().expect("mutex poisoned");
            repo.list_redaction_rules()
                .map_err(DaemonStateError::Storage)?
        };

        // 2. DB 物理重建：先释放连接（drop guard），删 db/wal/shm，重开建 schema
        {
            drop(self.repo.lock().expect("mutex poisoned"));
            let db = self.data_dir.join("threadock.db");
            for f in [
                db.clone(),
                self.data_dir.join("threadock.db-wal"),
                self.data_dir.join("threadock.db-shm"),
            ] {
                if f.exists() {
                    std::fs::remove_file(&f)?;
                }
            }
            let mut guard = self.repo.lock().expect("mutex poisoned");
            *guard = Repository::open(&db)?;
        }

        // 3. 回写脱敏规则
        self.repo
            .lock()
            .expect("mutex poisoned")
            .restore_redaction_rules(&redaction)
            .map_err(DaemonStateError::Storage)?;

        // 4. 搜索索引物理重建（派生数据，可重建；目录 300MB+ 时远快于 delete_all）
        {
            drop(self.search_index.lock().expect("mutex poisoned"));
            let idx_dir = self.data_dir.join("index");
            if idx_dir.exists() {
                std::fs::remove_dir_all(&idx_dir)?;
            }
            let mut guard = self.search_index.lock().expect("mutex poisoned");
            *guard = SearchIndex::open(&idx_dir)?;
        }

        // 5. raw blob 清空（remove_dir_all + 重建目录）
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
