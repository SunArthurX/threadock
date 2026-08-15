//! 敏感信息脱敏，对应 plan §14.6「自动脱敏密钥、Token 和邮箱」。
//!
//! 内置规则（可扩展）：
//! - AWS Access Key（`AKIA...`）
//! - AWS Secret Key（40 位 base64 风格）
//! - GitHub Token（`ghp_`/`gho_`/`ghu_`/`ghs_`/`ghr_` 前缀）
//! - 通用 Bearer Token（`Bearer xxx`）
//! - 私有 API Key（`sk-`/`api_key=`/`apikey=` 风格）
//! - 邮箱地址
//! - 私有 IP 段（10./172.16~31./192.168.）
//!
//! 替换为 `[REDACTED:type]`，保留可读结构但去除敏感值。
//! 同时统计命中次数（plan §14.8：导出可审计）。

use regex::Regex;
use serde::{Deserialize, Serialize};

/// 一条脱敏规则。
pub struct RedactionRule {
    pub name: &'static str,
    pub pattern: Regex,
}

/// 脱敏命中统计。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionStats {
    pub aws_access_key: usize,
    pub aws_secret_key: usize,
    pub github_token: usize,
    pub bearer_token: usize,
    pub api_key: usize,
    pub email: usize,
    pub private_ip: usize,
}

impl RedactionStats {
    #[must_use] 
    pub fn total(&self) -> usize {
        self.aws_access_key
            + self.aws_secret_key
            + self.github_token
            + self.bearer_token
            + self.api_key
            + self.email
            + self.private_ip
    }
}

/// 内置规则集。
#[must_use] 
pub fn builtin_rules() -> Vec<RedactionRule> {
    vec![
        RedactionRule {
            name: "aws_access_key",
            // AKIA 开头 + 16 位大写字母数字
            pattern: Regex::new(r"AKIA[0-9A-Z]{16}").expect("invalid regex"),
        },
        RedactionRule {
            name: "github_token",
            // ghp_/gho_/ghu_/ghs_/ghr_ + 36 位
            pattern: Regex::new(r"gh[posur]_[A-Za-z0-9]{36}").expect("invalid regex"),
        },
        RedactionRule {
            name: "bearer_token",
            // Bearer <token>，至少 8 位
            pattern: Regex::new(r"(?i)bearer\s+[A-Za-z0-9\-_.=]{8,}").expect("invalid regex"),
        },
        RedactionRule {
            name: "api_key",
            // sk- (OpenAI 风格) 或 api_key=/apikey= 后跟值
            pattern: Regex::new(r"(sk-[A-Za-z0-9]{20,}|(?i)api_?key\s*[:=]\s*[A-Za-z0-9\-_]{8,})")
                .expect("unexpected None"),
        },
        RedactionRule {
            name: "email",
            pattern: Regex::new(r"\b[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}\b").expect("invalid regex"),
        },
        RedactionRule {
            name: "private_ip",
            // 10.x / 192.168.x / 172.16-31.x
            pattern: Regex::new(
                r"\b(10\.\d{1,3}\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3}|172\.(1[6-9]|2[0-9]|3[01])\.\d{1,3}\.\d{1,3})\b",
            )
            .expect("unexpected None"),
        },
        // AWS Secret Key 单列：靠 key=value 上下文判断，避免误伤普通 40 位串
        RedactionRule {
            name: "aws_secret_key",
            pattern: Regex::new(r"(?i)(aws_secret|secret_access_key)\s*[:=]\s*[A-Za-z0-9/+=]{40}")
                .expect("unexpected None"),
        },
    ]
}

/// 对文本执行脱敏，返回脱敏后的文本与命中统计。
///
/// 同一处命中只算一次（按规则顺序应用）。`[REDACTED:type]` 占位便于审计定位。
#[must_use] 
pub fn redact(input: &str) -> (String, RedactionStats) {
    redact_with(input, &[])
}

/// 用户自定义脱敏规则（plan §14.6「忽略正则规则」）。
#[derive(Debug, Clone)]
pub struct CustomRule {
    /// 规则名（用作 `[REDACTED:name]` 占位）。
    pub name: String,
    /// 正则表达式。
    pub pattern: String,
}

impl CustomRule {
    pub fn new(name: impl Into<String>, pattern: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            pattern: pattern.into(),
        }
    }
}

/// 用内置规则 + 自定义规则执行脱敏（plan §14.6）。
///
/// 自定义规则在内置规则之后应用，允许用户覆盖项目特定的敏感模式。
#[must_use] 
pub fn redact_with(input: &str, custom: &[CustomRule]) -> (String, RedactionStats) {
    let mut text = input.to_string();
    let mut stats = RedactionStats::default();

    // 内置规则
    let builtin = builtin_rules();
    let mut all_rules: Vec<(&str, Option<&Regex>, Option<Regex>)> = builtin
        .iter()
        .map(|r| (r.name, Some(&r.pattern), None))
        .collect();

    // 自定义规则（编译正则）
    let compiled_custom: Vec<(&str, Option<&Regex>, Option<Regex>)> = custom
        .iter()
        .filter_map(|c| {
            Regex::new(&c.pattern)
                .ok()
                .map(|re| (c.name.as_str(), None, Some(re)))
        })
        .collect();
    all_rules.extend(compiled_custom);

    for (name, builtin_re, custom_re) in &all_rules {
        let re = builtin_re.or_else(|| custom_re.as_ref());
        if let Some(re) = re {
            let placeholder = format!("[REDACTED:{name}]");
            let count = re.find_iter(&text).count();
            if count > 0 {
                text = re.replace_all(&text, placeholder.as_str()).to_string();
                match *name {
                    "aws_access_key" => stats.aws_access_key += count,
                    "aws_secret_key" => stats.aws_secret_key += count,
                    "github_token" => stats.github_token += count,
                    "bearer_token" => stats.bearer_token += count,
                    "api_key" => stats.api_key += count,
                    "email" => stats.email += count,
                    "private_ip" => stats.private_ip += count,
                    _ => {}
                }
            }
        }
    }
    (text, stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_aws_access_key() {
        let (out, stats) = redact("key=AKIAIOSFODNN7EXAMPLE");
        assert!(out.contains("[REDACTED:aws_access_key]"));
        assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
        assert_eq!(stats.aws_access_key, 1);
    }

    #[test]
    fn redacts_github_token() {
        // ghp_ + 36 位字符
        let (out, stats) = redact("token: ghp_1234567890abcdefghijklmnopqrstuvwxyz1234");
        assert!(out.contains("[REDACTED:github_token]"));
        assert_eq!(stats.github_token, 1);
    }

    #[test]
    fn redacts_bearer_token() {
        let (out, stats) = redact("Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload.sig");
        assert!(out.contains("[REDACTED:bearer_token]"));
        assert_eq!(stats.bearer_token, 1);
    }

    #[test]
    fn redacts_openai_api_key() {
        let (out, stats) = redact("sk-projabcdefghijklmnopqrstuvwxyz0123456789");
        assert!(out.contains("[REDACTED:api_key]"));
        assert!(stats.api_key >= 1);
    }

    #[test]
    fn redacts_api_key_assignment() {
        let (out, _stats) = redact("api_key=abcdef1234567890");
        assert!(out.contains("[REDACTED:api_key]"));
    }

    #[test]
    fn redacts_email() {
        let (out, stats) = redact("联系 alice@example.com 或 bob@corp.io");
        assert!(out.contains("[REDACTED:email]"));
        assert_eq!(stats.email, 2);
    }

    #[test]
    fn redacts_private_ip() {
        let (out, stats) = redact("部署在 10.0.0.5 和 192.168.1.1");
        assert!(out.contains("[REDACTED:private_ip]"));
        assert_eq!(stats.private_ip, 2);
    }

    #[test]
    fn does_not_redact_public_ip() {
        let (out, stats) = redact("公网 IP 8.8.8.8 和 1.1.1.1");
        assert_eq!(stats.private_ip, 0);
        assert!(out.contains("8.8.8.8"));
    }

    #[test]
    fn redacts_aws_secret_in_context() {
        let (_out, stats) =
            redact("aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY");
        assert_eq!(stats.aws_secret_key, 1);
    }

    #[test]
    fn total_counts_all() {
        let (_out, stats) = redact("AKIAIOSFODNN7EXAMPLE and alice@test.com");
        assert_eq!(stats.total(), 2);
    }

    #[test]
    fn no_sensitive_content_passes_through() {
        let (out, stats) = redact("这是一段普通文字，没有敏感信息。");
        assert_eq!(out, "这是一段普通文字，没有敏感信息。");
        assert_eq!(stats.total(), 0);
    }

    #[test]
    fn multiple_same_type_counted() {
        let (_out, stats) = redact("a@x.com b@y.com c@z.com");
        assert_eq!(stats.email, 3);
    }

    // ── 自定义规则（plan §14.6）──────────────────────────────────────────

    #[test]
    fn custom_rule_redacts_pattern() {
        let rule = CustomRule::new("emp_id", r"EMP\d{6}");
        let (out, _stats) = redact_with("员工 EMP123456 提交", &[rule]);
        assert!(out.contains("[REDACTED:emp_id]"));
        assert!(!out.contains("EMP123456"));
    }

    #[test]
    fn custom_rule_alongside_builtin() {
        // 用非 email 格式的内部域名，避免被 email 规则先吃掉
        let rule = CustomRule::new("internal_host", r"host-\d+\.corp");
        let (out, _stats) = redact_with("部署在 host-42.corp 和 AKIAIOSFODNN7EXAMPLE", &[rule]);
        assert!(out.contains("[REDACTED:internal_host]"));
        assert!(out.contains("[REDACTED:aws_access_key]"));
    }

    #[test]
    fn custom_rule_invalid_regex_skipped() {
        let bad = CustomRule::new("bad", r"[invalid");
        let good = CustomRule::new("ok", r"target\d+");
        let (out, _stats) = redact_with("target123 bad", &[bad, good]);
        // 无效正则被跳过，有效规则仍工作
        assert!(out.contains("[REDACTED:ok]"));
    }

    #[test]
    fn no_custom_rules_equivalent_to_redact() {
        let input = "email: alice@test.com key: AKIAIOSFODNN7EXAMPLE";
        let (a, _) = redact(input);
        let (b, _) = redact_with(input, &[]);
        assert_eq!(a, b);
    }

    #[test]
    fn custom_rule_chinese_pattern() {
        let rule = CustomRule::new("id_card", r"\d{17}[\dXx]");
        let (out, _stats) = redact_with("身份证号 110101199003071234", &[rule]);
        assert!(out.contains("[REDACTED:id_card]"));
    }
}
