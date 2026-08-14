//! 数据库迁移，对应 plan §12.4。
//!
//! 原则：
//! - 严格顺序版本。
//! - 可重入（IF NOT EXISTS）。
//! - 失败不提交，旧版本保持可用。
//! - 启动时由 Repository 调用 [`migrate_to_latest`]。

use crate::error::{StorageError, StorageResult};
use ch_domain::now_utc;
use rusqlite::Connection;

/// 当前 schema 目标版本。
pub const LATEST_VERSION: u32 = 6;

/// 一个迁移步骤：版本号 + 描述 + SQL。
struct Migration {
    version: u32,
    description: &'static str,
    sql: &'static str,
}

/// 已知的全部迁移，按版本升序。
fn migrations() -> Vec<Migration> {
    vec![
        Migration {
            version: 1,
            description: "initial schema (v1)",
            sql: crate::schema::SCHEMA_V1,
        },
        Migration {
            version: 2,
            description: "favorite, tags, archive (plan §6.3/§6.4/§6.5)",
            sql: crate::schema::SCHEMA_V2,
        },
        Migration {
            version: 3,
            description: "knowledge extractions persistence (plan §13.5)",
            sql: crate::schema::SCHEMA_V3,
        },
        Migration {
            version: 4,
            description: "custom redaction rules (plan §14.6)",
            sql: crate::schema::SCHEMA_V4,
        },
        Migration {
            version: 5,
            description: "conversation parent/child relationship (source_parent_id)",
            sql: crate::schema::SCHEMA_V5,
        },
        Migration {
            version: 6,
            description: "CodeAgentOps metrics (usage_records / tool_call_records)",
            sql: crate::schema::SCHEMA_V6,
        },
    ]
}

/// 读取当前已应用的 schema 版本；空库返回 0。
pub fn current_version(conn: &Connection) -> StorageResult<u32> {
    // schema_version 表本身由 V1 创建；若不存在说明库为空。
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if table_exists == 0 {
        return Ok(0);
    }
    let v: Option<i64> = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |r| {
            r.get(0)
        })
        .unwrap_or(None);
    Ok(v.map(|v| v as u32).unwrap_or(0))
}

/// 把库迁移到 `LATEST_VERSION`。可重入。
pub fn migrate_to_latest(conn: &mut Connection) -> StorageResult<()> {
    migrate_to(conn, LATEST_VERSION)
}

/// 把库迁移到指定目标版本（必须 >= 当前版本）。
pub fn migrate_to(conn: &mut Connection, target: u32) -> StorageResult<()> {
    let mut current = current_version(conn)?;
    if target < current {
        return Err(StorageError::Migration {
            version: target,
            reason: format!(
                "target version {target} is lower than current {current}; downgrade not supported"
            ),
        });
    }
    if current == target {
        return Ok(());
    }

    for m in migrations() {
        if m.version <= current {
            continue;
        }
        apply_migration(conn, &m)?;
        current = m.version;
    }
    Ok(())
}

fn apply_migration(conn: &mut Connection, m: &Migration) -> StorageResult<()> {
    let tx = conn
        .transaction()
        .map_err(|e| StorageError::Migration {
            version: m.version,
            reason: format!("begin tx: {e}"),
        })?;

    // 逐条执行 SQL 语句（按 ; 分割），允许注释和空段
    for stmt in split_sql_statements(m.sql) {
        if stmt.trim().is_empty() {
            continue;
        }
        tx.execute(&stmt, []).map_err(|e| StorageError::Migration {
            version: m.version,
            reason: format!("statement failed: {e}\nstmt: {stmt}"),
        })?;
    }

    // 记录版本
    tx.execute(
        "INSERT INTO schema_version (version, applied_at) VALUES (?1, ?2)",
        rusqlite::params![m.version as i64, crate::timestamp::to_millis(Some(now_utc())).unwrap()],
    )
    .map_err(|e| StorageError::Migration {
        version: m.version,
        reason: format!("record version: {e}"),
    })?;

    tx.commit().map_err(|e| StorageError::Migration {
        version: m.version,
        reason: format!("commit: {e}"),
    })?;
    tracing::info!(version = m.version, desc = m.description, "migration applied");
    Ok(())
}

/// 把含注释的 SQL 脚本拆成单条语句。
///
/// 规则：
/// - `--` 到行尾为行注释，丢弃。
/// - `'...'` 为字符串字面量，内部字符忽略。
/// - `BEGIN ... END` 块（触发器体）内的 `;` 不切分；只在外层 `;` 处断句。
fn split_sql_statements(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut in_string = false;
    let mut depth = 0u32; // BEGIN/END 嵌套深度

    let bytes: Vec<char> = sql.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];

        // 行注释 --...（仅字符串外）
        if !in_string && c == '-' && i + 1 < bytes.len() && bytes[i + 1] == '-' {
            // 跳到行尾
            while i < bytes.len() && bytes[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // 字符串字面量
        if c == '\'' {
            in_string = !in_string;
            buf.push(c);
            i += 1;
            continue;
        }

        if !in_string {
            // 识别 BEGIN / END 关键字（大小写不敏感，词边界）
            if matches_keyword(&bytes, i, "begin") {
                depth += 1;
                buf.push_str("BEGIN");
                i += 5; // "begin".len()
                continue;
            }
            if matches_keyword(&bytes, i, "end") {
                if depth > 0 {
                    depth = depth.saturating_sub(1);
                }
                buf.push_str("END");
                i += 3; // "end".len()
                continue;
            }

            // 块外的分号 → 断句
            if c == ';' && depth == 0 {
                let stmt = buf.trim().to_string();
                if !stmt.is_empty() {
                    out.push(stmt);
                }
                buf.clear();
                i += 1;
                continue;
            }
        }

        buf.push(c);
        i += 1;
    }
    let tail = buf.trim().to_string();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

/// 判断 `bytes[pos..]` 是否以关键词 `kw` 开头（大小写不敏感），且前后是词边界。
fn matches_keyword(bytes: &[char], pos: usize, kw: &str) -> bool {
    let kw_chars: Vec<char> = kw.chars().collect();
    if pos + kw_chars.len() > bytes.len() {
        return false;
    }
    for (j, kc) in kw_chars.iter().enumerate() {
        if bytes[pos + j].to_ascii_lowercase() != *kc {
            return false;
        }
    }
    // 前一个字符必须是词边界（非字母数字下划线）
    if pos > 0 {
        let prev = bytes[pos - 1];
        if prev.is_alphanumeric() || prev == '_' {
            return false;
        }
    }
    // 后一个字符必须是词边界
    let after_idx = pos + kw_chars.len();
    if after_idx < bytes.len() {
        let after = bytes[after_idx];
        if after.is_alphanumeric() || after == '_' {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn
    }

    #[test]
    fn empty_db_has_version_zero() {
        let conn = fresh_conn();
        assert_eq!(current_version(&conn).unwrap(), 0);
    }

    #[test]
    fn migrate_creates_tables_and_sets_version() {
        let mut conn = fresh_conn();
        migrate_to_latest(&mut conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), LATEST_VERSION);

        // 抽查几张关键表存在
        for table in [
            "providers",
            "installations",
            "workspaces",
            "source_workspaces",
            "conversations",
            "turns",
            "messages",
            "events",
            "sync_cursors",
            "audit_logs",
            "conversation_tags", // V2
            "knowledge_extractions", // V3
            "redaction_rules",       // V4
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "table {table} should exist");
        }

        // FTS5 虚拟表与触发器
        for obj in [
            "messages_fts",
            "messages_ai_fts",
            "messages_ad_fts",
            "messages_au_fts",
        ] {
            let kind = if obj == "messages_fts" { "table" } else { "trigger" };
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type=? AND name=?",
                    rusqlite::params![kind, obj],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "{kind} {obj} should exist");
        }
    }

    #[test]
    fn migrate_is_idempotent() {
        let mut conn = fresh_conn();
        migrate_to_latest(&mut conn).unwrap();
        // 再跑一次不应报错
        migrate_to_latest(&mut conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), LATEST_VERSION);
    }

    #[test]
    fn downgrade_is_rejected() {
        let mut conn = fresh_conn();
        migrate_to_latest(&mut conn).unwrap();
        let err = migrate_to(&mut conn, 0).unwrap_err();
        assert!(matches!(err, StorageError::Migration { .. }));
    }

    #[test]
    fn split_sql_handles_comments_and_blanks() {
        let stmts = split_sql_statements(
            "-- a comment\nCREATE TABLE x (a INT); -- trailing\n\nSELECT 1;",
        );
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].contains("CREATE TABLE x"));
        assert!(!stmts[0].contains("--"));
    }
}
