//! 本地密钥密封：XChaCha20-Poly1305 AEAD + 0600 密钥文件主密钥。
//!
//! ## 威胁模型与设计（plan §14.3；2026-08-20 用户决策：不依赖 OS 钥匙串）
//!
//! - **主密钥**（32 字节随机）：存 `<data_dir>/keys/llm-master.key`，
//!   Unix 下权限 0600（仅本用户可读），Windows 下位于用户 profile 目录内。
//! - **密文**（API Key）以 `"v1." + base64(nonce[24] ‖ ciphertext‖tag[16])`
//!   存放在调用方的数据库里——数据库被单独拷走/泄露时无主密钥不可解。
//! - 每次密封随机 nonce；固定 AAD `threadock.llm.api_key` 防密文跨用途挪用；
//!   版本前缀 `v1` 为密钥轮换预留（plan §14.3「密钥轮换必须有版本字段」）。
//! - 主密钥持于 [`Zeroizing`] 缓冲，drop 时清零；错误信息不含密钥材料。

use std::path::Path;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{AeadCore, XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use crate::LlmError;

const SEALED_VERSION: &str = "v1";
const MASTER_KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;
/// 密封绑定的 AAD：固定用途串，防密文跨用途挪用。
const SEALED_AAD: &[u8] = b"threadock.llm.api_key";

/// API Key 密封保险库。
pub struct SecretVault {
    master_key: Zeroizing<[u8; MASTER_KEY_LEN]>,
}

impl SecretVault {
    /// 打开保险库：读取（不存在则创建）数据目录下的 0600 主密钥文件。
    ///
    /// # Errors
    /// 密钥文件不可创建/损坏时返回 [`LlmError::KeyStore`]。
    pub fn open(data_dir: &Path) -> Result<Self, LlmError> {
        let key = load_or_create_master_key(data_dir)?;
        Ok(Self {
            master_key: Zeroizing::new(key),
        })
    }

    /// 明文 → `"v1." + base64(nonce ‖ ciphertext‖tag)`。
    ///
    /// # Errors
    /// 加密失败（几乎不可能，密钥长度恒定）返回 [`LlmError::KeyStore`]。
    pub fn seal(&self, plaintext: &str) -> Result<String, LlmError> {
        let cipher = XChaCha20Poly1305::new(self.master_key.as_slice().into());
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let payload = chacha20poly1305::aead::Payload {
            msg: plaintext.as_bytes(),
            aad: SEALED_AAD,
        };
        let ct = cipher
            .encrypt(&nonce, payload)
            .map_err(|_| LlmError::KeyStore("加密失败".into()))?;
        let mut blob = nonce.to_vec();
        blob.extend_from_slice(&ct);
        Ok(format!("{SEALED_VERSION}.{}", B64.encode(blob)))
    }

    /// 密文 → 明文（认证失败即 [`LlmError::Decrypt`]，不含密钥材料）。
    ///
    /// # Errors
    /// 格式坏 / base64 坏 / 认证失败 / 非 UTF-8 均返回 [`LlmError::Decrypt`]。
    pub fn open_sealed(&self, sealed: &str) -> Result<String, LlmError> {
        let Some((version, b64)) = sealed.split_once('.') else {
            return Err(LlmError::Decrypt);
        };
        if version != SEALED_VERSION {
            return Err(LlmError::Decrypt);
        }
        let blob = B64.decode(b64).map_err(|_| LlmError::Decrypt)?;
        if blob.len() <= NONCE_LEN {
            return Err(LlmError::Decrypt);
        }
        let (nonce_bytes, ct) = blob.split_at(NONCE_LEN);
        let nonce = XNonce::from_slice(nonce_bytes);
        let cipher = XChaCha20Poly1305::new(self.master_key.as_slice().into());
        let payload = chacha20poly1305::aead::Payload {
            msg: ct,
            aad: SEALED_AAD,
        };
        let pt = cipher
            .decrypt(nonce, payload)
            .map_err(|_| LlmError::Decrypt)?;
        String::from_utf8(pt).map_err(|_| LlmError::Decrypt)
    }
}

/// API Key 打码显示：保留前 3 与后 4 字符（不足 9 位则只保留尾部）。
#[must_use]
pub fn mask_key(key: &str) -> String {
    let trimmed = key.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    let len = chars.len();
    if len == 0 {
        return String::new();
    }
    if len <= 8 {
        let tail: String = chars[len.saturating_sub(2)..].iter().collect();
        return format!("***{tail}");
    }
    let head: String = chars[..3].iter().collect();
    let tail: String = chars[len - 4..].iter().collect();
    format!("{head}***{tail}")
}

/// 从 `<data_dir>/keys/llm-master.key` 读/建主密钥（0600）。
fn load_or_create_master_key(data_dir: &Path) -> Result<[u8; MASTER_KEY_LEN], LlmError> {
    let dir = data_dir.join("keys");
    std::fs::create_dir_all(&dir).map_err(|e| LlmError::KeyStore(e.to_string()))?;
    let path = dir.join("llm-master.key");
    if let Ok(b64) = std::fs::read_to_string(&path) {
        if let Ok(bytes) = B64.decode(b64.trim()) {
            if bytes.len() == MASTER_KEY_LEN {
                let mut key = [0u8; MASTER_KEY_LEN];
                key.copy_from_slice(&bytes);
                return Ok(key);
            }
        }
        return Err(LlmError::KeyStore(
            "密钥文件损坏：请删除 keys/llm-master.key 后重试（已存密钥需重新录入）".into(),
        ));
    }
    let key = random_master_key();
    let write = || -> std::io::Result<()> {
        use std::io::Write as _;
        let mut out = std::fs::File::create(&path)?;
        out.write_all(B64.encode(key).as_bytes())?;
        out.sync_all()
    };
    write().map_err(|e| LlmError::KeyStore(e.to_string()))?;
    set_owner_only_perms(&path);
    Ok(key)
}

fn random_master_key() -> [u8; MASTER_KEY_LEN] {
    use chacha20poly1305::aead::rand_core::RngCore;
    let mut key = [0u8; MASTER_KEY_LEN];
    OsRng.fill_bytes(&mut key);
    key
}

#[cfg(unix)]
fn set_owner_only_perms(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_owner_only_perms(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let vault = SecretVault::open(dir.path()).expect("vault open");
        let sealed = vault.seal("sk-test-1234567890").expect("seal");
        assert!(sealed.starts_with("v1."), "版本前缀：{sealed}");
        assert_eq!(
            vault.open_sealed(&sealed).expect("open"),
            "sk-test-1234567890"
        );
    }

    #[test]
    fn seal_uses_random_nonce() {
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let vault = SecretVault::open(dir.path()).expect("vault open");
        let a = vault.seal("same-plaintext").expect("seal");
        let b = vault.seal("same-plaintext").expect("seal");
        assert_ne!(a, b, "随机 nonce → 同明文两次密文不同");
        assert_eq!(vault.open_sealed(&a).expect("open"), "same-plaintext");
    }

    #[test]
    fn tampered_ciphertext_rejected() {
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let vault = SecretVault::open(dir.path()).expect("vault open");
        let sealed = vault.seal("sk-abc").expect("seal");
        // 翻转密文末字节（base64 段为 ASCII，可安全按字节重建）
        let flip = |s: &str, idx: usize| {
            let mut bytes = s.as_bytes().to_vec();
            bytes[idx] = if bytes[idx] == b'A' { b'B' } else { b'A' };
            String::from_utf8(bytes).expect("ascii rebuild")
        };
        let broken = flip(&sealed, sealed.len() - 1);
        assert!(matches!(vault.open_sealed(&broken), Err(LlmError::Decrypt)));
        // 篡改中间
        let mid = flip(&sealed, 10);
        assert!(matches!(vault.open_sealed(&mid), Err(LlmError::Decrypt)));
    }

    #[test]
    fn malformed_sealed_strings_rejected() {
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let vault = SecretVault::open(dir.path()).expect("vault open");
        for bad in [
            "",
            "v1",
            "v2.AAAA",
            "v1.!!!not-base64!!!",
            "no-version-blob",
            "v1.QQ==",
        ] {
            assert!(
                matches!(vault.open_sealed(bad), Err(LlmError::Decrypt)),
                "应拒绝：{bad}"
            );
        }
    }

    #[test]
    fn cross_vault_cannot_decrypt() {
        // 主密钥不同（如换设备/删密钥文件）时旧密文不可解——预期行为
        let dir_a = tempfile::tempdir().expect("tempdir creation failed");
        let dir_b = tempfile::tempdir().expect("tempdir creation failed");
        let a = SecretVault::open(dir_a.path()).expect("vault open");
        let b = SecretVault::open(dir_b.path()).expect("vault open");
        let sealed = a.seal("sk-cross").expect("seal");
        assert!(matches!(b.open_sealed(&sealed), Err(LlmError::Decrypt)));
    }

    #[test]
    fn master_key_file_persists_across_reopen() {
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let first = SecretVault::open(dir.path()).expect("vault open");
        let sealed = first.seal("sk-persist").expect("seal");
        let second = SecretVault::open(dir.path()).expect("vault reopen");
        assert_eq!(
            second.open_sealed(&sealed).expect("open"),
            "sk-persist",
            "同一目录重开应复用密钥文件"
        );
    }

    #[cfg(unix)]
    #[test]
    fn key_file_has_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let _vault = SecretVault::open(dir.path()).expect("vault open");
        let mode = std::fs::metadata(dir.path().join("keys/llm-master.key"))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "密钥文件必须 0600");
    }

    #[test]
    fn plaintext_never_in_key_file() {
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let vault = SecretVault::open(dir.path()).expect("vault open");
        vault.seal("sk-SECRET-plaintext").expect("seal");
        let key_file =
            std::fs::read_to_string(dir.path().join("keys/llm-master.key")).expect("read key file");
        assert!(!key_file.contains("SECRET"));
        // 密文不落密钥文件（密文由调用方存 DB）
        assert!(!key_file.contains("v1."));
    }

    #[test]
    fn key_file_is_base64_of_32_bytes() {
        let dir = tempfile::tempdir().expect("tempdir creation failed");
        let _vault = SecretVault::open(dir.path()).expect("vault open");
        let raw =
            std::fs::read_to_string(dir.path().join("keys/llm-master.key")).expect("read key file");
        let bytes = B64.decode(raw.trim()).expect("valid base64");
        assert_eq!(bytes.len(), MASTER_KEY_LEN, "主密钥必须 32 字节");
    }

    #[test]
    fn mask_key_shapes() {
        assert_eq!(mask_key(""), "");
        assert_eq!(mask_key(" short "), "***rt");
        assert_eq!(mask_key("sk-AbCd1234"), "sk-***1234");
        assert_eq!(mask_key("0123456789"), "012***6789");
    }
}
