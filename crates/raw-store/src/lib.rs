//! 内容寻址原始数据存储，对应 plan §9.6「原始数据存储」与 §2.3「Raw + Normalized 双存储」。
//!
//! ## 为什么需要它
//!
//! 第三方来源（Codex/Cursor/...）的 Schema 会频繁变化。我们同时保存：
//! - **Raw**：来源原始字节（本文档实现），BLAKE3 内容寻址 + zstd 压缩。
//! - **Normalized**：标准化后的 `SQLite` 记录（在 storage crate）。
//!
//! 这样未来升级解析器时，可从 Raw 重新标准化，无需再访问第三方应用（plan §12.3）。
//!
//! ## 布局
//!
//! ```text
//! <root>/ab/cd/<blake3-64hex>.json.zst
//! ```
//!
//! - 用 hash 的前 2+2 字符分两级目录，避免单目录文件爆炸。
//! - 同内容天然去重（相同 hash 写入同一文件，幂等）。
//! - 压缩用 zstd level 9（plan §9.6 zstandard）。

use std::io::Write;
use std::path::{Path, PathBuf};

use thiserror::Error;

pub type RawResult<T> = std::result::Result<T, RawStoreError>;

#[derive(Debug, Error)]
pub enum RawStoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid content hash: {0}")]
    InvalidHash(String),

    #[error("hash mismatch: expected {expected}, computed {computed}")]
    HashMismatch { expected: String, computed: String },

    #[error("zstd encode error: {0}")]
    ZstdEncode(String),

    #[error("zstd decode error: {0}")]
    ZstdDecode(String),
}

/// 内容寻址存储。
///
/// 线程安全：所有方法获取 `&self`，内部路径计算无状态，可安全并发。
/// 写入靠文件系统原子性（先写临时文件再 rename）保证幂等。
pub struct RawStore {
    root: PathBuf,
    /// zstd 压缩级别（1-22，默认 9）。
    compression_level: i32,
}

/// 写入后的元信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPayload {
    /// BLAKE3 内容 hash（64 hex 字符）。
    pub hash: String,
    /// 该对象在 Raw Store 中的相对路径（如 `ab/cd/abcd...json.zst`）。
    pub rel_path: PathBuf,
    /// 原始（未压缩）字节数。
    pub original_size: u64,
    /// 压缩后字节数。
    pub stored_size: u64,
}

impl RawStore {
    /// 在 `root/raw` 下创建存储。`root` 通常为 app-data 根目录。
    pub fn new(root: impl AsRef<Path>) -> RawResult<Self> {
        let root = root.as_ref().join("raw");
        Ok(Self {
            root,
            compression_level: 9,
        })
    }

    /// 显式指定根目录（测试用）。
    pub fn with_root(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            compression_level: 9,
        }
    }

    /// 写入原始字节，返回 hash 与路径。相同内容幂等（覆盖写同一路径）。
    ///
    /// 内容寻址天然幂等：已存在的对象直接短路返回（读取现有文件大小），
    /// 跳过 zstd-9 重压缩与重写 + fsync——重复导入（增量同步的 stale 路径）
    /// 不再为已归档内容付整次压缩代价。
    pub fn put(&self, data: &[u8]) -> RawResult<RawPayload> {
        let hash = blake3::hash(data).to_hex().to_string();
        let rel_path = hash_to_rel_path(&hash);
        let abs_path = self.root.join(&rel_path);

        // 已存在短路（BLAKE3 抗碰撞，同 hash = 同内容）
        if abs_path.exists() {
            let stored_size = std::fs::metadata(&abs_path).map_or(0, |m| m.len());
            return Ok(RawPayload {
                hash,
                rel_path,
                original_size: data.len() as u64,
                stored_size,
            });
        }

        // 压缩
        let compressed = zstd::encode_all(data, self.compression_level)
            .map_err(|e| RawStoreError::ZstdEncode(e.to_string()))?;

        // 确保父目录存在
        if let Some(parent) = abs_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // 原子写入：先写临时文件再 rename，避免并发竞争产生半截文件
        let tmp = abs_path.with_extension("zstd.tmp");
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&compressed)?;
        f.sync_all()?;
        drop(f);

        // rename 在同文件系统上是原子的；若目标已存在则覆盖（幂等）
        std::fs::rename(&tmp, &abs_path)?;

        tracing::debug!(
            hash = %hash,
            original = data.len(),
            stored = compressed.len(),
            "raw payload stored"
        );

        Ok(RawPayload {
            hash,
            rel_path,
            original_size: data.len() as u64,
            stored_size: compressed.len() as u64,
        })
    }

    /// 写入一个 JSON 序列化对象（最常见用例）。
    pub fn put_json(&self, value: &impl serde::Serialize) -> RawResult<RawPayload> {
        let bytes = serde_json::to_vec(value)?;
        self.put(&bytes)
    }

    /// 按 hash 读取原始（解压后的）字节。
    pub fn get(&self, hash: &str) -> RawResult<Vec<u8>> {
        validate_hash(hash)?;
        let abs_path = self.root.join(hash_to_rel_path(hash));
        let compressed = std::fs::read(&abs_path)?;
        zstd::decode_all(&compressed[..]).map_err(|e| RawStoreError::ZstdDecode(e.to_string()))
    }

    /// 按 hash 读取并反序列化为 JSON。
    pub fn get_json<T: serde::de::DeserializeOwned>(&self, hash: &str) -> RawResult<T> {
        let bytes = self.get(hash)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// 该 hash 对应的对象是否已存在。
    pub fn exists(&self, hash: &str) -> RawResult<bool> {
        validate_hash(hash)?;
        let abs_path = self.root.join(hash_to_rel_path(hash));
        Ok(abs_path.exists())
    }

    /// 该 hash 对应对象的绝对路径。
    pub fn path_of(&self, hash: &str) -> RawResult<PathBuf> {
        validate_hash(hash)?;
        Ok(self.root.join(hash_to_rel_path(hash)))
    }

    /// 统计当前 Raw Store 中的对象数与总占用字节（递归扫描）。
    pub fn stats(&self) -> RawResult<RawStats> {
        let mut count = 0u64;
        let mut bytes = 0u64;
        if !self.root.exists() {
            return Ok(RawStats { count, bytes });
        }
        for entry in walk_files(&self.root)? {
            let meta = std::fs::metadata(&entry)?;
            count += 1;
            bytes += meta.len();
        }
        Ok(RawStats { count, bytes })
    }

    /// 清空所有原始 blob（用于「重置数据」）。
    pub fn clear(&self) -> RawResult<()> {
        if self.root.exists() {
            std::fs::remove_dir_all(&self.root)?;
        }
        std::fs::create_dir_all(&self.root)?;
        Ok(())
    }
}

/// 存储统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawStats {
    pub count: u64,
    pub bytes: u64,
}

// ── 内部工具 ──────────────────────────────────────────────────────────────

/// 把 64 字符 hex hash 映射到 `ab/cd/<hash>.json.zst` 两级目录路径。
///
/// 用 hash 前 2、3-4 字符做目录名，保证每个子目录文件数适中。
fn hash_to_rel_path(hash: &str) -> PathBuf {
    let (a, rest1) = hash.split_at(2);
    let (b, _rest2) = rest1.split_at(2);
    PathBuf::from(a).join(b).join(format!("{hash}.json.zst"))
}

/// 校验 hash 是否为合法的 64 位 hex。
fn validate_hash(hash: &str) -> RawResult<()> {
    if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(RawStoreError::InvalidHash(hash.to_string()))
    }
}

/// 递归收集目录下所有普通文件路径。
fn walk_files(dir: &Path) -> RawResult<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk_files(&path)?);
        } else {
            out.push(path);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::TempDir;

    fn store() -> (TempDir, RawStore) {
        let dir = TempDir::new().expect("tempdir creation failed");
        let store = RawStore::new(dir.path()).expect("unexpected None");
        (dir, store)
    }

    #[test]
    fn put_existing_content_short_circuits() {
        // 重复导入相同内容：不重写文件（mtime 不变），元信息仍完整
        let (_dir, store) = store();
        let data = b"some conversation payload that is not tiny";
        let p1 = store.put(data).expect("unexpected None");
        let abs1 = store.root.join(&p1.rel_path);
        let mtime1 = std::fs::metadata(&abs1)
            .expect("file I/O failed")
            .modified()
            .expect("file I/O failed");
        std::thread::sleep(std::time::Duration::from_millis(20));
        let p2 = store.put(data).expect("unexpected None");
        let mtime2 = std::fs::metadata(&abs1)
            .expect("file I/O failed")
            .modified()
            .expect("file I/O failed");
        assert_eq!(p1.hash, p2.hash);
        assert_eq!(p1.stored_size, p2.stored_size);
        assert_eq!(mtime1, mtime2, "existing object must not be rewritten");
    }

    #[test]
    fn put_returns_correct_hash() {
        let (_dir, store) = store();
        let data = b"hello world";
        let payload = store.put(data).expect("unexpected None");
        // 与 crate 内部算法独立计算对比，保证一致性
        let expected = blake3::hash(data).to_hex().to_string();
        assert_eq!(payload.hash, expected);
        assert_eq!(payload.hash.len(), 64);
        // hex 字符，且全小写
        assert!(payload
            .hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn put_then_get_roundtrips() {
        let (_dir, store) = store();
        let data = b"some raw conversation content \xe4\xb8\xad\xe6\x96\x87";
        let payload = store.put(data).expect("unexpected None");
        let back = store.get(&payload.hash).expect("unexpected None");
        assert_eq!(back, data);
    }

    #[test]
    fn identical_content_deduped() {
        let (_dir, store) = store();
        let p1 = store.put(b"same content").expect("unexpected None");
        let p2 = store.put(b"same content").expect("unexpected None");
        assert_eq!(p1.hash, p2.hash);
        assert_eq!(p1.rel_path, p2.rel_path);
        // 物理上只有一个文件
        assert_eq!(store.stats().expect("unexpected None").count, 1);
    }

    #[test]
    fn different_content_different_hash_and_path() {
        let (_dir, store) = store();
        let p1 = store.put(b"aaa").expect("unexpected None");
        let p2 = store.put(b"bbb").expect("unexpected None");
        assert_ne!(p1.hash, p2.hash);
        assert_ne!(p1.rel_path, p2.rel_path);
        assert_eq!(store.stats().expect("unexpected None").count, 2);
    }

    #[test]
    #[allow(clippy::items_after_statements)] // 测试内联 DTO 放在使用处旁更易读
    fn json_roundtrip() {
        let (_dir, store) = store();
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Conv {
            title: String,
            messages: Vec<String>,
        }
        let v = Conv {
            title: "test".into(),
            messages: vec!["hi".into(), "bye".into()],
        };
        let payload = store.put_json(&v).expect("unexpected None");
        let back: Conv = store.get_json(&payload.hash).expect("unexpected None");
        assert_eq!(back, v);
    }

    #[test]
    fn exists_works() {
        let (_dir, store) = store();
        let p = store.put(b"data").expect("unexpected None");
        assert!(store.exists(&p.hash).expect("unexpected None"));
        assert!(!store
            .exists("0000000000000000000000000000000000000000000000000000000000000000")
            .expect("unexpected None"));
    }

    #[test]
    fn invalid_hash_rejected() {
        let (_dir, store) = store();
        assert!(store.get("short").is_err());
        assert!(store
            .get("z00000000000000000000000000000000000000000000000000000000000000")
            .is_err()); // 非 hex
        assert!(store.exists("bad").is_err());
    }

    #[test]
    fn hash_to_rel_path_two_level_dirs() {
        let p = hash_to_rel_path("abcdef0123456789");
        // ab/cd/abcdef0123456789.json.zst
        let comps: Vec<_> = p.components().collect();
        assert_eq!(comps.len(), 3);
        assert_eq!(comps[0].as_os_str().to_string_lossy(), "ab");
        assert_eq!(comps[1].as_os_str().to_string_lossy(), "cd");
        assert!(comps[2]
            .as_os_str()
            .to_string_lossy()
            .ends_with(".json.zst"));
    }

    #[test]
    fn compression_reduces_redundant_data() {
        let (_dir, store) = store();
        // 高重复数据应被显著压缩
        let data: Vec<u8> = vec![b'A'; 10_000];
        let payload = store.put(&data).expect("unexpected None");
        assert_eq!(payload.original_size, 10_000);
        assert!(
            payload.stored_size < 200,
            "zstd should compress 10KB of 'A' to < 200 bytes, got {}",
            payload.stored_size
        );
    }

    #[test]
    fn stats_on_empty_store() {
        let (_dir, store) = store();
        let s = store.stats().expect("unexpected None");
        assert_eq!(s.count, 0);
        assert_eq!(s.bytes, 0);
    }

    #[test]
    fn path_of_returns_existing_layout() {
        let (_dir, store) = store();
        let p = store.put(b"x").expect("unexpected None");
        let path = store.path_of(&p.hash).expect("unexpected None");
        assert!(path.exists());
        assert!(path.to_string_lossy().ends_with(".json.zst"));
    }

    #[test]
    fn concurrent_same_content_safe() {
        // 模拟并发写同内容：多次调用不应报错，最终一个文件
        let (_dir, store) = store();
        let data = b"concurrent-test";
        for _ in 0..5 {
            store.put(data).expect("unexpected None");
        }
        assert_eq!(store.stats().expect("unexpected None").count, 1);
    }
}
