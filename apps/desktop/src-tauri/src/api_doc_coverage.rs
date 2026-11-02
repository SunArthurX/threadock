//! API 文档覆盖守卫：`generate_handler!` 里注册的每个 Tauri 命令，都必须
//! 出现在 `examples/typegen.rs` 的清单里（否则 `docs/api.md` 会静默漏收）。
//!
//! 背景：`docs/api.md` 虽标注「自动生成」，但命令清单是 typegen.rs 里**手工
//! 维护的数组**。新增命令忘了补清单 → 生成器照样产出「与仓库一致」的文档 →
//! CI 的 `git diff --exit-code docs/api.md` 也照样绿，文档永远缺项。
//! 本测试把这个洞堵上：清单与注册表双向比对，缺一个就红。

use std::collections::BTreeSet;

/// 扫出块内所有**元组首元素**的字符串字面量。
///
/// 只认 `( name, ... )` 这种形态：字符串前面若只有空白、空白之前是 `(`，
/// 才算元组的第一个元素。这样参数名/返回类型里的字符串不会被误当成命令名。
/// 元组可以跨行书写（rustfmt 会拆成多行），所以不能只找 `("`。
fn tuple_first_strings(block: &str) -> Vec<String> {
    let b: Vec<char> = block.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] != '"' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut j = start;
        let mut s = String::new();
        while j < b.len() {
            if b[j] == '\\' {
                s.push(b[j]);
                if let Some(&nc) = b.get(j + 1) {
                    s.push(nc);
                }
                j += 2;
                continue;
            }
            if b[j] == '"' {
                break;
            }
            s.push(b[j]);
            j += 1;
        }
        // 回看：字符串起点之前若只有空白、且空白之前是 '(' → 元组首元素
        let mut k = i;
        while k > 0 && b[k - 1].is_whitespace() {
            k -= 1;
        }
        if k > 0 && b[k - 1] == '(' {
            out.push(s);
        }
        i = j + 1;
    }
    out
}

/// 从 `generate_handler![...]` 里取出注册的命令名。
///
/// 这里是**裸标识符**列表（不是字符串），所以按逗号切分即可。
fn registered_commands() -> BTreeSet<String> {
    let src = include_str!("lib.rs");
    let start = src
        .find("generate_handler![")
        .expect("lib.rs 里找不到 generate_handler![");
    let body = &src[start + "generate_handler![".len()..];
    let end = body.find("])").expect("generate_handler![ 未闭合");
    body[..end]
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && !s.starts_with("//"))
        .map(|s| s.to_string())
        .collect()
}

/// 从 typegen.rs 的 `cmds` 清单里取出命令名（只取 Tauri 命令，不含 daemon）。
fn documented_commands() -> BTreeSet<String> {
    let src = include_str!("../examples/typegen.rs");
    let start = src
        .find("let cmds:")
        .expect("typegen.rs 里找不到 cmds 清单");
    let end = start + src[start..].find("];").expect("cmds 清单未闭合");
    tuple_first_strings(&src[start..end])
        .into_iter()
        // daemon 方法名带点号；这里只留 Tauri 命令名
        .filter(|n| {
            n.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
        .collect()
}

/// daemon 的 JSON-RPC 方法名（来自 crates/daemon 的 dispatch 表）。
fn registered_daemon_methods() -> BTreeSet<String> {
    let src = include_str!("../../../../crates/daemon/src/server.rs");
    let mut out = BTreeSet::new();
    let mut rest = src;
    while let Some(i) = rest.find('"') {
        let after = &rest[i + 1..];
        let Some(j) = after.find('"') else { break };
        let name = &after[..j];
        let tail = &after[j + 1..];
        // dispatch 表每行一个方法：`"x.y" => handle_...`
        let next_line = tail.lines().next().unwrap_or("");
        if next_line.contains("=>") && next_line.contains("handle_") {
            if let Some((head, method)) = name.split_once('.') {
                let valid = !head.is_empty()
                    && head.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                    && !method.is_empty()
                    && method
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_');
                if valid {
                    out.insert(name.to_string());
                }
            }
        }
        rest = tail;
    }
    out
}

/// typegen.rs 的 `daemon_cmds` 清单里的方法名。
fn documented_daemon_methods() -> BTreeSet<String> {
    let src = include_str!("../examples/typegen.rs");
    let start = src
        .find("let daemon_cmds:")
        .expect("typegen.rs 里找不到 daemon_cmds 清单");
    let end = start + src[start..].find("];").expect("daemon_cmds 清单未闭合");
    tuple_first_strings(&src[start..end])
        .into_iter()
        .filter(|n| n.contains('.'))
        .collect()
}

#[test]
fn every_registered_command_is_documented() {
    let registered = registered_commands();
    let documented = documented_commands();
    assert!(
        !registered.is_empty(),
        "解析到的已注册命令为空，解析逻辑坏了"
    );
    let missing: Vec<&String> = registered.difference(&documented).collect();
    assert!(
        missing.is_empty(),
        "已注册但未收录进 examples/typegen.rs 清单的命令（docs/api.md 会漏收）：{missing:?}"
    );
    let extra: Vec<&String> = documented.difference(&registered).collect();
    assert!(
        extra.is_empty(),
        "typegen.rs 清单里存在未注册的命令（generate_handler! 里没有）：{extra:?}"
    );
}

#[test]
fn every_daemon_method_is_documented() {
    let registered = registered_daemon_methods();
    let documented = documented_daemon_methods();
    assert!(
        !registered.is_empty(),
        "解析到的 daemon 方法为空，解析逻辑坏了"
    );
    let missing: Vec<&String> = registered.difference(&documented).collect();
    assert!(
        missing.is_empty(),
        "daemon dispatch 里存在未收录进 typegen.rs 清单的方法：{missing:?}"
    );
    let extra: Vec<&String> = documented.difference(&registered).collect();
    assert!(
        extra.is_empty(),
        "typegen.rs 清单里存在 daemon 未注册的方法：{extra:?}"
    );
}
