//! LLM 客户端与本地密钥密封（plan §13.5「AI 知识提取」+ §14.3「本地数据保护」）。
//!
//! ## 组成
//!
//! - [`config`]：LLM 配置模型（OpenAI 兼容端点；非敏感字段明文 + API Key 密文）。
//! - [`secret`]：密钥密封保险库（XChaCha20-Poly1305；主密钥为数据目录下
//!   0600 密钥文件，跨平台统一，不依赖 OS 钥匙串）。
//! - [`client`]：OpenAI 兼容 `chat/completions` 客户端（ureq + rustls）。
//!
//! ## 安全边界
//!
//! - API Key 明文只存在于进程内存（调用期）；落盘形态为 AEAD 密文。
//! - 错误类型 [`LlmError`] 的 Display 不携带任何凭据材料。
//! - 非本地端点强制 https（见 [`config::LlmConfig::validate`]）。

pub mod client;
pub mod config;
pub mod secret;

pub use client::{Chat, ChatReply, ChatRequest, HttpChat};
pub use config::{
    LlmConfig, DEFAULT_MAX_INPUT_CHARS, DEFAULT_TIMEOUT_SECS, MAX_INPUT_CHARS_LIMIT,
    MAX_TIMEOUT_SECS,
};
pub use secret::{mask_key, SecretVault};

/// LLM 域错误：Display 均为面向用户的中文短语，**不含 API Key 等凭据**。
#[derive(Debug, Clone, thiserror::Error)]
pub enum LlmError {
    #[error("本地密钥存储不可用：{0}")]
    KeyStore(String),

    #[error("解密失败：密文损坏或主密钥不匹配（请重新录入 API Key）")]
    Decrypt,

    #[error("网络请求失败：{0}")]
    Network(String),

    #[error("服务端返回错误（HTTP {code}）")]
    HttpStatus {
        code: u16,
        #[doc(hidden)]
        detail: String,
    },

    #[error("模型响应无法解析：{0}")]
    Parse(String),

    #[error("配置无效：{0}")]
    InvalidConfig(String),
}

impl LlmError {
    /// 服务端错误详情（已截断），用于诊断展示；不含本端凭据材料。
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        match self {
            LlmError::HttpStatus { detail, .. } => Some(detail.as_str()),
            _ => None,
        }
    }
}
