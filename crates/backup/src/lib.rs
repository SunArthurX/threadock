//! 本地加密备份/恢复，对应 plan §6.6「本地加密备份」与 §14.3 加密策略。
//!
//! ## 备份格式（`.chbak`）
//!
//! V2（当前写出的格式）：
//! ```text
//! [magic "CHBK2\0"]      6 字节魔数
//! [salt 16B]             Argon2id 盐（随机，每次备份新生成）
//! [nonce 24B]            XChaCha20-Poly1305 nonce
//! [ciphertext]           加密后的 zstd 压缩 payload
//! [tag 16B]              Poly1305 认证标签
//! ```
//!
//! V1（旧格式，仅 restore 兼容读取）：`[magic "CHBK1\0"][nonce][ciphertext+tag]`，
//! 密钥为无盐单次 BLAKE3——仅用于恢复旧备份，不再写出。
//!
//! ## payload 结构（加密前，zstd 压缩后）
//!
//! 一个 tar-like 的简单容器：长度前缀的 SQLite 数据库字节 + Raw Store 文件列表。
//!
//! ## 密钥派生
//!
//! V2：Argon2id（19MiB / t=2 / p=1）+ 每备份随机 16 字节盐，抗 GPU 离线穷举。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
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

    #[error("key derivation failed: {0}")]
    Kdf(String),

    #[error("zstd error: {0}")]
    Zstd(String),
}

const MAGIC_V1: &[u8; 6] = b"CHBK1\0";
const MAGIC_V2: &[u8; 6] = b"CHBK2\0";
const SALT_LEN: usize = 16;
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

/// V1 密钥派生（无盐单次 BLAKE3）。仅供恢复旧格式备份，不再用于新备份。
fn derive_key_v1(password: &str) -> BackupResult<[u8; 32]> {
    if password.len() < 8 {
        return Err(BackupError::PasswordTooShort);
    }
    let mut key = [0u8; 32];
    let hash = blake3::hash(password.as_bytes());
    key.copy_from_slice(hash.as_bytes());
    Ok(key)
}

/// V2 密钥派生：Argon2id（19MiB / t=2 / p=1）+ 随机盐，抗 GPU 离线穷举。
pub fn derive_key_v2(password: &str, salt: &[u8; SALT_LEN]) -> BackupResult<[u8; 32]> {
    if password.len() < 8 {
        return Err(BackupError::PasswordTooShort);
    }
    let params =
        Params::new(19_456, 2, 1, Some(32)).map_err(|e| BackupError::Kdf(e.to_string()))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| BackupError::Kdf(e.to_string()))?;
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
    // 每次备份随机盐派生密钥（同密码的两次备份密钥不同）
    use chacha20poly1305::aead::rand_core::RngCore;
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let key = derive_key_v2(password, &salt)?;
    let cipher = XChaCha20Poly1305::new(&key.into());

    // 1. 读取 db
    let db_bytes = std::fs::read(&source.db_path)?;
    let db_size = db_bytes.len() as u64;

    // 2. 读取 raw 文件列表
    let mut raw_files: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    if let Some(raw_root) = &source.raw_root {
        if raw_root.exists() {
            for path in walk_files(raw_root)? {
                let rel = path
                    .strip_prefix(raw_root)
                    .expect("unexpected None")
                    .to_path_buf();
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

    // 6. 写文件（V2：[magic][salt][nonce][ciphertext+tag]）
    let mut f = std::fs::File::create(out)?;
    f.write_all(MAGIC_V2)?;
    f.write_all(&salt)?;
    f.write_all(&nonce)?;
    f.write_all(&ciphertext)?;
    f.sync_all()?;

    Ok(BackupMeta {
        format_version: 2,
        created_at: now_millis(),
        db_size,
        raw_count,
        raw_bytes,
        payload_hash,
    })
}

/// 恢复备份到目标目录。
///
/// 流程：读取 → 校验魔数 → 按格式版本派生密钥 → 解密 → 解压 → 解析容器 → 写出 db + raw。
/// 路径条目经 zip-slip 校验（拒绝 `..`/绝对路径），长度字段与实际字节数校验防 panic。
pub fn restore_backup(
    backup_path: &Path,
    password: &str,
    target_dir: &Path,
) -> BackupResult<BackupMeta> {
    let mut f = std::fs::File::open(backup_path)?;
    let mut all = Vec::new();
    f.read_to_end(&mut all)?;

    // 解析头部：V2 = [magic][salt][nonce][ciphertext]，V1 = [magic][nonce][ciphertext]
    let header_len = if all.len() >= MAGIC_V2.len() && &all[..MAGIC_V2.len()] == MAGIC_V2 {
        MAGIC_V2.len() + SALT_LEN + NONCE_LEN
    } else if all.len() >= MAGIC_V1.len() && &all[..MAGIC_V1.len()] == MAGIC_V1 {
        MAGIC_V1.len() + NONCE_LEN
    } else {
        return Err(BackupError::InvalidFormat("bad magic".into()));
    };
    if all.len() < header_len + TAG_LEN {
        return Err(BackupError::InvalidFormat("file too short".into()));
    }

    let key = if &all[..MAGIC_V2.len()] == MAGIC_V2 {
        let mut salt = [0u8; SALT_LEN];
        salt.copy_from_slice(&all[MAGIC_V2.len()..MAGIC_V2.len() + SALT_LEN]);
        derive_key_v2(password, &salt)?
    } else {
        derive_key_v1(password)?
    };
    let cipher = XChaCha20Poly1305::new(&key.into());

    let nonce_start = header_len - NONCE_LEN;
    let nonce_bytes = &all[nonce_start..nonce_start + NONCE_LEN];
    let ciphertext = &all[header_len..];
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

    // 解析容器（read_slice 保证切片不越界，畸形文件返回错误而非 panic）
    let mut pos = 0usize;
    let db_len = read_u64(&payload, &mut pos)? as usize;
    let db_bytes = read_slice(&payload, &mut pos, db_len)?;

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
            let rel_bytes = read_slice(&payload, &mut pos, rel_len)?;
            let rel = std::str::from_utf8(rel_bytes)
                .map_err(|e| BackupError::InvalidFormat(format!("non-utf8 path: {e}")))?;
            if !is_safe_rel_path(rel) {
                return Err(BackupError::InvalidFormat(format!(
                    "unsafe path in backup (zip-slip): {rel}"
                )));
            }
            let abs = raw_root.join(rel);
            if let Some(parent) = abs.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let data_len = read_u64(&payload, &mut pos)? as usize;
            let data = read_slice(&payload, &mut pos, data_len)?;
            std::fs::write(&abs, data)?;
            raw_count += 1;
            raw_bytes += data_len as u64;
        }
    }

    let db_size = db_bytes.len() as u64;
    Ok(BackupMeta {
        format_version: if &all[..MAGIC_V2.len()] == MAGIC_V2 {
            2
        } else {
            1
        },
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

/// 读取 `len` 字节切片，越界返回错误而非 panic（防畸形备份文件）。
fn read_slice<'a>(buf: &'a [u8], pos: &mut usize, len: usize) -> BackupResult<&'a [u8]> {
    if len > buf.len().saturating_sub(*pos) {
        return Err(BackupError::InvalidFormat(format!(
            "truncated slice: need {len} bytes at {pos}, only {} left",
            buf.len() - *pos
        )));
    }
    let s = &buf[*pos..*pos + len];
    *pos += len;
    Ok(s)
}

/// 备份内相对路径安全校验（防 zip-slip）：非空、相对路径、无 `..` 组件。
fn is_safe_rel_path(rel: &str) -> bool {
    use std::path::Component;
    if rel.is_empty() || rel.starts_with('/') || rel.starts_with('\\') {
        return false;
    }
    Path::new(rel)
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
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
        .map_or(0, |d| d.as_millis() as i64)
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
        repo.upsert_provider(Provider::Generic)
            .expect("upsert failed");
        let c = Conversation::new(Provider::Generic, "src-backup");
        repo.upsert_conversation(&c).expect("upsert failed");
        (dir, db)
    }

    #[test]
    fn derive_key_v1_requires_min_length() {
        assert!(derive_key_v1("short").is_err());
        assert!(derive_key_v1("longenough").is_ok());
    }

    #[test]
    fn derive_key_v2_is_salted() {
        // 同盐确定性
        let a = derive_key_v2("my-password-123", &[0u8; SALT_LEN]).expect("unexpected None");
        let b = derive_key_v2("my-password-123", &[0u8; SALT_LEN]).expect("unexpected None");
        assert_eq!(a, b);
        // 不同盐 → 不同密钥（抗预计算/彩虹表）
        let c = derive_key_v2("my-password-123", &[1u8; SALT_LEN]).expect("unexpected None");
        assert_ne!(a, c);
        // 与 V1 无盐派生结果不同
        let v1 = derive_key_v1("my-password-123").expect("unexpected None");
        assert_ne!(a, v1);
        assert!(derive_key_v2("short", &[0u8; SALT_LEN]).is_err());
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
        assert_eq!(meta.format_version, 2);
        assert!(meta.db_size > 0);

        // 恢复到新目录
        let restore_dir = TempDir::new().expect("tempdir creation failed");
        let meta2 = restore_backup(&backup_path, "test-password-1", restore_dir.path())
            .expect("unexpected None");
        assert_eq!(meta2.format_version, 2);
        assert_eq!(meta.db_size, meta2.db_size);
        assert!(backup_path.exists());

        // 恢复后的库应能打开且有数据
        let restored_db = restore_dir.path().join("conversation-hub.db");
        let repo = Repository::open(&restored_db).expect("unexpected None");
        assert_eq!(repo.count_conversations().expect("unexpected None"), 1);
    }

    #[test]
    fn v1_backup_still_restorable() {
        // 旧格式（CHBK1 + 无盐 BLAKE3）必须能恢复：兼容升级前生成的备份
        let (_dir, db) = seeded();
        let db_bytes = std::fs::read(&db).expect("file I/O failed");
        let mut payload = Vec::new();
        write_u64(&mut payload, db_bytes.len() as u64);
        payload.extend_from_slice(&db_bytes);
        write_u64(&mut payload, 0); // 0 个 raw 文件

        let key = derive_key_v1("legacy-pw-123").expect("unexpected None");
        let cipher = XChaCha20Poly1305::new(&key.into());
        let compressed = zstd::encode_all(payload.as_slice(), 1)
            .map_err(|e| BackupError::Zstd(e.to_string()))
            .expect("unexpected None");
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, compressed.as_ref())
            .expect("unexpected None");

        let backup_dir = TempDir::new().expect("tempdir creation failed");
        let backup_path = backup_dir.path().join("legacy.chbak");
        let mut f = std::fs::File::create(&backup_path).expect("file I/O failed");
        f.write_all(MAGIC_V1).expect("file I/O failed");
        f.write_all(nonce.as_slice()).expect("file I/O failed");
        f.write_all(&ciphertext).expect("file I/O failed");

        let restore_dir = TempDir::new().expect("tempdir creation failed");
        let meta = restore_backup(&backup_path, "legacy-pw-123", restore_dir.path())
            .expect("v1 backup must restore");
        assert_eq!(meta.format_version, 1);
        assert!(restore_dir.path().join("conversation-hub.db").exists());
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
        let meta2 = restore_backup(&backup_path, "pw-12345678", restore_dir.path())
            .expect("unexpected None");
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
        assert_eq!(&bytes[..6], MAGIC_V2);
    }

    #[test]
    fn backup_salt_is_random_per_file() {
        // 两次备份即使同密码，盐也不同（前 6+16 字节应不同）
        let (_dir, db) = seeded();
        let backup_dir = TempDir::new().expect("tempdir creation failed");
        let p1 = backup_dir.path().join("a.chbak");
        let p2 = backup_dir.path().join("b.chbak");
        create_backup(
            &BackupSource {
                db_path: db.clone(),
                raw_root: None,
            },
            "pw-12345678",
            &p1,
        )
        .expect("unexpected None");
        create_backup(
            &BackupSource {
                db_path: db,
                raw_root: None,
            },
            "pw-12345678",
            &p2,
        )
        .expect("unexpected None");
        let b1 = std::fs::read(&p1).expect("file I/O failed");
        let b2 = std::fs::read(&p2).expect("file I/O failed");
        assert_ne!(&b1[6..22], &b2[6..22], "salt must be random per backup");
    }

    #[test]
    fn restore_rejects_zip_slip_paths() {
        // 构造带 `../evil` 路径的合法 V2 备份：解密能过，但路径必须被拒绝
        let (_dir, db) = seeded();
        let db_bytes = std::fs::read(&db).expect("file I/O failed");
        let mut payload = Vec::new();
        write_u64(&mut payload, db_bytes.len() as u64);
        payload.extend_from_slice(&db_bytes);
        write_u64(&mut payload, 1); // 1 个 raw 文件
        let evil = "../evil.txt";
        write_u64(&mut payload, evil.len() as u64);
        payload.extend_from_slice(evil.as_bytes());
        write_u64(&mut payload, 5);
        payload.extend_from_slice(b"evil!");

        let salt = [7u8; SALT_LEN];
        let key = derive_key_v2("pw-12345678", &salt).expect("unexpected None");
        let cipher = XChaCha20Poly1305::new(&key.into());
        let compressed = zstd::encode_all(payload.as_slice(), 1)
            .map_err(|e| BackupError::Zstd(e.to_string()))
            .expect("unexpected None");
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, compressed.as_ref())
            .expect("unexpected None");

        let backup_dir = TempDir::new().expect("tempdir creation failed");
        let backup_path = backup_dir.path().join("evil.chbak");
        let mut f = std::fs::File::create(&backup_path).expect("file I/O failed");
        f.write_all(MAGIC_V2).expect("file I/O failed");
        f.write_all(&salt).expect("file I/O failed");
        f.write_all(nonce.as_slice()).expect("file I/O failed");
        f.write_all(&ciphertext).expect("file I/O failed");

        let restore_dir = TempDir::new().expect("tempdir creation failed");
        let err = restore_backup(&backup_path, "pw-12345678", restore_dir.path());
        assert!(err.is_err(), "zip-slip path must be rejected");
        assert!(
            !backup_dir.path().join("evil.txt").exists(),
            "must not escape target dir"
        );
    }

    #[test]
    fn restore_truncated_length_field_is_error_not_panic() {
        // 声明的 db_len 超过实际 payload：应返回错误而非切片 panic
        let salt = [7u8; SALT_LEN];
        let key = derive_key_v2("pw-12345678", &salt).expect("unexpected None");
        let cipher = XChaCha20Poly1305::new(&key.into());
        let mut payload = Vec::new();
        write_u64(&mut payload, 999_999_999); // 超大长度
        payload.extend_from_slice(b"tiny");
        let compressed = zstd::encode_all(payload.as_slice(), 1)
            .map_err(|e| BackupError::Zstd(e.to_string()))
            .expect("unexpected None");
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, compressed.as_ref())
            .expect("unexpected None");

        let backup_dir = TempDir::new().expect("tempdir creation failed");
        let backup_path = backup_dir.path().join("trunc.chbak");
        let mut f = std::fs::File::create(&backup_path).expect("file I/O failed");
        f.write_all(MAGIC_V2).expect("file I/O failed");
        f.write_all(&salt).expect("file I/O failed");
        f.write_all(nonce.as_slice()).expect("file I/O failed");
        f.write_all(&ciphertext).expect("file I/O failed");

        let restore_dir = TempDir::new().expect("tempdir creation failed");
        assert!(restore_backup(&backup_path, "pw-12345678", restore_dir.path()).is_err());
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
