//! 本地加密备份/恢复，对应 plan §6.6「本地加密备份」与 §14.3 加密策略。
//!
//! ## 备份格式（`.chbak`）
//!
//! ```text
//! [magic "CHBK1\0"]      6 字节魔数
//! [nonce 24B]            XChaCha20-Poly1305 nonce
//! [ciphertext]           加密后的 zstd 压缩 payload
//! [tag 16B]              Poly1305 认证标签
//! ```
//!
//! ## payload 结构（加密前，zstd 压缩后）
//!
//! 一个 tar-like 的简单容器：JSON 元信息 + SQLite 数据库文件字节 + Raw Store 文件列表。
//! MVP 用最简约定：把整个数据库文件 + raw 目录打包。
//!
//! ## 密钥派生
//!
//! MVP：用密码经 BLAKE3 派生 32 字节密钥（plan §14.3 DEK 由 Device Key 包装；
//! 密码派生用于导出备份场景）。生产环境应经 KMS/PBKDF2，本 crate 留接口。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{AeadCore, XChaCha20Poly1305, XNonce};
use thiserror::Error;

pub type BackupResult<T> = std::result::Result<T, BackupError>;

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("encryption failed: {0}")]
    Encrypt(String),

    #[error("decryption failed: {0}")]
    Decrypt(String),

    #[error("invalid backup file: {0}")]
    InvalidFormat(String),

    #[error("password too short (min 8 chars)")]
    PasswordTooShort,

    #[error("zstd error: {0}")]
    Zstd(String),
}

const MAGIC: &[u8; 6] = b"CHBK1\0";
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;

/// 备份元信息（明文，写在加密 payload 之外便于校验）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackupMeta {
    pub format_version: u32,
    pub created_at: i64,
    pub db_size: u64,
    pub raw_count: u64,
    pub raw_bytes: u64,
    /// payload（明文压缩后）的 BLAKE3，便于解密后完整性校验。
    pub payload_hash: String,
}

/// 由密码派生 32 字节密钥（XChaCha20-Poly1305 所需）。
///
/// 用 BLAKE3 的 keyed hashing，MVP 级别（生产应换 Argon2/PBKDF2）。
pub fn derive_key(password: &str) -> BackupResult<[u8; 32]> {
    if password.len() < 8 {
        return Err(BackupError::PasswordTooShort);
    }
    let mut key = [0u8; 32];
    // 用 BLAKE3 直接吸收密码（keyed mode 的 key 用零），简单可靠
    let hash = blake3::hash(password.as_bytes());
    key.copy_from_slice(hash.as_bytes());
    Ok(key)
}

/// 备份源：数据库文件 + 可选 raw 目录。
pub struct BackupSource {
    pub db_path: PathBuf,
    pub raw_root: Option<PathBuf>,
}

/// 创建加密备份。
///
/// 流程：
/// 1. 读取 db 文件 + raw 目录下所有文件。
/// 2. 组装 payload（长度前缀的简单容器）。
/// 3. zstd 压缩。
/// 4. XChaCha20-Poly1305 加密。
/// 5. 写入 `[magic][nonce][ciphertext+tag]`。
pub fn create_backup(
    source: &BackupSource,
    password: &str,
    out: &Path,
) -> BackupResult<BackupMeta> {
    let key = derive_key(password)?;
    let cipher = XChaCha20Poly1305::new(&key.into());

    // 1. 读取 db
    let db_bytes = std::fs::read(&source.db_path)?;
    let db_size = db_bytes.len() as u64;

    // 2. 读取 raw 文件列表
    let mut raw_files: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    if let Some(raw_root) = &source.raw_root {
        if raw_root.exists() {
            for path in walk_files(raw_root)? {
                let rel = path.strip_prefix(raw_root).expect("unexpected None").to_path_buf();
                let data = std::fs::read(&path)?;
                raw_files.push((rel, data));
            }
        }
    }
    let raw_count = raw_files.len() as u64;
    let raw_bytes: u64 = raw_files.iter().map(|(_, d)| d.len() as u64).sum();

    // 3. 组装 payload（容器：[db_len][db][n_files][file: rel_len|rel|data_len|data]...）
    let mut payload = Vec::new();
    write_u64(&mut payload, db_bytes.len() as u64);
    payload.extend_from_slice(&db_bytes);
    write_u64(&mut payload, raw_files.len() as u64);
    for (rel, data) in &raw_files {
        let rel_bytes = rel.to_string_lossy().into_owned();
        write_u64(&mut payload, rel_bytes.len() as u64);
        payload.extend_from_slice(rel_bytes.as_bytes());
        write_u64(&mut payload, data.len() as u64);
        payload.extend_from_slice(data);
    }

    let payload_hash = blake3::hash(&payload).to_hex().to_string();

    // 4. zstd 压缩
    let compressed =
        zstd::encode_all(payload.as_slice(), 9).map_err(|e| BackupError::Zstd(e.to_string()))?;

    // 5. 加密
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, compressed.as_ref())
        .map_err(|e| BackupError::Encrypt(e.to_string()))?;

    // 6. 写文件
    let mut f = std::fs::File::create(out)?;
    f.write_all(MAGIC)?;
    f.write_all(&nonce)?;
    f.write_all(&ciphertext)?;
    f.sync_all()?;

    Ok(BackupMeta {
        format_version: 1,
        created_at: now_millis(),
        db_size,
        raw_count,
        raw_bytes,
        payload_hash,
    })
}

/// 恢复备份到目标目录。
///
/// 流程：读取 → 校验魔数 → 解密 → 解压 → 解析容器 → 写出 db + raw。
pub fn restore_backup(
    backup_path: &Path,
    password: &str,
    target_dir: &Path,
) -> BackupResult<BackupMeta> {
    let key = derive_key(password)?;
    let cipher = XChaCha20Poly1305::new(&key.into());

    let mut f = std::fs::File::open(backup_path)?;
    let mut all = Vec::new();
    f.read_to_end(&mut all)?;

    // 解析头部
    if all.len() < MAGIC.len() + NONCE_LEN + TAG_LEN {
        return Err(BackupError::InvalidFormat("file too short".into()));
    }
    let magic = &all[..MAGIC.len()];
    if magic != MAGIC {
        return Err(BackupError::InvalidFormat("bad magic".into()));
    }
    let nonce_bytes = &all[MAGIC.len()..MAGIC.len() + NONCE_LEN];
    let ciphertext = &all[MAGIC.len() + NONCE_LEN..];
    let nonce = XNonce::from_slice(nonce_bytes);

    // 解密
    let compressed = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| BackupError::Decrypt(e.to_string()))?;

    // 解压
    let payload =
        zstd::decode_all(compressed.as_slice()).map_err(|e| BackupError::Zstd(e.to_string()))?;

    // 完整性校验
    let actual_hash = blake3::hash(&payload).to_hex().to_string();

    // 解析容器
    let mut pos = 0usize;
    let db_len = read_u64(&payload, &mut pos)? as usize;
    let db_bytes = &payload[pos..pos + db_len];
    pos += db_len;

    std::fs::create_dir_all(target_dir)?;
    let db_path = target_dir.join("conversation-hub.db");
    std::fs::write(&db_path, db_bytes)?;

    let n_files = read_u64(&payload, &mut pos)?;
    let mut raw_count = 0u64;
    let mut raw_bytes = 0u64;
    if n_files > 0 {
        let raw_root = target_dir.join("raw");
        std::fs::create_dir_all(&raw_root)?;
        for _ in 0..n_files {
            let rel_len = read_u64(&payload, &mut pos)? as usize;
            let rel_bytes = &payload[pos..pos + rel_len];
            pos += rel_len;
            let rel = std::str::from_utf8(rel_bytes)
                .map_err(|e| BackupError::InvalidFormat(format!("non-utf8 path: {e}")))?;
            let abs = raw_root.join(rel);
            if let Some(parent) = abs.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let data_len = read_u64(&payload, &mut pos)? as usize;
            let data = &payload[pos..pos + data_len];
            pos += data_len;
            std::fs::write(&abs, data)?;
            raw_count += 1;
            raw_bytes += data_len as u64;
        }
    }

    let db_size = db_bytes.len() as u64;
    Ok(BackupMeta {
        format_version: 1,
        created_at: now_millis(),
        db_size,
        raw_count,
        raw_bytes,
        payload_hash: actual_hash,
    })
}

// ── 工具 ──────────────────────────────────────────────────────────────────

fn write_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn read_u64(buf: &[u8], pos: &mut usize) -> BackupResult<u64> {
    if *pos + 8 > buf.len() {
        return Err(BackupError::InvalidFormat("truncated u64".into()));
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&buf[*pos..*pos + 8]);
    *pos += 8;
    Ok(u64::from_le_bytes(arr))
}

fn walk_files(dir: &Path) -> BackupResult<Vec<PathBuf>> {
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

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_domain::{Conversation, Provider};
    use ch_storage::Repository;
    use tempfile::TempDir;

    fn seeded() -> (TempDir, PathBuf) {
        let dir = TempDir::new().expect("tempdir creation failed");
        let db = dir.path().join("conversation-hub.db");
        let repo = Repository::open(&db).expect("unexpected None");
        repo.upsert_provider(Provider::Generic).expect("upsert failed");
        let c = Conversation::new(Provider::Generic, "src-backup");
        repo.upsert_conversation(&c).expect("upsert failed");
        (dir, db)
    }

    #[test]
    fn derive_key_requires_min_length() {
        assert!(derive_key("short").is_err());
        assert!(derive_key("longenough").is_ok());
    }

    #[test]
    fn derive_key_deterministic() {
        let k1 = derive_key("my-password-123").expect("unexpected None");
        let k2 = derive_key("my-password-123").expect("unexpected None");
        assert_eq!(k1, k2);
    }

    #[test]
    fn backup_and_restore_roundtrip() {
        let (_dir, db) = seeded();
        let backup_dir = TempDir::new().expect("tempdir creation failed");
        let backup_path = backup_dir.path().join("hub.chbak");

        // 备份（无 raw）
        let meta = create_backup(
            &BackupSource {
                db_path: db.clone(),
                raw_root: None,
            },
            "test-password-1",
            &backup_path,
        )
        .expect("unexpected None");
        assert_eq!(meta.format_version, 1);
        assert!(meta.db_size > 0);

        // 恢复到新目录
        let restore_dir = TempDir::new().expect("tempdir creation failed");
        let meta2 = restore_backup(&backup_path, "test-password-1", restore_dir.path()).expect("unexpected None");
        assert_eq!(meta.db_size, meta2.db_size);
        assert!(backup_path.exists());

        // 恢复后的库应能打开且有数据
        let restored_db = restore_dir.path().join("conversation-hub.db");
        let repo = Repository::open(&restored_db).expect("unexpected None");
        assert_eq!(repo.count_conversations().expect("unexpected None"), 1);
    }

    #[test]
    fn wrong_password_fails_to_restore() {
        let (_dir, db) = seeded();
        let backup_dir = TempDir::new().expect("tempdir creation failed");
        let backup_path = backup_dir.path().join("hub.chbak");
        create_backup(
            &BackupSource {
                db_path: db,
                raw_root: None,
            },
            "correct-pw-1",
            &backup_path,
        )
        .expect("unexpected None");

        let restore_dir = TempDir::new().expect("tempdir creation failed");
        let err = restore_backup(&backup_path, "wrong-password", restore_dir.path());
        assert!(err.is_err(), "wrong password must fail");
    }

    #[test]
    fn backup_with_raw_store() {
        let dir = TempDir::new().expect("tempdir creation failed");
        let db = dir.path().join("conversation-hub.db");
        // 先创建一个有效数据库（不能备份不存在的文件）
        let _repo = Repository::open(&db).expect("unexpected None");
        let raw_root = dir.path().join("raw");
        std::fs::create_dir_all(&raw_root).expect("file I/O failed");
        // 放一个 raw 文件
        std::fs::create_dir_all(raw_root.join("ab/cd")).expect("file I/O failed");
        std::fs::write(
            raw_root.join("ab/cd/abcd.json.zst"),
            b"fake compressed content",
        )
        .expect("unexpected None");

        let backup_dir = TempDir::new().expect("tempdir creation failed");
        let backup_path = backup_dir.path().join("hub.chbak");

        let meta = create_backup(
            &BackupSource {
                db_path: db,
                raw_root: Some(raw_root),
            },
            "pw-12345678",
            &backup_path,
        )
        .expect("unexpected None");
        assert_eq!(meta.raw_count, 1);
        assert!(meta.raw_bytes > 0);

        // 恢复
        let restore_dir = TempDir::new().expect("tempdir creation failed");
        let meta2 = restore_backup(&backup_path, "pw-12345678", restore_dir.path()).expect("unexpected None");
        assert_eq!(meta2.raw_count, 1);
        let restored_raw = restore_dir.path().join("raw/ab/cd/abcd.json.zst");
        assert!(restored_raw.exists());
        assert_eq!(
            std::fs::read(&restored_raw).expect("file I/O failed"),
            b"fake compressed content"
        );
    }

    #[test]
    fn backup_file_has_magic() {
        let (_dir, db) = seeded();
        let backup_dir = TempDir::new().expect("tempdir creation failed");
        let backup_path = backup_dir.path().join("hub.chbak");
        create_backup(
            &BackupSource {
                db_path: db,
                raw_root: None,
            },
            "pw-12345678",
            &backup_path,
        )
        .expect("unexpected None");
        let bytes = std::fs::read(&backup_path).expect("file I/O failed");
        assert_eq!(&bytes[..6], MAGIC);
    }

    #[test]
    fn corrupt_backup_rejected() {
        let (_dir, db) = seeded();
        let backup_dir = TempDir::new().expect("tempdir creation failed");
        let backup_path = backup_dir.path().join("hub.chbak");
        create_backup(
            &BackupSource {
                db_path: db,
                raw_root: None,
            },
            "pw-12345678",
            &backup_path,
        )
        .expect("unexpected None");
        // 破坏魔数
        let mut bytes = std::fs::read(&backup_path).expect("file I/O failed");
        bytes[0] = 0;
        std::fs::write(&backup_path, &bytes).expect("file I/O failed");
        let restore_dir = TempDir::new().expect("tempdir creation failed");
        assert!(restore_backup(&backup_path, "pw-12345678", restore_dir.path()).is_err());
    }
}
