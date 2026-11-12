//! Stable numeric identity for both old INTEGER PRIMARY KEY schemas and new ones.
//! The durable high water survives deletion and includes orphaned memory references.
use crate::*;
use rusqlite::OpenFlags;

const TABLES: [&str; 9] = [
    "projects",
    "episodes",
    "scenes",
    "shots",
    "characters",
    "assets",
    "events",
    "event_nodes",
    "ai_action_runs",
];

pub(crate) fn initialize(tx: &Connection, memory_path: &Path) -> Result<()> {
    let memory_high = if memory_path.is_file() {
        let memory = Connection::open_with_flags(memory_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        memory.busy_timeout(Duration::from_secs(5))?;
        let snapshot = memory.unchecked_transaction()?;
        let high = collect_high_water(&snapshot, "main")?;
        snapshot.commit()?;
        high
    } else {
        [0; 9]
    };
    let main_high = collect_high_water(tx, "main")?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS aicomic_id_allocators(table_name TEXT PRIMARY KEY,high_water INTEGER NOT NULL CHECK(high_water>=0));")?;
    for (index, table) in TABLES.iter().enumerate() {
        let high = main_high[index].max(memory_high[index]);
        tx.execute("INSERT INTO aicomic_id_allocators(table_name,high_water) VALUES(?,?) ON CONFLICT(table_name) DO UPDATE SET high_water=MAX(high_water,excluded.high_water)",params![table,high])?;
        tx.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS aicomic_identity_{table} AFTER INSERT ON {} BEGIN INSERT INTO aicomic_id_allocators(table_name,high_water) VALUES('{table}',NEW.id) ON CONFLICT(table_name) DO UPDATE SET high_water=MAX(high_water,excluded.high_water);END;",quote(table)))?;
    }
    tx.execute(
        "INSERT OR IGNORE INTO aicomic_schema_migrations(version) VALUES(2)",
        [],
    )?;
    Ok(())
}

/// The caller must hold a write transaction until its corresponding INSERT commits.
pub(crate) fn allocate(conn: &Connection, table: &str) -> Result<i64> {
    if !TABLES.contains(&table) {
        return Err(Error::Invalid("unknown identity namespace".into()));
    }
    if conn.is_autocommit() {
        return Err(Error::Invalid(
            "identity allocation requires a transaction".into(),
        ));
    }
    let sql = format!(
        "UPDATE aicomic_id_allocators SET high_water=MAX(high_water,(SELECT COALESCE(MAX(id),0) FROM {}))+1 WHERE table_name=? AND MAX(high_water,(SELECT COALESCE(MAX(id),0) FROM {}))<9223372036854775807 RETURNING high_water",
        quote(table),
        quote(table)
    );
    conn.query_row(&sql, [table], |r| r.get(0))
        .optional()?
        .ok_or_else(|| Error::Invalid(format!("{table} numeric identifiers are exhausted")))
}

pub(crate) fn reserve_high_water(conn: &Connection, high: [i64; 9]) -> Result<()> {
    for (table, value) in TABLES.iter().zip(high) {
        conn.execute(
            "UPDATE aicomic_id_allocators SET high_water=MAX(high_water,?) WHERE table_name=?",
            params![value, table],
        )?;
    }
    Ok(())
}

pub(crate) fn collect_high_water(conn: &Connection, schema: &str) -> Result<[i64; 9]> {
    let mut high = [0; 9];
    let mut statement=conn.prepare(&format!("SELECT name FROM {}.sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'aicomic_%'",quote(schema)))?;
    let names = statement
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    for table in names {
        let columns = columns(conn, schema, &table)?;
        let qualified = format!("{}.{}", quote(schema), quote(&table));
        for column in &columns {
            let namespace = if column == "id" {
                TABLES.iter().position(|t| *t == table)
            } else {
                reference_namespace(column)
            };
            if let Some(index) = namespace {
                let column = quote(column);
                let n:i64=conn.query_row(&format!("SELECT COALESCE(MAX(CASE WHEN typeof({column})='integer' AND {column}>0 THEN {column} ELSE 0 END),0) FROM {qualified}"),[],|r|r.get(0))?;
                high[index] = high[index].max(n);
            }
            if column.ends_with("_json")
                || ["meta_data", "exec_artifacts", "graph_data"].contains(&column.as_str())
            {
                let mut query=conn.prepare(&format!("SELECT CAST(j.key AS TEXT),MAX(j.atom) FROM {qualified} t,json_tree(CASE WHEN json_valid(t.{}) THEN t.{} ELSE 'null' END) j WHERE j.type='integer' AND j.atom>0 GROUP BY j.key",quote(column),quote(column)))?;
                let references = query
                    .query_map([], |r| {
                        Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                for (key, value) in references {
                    if let Some(index) = key.as_deref().and_then(reference_namespace) {
                        high[index] = high[index].max(value);
                    }
                }
            }
        }
        if columns.iter().any(|c| c == "target_type") && columns.iter().any(|c| c == "target_id") {
            let mut query=conn.prepare(&format!("SELECT target_type,MAX(target_id) FROM {qualified} WHERE typeof(target_id)='integer' AND target_id>0 GROUP BY target_type"))?;
            let targets = query
                .query_map([], |r| {
                    Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (kind, n) in targets {
                let key = format!("{}_id", kind.unwrap_or_default());
                if let Some(index) = reference_namespace(&key) {
                    high[index] = high[index].max(n);
                }
            }
        }
    }
    let sequence_exists: i64 = conn.query_row(
        &format!(
            "SELECT count(*) FROM {}.sqlite_master WHERE type='table' AND name='sqlite_sequence'",
            quote(schema)
        ),
        [],
        |r| r.get(0),
    )?;
    if sequence_exists > 0 {
        let mut query = conn.prepare(&format!(
            "SELECT name,seq FROM {}.sqlite_sequence",
            quote(schema)
        ))?;
        for row in query.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (table, n) = row?;
            if let Some(index) = TABLES.iter().position(|t| *t == table) {
                high[index] = high[index].max(n);
            }
        }
    }
    Ok(high)
}

fn reference_namespace(column: &str) -> Option<usize> {
    match column {
        "project_id" => Some(0),
        "episode_id" | "source_episode_id" => Some(1),
        "scene_id" => Some(2),
        "shot_id" => Some(3),
        "character_id" | "asset_item_id" | "item_id" => Some(4),
        "asset_id" | "avatar_asset_id" | "selected_asset_id" => Some(5),
        "event_id" => Some(6),
        _ => None,
    }
}

pub(crate) fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
pub(crate) fn columns(conn: &Connection, schema: &str, table: &str) -> Result<Vec<String>> {
    let mut statement = conn.prepare(&format!(
        "PRAGMA {}.table_info({})",
        quote(schema),
        quote(table)
    ))?;
    Ok(statement
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}
