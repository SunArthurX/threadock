//! 规范化工具：路径、Git Remote、名称。匹配前必须先规范化，否则 `foo/` 与 `foo`
//! 会被当成不同项目。

/// 规范化文件路径：去末尾分隔符、去 `.`/`..`、统一正斜杠。
///
/// 不解析符号链接（那需要文件系统访问），只做词法规范化。
/// 这样在不同机器上、不同来源记录的同一路径能稳定匹配。
#[must_use] 
pub fn canonicalize_path(input: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in input.split(['/', '\\']) {
        match seg {
            "" | "." => continue,
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let is_absolute = input.starts_with('/') || input.starts_with('\\');
    let joined = parts.join("/");
    if is_absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

/// 规范化 Git Remote URL：
/// - 统一 HTTPS 与 SSH 形式（去用户名、去协议、去 `.git` 后缀）。
/// - `git@github.com:org/repo.git` → `github.com/org/repo`
/// - `https://github.com/org/repo.git` → `github.com/org/repo`
/// - `https://user@github.com/org/repo` → `github.com/org/repo`
#[must_use] 
pub fn canonicalize_git_remote(input: &str) -> String {
    let s = input.trim();
    if s.is_empty() {
        return String::new();
    }

    // SSH 形式：git@host:org/repo
    if let Some(rest) = s.strip_prefix("git@") {
        // rest = host:org/repo(.git)
        return rest
            .replacen(':', "/", 1)
            .trim_end_matches(".git")
            .to_string();
    }

    // 去协议
    let no_scheme = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .or_else(|| s.strip_prefix("ssh://"))
        .or_else(|| s.strip_prefix("git://"))
        .unwrap_or(s);

    // 去 userinfo（user@host → host）。只处理 @ 出现在第一个 / 之前的情况。
    let no_user = if let Some(at) = no_scheme.find('@') {
        let before_at = &no_scheme[..at];
        if before_at.contains('/') {
            no_scheme
        } else {
            // @ 在首个 / 之前 → userinfo
            &no_scheme[at + 1..]
        }
    } else {
        no_scheme
    };

    // 去 .git 后缀
    no_user.trim_end_matches(".git").to_string()
}

/// 规范化显示名称用于相似度比较：小写、去常见后缀、去空白与标点。
#[must_use] 
pub fn normalized_name(input: &str) -> String {
    let lower = input.to_lowercase();
    lower
        .trim()
        .trim_end_matches("-web")
        .trim_end_matches("-app")
        .trim_end_matches("-server")
        .trim_end_matches("-frontend")
        .trim_end_matches("-backend")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>()
}

/// 简单的名称相似度（0.0~1.0）。基于规范化后字符串的相等与包含关系。
///
/// MVP 不引入编辑距离库，用启发式：
/// - 完全相等 → 1.0
/// - 一个包含另一个（且长度差合理）→ 0.7
/// - 否则 → 0.0
///
/// plan §4.3 明确「名称相似度仅作为低置信度候选」，0.7 已低于
/// `AUTO_CONFIRM_THRESHOLD(0.75)，会触发用户确认`。
#[must_use] 
pub fn name_similarity(a: &str, b: &str) -> f64 {
    let na = normalized_name(a);
    let nb = normalized_name(b);
    if na.is_empty() || nb.is_empty() {
        return 0.0;
    }
    if na == nb {
        return 1.0;
    }
    let (longer, shorter) = if na.len() >= nb.len() {
        (&na, &nb)
    } else {
        (&nb, &na)
    };
    if longer.contains(shorter.as_str()) && shorter.len() >= 3 {
        return 0.7;
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_strips_trailing_slash() {
        assert_eq!(canonicalize_path("/a/b/"), "/a/b");
        assert_eq!(canonicalize_path("/a/b"), "/a/b");
    }

    #[test]
    fn path_resolves_dotdot() {
        assert_eq!(canonicalize_path("/a/b/../c"), "/a/c");
        assert_eq!(canonicalize_path("/a/./b"), "/a/b");
    }

    #[test]
    fn path_handles_backslashes() {
        // 反斜杠统一为正斜杠；盘符作为普通段保留
        assert_eq!(
            canonicalize_path("C:\\Users\\foo\\proj"),
            "C:/Users/foo/proj"
        );
    }

    #[test]
    fn path_relative_keeps_relative() {
        assert_eq!(canonicalize_path("a/b/c"), "a/b/c");
        assert_eq!(canonicalize_path("./a"), "a");
    }

    #[test]
    fn git_remote_ssh_form() {
        assert_eq!(
            canonicalize_git_remote("git@github.com:org/repo.git"),
            "github.com/org/repo"
        );
    }

    #[test]
    fn git_remote_https_form() {
        assert_eq!(
            canonicalize_git_remote("https://github.com/org/repo.git"),
            "github.com/org/repo"
        );
    }

    #[test]
    fn git_remote_with_userinfo() {
        assert_eq!(
            canonicalize_git_remote("https://token@github.com/org/repo"),
            "github.com/org/repo"
        );
    }

    #[test]
    fn git_remote_ssh_protocol() {
        // ssh:// 协议 + userinfo → 去协议去 user，与 HTTPS 归一
        assert_eq!(
            canonicalize_git_remote("ssh://git@github.com/org/repo.git"),
            "github.com/org/repo"
        );
    }

    #[test]
    fn git_remote_same_repo_matches_regardless_of_form() {
        let a = canonicalize_git_remote("git@github.com:org/repo.git");
        let b = canonicalize_git_remote("https://github.com/org/repo");
        assert_eq!(a, b, "SSH and HTTPS forms must canonicalize equal");
    }

    #[test]
    fn name_normalization() {
        // 小写 + 去常见后缀
        assert_eq!(normalized_name("My-Web-App"), "my-web"); // -app 被去
        assert_eq!(normalized_name("ProjWeb"), "projweb");
        assert_eq!(normalized_name("app-server"), "app"); // -server 被去
    }

    #[test]
    fn similarity_identical() {
        assert_eq!(name_similarity("foo", "foo"), 1.0);
    }

    #[test]
    fn similarity_contains() {
        // my-web-app 包含 web-app
        let s = name_similarity("my-web-app", "web-app");
        assert!(s > 0.0, "containment should give >0 similarity");
        assert!(s < 1.0);
    }

    #[test]
    fn similarity_disjoint() {
        assert_eq!(name_similarity("aaa", "zzz"), 0.0);
    }

    #[test]
    fn similarity_empty() {
        assert_eq!(name_similarity("", "foo"), 0.0);
    }
}
