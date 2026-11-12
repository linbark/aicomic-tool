//! Memory deletion is prepared before main COMMIT, committed only afterwards.
use crate::identity::{columns, quote};
use crate::*;
use std::collections::BTreeMap;

type ForeignKeyViolation = (String, Option<i64>, String, i64);

pub(crate) struct Prepared {
    conn: Connection,
}
impl Prepared {
    pub(crate) fn existing(
        main: &Connection,
        path: &Path,
        project_id: i64,
    ) -> Result<Option<Self>> {
        if !path.try_exists()? {
            return Ok(None);
        }
        if !path.is_file() {
            return Err(Error::Invalid(
                "memory_store.db is not a database file".into(),
            ));
        }
        // Never create an absent memory database or run its schema migrations.
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA foreign_keys=ON; BEGIN IMMEDIATE; PRAGMA defer_foreign_keys=ON;",
        )?;
        let prepared = Self { conn };
        let high = identity::collect_high_water(&prepared.conn, "main")?;
        identity::reserve_high_water(main, high)?;
        let before = violations(&prepared.conn)?;
        delete_project(&prepared.conn, project_id)?;
        let after = violations(&prepared.conn)?;
        if after
            .iter()
            .any(|(violation, count)| *count > before.get(violation).copied().unwrap_or(0))
        {
            return Err(Error::Invalid(
                "memory cleanup introduces a deferred foreign key violation".into(),
            ));
        }
        Ok(Some(prepared))
    }
    pub(crate) fn commit(self) -> Result<()> {
        #[cfg(test)]
        if crate::operation_tests::error_at("memory_commit") {
            return Err(Error::Invalid("injected memory commit failure".into()));
        }
        self.conn.execute_batch("COMMIT")?;
        Ok(())
    }
}
impl Drop for Prepared {
    fn drop(&mut self) {
        // RAISE(ABORT) only rolls back one statement. Explicitly roll back the
        // entire prepared transaction, including already-deleted vector rows.
        if !self.conn.is_autocommit() {
            let _ = self.conn.execute_batch("ROLLBACK");
        }
    }
}
fn violations(conn: &Connection) -> Result<BTreeMap<ForeignKeyViolation, usize>> {
    let mut query = conn.prepare("PRAGMA foreign_key_check")?;
    let mut result = BTreeMap::new();
    for violation in query.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))? {
        *result.entry(violation?).or_insert(0) += 1;
    }
    Ok(result)
}
fn delete_project(conn: &Connection, project_id: i64) -> Result<()> {
    let mut query = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )?;
    let tables = query
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(query);
    let metadata = tables
        .into_iter()
        .map(|name| {
            let fields = columns(conn, "main", &name)?;
            Ok((name, fields))
        })
        .collect::<Result<Vec<_>>>()?;
    if metadata.iter().any(|(name, fields)| {
        name == "memory_records"
            && fields.iter().any(|c| c == "project_id")
            && fields.iter().any(|c| c == "id")
    }) {
        for (table, fields) in &metadata {
            if fields.iter().any(|c| c == "record_id") {
                conn.execute(&format!("DELETE FROM {} WHERE record_id IN(SELECT id FROM memory_records WHERE project_id=?)", quote(table)), [project_id])?;
            }
        }
    }
    for (table, fields) in metadata {
        if fields.iter().any(|c| c == "project_id") {
            conn.execute(
                &format!("DELETE FROM {} WHERE project_id=?", quote(&table)),
                [project_id],
            )?;
        }
    }
    Ok(())
}
