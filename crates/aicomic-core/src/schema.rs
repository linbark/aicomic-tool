use crate::Result;
use rusqlite::{Connection, params};

const TABLES: &[(&str, &[(&str, &str)])] = &[
    (
        "projects",
        &[
            ("uuid", "TEXT"),
            ("name", "TEXT"),
            ("description", "TEXT"),
            ("created_at", "DATETIME"),
        ],
    ),
    (
        "characters",
        &[
            ("project_id", "INTEGER REFERENCES projects(id)"),
            ("name", "TEXT"),
            ("description", "TEXT"),
            ("base_prompt", "TEXT"),
            ("negative_prompt", "TEXT"),
            ("category", "TEXT DEFAULT 'persona_visual'"),
            ("avatar_asset_id", "INTEGER REFERENCES assets(id)"),
        ],
    ),
    (
        "episodes",
        &[
            ("project_id", "INTEGER REFERENCES projects(id)"),
            ("title", "TEXT"),
            ("order", "INTEGER DEFAULT 0"),
            ("description", "TEXT"),
            ("action_text", "TEXT"),
            ("prompt", "TEXT"),
            ("script_locked", "BOOLEAN DEFAULT 0"),
            ("script_locked_at", "DATETIME"),
            ("last_exec_run_id", "TEXT"),
            ("exec_status", "TEXT DEFAULT 'idle'"),
            ("exec_artifacts", "JSON"),
        ],
    ),
    (
        "scenes",
        &[
            ("episode_id", "INTEGER REFERENCES episodes(id)"),
            ("sequence_number", "INTEGER DEFAULT 1"),
            ("title", "TEXT"),
            ("description", "TEXT"),
            ("action_text", "TEXT"),
            ("dialogue", "TEXT"),
            ("prompt", "TEXT"),
        ],
    ),
    (
        "shots",
        &[
            ("scene_id", "INTEGER REFERENCES scenes(id)"),
            ("sequence_number", "INTEGER DEFAULT 1"),
            ("title", "TEXT"),
            ("action_text", "TEXT"),
            ("dialogue", "TEXT"),
            ("prompt", "TEXT"),
            ("negative_prompt", "TEXT"),
            ("selected_asset_id", "INTEGER REFERENCES assets(id)"),
            ("status", "TEXT DEFAULT 'draft'"),
            ("video_path", "TEXT"),
        ],
    ),
    (
        "assets",
        &[
            ("character_id", "INTEGER REFERENCES characters(id)"),
            ("shot_id", "INTEGER REFERENCES shots(id)"),
            ("file_path", "TEXT"),
            ("file_type", "TEXT"),
            ("meta_data", "JSON"),
            ("created_at", "DATETIME"),
            ("is_favorite", "BOOLEAN DEFAULT 0"),
        ],
    ),
    (
        "events",
        &[
            ("project_id", "INTEGER REFERENCES projects(id)"),
            ("name", "TEXT"),
            ("color", "TEXT DEFAULT '#3B82F6'"),
            ("start_time_sort_key", "INTEGER DEFAULT 0"),
            ("description", "TEXT"),
            ("graph_data", "JSON"),
        ],
    ),
    (
        "event_nodes",
        &[
            ("event_id", "INTEGER REFERENCES events(id)"),
            ("target_type", "TEXT"),
            ("target_id", "INTEGER"),
            ("description", "TEXT"),
        ],
    ),
    (
        "ai_action_runs",
        &[
            ("project_id", "INTEGER REFERENCES projects(id)"),
            ("target_type", "TEXT DEFAULT 'episode'"),
            ("target_id", "INTEGER"),
            ("action_key", "TEXT"),
            ("input_text", "TEXT"),
            ("output_text", "TEXT"),
            ("meta_data", "JSON"),
            ("created_at", "DATETIME"),
        ],
    ),
];

pub(crate) fn needs_migration(conn: &Connection) -> Result<bool> {
    let exists:i64=conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='aicomic_schema_migrations'",[],|r|r.get(0))?;
    if exists == 0 {
        return Ok(true);
    }
    let version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version),0) FROM aicomic_schema_migrations",
        [],
        |r| r.get(0),
    )?;
    Ok(version < 4)
}

pub(crate) fn migrate(tx: &Connection) -> Result<()> {
    for (table, columns) in TABLES {
        let mut definitions = vec!["id INTEGER PRIMARY KEY AUTOINCREMENT".to_string()];
        definitions.extend(
            columns
                .iter()
                .map(|(name, kind)| format!("\"{name}\" {kind}")),
        );
        tx.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS {table} ({})",
            definitions.join(",")
        ))?;
        let mut statement = tx.prepare(&format!("PRAGMA table_info({table})"))?;
        let existing = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        for (name, kind) in *columns {
            if !existing.iter().any(|n| n == name) {
                tx.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN \"{name}\" {kind}"))?;
            }
        }
    }
    let ids = {
        let mut q = tx.prepare("SELECT id FROM projects WHERE uuid IS NULL OR uuid=''")?;
        q.query_map([], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for id in ids {
        tx.execute(
            "UPDATE projects SET uuid=? WHERE id=?",
            params![uuid::Uuid::new_v4().simple().to_string(), id],
        )?;
    }
    tx.execute_batch("UPDATE characters SET category='persona_visual' WHERE category IS NULL OR category='persona' OR category=''; CREATE UNIQUE INDEX IF NOT EXISTS ux_projects_uuid ON projects(uuid); CREATE INDEX IF NOT EXISTS ix_episodes_project_id ON episodes(project_id); CREATE INDEX IF NOT EXISTS ix_characters_project_id ON characters(project_id); CREATE INDEX IF NOT EXISTS ix_scenes_episode_id ON scenes(episode_id); CREATE INDEX IF NOT EXISTS ix_shots_scene_id ON shots(scene_id); CREATE INDEX IF NOT EXISTS ix_assets_character_id ON assets(character_id); CREATE INDEX IF NOT EXISTS ix_assets_shot_id ON assets(shot_id); CREATE INDEX IF NOT EXISTS ix_events_project_id ON events(project_id); CREATE INDEX IF NOT EXISTS ix_event_nodes_event_id ON event_nodes(event_id); CREATE INDEX IF NOT EXISTS ix_ai_action_runs_project_id ON ai_action_runs(project_id); CREATE TABLE IF NOT EXISTS aicomic_schema_migrations(version INTEGER PRIMARY KEY, applied_at DATETIME DEFAULT CURRENT_TIMESTAMP); INSERT OR IGNORE INTO aicomic_schema_migrations(version) VALUES(1);")?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS workflow_applied(project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,source_episode_id INTEGER NOT NULL REFERENCES episodes(id) ON DELETE CASCADE,run_id TEXT NOT NULL,action TEXT NOT NULL,applied_at DATETIME DEFAULT CURRENT_TIMESTAMP,PRIMARY KEY(project_id,source_episode_id,run_id,action));")?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS aicomic_operation_receipts(operation_id TEXT PRIMARY KEY,manifest_json TEXT NOT NULL,committed_at DATETIME DEFAULT CURRENT_TIMESTAMP); CREATE TABLE IF NOT EXISTS aicomic_storage_identity(singleton INTEGER PRIMARY KEY CHECK(singleton=1),database_id TEXT NOT NULL);")?;
    tx.execute(
        "INSERT OR IGNORE INTO aicomic_storage_identity(singleton,database_id) VALUES(1,?)",
        [uuid::Uuid::new_v4().simple().to_string()],
    )?;
    initialize_journal_authentication(tx)?;
    tx.execute(
        "INSERT OR IGNORE INTO aicomic_schema_migrations(version) VALUES(3),(4)",
        [],
    )?;
    Ok(())
}

fn initialize_journal_authentication(tx: &Connection) -> Result<()> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut query = tx.prepare("PRAGMA table_info(aicomic_storage_identity)")?;
    let columns = query
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(query);
    if !columns.iter().any(|column| column == "journal_auth_key") {
        tx.execute_batch("ALTER TABLE aicomic_storage_identity ADD COLUMN journal_auth_key BLOB")?;
    }
    let existing: Option<Vec<u8>> = tx.query_row(
        "SELECT journal_auth_key FROM aicomic_storage_identity WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    if let Some(key) = existing {
        if key.len() != 32 {
            return Err(crate::Error::Invalid(
                "journal authentication key is invalid".into(),
            ));
        }
    } else {
        let mut key = [0u8; 32];
        SystemRandom::new().fill(&mut key).map_err(|_| {
            crate::Error::Invalid("journal authentication key generation failed".into())
        })?;
        tx.execute(
            "UPDATE aicomic_storage_identity SET journal_auth_key=? WHERE singleton=1",
            [key.as_slice()],
        )?;
    }
    Ok(())
}
