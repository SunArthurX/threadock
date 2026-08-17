//! 搜索查询语法（plan §13.2）：在裸关键词之外支持结构化过滤前缀。
//!
//! 语法：`关键词 provider:codex workspace:my-app type:assistant status:favorite file:main.rs model:gpt after:2026-01-01 before:2026-06-30`
//!
//! - 过滤前缀大小写不敏感；值可用双引号包住以包含空格：`workspace:"my project"`。
//! - `type:` 取 `user` / `assistant` / `system` / `tool` 映射为角色过滤；
//!   其他值（如 `type:command`）按字面量留在自由文本里，不静默丢弃。
//! - 未知的 `foo:bar` token 原样保留为自由文本（由下游按字面量匹配）。
//! - `after:` / `before:` 接受 `YYYY-MM-DD`（或 `/` 分隔），按 UTC 解释；
//!   after 含当日 00:00，before 含当日 23:59:59.999。

use time::{Date, Month};

/// 解析结果：自由文本 + 各维过滤条件。
///
/// 全部字段为「用户未写则为 None」；provider 保留原始字符串
/// （如 `codex`、`claude-code`），由调用方转 `Provider`，
/// 避免 domain 反向依赖解析失败语义。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedQuery {
    /// 去掉过滤前缀后的自由文本（多 token 以空格连接；可能为空 = 纯过滤查询）。
    pub text: String,
    pub provider: Option<String>,
    /// workspace 的 id 或 display_name（调用方解析，二者都试）。
    pub workspace: Option<String>,
    /// `type:user|assistant|system|tool`（或 `role:` 别名）。
    pub role: Option<String>,
    /// `status:favorite|archived|deleted|active`。
    pub status: Option<String>,
    /// 文件路径子串（匹配事件 payload / 摘要）。
    pub file: Option<String>,
    /// 模型名子串。
    pub model: Option<String>,
    /// 起始时间（含），Unix epoch 毫秒。
    pub after_ms: Option<i64>,
    /// 结束时间（含），Unix epoch 毫秒。
    pub before_ms: Option<i64>,
}

impl ParsedQuery {
    /// 是否携带任何需要在数据库层生效的过滤（时间/状态/文件/模型）。
    /// Tantivy 索引内只覆盖 provider/workspace/role，其余靠这批条件后过滤。
    #[must_use]
    pub fn needs_db_filter(&self) -> bool {
        self.status.is_some()
            || self.file.is_some()
            || self.model.is_some()
            || self.after_ms.is_some()
            || self.before_ms.is_some()
    }
}

/// 支持的过滤键（用于文档展示与测试）。
pub const FILTER_KEYS: &[&str] = &[
    "provider",
    "workspace",
    "type",
    "role",
    "status",
    "file",
    "model",
    "after",
    "before",
];

const ROLES: &[&str] = &["user", "assistant", "system", "tool"];

/// 解析用户输入。永远成功：无法识别的部分退化为自由文本。
#[must_use]
pub fn parse(input: &str) -> ParsedQuery {
    let mut out = ParsedQuery::default();
    let mut text_tokens: Vec<String> = Vec::new();

    for token in tokenize(input) {
        match split_filter(&token) {
            Some((key, value)) if is_filter_key(key) => {
                apply_filter(&mut out, &mut text_tokens, key, value);
            }
            _ => text_tokens.push(token),
        }
    }
    out.text = text_tokens.join(" ");
    out
}

/// 按空白切分，双引号内的空白不切分（引号本身被去掉）。
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for ch in input.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

/// `key:value` 切分：只认第一个冒号，且 key 部分不含引号/空白。
fn split_filter(token: &str) -> Option<(&str, &str)> {
    let (key, value) = token.split_once(':')?;
    if key.is_empty() || value.is_empty() || key.contains('"') {
        return None;
    }
    Some((key, value))
}

fn is_filter_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    FILTER_KEYS.contains(&lower.as_str())
}

fn apply_filter(out: &mut ParsedQuery, text_tokens: &mut Vec<String>, key: &str, value: &str) {
    let lower_key = key.to_ascii_lowercase();
    let lower_value = value.to_ascii_lowercase();
    match lower_key.as_str() {
        "provider" => out.provider = Some(lower_value),
        "workspace" => out.workspace = Some(value.to_string()),
        "role" => out.role = Some(lower_value),
        "type" => {
            if ROLES.contains(&lower_value.as_str()) {
                out.role = Some(lower_value);
            } else {
                // type:command 等非角色值按字面量保留，不静默吞掉
                text_tokens.push(format!("{key}:{value}"));
            }
        }
        "status" => out.status = Some(lower_value),
        "file" => out.file = Some(value.to_string()),
        "model" => out.model = Some(value.to_string()),
        "after" => match parse_date_ms(value, false) {
            Some(ms) => out.after_ms = Some(ms),
            None => text_tokens.push(format!("{key}:{value}")),
        },
        "before" => match parse_date_ms(value, true) {
            Some(ms) => out.before_ms = Some(ms),
            None => text_tokens.push(format!("{key}:{value}")),
        },
        _ => unreachable!("is_filter_key 已过滤"),
    }
}

/// `YYYY-MM-DD`（或 `YYYY/MM/DD`）→ epoch 毫秒。
/// `end_of_day=true` 时取当日 23:59:59.999（before 语义：含当日）。
fn parse_date_ms(input: &str, end_of_day: bool) -> Option<i64> {
    let normalized = input.replace('/', "-");
    let parts: Vec<&str> = normalized.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let year: i32 = parts[0].parse().ok()?;
    let month: u8 = parts[1].parse().ok()?;
    let day: u8 = parts[2].parse().ok()?;
    let date = Date::from_calendar_date(year, Month::try_from(month).ok()?, day).ok()?;
    let (h, mi, s, nano) = if end_of_day {
        (23, 59, 59, 999_999_999)
    } else {
        (0, 0, 0, 0)
    };
    let dt = time::PrimitiveDateTime::new(date, time::Time::from_hms_nano(h, mi, s, nano).ok()?)
        .assume_utc();
    i64::try_from(dt.unix_timestamp_nanos() / 1_000_000).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_passes_through() {
        let p = parse("tauri android 后台任务");
        assert_eq!(p.text, "tauri android 后台任务");
        assert!(p.provider.is_none());
    }

    #[test]
    fn provider_and_text() {
        let p = parse("provider:codex 错误处理");
        assert_eq!(p.provider.as_deref(), Some("codex"));
        assert_eq!(p.text, "错误处理");
    }

    #[test]
    fn provider_with_alias_value() {
        let p = parse("provider:claude-code hello");
        assert_eq!(p.provider.as_deref(), Some("claude-code"));
    }

    #[test]
    fn filter_keys_case_insensitive() {
        let p = parse("Provider:Cursor WorkManager");
        assert_eq!(p.provider.as_deref(), Some("cursor"));
        assert_eq!(p.text, "WorkManager");
    }

    #[test]
    fn quoted_workspace_name() {
        let p = parse("workspace:\"my project\" keyword");
        assert_eq!(p.workspace.as_deref(), Some("my project"));
        assert_eq!(p.text, "keyword");
    }

    #[test]
    fn type_role_mapping() {
        let p = parse("type:assistant answer");
        assert_eq!(p.role.as_deref(), Some("assistant"));
        assert_eq!(p.text, "answer");
    }

    #[test]
    fn type_non_role_stays_text() {
        let p = parse("type:command run");
        assert!(p.role.is_none());
        assert_eq!(p.text, "type:command run");
    }

    #[test]
    fn role_alias() {
        let p = parse("role:user question");
        assert_eq!(p.role.as_deref(), Some("user"));
    }

    #[test]
    fn unknown_colon_token_stays_text() {
        let p = parse("foo:bar baz");
        assert_eq!(p.text, "foo:bar baz");
    }

    #[test]
    fn url_like_token_stays_text() {
        // https://... 的第一个冒号后无字母歧义，但 key 含 '/' 非法 → 整体保留
        let p = parse("看这个 https://example.com doc");
        assert_eq!(p.text, "看这个 https://example.com doc");
    }

    #[test]
    fn date_range() {
        let p = parse("after:2026-01-01 before:2026/06/30 deploy");
        assert_eq!(p.text, "deploy");
        let after = p.after_ms.expect("after parsed");
        let before = p.before_ms.expect("before parsed");
        assert_eq!(after, 1_767_225_600_000); // 2026-01-01T00:00:00Z
        assert!(before > after);
        // before 为当日末尾：23:59:59.999
        assert_eq!((before - after) % 86_400_000, 86_399_999);
    }

    #[test]
    fn invalid_date_stays_text() {
        let p = parse("after:not-a-date x");
        assert!(p.after_ms.is_none());
        assert!(p.text.contains("after:not-a-date"));
    }

    #[test]
    fn status_file_model_filters() {
        let p = parse("status:favorite file:main.rs model:gpt-4 retry");
        assert_eq!(p.status.as_deref(), Some("favorite"));
        assert_eq!(p.file.as_deref(), Some("main.rs"));
        assert_eq!(p.model.as_deref(), Some("gpt-4"));
        assert_eq!(p.text, "retry");
        assert!(p.needs_db_filter());
    }

    #[test]
    fn pure_filters_empty_text() {
        let p = parse("provider:zcode status:archived");
        assert_eq!(p.text, "");
        assert_eq!(p.provider.as_deref(), Some("zcode"));
    }

    #[test]
    fn duplicate_filter_last_wins() {
        let p = parse("provider:codex provider:cursor k");
        assert_eq!(p.provider.as_deref(), Some("cursor"));
    }

    #[test]
    fn empty_input() {
        let p = parse("");
        assert_eq!(p, ParsedQuery::default());
    }

    #[test]
    fn unmatched_quote_is_tolerated() {
        // 引号未闭合：后续空白不再切分，整个尾巴并入该值（容错不丢数据）
        let p = parse("workspace:\"unclosed k");
        assert_eq!(p.workspace.as_deref(), Some("unclosed k"));
        assert_eq!(p.text, "");
    }

    #[test]
    fn colon_only_token_stays_text() {
        let p = parse(":: x");
        assert_eq!(p.text, ":: x");
    }
}
