//! 自动化/定时任务采集（M8）：
//! - Codex：`~/.codex/automations/*/*/automation.toml`（name/schedule/prompt 摘要）
//! - `ZCode`：`workflow_definition` 表（builtin/user，enabled/trusted）+ `workflow_run` 最近状态
//! - `MiniMax`：`~/.minimax/background-tasks/`（目录名即任务，读元数据 json 兜底）

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

/// 文件在 `secs` 秒内被写过（运行中的后台任务会持续追加 output.log）。
fn recently_modified(path: &Path, secs: u64) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() < secs)
}

/// MiniMax 后台任务目录 → 摘要首行（给 `bg_<uuid>` 这类名字一条可读线索）。
fn summary_first_line(dir: &Path) -> Option<String> {
    let txt = std::fs::read_to_string(dir.join("summary.txt")).ok()?;
    txt.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(60).collect::<String>())
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
        let dir = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        // 元数据兜底：任一 json 里找 title/status
        let mut title = None;
        let mut status = None;
        if let Ok(subs) = std::fs::read_dir(&dir) {
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
        // 无元数据时按产物推断（2026-08 用户反馈：174 个历史任务全被当成「进行中」）：
        // summary.txt 只有任务终结才产出 → finished；
        // 无 summary 但 output.log 15 分钟内还在写 → running；否则视为已结束
        if status.is_none() {
            status = Some(
                if dir.join("summary.txt").exists() {
                    "finished"
                } else if recently_modified(&dir.join("output.log"), 15 * 60) {
                    "running"
                } else {
                    "finished"
                }
                .into(),
            );
        }
        let detail = summary_first_line(&dir);
        out.push(AutomationRecord {
            id: format!("au_mm_{name}"),
            provider: Provider::MinimaxCode,
            name: title.unwrap_or(name),
            kind: "background_task".into(),
            schedule: None,
            status,
            detail,
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
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let au = dir.path().join(".codex/automations/mkt/hourly-build");
        std::fs::create_dir_all(&au).expect("file I/O failed");
        std::fs::write(
            au.join("automation.toml"),
            "name = \"hourly-build\"\nschedule = \"0 * * * *\"\nprompt = \"每小时跑一次构建\"",
        )
        .expect("unexpected None");
        let recs = collect_automations(dir.path()).expect("unexpected None");
        let a = recs
            .iter()
            .find(|r| r.name == "hourly-build")
            .expect("unexpected None");
        assert_eq!(a.schedule.as_deref(), Some("0 * * * *"));
        assert!(a
            .detail
            .as_deref()
            .expect("unexpected None")
            .contains("构建"));
    }

    #[test]
    fn collects_zcode_workflows() {
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let db = dir.path().join(".zcode/cli/db/db.sqlite");
        std::fs::create_dir_all(db.parent().expect("file I/O failed")).expect("file I/O failed");
        let conn = rusqlite::Connection::open(&db).expect("database connection failed");
        conn.execute_batch(
            "CREATE TABLE workflow_definition (name TEXT, source TEXT, enabled INTEGER, trusted INTEGER);
             CREATE TABLE workflow_run (name TEXT, status TEXT, time_created INTEGER);",
        )
        .expect("unexpected None");
        conn.execute(
            "INSERT INTO workflow_definition VALUES ('daily','user',1,1)",
            [],
        )
        .expect("unexpected None");
        conn.execute(
            "INSERT INTO workflow_run VALUES ('daily','completed',1784560908997)",
            [],
        )
        .expect("unexpected None");
        drop(conn);
        let recs = collect_automations(dir.path()).expect("unexpected None");
        let a = recs
            .iter()
            .find(|r| r.name == "daily")
            .expect("unexpected None");
        assert_eq!(a.status.as_deref(), Some("completed"));
        assert!(a
            .schedule
            .as_deref()
            .expect("unexpected None")
            .contains("last:"));
    }

    fn mm_task(dir: &Path, name: &str) -> std::path::PathBuf {
        let t = dir.join(".minimax/background-tasks").join(name);
        std::fs::create_dir_all(&t).expect("mkdir failed");
        t
    }

    #[test]
    fn minimax_summary_infers_finished_with_detail() {
        // 回归：174 个历史任务（仅 output.log + summary.txt）曾被全部当成「进行中」
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let t = mm_task(dir.path(), "bg_done");
        // output.log 刚写（模拟仍在写）也不影响：summary 存在即终结
        std::fs::write(t.join("output.log"), "writing…").expect("write failed");
        std::fs::write(t.join("summary.txt"), "\n汇报当前进度：Day11 全部补完\n")
            .expect("write failed");
        let recs = collect_automations(dir.path()).expect("unexpected None");
        let a = recs
            .iter()
            .find(|r| r.name == "bg_done")
            .expect("unexpected None");
        assert_eq!(a.status.as_deref(), Some("finished"));
        assert_eq!(a.detail.as_deref(), Some("汇报当前进度：Day11 全部补完"));
    }

    #[test]
    fn minimax_fresh_log_without_summary_infers_running() {
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let t = mm_task(dir.path(), "bg_live");
        std::fs::write(t.join("output.log"), "just started").expect("write failed");
        let recs = collect_automations(dir.path()).expect("unexpected None");
        let a = recs
            .iter()
            .find(|r| r.name == "bg_live")
            .expect("unexpected None");
        assert_eq!(a.status.as_deref(), Some("running"));
    }

    #[test]
    fn minimax_stale_folder_infers_finished() {
        // 空目录（无 summary、无 json、log 缺失/陈旧）→ 不再误判为活动
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        mm_task(dir.path(), "bg_old");
        let recs = collect_automations(dir.path()).expect("unexpected None");
        let a = recs
            .iter()
            .find(|r| r.name == "bg_old")
            .expect("unexpected None");
        assert_eq!(a.status.as_deref(), Some("finished"));
    }

    #[test]
    fn minimax_json_status_takes_priority() {
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let t = mm_task(dir.path(), "bg_json");
        std::fs::write(
            t.join("meta.json"),
            r#"{"title":"定时巡检","status":"running"}"#,
        )
        .expect("write failed");
        std::fs::write(t.join("summary.txt"), "已产出").expect("write failed");
        let recs = collect_automations(dir.path()).expect("unexpected None");
        let a = recs
            .iter()
            .find(|r| r.name == "定时巡检")
            .expect("unexpected None");
        assert_eq!(
            a.status.as_deref(),
            Some("running"),
            "json 元数据优先于产物推断"
        );
    }

    /// 真实 HOME 验证：历史任务目录全部推断出明确状态（不再 NULL）。
    /// cargo test -p ch-ops-metrics minimax_real -- --ignored --nocapture
    #[test]
    #[ignore = "读取本机真实 ~/.minimax / ~/.codex / ~/.zcode"]
    fn minimax_real_home_statuses() {
        let home = std::env::var("HOME").expect("no HOME");
        let recs = collect_automations(&home).expect("collect failed");
        let mut by_status: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::default();
        for r in &recs {
            *by_status
                .entry(r.status.clone().unwrap_or_else(|| "(null)".into()))
                .or_default() += 1;
        }
        println!("total={} by_status={:?}", recs.len(), by_status);
        assert!(
            recs.iter().all(|r| r.status.is_some()),
            "所有任务必须有明确状态（json 元数据或产物推断）"
        );
        let detail_hits = recs
            .iter()
            .filter(|r| r.provider == Provider::MinimaxCode && r.detail.is_some())
            .count();
        println!("minimax detail(摘要首行) 覆盖 {detail_hits} 条");
    }
}
