//! HTML 文本转义（防存储型 XSS）。
//!
//! 会话正文来自外部导入文件，可能包含任意 HTML/JS。
//! 所有拼进 HTML 上下文的片段（搜索高亮、审计报告）必须先经过本模块转义。

/// 转义 HTML 文本上下文中的特殊字符：`& < > " '`。
#[must_use]
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_all_special_chars() {
        assert_eq!(escape_html("<img src=x>"), "&lt;img src=x&gt;");
        assert_eq!(escape_html("a&b"), "a&amp;b");
        assert_eq!(escape_html("\"'"), "&quot;&#39;");
    }

    #[test]
    fn plain_text_unchanged() {
        assert_eq!(escape_html("hello 世界 123"), "hello 世界 123");
    }
}
