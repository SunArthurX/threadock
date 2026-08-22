//! LLM 配置模型：非敏感字段明文 + API Key 密文，整体作为 JSON 持久化
//! （由调用方存入 `app_settings`，键 `llm_config`）。

use serde::{Deserialize, Serialize};

use crate::LlmError;

pub const DEFAULT_TIMEOUT_SECS: u64 = 60;
pub const MAX_TIMEOUT_SECS: u64 = 300;
/// 转录默认字符上限。2026-08 从 48k 降到 24k：AI 提取要的是「用户需求 + 关键过程 +
/// 结论」，配合单条消息截断（见 knowledge::llm::build_transcript），24k 覆盖绝大多数
/// 会话，耗时/费用约减半。设置中可调（clamp 到 1k..200k）。
pub const DEFAULT_MAX_INPUT_CHARS: usize = 24_000;
pub const MAX_INPUT_CHARS_LIMIT: usize = 200_000;

/// LLM 提取配置。默认关闭（plan §13.5：显式启用）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_input_chars")]
    pub max_input_chars: usize,
    /// API Key 密文（`SecretVault::seal` 产物）；明文永不落盘。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_sealed: Option<String>,
}

fn default_timeout_secs() -> u64 {
    DEFAULT_TIMEOUT_SECS
}

fn default_max_input_chars() -> usize {
    DEFAULT_MAX_INPUT_CHARS
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: String::new(),
            model: String::new(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_input_chars: DEFAULT_MAX_INPUT_CHARS,
            api_key_sealed: None,
        }
    }
}

impl LlmConfig {
    /// 校验并归一化：trim、数值钳制到界内；启用时要求端点/模型齐备，
    /// 且非本地端点必须 https（防凭据明文出网）。
    ///
    /// # Errors
    /// 配置不满足上述约束时返回 [`LlmError::InvalidConfig`]。
    pub fn validate(&mut self) -> Result<(), LlmError> {
        self.base_url = self.base_url.trim().trim_end_matches('/').to_string();
        self.model = self.model.trim().to_string();
        self.timeout_secs = if self.timeout_secs == 0 {
            DEFAULT_TIMEOUT_SECS
        } else {
            self.timeout_secs.clamp(1, MAX_TIMEOUT_SECS)
        };
        self.max_input_chars = if self.max_input_chars == 0 {
            DEFAULT_MAX_INPUT_CHARS
        } else {
            self.max_input_chars.clamp(1_000, MAX_INPUT_CHARS_LIMIT)
        };
        if self.enabled {
            let scheme_ok =
                self.base_url.starts_with("https://") || self.base_url.starts_with("http://");
            if self.base_url.is_empty() || !scheme_ok {
                return Err(LlmError::InvalidConfig(
                    "base_url 必须以 http:// 或 https:// 开头".into(),
                ));
            }
            if self.base_url.starts_with("http://") && !self.is_local_endpoint() {
                return Err(LlmError::InvalidConfig(
                    "非本地端点必须使用 https（防止明文传输）".into(),
                ));
            }
            if is_anthropic_endpoint(&self.base_url) {
                return Err(LlmError::InvalidConfig(
                    "这是 Anthropic 协议端点，与 OpenAI 兼容协议不互通（会报「响应无法解析」）。\
                     GLM 请填 https://open.bigmodel.cn/api/paas/v4"
                        .into(),
                ));
            }
            if self.model.is_empty() {
                return Err(LlmError::InvalidConfig("model 不能为空".into()));
            }
        }
        Ok(())
    }

    /// 是否本地推理端点（localhost / 127.x / [::1] / 0.0.0.0，数据不出本机）。
    #[must_use]
    pub fn is_local_endpoint(&self) -> bool {
        is_local_base_url(&self.base_url)
    }

    /// 具备调用条件：已启用 + 端点/模型齐备 +（云端必须有 Key；本地可不带）。
    #[must_use]
    pub fn is_ready(&self, has_api_key: bool) -> bool {
        self.enabled
            && !self.base_url.is_empty()
            && !self.model.is_empty()
            && (self.is_local_endpoint() || has_api_key)
    }
}

/// 从 base_url 提取 host（小写；兼容 `[::1]:port` 括号形态）。
fn host_of(base_url: &str) -> String {
    let rest = base_url.split_once("://").map_or(base_url, |(_, r)| r);
    if let Some(stripped) = rest.strip_prefix('[') {
        return stripped
            .split(']')
            .next()
            .unwrap_or_default()
            .to_lowercase();
    }
    rest.split(['/', ':', '?', '#'])
        .next()
        .unwrap_or_default()
        .to_lowercase()
}

fn is_local_base_url(base_url: &str) -> bool {
    let host = host_of(base_url);
    host == "localhost" || host == "::1" || host == "0.0.0.0" || host.starts_with("127.")
}

/// 是否 Anthropic 协议端点（路径以 `/anthropic` 结尾，如 GLM Coding Plan 用的
/// `https://open.bigmodel.cn/api/anthropic`）。2026-08 实测：用户从 Coding Plan
/// 配置里复制该地址，OpenAI 客户端请求后只得到「响应无法解析」。
fn is_anthropic_endpoint(base_url: &str) -> bool {
    base_url
        .split(['#', '?'])
        .next()
        .unwrap_or_default()
        .trim_end_matches('/')
        .ends_with("/anthropic")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_disabled_and_safe() {
        let c = LlmConfig::default();
        assert!(!c.enabled);
        assert_eq!(c.timeout_secs, DEFAULT_TIMEOUT_SECS);
        assert_eq!(c.max_input_chars, DEFAULT_MAX_INPUT_CHARS);
        assert!(c.api_key_sealed.is_none());
        assert!(!c.is_ready(true), "未启用即不就绪");
    }

    #[test]
    fn serde_defaults_fill_missing_fields() {
        // 旧版本 JSON 缺字段（未来加字段）时反序列化得到默认值
        let c: LlmConfig =
            serde_json::from_str(r#"{"base_url":"https://x.example/v1"}"#).expect("parse failed");
        assert!(!c.enabled);
        assert_eq!(c.timeout_secs, DEFAULT_TIMEOUT_SECS);
        assert_eq!(c.max_input_chars, DEFAULT_MAX_INPUT_CHARS);
    }

    #[test]
    fn validate_trims_and_clamps() {
        let mut c = LlmConfig {
            enabled: false,
            base_url: "  https://api.example.com/v1/  ".into(),
            model: " gpt-4o-mini ".into(),
            timeout_secs: 999,
            max_input_chars: 999_999,
            api_key_sealed: None,
        };
        c.validate().expect("validate failed");
        assert_eq!(c.base_url, "https://api.example.com/v1");
        assert_eq!(c.model, "gpt-4o-mini");
        assert_eq!(c.timeout_secs, MAX_TIMEOUT_SECS);
        assert_eq!(c.max_input_chars, MAX_INPUT_CHARS_LIMIT);
    }

    #[test]
    fn validate_zero_values_become_defaults() {
        let mut c = LlmConfig {
            timeout_secs: 0,
            max_input_chars: 0,
            ..LlmConfig::default()
        };
        c.validate().expect("validate failed");
        assert_eq!(c.timeout_secs, DEFAULT_TIMEOUT_SECS);
        assert_eq!(c.max_input_chars, DEFAULT_MAX_INPUT_CHARS);
    }

    #[test]
    fn validate_rejects_plain_http_for_remote() {
        let mut c = LlmConfig {
            enabled: true,
            base_url: "http://api.example.com/v1".into(),
            model: "m".into(),
            ..LlmConfig::default()
        };
        assert!(c.validate().is_err(), "远程 http 必须被拒绝");
    }

    #[test]
    fn validate_allows_plain_http_for_localhost() {
        for url in [
            "http://127.0.0.1:11434/v1",
            "http://localhost:11434/v1",
            "http://[::1]:8080/v1",
            "http://0.0.0.0:8080/v1",
        ] {
            let mut c = LlmConfig {
                enabled: true,
                base_url: url.into(),
                model: "m".into(),
                ..LlmConfig::default()
            };
            c.validate().unwrap_or_else(|e| panic!("{url}: {e}"));
            assert!(c.is_local_endpoint(), "{url}");
        }
    }

    #[test]
    fn validate_rejects_bad_scheme_and_empty_model() {
        let mut c = LlmConfig {
            enabled: true,
            base_url: "ftp://example.com".into(),
            model: "m".into(),
            ..LlmConfig::default()
        };
        assert!(c.validate().is_err());
        let mut c = LlmConfig {
            enabled: true,
            base_url: "https://api.example.com/v1".into(),
            model: "  ".into(),
            ..LlmConfig::default()
        };
        assert!(c.validate().is_err(), "启用时空 model 必须被拒绝");
    }

    #[test]
    fn validate_rejects_anthropic_protocol_endpoint() {
        // 回归：GLM Coding Plan 的 Anthropic 端点被误配为 OpenAI 兼容 base_url
        for url in [
            "https://open.bigmodel.cn/api/anthropic",
            "https://open.bigmodel.cn/api/anthropic/",
            "https://open.bigmodel.cn/api/anthropic#coding-plan",
            "https://api.anthropic.com/anthropic",
        ] {
            let mut c = LlmConfig {
                enabled: true,
                base_url: url.into(),
                model: "glm-5.3".into(),
                ..LlmConfig::default()
            };
            let err = c.validate().expect_err("anthropic 必须被拒").to_string();
            assert!(err.contains("Anthropic"), "{url}：{err}");
            assert!(err.contains("paas/v4"), "应给出正确端点指引：{err}");
        }
        // OpenAI 兼容端点不受影响
        let mut ok = LlmConfig {
            enabled: true,
            base_url: "https://open.bigmodel.cn/api/paas/v4".into(),
            model: "glm-5.3".into(),
            ..LlmConfig::default()
        };
        ok.validate()
            .unwrap_or_else(|e| panic!("paas/v4 应通过：{e}"));
        // 未启用（草稿）不强校验
        let mut draft = LlmConfig {
            enabled: false,
            base_url: "https://open.bigmodel.cn/api/anthropic".into(),
            model: "m".into(),
            ..LlmConfig::default()
        };
        draft
            .validate()
            .unwrap_or_else(|e| panic!("草稿不应被拦截：{e}"));
    }

    #[test]
    fn validate_disabled_skips_endpoint_checks() {
        // 未启用时允许半成品配置（草稿），只在启用时强校验
        let mut c = LlmConfig {
            enabled: false,
            base_url: String::new(),
            model: String::new(),
            ..LlmConfig::default()
        };
        c.validate().expect("disabled config should pass");
    }

    #[test]
    fn is_local_endpoint_matrix() {
        for (url, expect) in [
            ("https://api.openai.com/v1", false),
            ("http://127.0.0.1:11434/v1", true),
            ("http://127.0.1.5:1/v1", true),
            ("http://localhost:1234", true),
            ("http://[::1]:1234/v1", true),
            ("https://my.127.0.0.1.evil.com/v1", false),
            ("", false),
        ] {
            let c = LlmConfig {
                base_url: url.into(),
                ..LlmConfig::default()
            };
            assert_eq!(c.is_local_endpoint(), expect, "{url}");
        }
    }

    #[test]
    fn is_ready_matrix() {
        let base = |local: bool| LlmConfig {
            enabled: true,
            base_url: if local {
                "http://127.0.0.1:11434/v1".into()
            } else {
                "https://api.example.com/v1".into()
            },
            model: "m".into(),
            ..LlmConfig::default()
        };
        assert!(!base(false).is_ready(false), "云端口无 Key 不就绪");
        assert!(base(false).is_ready(true), "云端口有 Key 就绪");
        assert!(base(true).is_ready(false), "本地端点可不带 Key");
        let mut disabled = base(false);
        disabled.enabled = false;
        assert!(!disabled.is_ready(true), "未启用不就绪");
    }
}
