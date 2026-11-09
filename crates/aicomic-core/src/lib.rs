//! Local domain storage. Existing SQLAlchemy tables and JSON files stay readable.
mod archive;
mod crud;
mod files;
mod identity;
mod media;
mod memory_cleanup;
pub mod models;
mod paths;
mod schema;
mod workflow;
pub use archive::{ExportSummary, ImportSummary, export_project, import_project};
pub use models::*;
pub use paths::AppPaths;
use rusqlite::{
    Connection, OptionalExtension, Params, params,
    types::{Value as SqlValue, ValueRef},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("file: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0} was not found")]
    NotFound(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("unsafe path: {0}")]
    UnsafePath(String),
    #[error("storage lock was poisoned")]
    Poisoned,
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct Store {
    connection: Arc<Mutex<Connection>>,
    paths: AppPaths,
}

impl Store {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let paths = AppPaths::new(root)?;
        let existing = paths.database_path.is_file() && paths.database_path.metadata()?.len() > 0;
        let mut connection = Connection::open(&paths.database_path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let needs_migration = schema::needs_migration(&connection)?;
        if existing && needs_migration {
            let backup_dir = paths.resolve("backups")?;
            std::fs::create_dir_all(&backup_dir)?;
            connection.backup(
                "main",
                backup_dir.join(format!(
                    "database-before-rust-{}.db",
                    uuid::Uuid::new_v4().simple()
                )),
                None,
            )?;
        }
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if needs_migration {
            schema::migrate(&tx)?;
        }
        identity::initialize(&tx, &paths.resolve("memory_store.db")?)?;
        tx.commit()?;
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            paths,
        })
    }
    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }
    fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection.lock().map_err(|_| Error::Poisoned)
    }
}

fn rows<T: DeserializeOwned>(
    conn: &Connection,
    sql: &str,
    parameters: impl Params,
) -> Result<Vec<T>> {
    let mut statement = conn.prepare(sql)?;
    let names = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut cursor = statement.query(parameters)?;
    let mut output = vec![];
    while let Some(row) = cursor.next()? {
        let mut object = Map::new();
        for (i, name) in names.iter().enumerate() {
            let mut value = match row.get_ref(i)? {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(n) => Value::from(n),
                ValueRef::Real(n) => Value::from(n),
                ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
                ValueRef::Blob(_) => {
                    return Err(Error::Invalid(format!("unexpected blob in {name}")));
                }
            };
            if ["script_locked", "is_favorite"].contains(&name.as_str()) {
                value = Value::Bool(value.as_i64().unwrap_or(0) != 0)
            }
            if ["meta_data", "exec_artifacts", "graph_data"].contains(&name.as_str())
                && let Some(text) = value.as_str()
            {
                value = serde_json::from_str(text)?;
            }
            if value.is_null()
                && [
                    "name",
                    "title",
                    "uuid",
                    "file_path",
                    "file_type",
                    "output_text",
                    "action_key",
                    "target_type",
                    "created_at",
                    "exec_status",
                    "status",
                    "category",
                    "color",
                ]
                .contains(&name.as_str())
            {
                value = Value::String(String::new());
            }
            if value.is_null()
                && ["order", "sequence_number", "start_time_sort_key"].contains(&name.as_str())
            {
                value = Value::from(0);
            }
            object.insert(name.clone(), value);
        }
        output.push(serde_json::from_value(Value::Object(object))?);
    }
    Ok(output)
}
fn get<T: DeserializeOwned>(conn: &Connection, table: &str, id: i64) -> Result<T> {
    rows(conn, &format!("SELECT * FROM {table} WHERE id=?"), [id])?
        .into_iter()
        .next()
        .ok_or_else(|| Error::NotFound(format!("{table} {id}")))
}
fn require(conn: &Connection, table: &str, id: i64) -> Result<()> {
    let n: i64 = conn.query_row(
        &format!("SELECT count(*) FROM {table} WHERE id=?"),
        [id],
        |r| r.get(0),
    )?;
    if n == 0 {
        Err(Error::NotFound(format!("{table} {id}")))
    } else {
        Ok(())
    }
}
fn nonempty(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(Error::Invalid(format!("{label} cannot be empty")))
    } else {
        Ok(())
    }
}
fn normalize_category(value: &str) -> &str {
    if value.is_empty() || value == "persona" {
        "persona_visual"
    } else {
        value
    }
}
fn unique_item_name(conn: &Connection, pid: i64, name: &str, except: Option<i64>) -> Result<()> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM characters WHERE project_id=? AND name=? AND (? IS NULL OR id<>?)",
        params![pid, name, except, except],
        |r| r.get(0),
    )?;
    if count > 0 {
        Err(Error::Invalid(
            "asset item with this name already exists".into(),
        ))
    } else {
        Ok(())
    }
}
fn next_sequence(conn: &Connection, table: &str, parent: &str, id: i64, seq: i64) -> Result<i64> {
    if seq > 0 {
        return Ok(seq);
    }
    Ok(conn.query_row(
        &format!("SELECT COALESCE(MAX(sequence_number),0)+1 FROM {table} WHERE {parent}=?"),
        [id],
        |r| r.get(0),
    )?)
}
fn patch_row(conn: &Connection, table: &str, id: i64, patch: impl Serialize) -> Result<()> {
    require(conn, table, id)?;
    let value = serde_json::to_value(patch)?;
    let object = value
        .as_object()
        .ok_or_else(|| Error::Invalid("patch must be object".into()))?;
    let mut columns = vec![];
    let mut values = vec![];
    for (key, value) in object {
        if value.is_null() {
            continue;
        }
        columns.push(format!("\"{key}\"=?"));
        values.push(match value {
            Value::String(s) => SqlValue::Text(s.clone()),
            Value::Bool(b) => SqlValue::Integer(i64::from(*b)),
            Value::Number(n) => SqlValue::Integer(
                n.as_i64()
                    .ok_or_else(|| Error::Invalid("integer required".into()))?,
            ),
            _ => SqlValue::Text(value.to_string()),
        });
    }
    if !columns.is_empty() {
        values.push(SqlValue::Integer(id));
        conn.execute(
            &format!("UPDATE {table} SET {} WHERE id=?", columns.join(",")),
            rusqlite::params_from_iter(values),
        )?;
    }
    Ok(())
}
fn target_project(conn: &Connection, kind: &str, id: i64) -> Result<i64> {
    let sql = match kind {
        "episode" => "SELECT project_id FROM episodes WHERE id=?",
        "scene" => {
            "SELECT e.project_id FROM scenes s JOIN episodes e ON e.id=s.episode_id WHERE s.id=?"
        }
        "shot" => {
            "SELECT e.project_id FROM shots sh JOIN scenes s ON s.id=sh.scene_id JOIN episodes e ON e.id=s.episode_id WHERE sh.id=?"
        }
        _ => {
            return Err(Error::Invalid(
                "target type must be episode, scene or shot".into(),
            ));
        }
    };
    conn.query_row(sql, [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| Error::NotFound(format!("{kind} {id}")))
}
