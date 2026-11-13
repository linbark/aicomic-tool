//! Explicit, transactional migrations for the research CLI.
//!
//! A migration never runs implicitly against the user's default database. The CLI
//! requires an explicit path and creates a consistent SQLite backup first.

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const LATEST_SCHEMA: u32 = 26;
const BASE: &str = include_str!("../../../../docs/design/inspiration/schema.v2.sql");

const MIGRATIONS: &[(u32, &str)] = &[
    (
        3,
        include_str!("../../../../docs/design/inspiration/migrations/002-personal-layer.sql"),
    ),
    (
        4,
        include_str!("../../../../docs/design/inspiration/migrations/003-work-vector.sql"),
    ),
    (
        5,
        include_str!("../../../../docs/design/inspiration/migrations/004-observation-unique.sql"),
    ),
    (
        6,
        include_str!("../../../../docs/design/inspiration/migrations/005-primitive-evidence.sql"),
    ),
    (
        7,
        include_str!("../../../../docs/design/inspiration/migrations/006-alchemy.sql"),
    ),
    (
        8,
        include_str!("../../../../docs/design/inspiration/migrations/007-builtin-techniques.sql"),
    ),
    (
        9,
        include_str!("../../../../docs/design/inspiration/migrations/008-extract-version.sql"),
    ),
    (
        10,
        include_str!("../../../../docs/design/inspiration/migrations/009-layout-array-schema.sql"),
    ),
    (
        11,
        include_str!("../../../../docs/design/inspiration/migrations/010-donts-technique.sql"),
    ),
    (
        12,
        include_str!("../../../../docs/design/inspiration/migrations/011-slot-polarity.sql"),
    ),
    (
        13,
        include_str!("../../../../docs/design/inspiration/migrations/012-judge-pipeline.sql"),
    ),
    (
        14,
        include_str!("../../../../docs/design/inspiration/migrations/013-pill-check.sql"),
    ),
    (
        15,
        include_str!("../../../../docs/design/inspiration/migrations/014-typed-tokens.sql"),
    ),
    (
        16,
        include_str!("../../../../docs/design/inspiration/migrations/015-layout-enum.sql"),
    ),
    (
        17,
        include_str!("../../../../docs/design/inspiration/migrations/016-donts-abstract.sql"),
    ),
    (
        18,
        include_str!("../../../../docs/design/inspiration/migrations/017-spacing-measured.sql"),
    ),
    (
        19,
        include_str!("../../../../docs/design/inspiration/migrations/018-states-technique.sql"),
    ),
    (
        20,
        include_str!("../../../../docs/design/inspiration/migrations/019-motion-technique.sql"),
    ),
    (
        21,
        include_str!("../../../../docs/design/inspiration/migrations/020-judge-model.sql"),
    ),
    (
        22,
        include_str!("../../../../docs/design/inspiration/migrations/021-immutable-extracts.sql"),
    ),
    (
        23,
        include_str!("../../../../docs/design/inspiration/migrations/022-judgment-provenance.sql"),
    ),
    (
        24,
        include_str!("../../../../docs/design/inspiration/migrations/023-artifact-experiments.sql"),
    ),
    (
        25,
        include_str!("../../../../docs/design/inspiration/migrations/024-personal-evidence.sql"),
    ),
    (
        26,
        include_str!("../../../../docs/design/inspiration/migrations/025-atelier.sql"),
    ),
];

pub fn migrate(conn: &mut Connection) -> Result<()> {
    migrate_to(conn, LATEST_SCHEMA)
}

/// New commands fail before writing when an old database has not been upgraded.
pub fn require_current(path: &Path) -> Result<()> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("数据库未就绪，请先运行 ie migrate --db {}", path.display()))?;
    if version(&conn)? != Some(LATEST_SCHEMA) {
        bail!(
            "该命令需要 schema {LATEST_SCHEMA}，请先运行 ie migrate --db {}；不会自动修改旧库",
            path.display()
        );
    }
    Ok(())
}

/// Exposed within the crate for compatibility fixtures, never as a downgrade.
pub(crate) fn migrate_to(conn: &mut Connection, target: u32) -> Result<()> {
    if !conn.is_autocommit() {
        bail!("数据库迁移不能嵌套在现有事务内");
    }
    if !(2..=LATEST_SCHEMA).contains(&target) {
        bail!("不支持目标 schema {target}");
    }
    conn.busy_timeout(Duration::from_secs(10))?;
    let current = version(conn)?.unwrap_or(0);
    if current > target || (current != 0 && current < 2) {
        bail!("不支持从 schema {current} 迁移到 {target}，不会降级或猜测旧格式");
    }
    // Rebuilding extract must not cascade-delete inventory. SQLite ignores FK
    // toggles inside a transaction, so disable before BEGIN and verify before COMMIT.
    conn.pragma_update(None, "foreign_keys", false)?;
    let result = (|| -> Result<()> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if current == 0 {
            let base = BASE.replace("PRAGMA journal_mode = WAL;", "");
            tx.execute_batch(&base).context("初始化 schema v2 失败")?;
        }
        for (next, sql) in MIGRATIONS {
            if *next > current && *next <= target {
                tx.execute_batch(sql)
                    .with_context(|| format!("迁移到 schema {next} 失败"))?;
                let actual = version(&tx)?.context("迁移后 schema_version 缺失")?;
                if actual != *next {
                    bail!("迁移版本不一致：期望 {next}，实际 {actual}");
                }
            }
        }
        let invalid: Option<(String, Option<i64>, String)> = tx
            .query_row("PRAGMA foreign_key_check", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        if let Some((table, row, parent)) = invalid {
            bail!("迁移外键检查失败：{table} 行 {row:?} → {parent}，已回滚");
        }
        tx.commit()?;
        Ok(())
    })();
    // Restore FK enforcement even when the transaction failed and rolled back.
    let restored = conn.pragma_update(None, "foreign_keys", true);
    result?;
    restored?;
    Ok(())
}

fn version(conn: &Connection) -> Result<Option<u32>> {
    let has_meta: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_meta')",
        [],
        |r| r.get(0),
    )?;
    if !has_meta {
        let tables: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        if tables != 0 {
            bail!("该数据库不是空库，且没有 schema_meta；拒绝覆盖未知数据");
        }
        return Ok(None);
    }
    let text: String = conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .context("缺少 schema_version，拒绝猜测数据库版本")?;
    Ok(Some(text.parse().context("schema_version 不是合法整数")?))
}

/// Backups use VACUUM INTO, which includes committed WAL content consistently.
/// The original database is never moved or deleted, including on migration error.
pub fn migrate_path(path: &Path) -> Result<Option<PathBuf>> {
    let existed = path.exists() && std::fs::metadata(path)?.len() > 0;
    let mut conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(10))?;
    let current = version(&conn)?;
    if current == Some(LATEST_SCHEMA) {
        migrate(&mut conn)?;
        return Ok(None);
    }
    if current.is_some_and(|v| v > LATEST_SCHEMA) {
        bail!("数据库比本程序更新，拒绝降级");
    }
    let backup = if existed {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let name = path
            .file_name()
            .context("数据库路径没有文件名")?
            .to_string_lossy();
        let backup = path.with_file_name(format!("{name}.before-migrate-{stamp}.bak"));
        conn.execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])?;
        Some(backup)
    } else {
        None
    };
    migrate(&mut conn).with_context(|| match &backup {
        Some(path) => format!("迁移失败，原始数据未被替换；备份保留在 {}", path.display()),
        None => "数据库初始化失败，未替换任何已有用户数据".to_string(),
    })?;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_database_can_be_initialized_and_migrated_twice() -> Result<()> {
        let mut conn = Connection::open_in_memory()?;
        migrate(&mut conn)?;
        let version: String = conn.query_row(
            "SELECT value FROM schema_meta WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(version, LATEST_SCHEMA.to_string());
        conn.execute(
            "INSERT INTO recipe(key,name,goal,combination_prompt,created_at)
             VALUES('recipe.keep','keep','test','test','test')",
            [],
        )?;
        migrate(&mut conn)?;
        let count: i64 = conn.query_row("SELECT count(*) FROM recipe", [], |r| r.get(0))?;
        assert_eq!(count, 1);
        assert!(conn.pragma_query_value(None, "foreign_keys", |r| r.get::<_, bool>(0))?);
        Ok(())
    }

    #[test]
    fn upgrade_preserves_legacy_scores_without_trusting_legacy_weights() -> Result<()> {
        let mut conn = Connection::open_in_memory()?;
        migrate_to(&mut conn, 21)?;
        conn.execute(
            "INSERT INTO pill(id,output_md,created_at) VALUES(1,'keep','old')",
            [],
        )?;
        conn.execute(
            "INSERT INTO judge_run(id,stage,created_at,updated_at) VALUES(1,'panel','old','old')",
            [],
        )?;
        conn.execute("INSERT INTO judge_item(id,run_id,pill_id,label,display_order,human_score,human_note) VALUES(1,1,1,'A',0,84,'legacy note')", [])?;
        conn.execute("INSERT INTO judge_model(name,samples,agreements,weight,updated_at) VALUES('local',3,2,1.14,'old')", [])?;
        migrate(&mut conn)?;
        let (score, source, reviewer): (f64, String, Option<String>) = conn.query_row(
            "SELECT human_score,human_source,human_reviewer FROM judge_item WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        assert_eq!(score, 84.0);
        assert_eq!(source, "legacy");
        assert_eq!(reviewer, None);
        let (weight, trusted): (f64, bool) = conn.query_row(
            "SELECT weight,calibration_trusted FROM judge_model WHERE name='local'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(weight, 1.14);
        assert!(!trusted);
        let reviews: i64 = conn.query_row(
            "SELECT count(*) FROM judge_review WHERE source='legacy'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(reviews, 1);
        migrate(&mut conn)?;
        assert_eq!(
            conn.query_row("SELECT count(*) FROM judge_review", [], |r| r
                .get::<_, i64>(0))?,
            1
        );
        Ok(())
    }

    #[test]
    fn failed_foreign_key_check_rolls_back_the_entire_upgrade() -> Result<()> {
        let mut conn = Connection::open_in_memory()?;
        migrate_to(&mut conn, 21)?;
        conn.pragma_update(None, "foreign_keys", false)?;
        conn.execute(
            "INSERT INTO inventory(extract_id,added_at) VALUES(777,'legacy orphan')",
            [],
        )?;
        conn.pragma_update(None, "foreign_keys", true)?;
        assert!(migrate(&mut conn).is_err());
        assert_eq!(version(&conn)?, Some(21));
        assert_eq!(
            conn.query_row("SELECT count(*) FROM inventory", [], |r| r.get::<_, i64>(0))?,
            1
        );
        let added: i64 = conn.query_row(
            "SELECT count(*) FROM pragma_table_info('judge_item') WHERE name='human_source'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(added, 0);
        assert!(conn.pragma_query_value(None, "foreign_keys", |r| r.get::<_, bool>(0))?);
        Ok(())
    }

    #[test]
    fn existing_database_is_backed_up_before_upgrade() -> Result<()> {
        let parent = std::env::temp_dir().canonicalize()?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let dir = parent.join(format!("ie-migrate-backup-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&dir)?;
        let path = dir.join("legacy.db");
        {
            let mut conn = Connection::open(&path)?;
            migrate_to(&mut conn, 21)?;
            conn.execute(
                "INSERT INTO pill(output_md,created_at) VALUES('original evidence','old')",
                [],
            )?;
        }
        let backup = migrate_path(&path)?.expect("existing database must have a backup");
        {
            let upgraded = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            let old = Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            assert_eq!(version(&upgraded)?, Some(LATEST_SCHEMA));
            assert_eq!(version(&old)?, Some(21));
            for conn in [&upgraded, &old] {
                assert_eq!(
                    conn.query_row("SELECT output_md FROM pill", [], |r| r.get::<_, String>(0))?,
                    "original evidence"
                );
            }
        }
        assert!(
            migrate_path(&path)?.is_none(),
            "idempotent migration needs no second backup"
        );
        // Delete only the exact, owned directory this test created.
        let resolved = dir.canonicalize()?;
        assert_eq!(resolved, dir);
        assert_eq!(resolved.parent(), Some(parent.as_path()));
        assert!(
            resolved
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("ie-migrate-backup-")
        );
        std::fs::remove_dir_all(resolved)?;
        Ok(())
    }

    #[test]
    fn unknown_or_newer_databases_are_never_overwritten() -> Result<()> {
        let mut unknown = Connection::open_in_memory()?;
        unknown.execute("CREATE TABLE user_data(value TEXT)", [])?;
        assert!(migrate(&mut unknown).is_err());
        assert_eq!(
            unknown.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='schema_meta'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        let mut newer = Connection::open_in_memory()?;
        migrate(&mut newer)?;
        newer.execute(
            "UPDATE schema_meta SET value='999' WHERE key='schema_version'",
            [],
        )?;
        assert!(migrate(&mut newer).is_err());
        assert_eq!(version(&newer)?, Some(999));
        Ok(())
    }
}
