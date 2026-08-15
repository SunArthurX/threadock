//! 自动化/定时任务采集（M8）：
//! - Codex：`~/.codex/automations/*/*/automation.toml`（name/schedule/prompt 摘要）
//! - ZCode：`workflow_definition` 表（builtin/user，enabled/trusted）+ `workflow_run` 最近状态
//! - MiniMax：`~/.minimax/background-tasks/`（目录名即任务，读元数据 json 兜底）

use crate::{open_ro, OpsResult};
use ch_domain::{AutomationRecord, Provider};
use std::path::Path;

/// 轻量 TOML 字段提取（避免引 toml 依赖）：name = "x" / schedule/cron 行。
fn toml_field(txt: &str, key: &str) -> Option<String> {
    for line in txt.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(v) = rest.strip_prefix('=') {
                let v = v.trim().trim_matches('"').trim_matches('\'');
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

fn collect_codex_automations(home: &Path, out: &mut Vec<AutomationRecord>) {
    let base = home.join(".codex/automations");
    let Ok(entries) = std::fs::read_dir(&base) else {
        return;
    };
    for e in entries.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        let Ok(subs) = std::fs::read_dir(e.path()) else {
            continue;
        };
        for s in subs.flatten() {
            let toml = s.path().join("automation.toml");
            if !toml.exists() {
                continue;
            }
            let Ok(txt) = std::fs::read_to_string(&toml) else {
                continue;
            };
            let name = toml_field(&txt, "name")
                .unwrap_or_else(|| s.file_name().to_string_lossy().into_owned());
            let schedule = toml_field(&txt, "schedule")
                .or_else(|| toml_field(&txt, "cron"))
                .or_else(|| toml_field(&txt, "rrule"));
            let prompt = toml_field(&txt, "prompt");
            out.push(AutomationRecord {
                id: format!("au_codex_{name}"),
                provider: Provider::Codex,
                name,
                kind: "cron".into(),
                schedule,
                status: Some("configured".into()),
                detail: prompt.map(|p| p.chars().take(80).collect::<String>()),
            });
        }
    }
}

fn collect_zcode_automations(db: &Path, out: &mut Vec<AutomationRecord>) -> OpsResult<()> {
    if !db.exists() {
        return Ok(());
    }
    let conn = open_ro(db)?;
    // workflow_definition：启用的定义
    let mut stmt =
        conn.prepare("SELECT name, source, enabled, trusted FROM workflow_definition")?;
    let rows = stmt.query_map([], |r| {
        let enabled: i64 = r.get(2)?;
        let trusted: i64 = r.get(3)?;
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            enabled != 0,
            trusted != 0,
        ))
    })?;
    for row in rows {
        let (name, source, enabled, trusted) = row?;
        let status = if enabled {
            if trusted {
                "enabled·trusted"
            } else {
                "enabled·untrusted"
            }
        } else {
            "disabled"
        };
        out.push(AutomationRecord {
            id: format!("au_zcode_{name}"),
            provider: Provider::ZCode,
            name,
            kind: format!("workflow/{source}"),
            schedule: None,
            status: Some(status.into()),
            detail: None,
        });
    }
    // 最近一次 run 状态
    let mut stmt = conn.prepare(
        "SELECT name, status, datetime(MAX(time_created)/1000,'unixepoch','localtime')
         FROM workflow_run GROUP BY name",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (name, status, last) = row?;
        if let Some(rec) = out
            .iter_mut()
            .find(|a| a.provider == Provider::ZCode && a.name == name)
        {
            rec.schedule = Some(format!("last: {last}"));
            rec.status = Some(status);
        } else {
            out.push(AutomationRecord {
                id: format!("au_zcode_{name}"),
                provider: Provider::ZCode,
                name,
                kind: "workflow/run".into(),
                schedule: Some(format!("last: {last}")),
                status: Some(status),
                detail: None,
            });
        }
    }
    Ok(())
}

fn collect_minimax_background(home: &Path, out: &mut Vec<AutomationRecord>) {
    let base = home.join(".minimax/background-tasks");
    let Ok(entries) = std::fs::read_dir(&base) else {
        return;
    };
    for e in entries.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        // 元数据兜底：任一 json 里找 title/status
        let mut title = None;
        let mut status = None;
        if let Ok(subs) = std::fs::read_dir(e.path()) {
            for s in subs.flatten() {
                let p = s.path();
                if p.extension().and_then(|x| x.to_str()) == Some("json") {
                    if let Ok(txt) = std::fs::read_to_string(&p) {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                            title =
                                title.or(v.get("title").and_then(|x| x.as_str()).map(String::from));
                            status = status
                                .or(v.get("status").and_then(|x| x.as_str()).map(String::from));
                        }
                    }
                }
            }
        }
        out.push(AutomationRecord {
            id: format!("au_mm_{name}"),
            provider: Provider::MinimaxCode,
            name: title.unwrap_or(name),
            kind: "background_task".into(),
            schedule: None,
            status,
            detail: None,
        });
    }
}

/// 采集全部自动化任务。
pub fn collect_automations(home: impl AsRef<Path>) -> OpsResult<Vec<AutomationRecord>> {
    let home = home.as_ref();
    let mut out = Vec::new();
    collect_codex_automations(home, &mut out);
    collect_zcode_automations(&home.join(".zcode/cli/db/db.sqlite"), &mut out)?;
    collect_minimax_background(home, &mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_codex_automation_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        let au = dir.path().join(".codex/automations/mkt/hourly-build");
        std::fs::create_dir_all(&au).unwrap();
        std::fs::write(
            au.join("automation.toml"),
            "name = \"hourly-build\"\nschedule = \"0 * * * *\"\nprompt = \"每小时跑一次构建\"",
        )
        .unwrap();
        let recs = collect_automations(dir.path()).unwrap();
        let a = recs.iter().find(|r| r.name == "hourly-build").unwrap();
        assert_eq!(a.schedule.as_deref(), Some("0 * * * *"));
        assert!(a.detail.as_deref().unwrap().contains("构建"));
    }

    #[test]
    fn collects_zcode_workflows() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join(".zcode/cli/db/db.sqlite");
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE workflow_definition (name TEXT, source TEXT, enabled INTEGER, trusted INTEGER);
             CREATE TABLE workflow_run (name TEXT, status TEXT, time_created INTEGER);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO workflow_definition VALUES ('daily','user',1,1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO workflow_run VALUES ('daily','completed',1784560908997)",
            [],
        )
        .unwrap();
        drop(conn);
        let recs = collect_automations(dir.path()).unwrap();
        let a = recs.iter().find(|r| r.name == "daily").unwrap();
        assert_eq!(a.status.as_deref(), Some("completed"));
        assert!(a.schedule.as_deref().unwrap().contains("last:"));
    }
}
