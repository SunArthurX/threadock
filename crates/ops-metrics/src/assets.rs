//! 资产清单采集（M6）：扫描各 Agent 的 skills / plugins / 内置技能。
//!
//! | Agent | 来源 |
//! |-------|------|
//! | ZCode | `~/.zcode/skills/*/SKILL.md` + `~/.zcode/cli/plugins/cache/<市场>/<插件>/<版本>` |
//! | Codex | `~/.codex/skills/*/SKILL.md` + `~/.codex/plugins/cache/...` |
//! | Claude Code | `~/.claude/skills/*/SKILL.md` + `installed_plugins.json`（含版本/安装时间） |
//! | MiniMax | `~/.minimax/skills/*` + `.builtin-skills/*`（内置） |
//!
//! 安全扫描：SKILL.md 全文按内置危险命令正则计数 → `risky_hits`。

use crate::OpsResult;
use ch_domain::{AssetRecord, Provider};
use std::path::Path;

/// 危险模式（精简版，与审计引擎同源思想）
fn risky_hits(text: &str) -> i64 {
    let patterns = [
        r"rm\s+-[a-zA-Z]*r",
        r"git\s+push\s+.*(--force|-f\b)",
        r"(curl|wget)\s+[^\n|;]*\|\s*(sudo\s+)?(ba)?sh",
        r"\bsudo\b",
        r"chmod\s+777",
        r"ghp_[A-Za-z0-9]{20,}",
        r"sk-[A-Za-z0-9]{20,}",
    ];
    patterns
        .iter()
        .filter(|p| {
            regex::Regex::new(p)
                .map(|re| re.is_match(text))
                .unwrap_or(false)
        })
        .count() as i64
}

/// 解析 SKILL.md frontmatter 的 name/description（无 frontmatter 用目录名）。
fn parse_skill_md(path: &Path, fallback_name: &str) -> (String, Option<String>, i64) {
    let Ok(body) = std::fs::read_to_string(path) else {
        return (fallback_name.to_string(), None, 0);
    };
    let hits = risky_hits(&body);
    let mut name = fallback_name.to_string();
    let mut desc = None;
    if let Some(rest) = body.strip_prefix("---") {
        if let Some(end) = rest.find("---") {
            let fm = &rest[..end];
            for line in fm.lines() {
                let l = line.trim();
                if let Some(v) = l.strip_prefix("name:") {
                    name = v.trim().trim_matches('"').to_string();
                } else if let Some(v) = l.strip_prefix("description:") {
                    let v = v.trim().trim_matches('"');
                    desc = Some(v.chars().take(120).collect::<String>());
                }
            }
        }
    }
    (name, desc, hits)
}

fn scan_skills_dir(dir: &Path, provider: Provider, kind: &str, out: &mut Vec<AssetRecord>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let dname = e.file_name().to_string_lossy().into_owned();
        let sm = p.join("SKILL.md");
        let (name, desc, hits) = if sm.exists() {
            parse_skill_md(&sm, &dname)
        } else {
            (dname.clone(), None, 0)
        };
        out.push(AssetRecord {
            id: format!("as_{}_{}", provider.as_str(), name),
            provider,
            kind: kind.into(),
            name,
            version: None,
            description: desc,
            risky_hits: hits,
            installed_at: None,
            path: Some(p.to_string_lossy().into_owned()),
        });
    }
}

fn scan_plugins_cache(dir: &Path, provider: Provider, out: &mut Vec<AssetRecord>) {
    // 结构 cache/<市场>/<插件>/<版本>（扁平两层则视为插件目录）
    let Ok(markets) = std::fs::read_dir(dir) else {
        return;
    };
    for m in markets.flatten() {
        let mp = m.path();
        if !mp.is_dir() {
            continue;
        }
        let Ok(plugins) = std::fs::read_dir(&mp) else {
            continue;
        };
        for pl in plugins.flatten() {
            let pp = pl.path();
            if !pp.is_dir() {
                continue;
            }
            let pname = pl.file_name().to_string_lossy().into_owned();
            // 版本目录
            let Ok(vers) = std::fs::read_dir(&pp) else {
                continue;
            };
            let mut version = None;
            for v in vers.flatten() {
                if v.path().is_dir() {
                    version = Some(v.file_name().to_string_lossy().into_owned());
                    break;
                }
            }
            // 版本不存在说明 pp 本身是插件名（无版本层）
            if version.is_none() {
                // pp 可能就是插件（cache/<市场>/<插件>）——检查兄弟结构：
                // 若 mp 直接下有 plugin.json/SKILL.md 则 pp 为插件
                if pp.join("plugin.json").exists() || pp.join("SKILL.md").exists() {
                    version = None;
                } else {
                    continue;
                }
            }
            let hits = std::fs::read_to_string(pp.join("SKILL.md"))
                .map(|b| risky_hits(&b))
                .unwrap_or(0);
            out.push(AssetRecord {
                id: format!("ap_{}_{}", provider.as_str(), pname),
                provider,
                kind: "plugin".into(),
                name: pname,
                version,
                description: None,
                risky_hits: hits,
                installed_at: None,
                path: Some(pp.to_string_lossy().into_owned()),
            });
        }
    }
}

/// 采集一个 Agent 的全部资产。
pub fn collect_agent_assets(home: &Path, provider: Provider) -> OpsResult<Vec<AssetRecord>> {
    let mut out = Vec::new();
    match provider {
        Provider::ZCode => {
            scan_skills_dir(&home.join(".zcode/skills"), provider, "skill", &mut out);
            scan_plugins_cache(&home.join(".zcode/cli/plugins/cache"), provider, &mut out);
        }
        Provider::Codex => {
            scan_skills_dir(&home.join(".codex/skills"), provider, "skill", &mut out);
            scan_plugins_cache(&home.join(".codex/plugins/cache"), provider, &mut out);
        }
        Provider::ClaudeCode => {
            scan_skills_dir(&home.join(".claude/skills"), provider, "skill", &mut out);
            // installed_plugins.json：{plugins: {"name@market":[{version,installedAt,scope}]}}
            if let Ok(txt) =
                std::fs::read_to_string(home.join(".claude/plugins/installed_plugins.json"))
            {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                    if let Some(map) = v.pointer("/plugins").and_then(|p| p.as_object()) {
                        for (full_name, installs) in map {
                            let (name, ver, at) = match installs.as_array().and_then(|a| a.first())
                            {
                                Some(inst) => (
                                    full_name.split('@').next().unwrap_or(full_name).to_string(),
                                    inst.get("version")
                                        .and_then(|x| x.as_str())
                                        .map(|s| s.to_string()),
                                    inst.get("installedAt")
                                        .and_then(|x| x.as_str())
                                        .map(|s| s.to_string()),
                                ),
                                None => (full_name.clone(), None, None),
                            };
                            out.push(AssetRecord {
                                id: format!("ap_claude-code_{name}"),
                                provider,
                                kind: "plugin".into(),
                                name,
                                version: ver,
                                description: None,
                                risky_hits: 0,
                                installed_at: at,
                                path: installs
                                    .as_array()
                                    .and_then(|a| a.first())
                                    .and_then(|i| i.get("installPath"))
                                    .and_then(|x| x.as_str())
                                    .map(|s| s.to_string()),
                            });
                        }
                    }
                }
            }
        }
        Provider::MinimaxCode => {
            scan_skills_dir(&home.join(".minimax/skills"), provider, "skill", &mut out);
            scan_skills_dir(
                &home.join(".minimax/.builtin-skills"),
                provider,
                "builtin_skill",
                &mut out,
            );
        }
        _ => {}
    }
    Ok(out)
}

/// 采集全部 Agent 资产。
pub fn collect_assets(home: impl AsRef<Path>) -> OpsResult<Vec<AssetRecord>> {
    let home = home.as_ref();
    let mut all = Vec::new();
    for p in [
        Provider::ZCode,
        Provider::Codex,
        Provider::ClaudeCode,
        Provider::MinimaxCode,
    ] {
        all.extend(collect_agent_assets(home, p)?);
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_skill_with_frontmatter_and_risky() {
        let dir = tempfile::TempDir::new().unwrap();
        let sk = dir.path().join(".zcode/skills/demo-skill");
        std::fs::create_dir_all(&sk).unwrap();
        std::fs::write(
            sk.join("SKILL.md"),
            "---\nname: demo\ndescription: \"一个演示\"\n---\n\n运行 rm -rf /tmp/x 清理",
        )
        .unwrap();
        let recs = collect_agent_assets(dir.path(), Provider::ZCode).unwrap();
        let s = recs.iter().find(|r| r.name == "demo").unwrap();
        assert_eq!(s.kind, "skill");
        assert_eq!(s.description.as_deref(), Some("一个演示"));
        assert!(s.risky_hits >= 1, "rm -rf 应命中危险模式");
    }

    #[test]
    fn collects_claude_plugins_from_json() {
        let dir = tempfile::TempDir::new().unwrap();
        let pd = dir.path().join(".claude/plugins");
        std::fs::create_dir_all(&pd).unwrap();
        std::fs::write(
            pd.join("installed_plugins.json"),
            r#"{"version":2,"plugins":{"foo@market":[{"version":"1.2.0","installedAt":"2026-08-01T00:00:00Z","installPath":"/x"}]}}"#,
        )
        .unwrap();
        let recs = collect_agent_assets(dir.path(), Provider::ClaudeCode).unwrap();
        let p = recs.iter().find(|r| r.name == "foo").unwrap();
        assert_eq!(p.kind, "plugin");
        assert_eq!(p.version.as_deref(), Some("1.2.0"));
        assert_eq!(p.installed_at.as_deref(), Some("2026-08-01T00:00:00Z"));
    }

    #[test]
    fn minimax_builtin_kind() {
        let dir = tempfile::TempDir::new().unwrap();
        let sk = dir.path().join(".minimax/.builtin-skills/docx");
        std::fs::create_dir_all(&sk).unwrap();
        std::fs::write(sk.join("SKILL.md"), "---\nname: docx\n---\n内容").unwrap();
        let recs = collect_agent_assets(dir.path(), Provider::MinimaxCode).unwrap();
        assert!(recs
            .iter()
            .any(|r| r.kind == "builtin_skill" && r.name == "docx"));
    }
}
