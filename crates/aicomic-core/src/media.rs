use crate::*;
use std::path::PathBuf;

impl Store {
    pub fn asset_path(&self, relative: &str) -> Result<PathBuf> {
        paths::safe_join(&self.paths.data_dir, relative)
    }
    pub fn import_asset(&self, source: &Path, target: AssetTarget) -> Result<Asset> {
        if !source.is_file() {
            return Err(Error::Invalid(
                "media source must be an existing file".into(),
            ));
        }
        let mut conn = self.lock()?;
        let (pid, item_id, shot_id, folder) = match target {
            AssetTarget::Item(id) => {
                let item: AssetItem = get(&conn, "characters", id)?;
                (
                    item.project_id,
                    Some(id),
                    None,
                    category_folder(&item.category).to_owned(),
                )
            }
            AssetTarget::Shot(id) => {
                let shot: Shot = get(&conn, "shots", id)?;
                let scene: Scene = get(&conn, "scenes", shot.scene_id)?;
                (
                    target_project(&conn, "shot", id)?,
                    None,
                    Some(id),
                    format!(
                        "storyboard/episode_{}/scene_{}/shot_{}",
                        scene.episode_id, scene.id, id
                    ),
                )
            }
        };
        let project: Project = get(&conn, "projects", pid)?;
        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| e.chars().all(|c| c.is_ascii_alphanumeric()))
            .unwrap_or("bin")
            .to_lowercase();
        let relative = format!(
            "{}/{folder}/{}.{}",
            project.uuid,
            uuid::Uuid::new_v4().simple(),
            extension
        );
        let destination = self.asset_path(&relative)?;
        std::fs::create_dir_all(
            destination
                .parent()
                .ok_or_else(|| Error::Invalid("missing media parent".into()))?,
        )?;
        std::fs::copy(source, &destination)?;
        let mut copied = CopiedFile {
            path: destination,
            keep: false,
        };
        let file_type = file_type(&extension);
        let metadata = if file_type == "image" {
            image_metadata(&copied.path)
        } else {
            serde_json::json!({})
        };
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let id = identity::allocate(&tx, "assets")?;
        tx.execute("INSERT INTO assets(id,character_id,shot_id,file_path,file_type,meta_data,is_favorite,created_at) VALUES(?,?,?,?,?,?,?,CURRENT_TIMESTAMP)",params![id,item_id,shot_id,relative,file_type,metadata.to_string(),item_id.is_some()])?;
        let asset = get(&tx, "assets", id)?;
        tx.commit()?;
        copied.keep = true;
        Ok(asset)
    }
    pub fn register_asset_for_shot(
        &self,
        shot_id: i64,
        relative: &str,
        metadata: Option<Value>,
    ) -> Result<Asset> {
        if !self.asset_path(relative)?.is_file() {
            return Err(Error::Invalid(
                "registered media must exist in data directory".into(),
            ));
        }
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        require(&tx, "shots", shot_id)?;
        let extension = Path::new(relative)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let id = identity::allocate(&tx, "assets")?;
        tx.execute("INSERT INTO assets(id,shot_id,file_path,file_type,meta_data,is_favorite,created_at) VALUES(?,?,?,?,?,0,CURRENT_TIMESTAMP)",params![id,shot_id,relative.replace('\\',"/"),file_type(&extension),metadata.map(|v|v.to_string())])?;
        let row = get(&tx, "assets", id)?;
        tx.commit()?;
        Ok(row)
    }
    pub fn import_shot_video(&self, shot_id: i64, source: &Path) -> Result<Shot> {
        if !source.is_file() {
            return Err(Error::Invalid("video source must exist".into()));
        }
        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if file_type(&extension) != "video" {
            return Err(Error::Invalid(
                "source is not a supported video format".into(),
            ));
        }
        let mut conn = self.lock()?;
        let shot: Shot = get(&conn, "shots", shot_id)?;
        let scene: Scene = get(&conn, "scenes", shot.scene_id)?;
        let project: Project = get(&conn, "projects", target_project(&conn, "shot", shot_id)?)?;
        let relative = format!(
            "{}/storyboard/episode_{}/scene_{}/shot_{}/video/{}.{}",
            project.uuid,
            scene.episode_id,
            scene.id,
            shot_id,
            uuid::Uuid::new_v4().simple(),
            extension
        );
        let destination = self.asset_path(&relative)?;
        std::fs::create_dir_all(destination.parent().unwrap())?;
        std::fs::copy(source, &destination)?;
        let mut copied = CopiedFile {
            path: destination,
            keep: false,
        };
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE shots SET video_path=? WHERE id=?",
            params![relative, shot_id],
        )?;
        let mut removal = StagedRemoval::new(&self.paths.root)?;
        if let Some(old) = shot.video_path {
            stage_unreferenced(self, &tx, &mut removal, &old)?;
        }
        let updated = get(&tx, "shots", shot_id)?;
        tx.commit()?;
        copied.keep = true;
        removal.committed = true;
        Ok(updated)
    }
    pub fn delete_project(&self, id: i64) -> Result<()> {
        self.delete_domain("projects", id)
    }
    pub fn delete_episode(&self, id: i64) -> Result<()> {
        self.delete_domain("episodes", id)
    }
    pub fn delete_scene(&self, id: i64) -> Result<()> {
        self.delete_domain("scenes", id)
    }
    pub fn delete_shot(&self, id: i64) -> Result<()> {
        self.delete_domain("shots", id)
    }
    pub fn delete_asset_item(&self, id: i64) -> Result<()> {
        self.delete_domain("characters", id)
    }
    pub fn delete_asset(&self, id: i64) -> Result<()> {
        self.delete_domain("assets", id)
    }

    fn delete_domain(&self, table: &str, id: i64) -> Result<()> {
        let mut conn = self.lock()?;
        require(&conn, table, id)?;
        let attached = if table == "projects" {
            memory_cleanup::attach_existing(&conn, &self.paths.resolve("memory_store.db")?)?
        } else {
            false
        };
        let result = self.delete_domain_transaction(&mut conn, table, id, attached);
        if attached {
            let detached = conn.execute_batch("DETACH DATABASE aicomic_project_memory");
            if result.is_ok() {
                detached?;
            }
        }
        result
    }
    fn delete_domain_transaction(
        &self,
        conn: &mut Connection,
        table: &str,
        id: i64,
        attached: bool,
    ) -> Result<()> {
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let eps = if table == "projects" {
            ids(
                &tx,
                &format!("SELECT id FROM episodes WHERE project_id={id}"),
            )?
        } else if table == "episodes" {
            vec![id]
        } else {
            vec![]
        };
        let scenes = if table == "scenes" {
            vec![id]
        } else {
            ids(
                &tx,
                &format!(
                    "SELECT id FROM scenes WHERE episode_id IN ({})",
                    in_ids(&eps)
                ),
            )?
        };
        let shots = if table == "shots" {
            vec![id]
        } else {
            ids(
                &tx,
                &format!(
                    "SELECT id FROM shots WHERE scene_id IN ({})",
                    in_ids(&scenes)
                ),
            )?
        };
        let items = if table == "projects" {
            ids(
                &tx,
                &format!("SELECT id FROM characters WHERE project_id={id}"),
            )?
        } else if table == "characters" {
            vec![id]
        } else {
            vec![]
        };
        let asset_filter = if table == "assets" {
            format!("id={id}")
        } else {
            format!(
                "character_id IN ({}) OR shot_id IN ({})",
                in_ids(&items),
                in_ids(&shots)
            )
        };
        let assets: Vec<Asset> = rows(
            &tx,
            &format!("SELECT * FROM assets WHERE {asset_filter}"),
            [],
        )?;
        let asset_ids = assets.iter().map(|a| a.id).collect::<Vec<_>>();
        let mut files = assets.into_iter().map(|a| a.file_path).collect::<Vec<_>>();
        let shot_rows: Vec<Shot> = rows(
            &tx,
            &format!("SELECT * FROM shots WHERE id IN ({})", in_ids(&shots)),
            [],
        )?;
        files.extend(shot_rows.into_iter().filter_map(|s| s.video_path));
        tx.execute_batch(&format!("UPDATE shots SET selected_asset_id=NULL WHERE selected_asset_id IN ({}); UPDATE characters SET avatar_asset_id=NULL WHERE avatar_asset_id IN ({}); DELETE FROM assets WHERE id IN ({});",in_ids(&asset_ids),in_ids(&asset_ids),in_ids(&asset_ids)))?;
        for (kind, targets) in [("episode", &eps), ("scene", &scenes), ("shot", &shots)] {
            tx.execute_batch(&format!("DELETE FROM event_nodes WHERE target_type='{kind}' AND target_id IN ({}); DELETE FROM ai_action_runs WHERE target_type='{kind}' AND target_id IN ({});",in_ids(targets),in_ids(targets)))?;
        }
        for (kind, targets) in [
            ("shots", &shots),
            ("scenes", &scenes),
            ("episodes", &eps),
            ("characters", &items),
        ] {
            tx.execute_batch(&format!(
                "DELETE FROM {kind} WHERE id IN ({})",
                in_ids(targets)
            ))?;
        }
        if table == "projects" {
            tx.execute("DELETE FROM event_nodes WHERE event_id IN (SELECT id FROM events WHERE project_id=?)",[id])?;
            tx.execute("DELETE FROM events WHERE project_id=?", [id])?;
            tx.execute("DELETE FROM ai_action_runs WHERE project_id=?", [id])?;
            tx.execute("DELETE FROM projects WHERE id=?", [id])?;
        }
        let mut removal = StagedRemoval::new(&self.paths.root)?;
        for file in files {
            if !file.is_empty() {
                stage_unreferenced(self, &tx, &mut removal, &file)?;
            }
        }
        if table == "projects" {
            removal.take(self.paths.project_dir(id)?)?;
            if attached {
                memory_cleanup::delete_project(&tx, id)?;
            }
        }
        tx.commit()?;
        removal.committed = true;
        Ok(())
    }
}

fn ids(conn: &Connection, sql: &str) -> Result<Vec<i64>> {
    let mut s = conn.prepare(sql)?;
    Ok(s.query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}
fn in_ids(ids: &[i64]) -> String {
    if ids.is_empty() {
        "NULL".into()
    } else {
        ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
    }
}
fn stage_unreferenced(
    store: &Store,
    conn: &Connection,
    removal: &mut StagedRemoval,
    relative: &str,
) -> Result<()> {
    let normalized = relative.replace('\\', "/");
    let n:i64=conn.query_row("SELECT (SELECT count(*) FROM assets WHERE replace(file_path,'\\','/')=?)+(SELECT count(*) FROM shots WHERE replace(video_path,'\\','/')=?)",params![normalized,normalized],|r|r.get(0))?;
    if n == 0 {
        removal.take(store.asset_path(relative)?)?;
    }
    Ok(())
}
struct CopiedFile {
    path: PathBuf,
    keep: bool,
}
impl Drop for CopiedFile {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
// Files are quarantined until SQLite commits. Any failure restores the originals.
struct StagedRemoval {
    temp: tempfile::TempDir,
    moved: Vec<(PathBuf, PathBuf)>,
    committed: bool,
}
impl StagedRemoval {
    fn new(root: &Path) -> Result<Self> {
        Ok(Self {
            temp: tempfile::Builder::new()
                .prefix(".aicomic-trash-")
                .tempdir_in(root)?,
            moved: vec![],
            committed: false,
        })
    }
    fn take(&mut self, path: PathBuf) -> Result<()> {
        if path.try_exists()? {
            let target = self.temp.path().join(self.moved.len().to_string());
            std::fs::rename(&path, &target)?;
            self.moved.push((path, target));
        }
        Ok(())
    }
}
impl Drop for StagedRemoval {
    fn drop(&mut self) {
        if !self.committed {
            for (original, staged) in self.moved.iter().rev() {
                let _ = std::fs::rename(staged, original);
            }
        }
    }
}
fn category_folder(category: &str) -> &str {
    match category {
        "persona" | "persona_visual" => "characters",
        "persona_voice" => "voices",
        "background" => "backgrounds",
        "element" => "elements",
        "prop" => "props",
        "pose" => "poses",
        "vfx" => "vfx",
        "layout" => "layout",
        "audio_music" => "audio_music",
        "audio_sfx" => "audio_sfx",
        "branding" => "branding",
        "ai_preset" => "ai_preset",
        _ => "other",
    }
}
pub fn file_type(extension: &str) -> &'static str {
    match extension {
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "tiff" => "image",
        "mp4" | "mov" | "avi" | "webm" | "mkv" => "video",
        "mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg" => "audio",
        "txt" | "md" | "json" | "pdf" | "doc" | "docx" => "text",
        _ => "other",
    }
}

fn image_metadata(path: &Path) -> Value {
    let Ok((width, height)) = image::image_dimensions(path) else {
        return serde_json::json!({});
    };
    let mut metadata = serde_json::json!({"width":width,"height":height,"raw_parameters":""});
    if let Some(parameters) = png_parameters(path) {
        metadata["raw_parameters"] = Value::String(parameters.clone());
        let (positive, negative) = if let Some((p, n)) = parameters.split_once("Negative prompt:") {
            (p, Some(n))
        } else {
            (parameters.as_str(), None)
        };
        let (prompt, parameter_text) = if let Some(negative) = negative {
            let (negative, rest) = negative
                .split_once("Steps:")
                .map(|(n, r)| (n, Some(r)))
                .unwrap_or((negative, None));
            metadata["negative_prompt"] = Value::String(negative.trim().into());
            (positive, rest)
        } else {
            positive
                .split_once("Steps:")
                .map(|(p, r)| (p, Some(r)))
                .unwrap_or((positive, None))
        };
        metadata["prompt"] = Value::String(prompt.trim().into());
        if let Some(text) = parameter_text {
            for field in text.split(',') {
                let Some((key, value)) = field.split_once(':') else {
                    continue;
                };
                let value = value.trim();
                match key.trim() {
                    "Seed" => {
                        if let Ok(seed) = value.parse::<u64>() {
                            metadata["seed"] = Value::from(seed);
                        }
                    }
                    "Sampler" => metadata["sampler"] = Value::String(value.into()),
                    "CFG scale" => {
                        if let Ok(scale) = value.parse::<f64>() {
                            metadata["cfg_scale"] = Value::from(scale);
                        }
                    }
                    "Model hash" => metadata["model_hash"] = Value::String(value.into()),
                    _ => {}
                }
            }
        }
    }
    metadata
}
fn png_parameters(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .ok()?;
    reader.finish().ok()?;
    let info = reader.info();
    if let Some(text) = info
        .uncompressed_latin1_text
        .iter()
        .find(|t| t.keyword == "parameters")
    {
        return Some(text.text.clone());
    }
    if let Some(text) = info
        .compressed_latin1_text
        .iter()
        .find(|t| t.keyword == "parameters")
    {
        let mut text = text.clone();
        text.decompress_text_with_limit(2 * 1024 * 1024).ok()?;
        return text.get_text().ok();
    }
    if let Some(text) = info.utf8_text.iter().find(|t| t.keyword == "parameters") {
        let mut text = text.clone();
        text.decompress_text_with_limit(2 * 1024 * 1024).ok()?;
        return text.get_text().ok();
    }
    None
}
