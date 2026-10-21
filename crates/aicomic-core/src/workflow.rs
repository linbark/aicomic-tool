use crate::*;
use std::{io::Write, path::PathBuf};

impl Store {
    /// Apply reviewed split/ingest output once per run and action. SQL and context
    /// writes roll back together on ordinary errors; retry after a crash is safe.
    pub fn apply_workflow_artifacts(
        &self,
        project_id: i64,
        source_episode_id: i64,
        run_id: &str,
        artifacts: &Value,
    ) -> Result<()> {
        paths::component(run_id)?;
        let mut conn = self.lock()?;
        if target_project(&conn, "episode", source_episode_id)? != project_id {
            return Err(Error::Invalid(
                "source episode belongs to another project".into(),
            ));
        }
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut changes = JsonChanges::default();
        for (action, key) in [
            ("split", "approved_split_episodes"),
            ("ingest", "approved_asset_ingest_plan"),
        ] {
            let Some(payload) = artifacts.get(key).filter(|p| !p.is_null()) else {
                continue;
            };
            let applied:i64=tx.query_row("SELECT count(*) FROM workflow_applied WHERE project_id=? AND source_episode_id=? AND run_id=? AND action=?",params![project_id,source_episode_id,run_id,action],|r|r.get(0))?;
            if applied > 0 {
                continue;
            }
            match action {
                "split" => {
                    let episodes = payload
                        .get("episodes")
                        .and_then(Value::as_array)
                        .ok_or_else(|| {
                            Error::Invalid("approved split must contain episodes array".into())
                        })?;
                    let base: i64 = tx.query_row(
                        "SELECT COALESCE(MAX(\"order\"),-1)+1 FROM episodes WHERE project_id=?",
                        [project_id],
                        |r| r.get(0),
                    )?;
                    for (i, episode) in episodes.iter().enumerate() {
                        if !episode.is_object() {
                            return Err(Error::Invalid("split episode must be an object".into()));
                        }
                        let title = string(episode, "title");
                        let title = if title.is_empty() {
                            format!("新建第{}集", i + 1)
                        } else {
                            title.into()
                        };
                        let script = string(episode, "script");
                        let outline = string(episode, "outline");
                        let meta = if outline.is_empty() {
                            serde_json::json!({})
                        } else {
                            serde_json::json!({"outline":outline})
                        };
                        let id = identity::allocate(&tx, "episodes")?;
                        tx.execute("INSERT INTO episodes(id,project_id,title,\"order\",description,script_locked,exec_status,exec_artifacts) VALUES(?,?,?,?,?,0,'idle',?)",params![id,project_id,title,base+i as i64,if script.is_empty(){None}else{Some(script)},meta.to_string()])?;
                    }
                }
                "ingest" => {
                    let items =
                        payload
                            .get("items")
                            .and_then(Value::as_array)
                            .ok_or_else(|| {
                                Error::Invalid("approved ingest must contain items array".into())
                            })?;
                    for item in items {
                        let name = string(item, "name");
                        nonempty(name, "ingest asset name")?;
                        let category = string(item, "category");
                        let category = if category.is_empty() {
                            "element"
                        } else {
                            normalize_category(category)
                        };
                        let description = string(item, "description");
                        let prompt = string(item, "base_prompt");
                        let existing=tx.query_row("SELECT id FROM characters WHERE project_id=? AND name=? ORDER BY id LIMIT 1",params![project_id,name],|r|r.get::<_,i64>(0)).optional()?;
                        let id = if let Some(id) = existing {
                            tx.execute("UPDATE characters SET category=?,description=?,base_prompt=? WHERE id=?",params![category,description,prompt,id])?;
                            id
                        } else {
                            let id = identity::allocate(&tx, "characters")?;
                            tx.execute("INSERT INTO characters(id,project_id,name,category,description,base_prompt) VALUES(?,?,?,?,?,?)",params![id,project_id,name,category,description,prompt])?;
                            id
                        };
                        if string(item, "kind") == "character" {
                            let filename = format!("visual_dna.asset_item_{id}.v1.json");
                            let path =
                                paths::safe_join(&self.paths.context_dir(project_id)?, &filename)?;
                            let dna = item
                                .get("visual_dna")
                                .filter(|v| v.is_object())
                                .cloned()
                                .unwrap_or_else(|| serde_json::json!({}));
                            changes.write(path,&serde_json::json!({"name":name,"visual_dna":dna,"stable_diffusion_tags":string(item,"stable_diffusion_tags")}))?;
                        }
                    }
                    if let Some(style) = payload.get("series_style").filter(|v| v.is_object()) {
                        let path = paths::safe_join(
                            &self.paths.context_dir(project_id)?,
                            "series_bible.v1.json",
                        )?;
                        let mut bible =
                            paths::read_json(&path)?.unwrap_or_else(|| serde_json::json!({}));
                        let object = bible.as_object_mut().ok_or_else(|| {
                            Error::Invalid("existing series bible must be an object".into())
                        })?;
                        object.insert("series_style".into(), style.clone());
                        changes.write(path, &bible)?;
                    }
                }
                _ => unreachable!(),
            }
            tx.execute("INSERT INTO workflow_applied(project_id,source_episode_id,run_id,action) VALUES(?,?,?,?)",params![project_id,source_episode_id,run_id,action])?;
        }
        tx.commit()?;
        changes.committed = true;
        Ok(())
    }
}
fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("").trim()
}

#[derive(Default)]
struct JsonChanges {
    originals: Vec<(PathBuf, Option<Vec<u8>>)>,
    committed: bool,
}
impl JsonChanges {
    fn write(&mut self, path: PathBuf, value: &Value) -> Result<()> {
        if !self.originals.iter().any(|(p, _)| p == &path) {
            let original = match std::fs::read(&path) {
                Ok(b) => Some(b),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.into()),
            };
            self.originals.push((path.clone(), original));
        }
        paths::write_json(&path, value)
    }
}
impl Drop for JsonChanges {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for (path, original) in self.originals.iter().rev() {
            if let Some(bytes) = original {
                let restore = (|| -> std::io::Result<()> {
                    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
                    temp.write_all(bytes)?;
                    temp.as_file().sync_all()?;
                    temp.persist(path).map_err(|e| e.error)?;
                    Ok(())
                })();
                if let Err(e) = restore {
                    eprintln!("Could not restore workflow context {}: {e}", path.display());
                }
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}
