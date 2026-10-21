use crate::identity::{columns, quote};
use crate::*;

const SCHEMA: &str = "aicomic_project_memory";

pub(crate) fn attach_existing(conn: &Connection, path: &Path) -> Result<bool> {
    if !path.try_exists()? {
        return Ok(false);
    }
    if !path.is_file() {
        return Err(Error::Invalid(
            "memory_store.db is not a database file".into(),
        ));
    }
    let filename = path
        .to_str()
        .ok_or_else(|| Error::UnsafePath(path.display().to_string()))?;
    conn.execute("ATTACH DATABASE ? AS aicomic_project_memory", [filename])?;
    Ok(true)
}

pub(crate) fn delete_project(conn: &Connection, project_id: i64) -> Result<()> {
    // Preserve even orphaned high identifiers before deleting their last references.
    identity::reserve_references(conn, SCHEMA)?;
    conn.execute_batch("PRAGMA defer_foreign_keys=ON")?;
    let mut query=conn.prepare("SELECT name FROM aicomic_project_memory.sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")?;
    let tables = query
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(query);
    let metadata = tables
        .into_iter()
        .map(|name| {
            let fields = columns(conn, SCHEMA, &name)?;
            Ok((name, fields))
        })
        .collect::<Result<Vec<_>>>()?;
    let records = metadata.iter().find(|(name, fields)| {
        name == "memory_records"
            && fields.iter().any(|c| c == "project_id")
            && fields.iter().any(|c| c == "id")
    });
    if records.is_some() {
        // Vector rows have no project_id. Their owner is the referenced memory record.
        for (table, fields) in &metadata {
            if fields.iter().any(|c| c == "record_id") {
                conn.execute(&format!("DELETE FROM {}.{} WHERE record_id IN(SELECT id FROM {}.memory_records WHERE project_id=?)",quote(SCHEMA),quote(table),quote(SCHEMA)),[project_id])?;
            }
        }
    }
    for (table, fields) in metadata {
        if fields.iter().any(|c| c == "project_id") {
            conn.execute(
                &format!(
                    "DELETE FROM {}.{} WHERE project_id=?",
                    quote(SCHEMA),
                    quote(&table)
                ),
                [project_id],
            )?;
        }
    }
    Ok(())
}
