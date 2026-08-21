//! Codex JS 工具桥解析：从 `custom_tool_call` 的 `input`（JS 代码）中提取
//! `tools.<name>({...})` 调用并还原参数。
//!
//! 背景：新版 Codex CLI 把 shell 执行、生图、看图、计划/目标更新等全部封装为
//! `name="exec"` 的 JS 片段（如 `const r=await tools.exec_command({cmd:"…"});`），
//! 记录的 `arguments` 字段为空——不解析 JS 就只剩无意义的 "Codex: exec"。
//!
//! 解析策略（无正则依赖、字节级扫描）：
//! 1. 定位 `tools.` + 标识符 + `(`；
//! 2. 字符串感知的括号配平，截取实参子串；
//! 3. 实参先按 JSON 解析（键带引号时就是合法 JSON）；失败则回退为
//!    「字符串键 : 字符串值」对扫描（JS 对象字面量键常不带引号）。

use serde_json::{Map, Value};

/// 一次 `tools.xxx(...)` 调用：工具名 + 能还原出的参数对象。
#[derive(Debug, Clone, PartialEq)]
#[must_use]
pub struct JsToolCall {
    pub tool: String,
    pub args: Value,
}

/// 提取 JS 片段中的全部 `tools.*` 调用（按出现顺序）。
#[must_use]
pub fn extract_js_tool_calls(js: &str) -> Vec<JsToolCall> {
    let mut out = Vec::new();
    let bytes = js.as_bytes();
    let mut i = 0usize;
    while let Some(rel) = js[i..].find("tools.") {
        let dot_at = i + rel;
        let after = dot_at + "tools.".len();
        // 工具名：字母数字下划线
        let mut name_end = after;
        while name_end < bytes.len()
            && (bytes[name_end].is_ascii_alphanumeric() || bytes[name_end] == b'_')
        {
            name_end += 1;
        }
        if name_end == after {
            i = after;
            continue;
        }
        let name = js[after..name_end].to_string();
        // 跳过对象属性访问（如 `tools.foo.bar(`）：要求紧跟 '('
        if name_end >= bytes.len() || bytes[name_end] != b'(' {
            i = name_end;
            continue;
        }
        match scan_balanced_parens(js, name_end) {
            Some(close) => {
                let inner = js[name_end + 1..close].trim();
                out.push(JsToolCall {
                    tool: name,
                    args: parse_objectish(inner),
                });
                i = close + 1;
            }
            None => break, // 括号不配平：后面也不必再看
        }
    }
    out
}

/// 从 `open_at`（指向 `(`）做字符串感知的括号配平，返回配平的 `)` 下标。
fn scan_balanced_parens(s: &str, open_at: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut i = open_at;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else if b == b'"' {
            in_string = true;
        } else if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 把实参子串解析为 JSON：先试整体 JSON；失败则扫「字符串键: 字符串值」对。
fn parse_objectish(inner: &str) -> Value {
    let trimmed = inner.trim();
    if trimmed.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
            return v;
        }
    }
    let map = scan_string_pairs(trimmed);
    if map.is_empty() {
        Value::Null
    } else {
        Value::Object(map)
    }
}

/// 扫描 `key: "value"` 对（键可带引号或不带——Codex 的对象字面量键不带引号）。
/// 值做常见 JS 转义还原。
fn scan_string_pairs(s: &str) -> Map<String, Value> {
    let mut map = Map::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        // 路径 A：带引号的键
        if b == b'"' {
            let Some((key_raw, key_end)) = read_string_literal(s, i) else {
                break;
            };
            if let Some((val_raw, val_end)) = value_after_colon(s, key_end) {
                map.insert(unescape_js(key_raw), Value::String(unescape_js(val_raw)));
                i = val_end;
            } else {
                i = key_end;
            }
            continue;
        }
        // 路径 B：不带引号的标识符键（cmd: / workdir: …）
        if b.is_ascii_alphabetic() || b == b'_' || b == b'$' {
            let mut j = i;
            while j < bytes.len()
                && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' || bytes[j] == b'$')
            {
                j += 1;
            }
            let key = s[i..j].to_string();
            if let Some((val_raw, val_end)) = value_after_colon(s, j) {
                map.insert(key, Value::String(unescape_js(val_raw)));
                i = val_end;
            } else {
                i = j;
            }
            continue;
        }
        i += 1;
    }
    map
}

/// 从 `pos` 起跳过空白，要求 `:`，再跳过空白，读一个字符串字面量。
fn value_after_colon(s: &str, pos: usize) -> Option<(&str, usize)> {
    let bytes = s.as_bytes();
    let mut j = pos;
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    if j >= bytes.len() || bytes[j] != b':' {
        return None;
    }
    j += 1;
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    if j < bytes.len() && bytes[j] == b'"' {
        return read_string_literal(s, j);
    }
    None
}

/// 从 `open_at`（指向 `"`）读一个字符串字面量，返回（原文切片、闭合引号后下标）。
/// 原文切片保留转义序列，交给 [`unescape_js`]；多字节 UTF-8 按原样保留
/// （闭合引号是 ASCII，切片边界安全）。
fn read_string_literal(s: &str, open_at: usize) -> Option<(&str, usize)> {
    let bytes = s.as_bytes();
    let mut i = open_at + 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' {
            i += 2; // 转义序列：跳过下一个字节
            continue;
        }
        if b == b'"' {
            return Some((&s[open_at + 1..i], i + 1));
        }
        i += 1;
    }
    None
}

/// JS 字符串转义还原（\n \t \r \" \\ \/ \uXXXX）。
fn unescape_js(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            // '\\' 转义与结尾孤立反斜杠都还原为单个反斜杠（语义一致，合并）
            Some('\\') | None => out.push('\\'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                    // 代理对（U+D800-DFFF）不是标量值，跳过即可
                    if let Some(ch) = char::from_u32(cp) {
                        out.push(ch);
                    }
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

/// 从 custom_tool_call / function_call 的 output 载荷提取文本：
/// 字符串原样；数组取各项的 text 字段拼接；其他形态序列化。
#[must_use]
pub fn output_to_text(output: &Value) -> String {
    match output {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_single_exec_command() {
        let js = r#"const r=await tools.exec_command({cmd:"printf 'hi'; ls",workdir:"/tmp"});text(r.output);"#;
        let calls = extract_js_tool_calls(js);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool, "exec_command");
        assert_eq!(calls[0].args["cmd"], "printf 'hi'; ls");
        assert_eq!(calls[0].args["workdir"], "/tmp");
    }

    #[test]
    fn extracts_multiple_calls_in_order() {
        let js = r#"const a=await tools.create_goal({objective:"整理角色"});const b=await tools.update_plan({steps:[]});tools.view_image({path:"/tmp/a b.png"});"#;
        let calls = extract_js_tool_calls(js);
        assert_eq!(
            calls.iter().map(|c| c.tool.as_str()).collect::<Vec<_>>(),
            vec!["create_goal", "update_plan", "view_image"]
        );
        assert_eq!(calls[2].args["path"], "/tmp/a b.png");
    }

    #[test]
    fn braces_and_parens_inside_strings_do_not_break_balance() {
        let js = r#"tools.exec_command({cmd:"echo '} ) ( {' && printf \"x\""});tools.view_image({path:"/a(b).png"});"#;
        let calls = extract_js_tool_calls(js);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].args["cmd"], "echo '} ) ( {' && printf \"x\"");
        assert_eq!(calls[1].args["path"], "/a(b).png");
    }

    #[test]
    fn unquoted_keys_fall_back_to_string_pairs() {
        let js = r#"tools.exec_command({cmd:"rg -c '^x'", yield_time_ms:10000});"#;
        let calls = extract_js_tool_calls(js);
        assert_eq!(calls[0].args["cmd"], "rg -c '^x'");
        assert!(
            calls[0].args.get("yield_time_ms").is_none(),
            "非字符串值不识别（保守）"
        );
    }

    #[test]
    fn escapes_are_unescaped() {
        let js = r#"tools.exec_command({cmd:"printf 'a\\nb\\t齐天大圣'"});"#;
        let calls = extract_js_tool_calls(js);
        let cmd = calls[0].args["cmd"].as_str().expect("unexpected None");
        assert!(cmd.contains("齐天大圣"), "非 ASCII 保留：{cmd}");
    }

    #[test]
    fn no_tools_calls_returns_empty() {
        assert!(extract_js_tool_calls("ls -la; echo done").is_empty());
        assert!(extract_js_tool_calls("").is_empty());
    }

    #[test]
    fn property_access_skipped_but_method_kept() {
        // tools.config.get( 不算（属性链）；紧随的 tools.exec_command( 算
        let js = r#"const c=tools.config.get;tools.exec_command({cmd:"ls"});"#;
        let calls = extract_js_tool_calls(js);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool, "exec_command");
    }

    #[test]
    fn output_to_text_shapes() {
        assert_eq!(output_to_text(&json!("done")), "done");
        assert_eq!(
            output_to_text(
                &json!([{"type":"input_text","text":"Script completed"},{"type":"input_text","text":"Wall time 3s"}])
            ),
            "Script completed\nWall time 3s"
        );
        assert_eq!(output_to_text(&Value::Null), "");
    }

    #[test]
    fn real_world_goal_input() {
        // 真实会话形态：input 内含 create_goal + update_plan 双调用
        let js = "const a = await tools.create_goal({objective:\"整理《西游记》角色\"});\nconst b = await tools.update_plan({p";
        let calls = extract_js_tool_calls(js);
        assert_eq!(calls[0].tool, "create_goal");
        assert_eq!(calls[0].args["objective"], "整理《西游记》角色");
        // update_plan 括号不配平（截断）→ 只提取到第一个完整调用
        assert_eq!(calls.len(), 1);
    }
}
