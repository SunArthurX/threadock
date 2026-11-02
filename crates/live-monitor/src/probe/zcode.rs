//! ZCode 实时探测：`~/.zcode/cli/db/db.sqlite`（WAL）。
//! 运行态优先用 `turn_usage.status='running'`（源侧显式信号，精确），
//! 兜底 `session.time_updated` 新鲜度；进度取 `todo` 表与消息数。

use crate::types::{LiveActivity, LiveProgress, LiveSource, LiveState, LiveTask};
use crate::{classify, home_dir, snippet, today_start_ms};
use ch_adapter_sdk::open_readonly;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::path::Path;

pub fn scan(now_ms: i64) -> Result<Vec<LiveTask>, String> {
    scan_window(
        &home_dir()?.join(".zcode/cli/db/db.sqlite"),
        now_ms,
        today_start_ms(now_ms),
    )
}

/// 指定库 + 窗口起点扫描（测试注入用；`since_ms` 通常为本地零点）。
pub fn scan_window(db_path: &Path, now_ms: i64, since_ms: i64) -> Result<Vec<LiveTask>, String> {
    if !db_path.is_file() {
        return Ok(vec![]);
    }
    let conn = open_readonly(db_path).map_err(|e| format!("open zcode db: {e}"))?;
    let cutoff = since_ms;
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.title, s.directory, s.time_created,
                    MAX(s.time_updated,
                        COALESCE((SELECT MAX(c.time_updated) FROM session c
                                  WHERE c.parent_id = s.id), s.time_updated)) AS eff_updated
             FROM session s
             WHERE (s.parent_id IS NULL OR s.parent_id = '') AND s.time_updated > ?1
             ORDER BY eff_updated DESC
             LIMIT 200",
        )
        .map_err(|e| format!("prepare zcode: {e}"))?;
    let rows = stmt
        .query_map(params![cutoff], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })
        .map_err(|e| format!("query zcode: {e}"))?;
    let mut tasks = Vec::new();
    for row in rows {
        let (session_id, title, directory, time_created, eff_updated) =
            row.map_err(|e| format!("zcode row: {e}"))?;
        let running_turns = family_running_turns(&conn, &session_id);
        let state = if running_turns > 0 {
            LiveState::Running
        } else {
            classify(now_ms, eff_updated)
        };
        let activity = last_activity(&conn, &session_id);
        let progress = family_progress(&conn, &session_id);
        tasks.push(LiveTask {
            source: LiveSource::ZCode,
            session_id,
            title: (!title.is_empty()).then_some(title),
            workspace: (!directory.is_empty()).then_some(directory),
            originator: None,
            started_at_ms: Some(time_created),
            last_activity_ms: eff_updated,
            state,
            activity,
            progress: Some(progress),
        });
    }
    Ok(tasks)
}

/// 主任务 + 子任务家族中 running 状态的 turn 数。
fn family_running_turns(conn: &Connection, session_id: &str) -> u32 {
    conn.query_row(
        "SELECT count(*) FROM turn_usage
         WHERE status = 'running'
           AND session_id IN (SELECT id FROM session WHERE id = ?1 OR parent_id = ?1)",
        params![session_id],
        |r| r.get::<_, i64>(0),
    )
    .unwrap_or(0) as u32
}

/// 家族进度：todo 完成 / 总数 + 消息数。
fn family_progress(conn: &Connection, session_id: &str) -> LiveProgress {
    let mut p = LiveProgress::default();
    if let Ok((total, done)) = conn.query_row(
        "SELECT count(*), COALESCE(SUM(status = 'completed'), 0) FROM todo WHERE session_id = ?1",
        params![session_id],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
    ) {
        p.todos_total = u32::try_from(total).ok();
        p.todos_done = u32::try_from(done).ok();
    }
    if let Ok(n) = conn.query_row(
        "SELECT count(*) FROM message
         WHERE session_id IN (SELECT id FROM session WHERE id = ?1 OR parent_id = ?1)",
        params![session_id],
        |r| r.get::<_, i64>(0),
    ) {
        p.message_count = Some(n);
    }
    p
}

/// 家族最近一条消息 → 活动摘要。
#[must_use]
fn last_activity(conn: &Connection, session_id: &str) -> Option<LiveActivity> {
    let (msg_id, msg_time, msg_data): (String, i64, String) = conn
        .query_row(
            "SELECT m.id, m.time_created, m.data FROM message m
             WHERE m.session_id IN (SELECT id FROM session WHERE id = ?1 OR parent_id = ?1)
             ORDER BY m.time_created DESC, m.id DESC
             LIMIT 1",
            params![session_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok()?;
    let data: Value = serde_json::from_str(&msg_data).unwrap_or_default();
    let role = data.get("role").and_then(Value::as_str).unwrap_or("");
    // 取该消息最后一个可展示 part（跳过 system-reminder 注入等）
    let mut stmt = conn
        .prepare("SELECT data FROM part WHERE message_id = ?1 ORDER BY sequence DESC")
        .ok()?;
    let parts: Vec<String> = stmt
        .query_map(params![msg_id], |r| r.get::<_, String>(0))
        .ok()?
        .filter_map(std::result::Result::ok)
        .collect();
    displayable_part(parts.iter().rev(), role).map(|mut act| {
        act.at_ms = msg_time;
        act
    })
}

/// transcript：家族最近 `limit` 条活动（时间升序）。
#[must_use]
pub fn transcript(db_path: &Path, session_id: &str, limit: usize) -> Vec<LiveActivity> {
    let Ok(conn) = open_readonly(db_path) else {
        return vec![];
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT m.id, m.time_created, m.data FROM message m
         WHERE m.session_id IN (SELECT id FROM session WHERE id = ?1 OR parent_id = ?1)
         ORDER BY m.time_created DESC, m.id DESC
         LIMIT 200",
    ) else {
        return vec![];
    };
    let Ok(rows) = stmt.query_map(params![session_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
        ))
    }) else {
        return vec![];
    };
    // 消息按新→旧遍历；每组 msg_acts 内部旧→新。
    let mut groups: Vec<Vec<LiveActivity>> = Vec::new();
    for row in rows.filter_map(std::result::Result::ok) {
        let (msg_id, msg_time, msg_data) = row;
        let data: Value = serde_json::from_str(&msg_data).unwrap_or_default();
        let role = data.get("role").and_then(Value::as_str).unwrap_or("");
        let Ok(mut pstmt) =
            conn.prepare("SELECT data FROM part WHERE message_id = ?1 ORDER BY sequence DESC")
        else {
            break;
        };
        let Ok(parts) = pstmt.query_map(params![msg_id], |r| r.get::<_, String>(0)) else {
            break;
        };
        let part_list: Vec<String> = parts.filter_map(std::result::Result::ok).collect();
        // 消息内 parts 正序展开（每条消息至多 8 个 part，防单消息刷屏）
        let mut msg_acts: Vec<LiveActivity> = Vec::new();
        for p in part_list.iter().rev() {
            if msg_acts.len() >= 8 {
                break;
            }
            if let Some(act) = displayable_part(std::iter::once(p), role) {
                msg_acts.push(LiveActivity {
                    at_ms: msg_time,
                    ..act
                });
            }
        }
        if !msg_acts.is_empty() {
            groups.push(msg_acts);
        }
        // 已收集的活动数足够覆盖 limit（组间新→旧，前面的组一定是更新的）
        if groups.iter().map(Vec::len).sum::<usize>() >= limit {
            break;
        }
    }
    // 组间翻转为旧→新后拉平，取最后 limit 条
    groups.reverse();
    let flat: Vec<LiveActivity> = groups.into_iter().flatten().collect();
    let start = flat.len().saturating_sub(limit);
    flat[start..].to_vec()
}

/// 单个 part → 可展示活动；不可展示返回 None（跳过 system-reminder 注入等）。
#[must_use]
fn displayable_part<'a, I>(parts: I, msg_role: &str) -> Option<LiveActivity>
where
    I: Iterator<Item = &'a String>,
{
    for part_json in parts {
        let Ok(obj) = serde_json::from_str::<Value>(part_json) else {
            continue;
        };
        match obj.get("type").and_then(Value::as_str).unwrap_or("") {
            "text" => {
                let text = obj.get("text").and_then(Value::as_str).unwrap_or("");
                let trimmed = text.trim();
                if trimmed.is_empty() || trimmed.starts_with("<system-reminder>") {
                    continue;
                }
                return Some(LiveActivity {
                    kind: "message".into(),
                    role: (!msg_role.is_empty()).then(|| msg_role.to_string()),
                    snippet: snippet(trimmed, 160),
                    at_ms: 0,
                });
            }
            "reasoning" => {
                let text = obj.get("text").and_then(Value::as_str).unwrap_or("").trim();
                if text.is_empty() {
                    continue;
                }
                return Some(LiveActivity {
                    kind: "thinking".into(),
                    role: Some("assistant".into()),
                    snippet: snippet(text, 160),
                    at_ms: 0,
                });
            }
            "tool" => {
                let name = obj.get("tool").and_then(Value::as_str).unwrap_or("tool");
                let state = obj.get("state").unwrap_or(&Value::Null);
                let running = state.get("status").and_then(Value::as_str) == Some("running");
                let input = state.get("input").unwrap_or(&Value::Null);
                let arg = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("");
                let body = match name {
                    "Bash" => {
                        let cmd = arg("command");
                        if cmd.is_empty() {
                            "Bash".to_string()
                        } else {
                            cmd.lines().next().unwrap_or("").trim().to_string()
                        }
                    }
                    "Read" => format!("读取 {}", basename(arg("file_path"))),
                    "Write" => format!("写入 {}", basename(arg("file_path"))),
                    "Edit" | "MultiEdit" | "NotebookEdit" => {
                        format!("修改 {}", basename(arg("file_path")))
                    }
                    "TodoWrite" => "更新 TODO".to_string(),
                    _ => format!("工具 {name}"),
                };
                return Some(LiveActivity {
                    kind: "tool".into(),
                    role: Some("assistant".into()),
                    snippet: snippet(&if running { format!("▶ {body}") } else { body }, 160),
                    at_ms: 0,
                });
            }
            _ => {}
        }
    }
    None
}

fn basename(p: &str) -> String {
    p.rsplit('/').next().unwrap_or(p).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建最小 zcode schema（列与真实库查询用到的对齐）。
    fn create_db(dir: &Path) -> Connection {
        let db = dir.join("db.sqlite");
        let conn = Connection::open(&db).expect("open");
        conn.execute_batch(
            r"
            CREATE TABLE session (
                id TEXT PRIMARY KEY, project_id TEXT NOT NULL DEFAULT '',
                parent_id TEXT, directory TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                time_created INTEGER NOT NULL DEFAULT 0,
                time_updated INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL DEFAULT 0,
                time_updated INTEGER NOT NULL DEFAULT 0,
                data TEXT NOT NULL, sequence INTEGER
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY, message_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL DEFAULT 0,
                time_updated INTEGER NOT NULL DEFAULT 0,
                data TEXT NOT NULL, sequence INTEGER
            );
            CREATE TABLE turn_usage (
                session_id TEXT NOT NULL, turn_id TEXT NOT NULL,
                status TEXT NOT NULL, started_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE todo (
                session_id TEXT NOT NULL, content TEXT NOT NULL,
                status TEXT NOT NULL, priority TEXT NOT NULL DEFAULT '',
                position INTEGER NOT NULL, time_created INTEGER NOT NULL DEFAULT 0,
                time_updated INTEGER NOT NULL DEFAULT 0
            );
            ",
        )
        .expect("schema");
        conn
    }

    #[test]
    fn detects_running_turn_todo_and_activity() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = 1_793_618_000_000i64;
        {
            let conn = create_db(dir.path());
            conn.execute(
                "INSERT INTO session (id, title, directory, time_created, time_updated)
                 VALUES ('s1','实时会话','/tmp/proj', ?1, ?2)",
                params![now - 60_000, now - 5_000],
            )
            .expect("insert session");
            conn.execute(
                "INSERT INTO turn_usage (session_id, turn_id, status, started_at)
                 VALUES ('s1','t1','running', ?1)",
                params![now - 10_000],
            )
            .expect("insert turn");
            conn.execute_batch(
                r#"
                INSERT INTO todo VALUES ('s1','任务A','completed','high',0,0,0);
                INSERT INTO todo VALUES ('s1','任务B','pending','high',1,0,0);
                INSERT INTO message VALUES ('m1','s1',100,100,'{"role":"user"}',0);
                INSERT INTO part VALUES ('p1','m1','s1',100,100,'{"type":"text","text":"开始干活"}',0);
                INSERT INTO message VALUES ('m2','s1',200,200,'{"role":"assistant"}',1);
                INSERT INTO part VALUES ('p2','m2','s1',200,200,'{"type":"text","text":"<system-reminder>注入</system-reminder>"}',0);
                INSERT INTO part VALUES ('p3','m2','s1',200,200,'{"type":"tool","callID":"c1","tool":"Bash","state":{"status":"running","input":{"command":"cargo test --release"}}}',1);
                "#,
            )
            .expect("insert messages");
        }
        let tasks =
            scan_window(&dir.path().join("db.sqlite"), now, now - 30 * 60_000).expect("scan");
        assert_eq!(tasks.len(), 1);
        let t = &tasks[0];
        assert_eq!(t.state, LiveState::Running, "running turn → Running");
        let act = t.activity.as_ref().expect("activity");
        assert_eq!(act.kind, "tool");
        assert!(act.snippet.contains("cargo test"), "{:?}", act.snippet);
        assert!(act.snippet.starts_with("▶"), "running 工具应带 ▶ 标记");
        let p = t.progress.as_ref().expect("progress");
        assert_eq!(p.todos_done, Some(1));
        assert_eq!(p.todos_total, Some(2));
        assert_eq!(p.message_count, Some(2));
    }

    #[test]
    fn freshness_fallback_without_running_turn() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = 1_793_618_000_000i64;
        {
            let conn = create_db(dir.path());
            // 5 分钟前更新 → Idle
            conn.execute(
                "INSERT INTO session (id, title, directory, time_created, time_updated)
                 VALUES ('s2','空闲会话','/tmp/proj2', ?1, ?2)",
                params![now - 600_000, now - 300_000],
            )
            .expect("insert");
        }
        let tasks =
            scan_window(&dir.path().join("db.sqlite"), now, now - 30 * 60_000).expect("scan");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].state, LiveState::Idle);
        assert!(tasks[0].activity.is_none());
    }

    #[test]
    fn transcript_returns_ascending_items() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = 1_793_618_000_000i64;
        {
            let conn = create_db(dir.path());
            conn.execute(
                "INSERT INTO session (id, title, directory, time_created, time_updated)
                 VALUES ('s1','t','/p', ?1, ?2)",
                params![now - 60_000, now - 1_000],
            )
            .expect("insert");
            conn.execute_batch(
                "INSERT INTO message VALUES ('m1','s1',100,100,'{\"role\":\"user\"}',0);
                 INSERT INTO part VALUES ('p1','m1','s1',100,100,'{\"type\":\"text\",\"text\":\"第一条\"}',0);
                 INSERT INTO message VALUES ('m2','s1',200,200,'{\"role\":\"assistant\"}',1);
                 INSERT INTO part VALUES ('p2','m2','s1',200,200,'{\"type\":\"text\",\"text\":\"回复\"}',0);",
            )
            .expect("insert messages");
        }
        let items = transcript(&dir.path().join("db.sqlite"), "s1", 10);
        assert_eq!(items.len(), 2, "时间升序");
        assert_eq!(items[0].snippet, "第一条");
        assert_eq!(items[1].snippet, "回复");
    }

    #[test]
    fn today_window_includes_older_sessions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = 1_793_618_000_000i64;
        {
            let conn = create_db(dir.path());
            // 2 小时前更新 → since=3 小时前时应上板为 Today
            conn.execute(
                "INSERT INTO session (id, title, directory, time_created, time_updated)
                 VALUES ('s3','早上的会话','/tmp/proj3', ?1, ?2)",
                params![now - 3 * 3_600_000, now - 2 * 3_600_000],
            )
            .expect("insert");
        }
        let tasks =
            scan_window(&dir.path().join("db.sqlite"), now, now - 3 * 3_600_000).expect("scan");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].state, LiveState::Today);
    }

    #[test]
    fn missing_db_is_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tasks = scan_window(&dir.path().join("nope.sqlite"), 0, 0).expect("scan");
        assert!(tasks.is_empty());
    }
}
