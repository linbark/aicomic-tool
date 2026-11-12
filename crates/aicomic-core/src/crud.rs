use crate::*;

impl Store {
    pub fn projects(&self) -> Result<Vec<Project>> {
        rows(
            &*self.lock()?,
            "SELECT * FROM projects ORDER BY created_at,id",
            [],
        )
    }
    pub fn project(&self, id: i64) -> Result<Project> {
        get(&*self.lock()?, "projects", id)
    }
    pub fn resolve_project(&self, id: &str) -> Result<Project> {
        let conn = self.lock()?;
        let found = rows(&conn, "SELECT * FROM projects WHERE uuid=?", [id])?
            .into_iter()
            .next();
        if let Some(p) = found {
            return Ok(p);
        }
        if let Ok(n) = id.parse::<i64>() {
            return get(&conn, "projects", n);
        }
        Err(Error::NotFound(format!("project {id}")))
    }
    pub fn create_project(&self, name: &str, description: Option<&str>) -> Result<Project> {
        nonempty(name, "project name")?;
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let id = identity::allocate(&tx, "projects")?;
        tx.execute("INSERT INTO projects(id,uuid,name,description,created_at) VALUES(?,?,?,?,CURRENT_TIMESTAMP)",params![id,uuid::Uuid::new_v4().simple().to_string(),name,description])?;
        let row = get(&tx, "projects", id)?;
        tx.commit()?;
        Ok(row)
    }
    pub fn update_project(
        &self,
        id: i64,
        name: &str,
        description: Option<&str>,
    ) -> Result<Project> {
        nonempty(name, "project name")?;
        let conn = self.lock()?;
        require(&conn, "projects", id)?;
        conn.execute(
            "UPDATE projects SET name=?,description=? WHERE id=?",
            params![name, description, id],
        )?;
        get(&conn, "projects", id)
    }
    pub fn episodes(&self, project_id: i64) -> Result<Vec<Episode>> {
        let conn = self.lock()?;
        require(&conn, "projects", project_id)?;
        rows(
            &conn,
            "SELECT * FROM episodes WHERE project_id=? ORDER BY \"order\",id",
            [project_id],
        )
    }
    pub fn episode(&self, id: i64) -> Result<Episode> {
        get(&*self.lock()?, "episodes", id)
    }
    pub fn create_episode(
        &self,
        project_id: i64,
        title: &str,
        description: Option<&str>,
    ) -> Result<Episode> {
        nonempty(title, "episode title")?;
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        require(&tx, "projects", project_id)?;
        let id = identity::allocate(&tx, "episodes")?;
        tx.execute("INSERT INTO episodes(id,project_id,title,description,\"order\",script_locked,exec_status) VALUES(?,?,?,?,COALESCE((SELECT MAX(\"order\")+1 FROM episodes WHERE project_id=?),0),0,'idle')",params![id,project_id,title,description,project_id])?;
        let row = get(&tx, "episodes", id)?;
        tx.commit()?;
        Ok(row)
    }
    pub fn update_episode(
        &self,
        id: i64,
        title: &str,
        description: Option<&str>,
    ) -> Result<Episode> {
        nonempty(title, "episode title")?;
        let conn = self.lock()?;
        require(&conn, "episodes", id)?;
        // Compare against the old body inside the same SQL write. All callers,
        // including close-time saves, must revoke a lock when its text changes.
        conn.execute(
            "UPDATE episodes SET title=?1,description=?2,
             script_locked=CASE WHEN description IS NOT ?2 THEN 0 ELSE script_locked END,
             script_locked_at=CASE WHEN description IS NOT ?2 THEN NULL ELSE script_locked_at END
             WHERE id=?3",
            params![title, description, id],
        )?;
        get(&conn, "episodes", id)
    }
    pub fn patch_episode(&self, id: i64, patch: EpisodeUpdate) -> Result<Episode> {
        let conn = self.lock()?;
        patch_row(&conn, "episodes", id, patch)?;
        get(&conn, "episodes", id)
    }
    pub fn scenes(&self, episode_id: i64) -> Result<Vec<Scene>> {
        let conn = self.lock()?;
        require(&conn, "episodes", episode_id)?;
        rows(
            &conn,
            "SELECT * FROM scenes WHERE episode_id=? ORDER BY sequence_number,id",
            [episode_id],
        )
    }
    pub fn scene(&self, id: i64) -> Result<Scene> {
        get(&*self.lock()?, "scenes", id)
    }
    pub fn create_scene(
        &self,
        episode_id: i64,
        title: &str,
        sequence_number: i64,
    ) -> Result<Scene> {
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        require(&tx, "episodes", episode_id)?;
        let seq = next_sequence(&tx, "scenes", "episode_id", episode_id, sequence_number)?;
        let title = if title.is_empty() {
            format!("Scene {seq}")
        } else {
            title.into()
        };
        let id = identity::allocate(&tx, "scenes")?;
        tx.execute(
            "INSERT INTO scenes(id,episode_id,title,sequence_number) VALUES(?,?,?,?)",
            params![id, episode_id, title, seq],
        )?;
        let row = get(&tx, "scenes", id)?;
        tx.commit()?;
        Ok(row)
    }
    pub fn update_scene(&self, id: i64, patch: SceneUpdate) -> Result<Scene> {
        let conn = self.lock()?;
        patch_row(&conn, "scenes", id, patch)?;
        get(&conn, "scenes", id)
    }
    pub fn shots(&self, scene_id: i64) -> Result<Vec<Shot>> {
        let conn = self.lock()?;
        require(&conn, "scenes", scene_id)?;
        rows(
            &conn,
            "SELECT * FROM shots WHERE scene_id=? ORDER BY sequence_number,id",
            [scene_id],
        )
    }
    pub fn shot(&self, id: i64) -> Result<Shot> {
        get(&*self.lock()?, "shots", id)
    }
    pub fn create_shot(&self, scene_id: i64, title: &str, sequence_number: i64) -> Result<Shot> {
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        require(&tx, "scenes", scene_id)?;
        let seq = next_sequence(&tx, "shots", "scene_id", scene_id, sequence_number)?;
        let id = identity::allocate(&tx, "shots")?;
        tx.execute(
            "INSERT INTO shots(id,scene_id,title,sequence_number,status) VALUES(?,?,?,?,'draft')",
            params![id, scene_id, title, seq],
        )?;
        let row = get(&tx, "shots", id)?;
        tx.commit()?;
        Ok(row)
    }
    pub fn update_shot(&self, id: i64, patch: ShotUpdate) -> Result<Shot> {
        let conn = self.lock()?;
        patch_row(&conn, "shots", id, patch)?;
        get(&conn, "shots", id)
    }
    pub fn script(&self, project_id: i64) -> Result<Vec<EpisodeTree>> {
        self.episodes(project_id)?
            .into_iter()
            .map(|episode| {
                let scenes = self
                    .scenes(episode.id)?
                    .into_iter()
                    .map(|scene| {
                        let shots = self
                            .shots(scene.id)?
                            .into_iter()
                            .map(|shot| {
                                let assets = self.assets_for_shot(shot.id)?;
                                Ok(ShotTree { shot, assets })
                            })
                            .collect::<Result<Vec<_>>>()?;
                        Ok(SceneTree { scene, shots })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(EpisodeTree { episode, scenes })
            })
            .collect()
    }
    pub fn asset_items(&self, project_id: i64) -> Result<Vec<AssetItem>> {
        let conn = self.lock()?;
        require(&conn, "projects", project_id)?;
        rows(
            &conn,
            "SELECT * FROM characters WHERE project_id=? ORDER BY id",
            [project_id],
        )
    }
    pub fn asset_item(&self, id: i64) -> Result<AssetItem> {
        get(&*self.lock()?, "characters", id)
    }
    pub fn create_asset_item(
        &self,
        project_id: i64,
        name: &str,
        category: &str,
        description: Option<&str>,
    ) -> Result<AssetItem> {
        nonempty(name, "asset name")?;
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        require(&tx, "projects", project_id)?;
        unique_item_name(&tx, project_id, name, None)?;
        let id = identity::allocate(&tx, "characters")?;
        tx.execute(
            "INSERT INTO characters(id,project_id,name,category,description) VALUES(?,?,?,?,?)",
            params![
                id,
                project_id,
                name,
                normalize_category(category),
                description
            ],
        )?;
        let row = get(&tx, "characters", id)?;
        tx.commit()?;
        Ok(row)
    }
    pub fn update_asset_item(&self, id: i64, mut patch: AssetItemUpdate) -> Result<AssetItem> {
        let conn = self.lock()?;
        let item: AssetItem = get(&conn, "characters", id)?;
        if let Some(name) = &patch.name {
            nonempty(name, "asset name")?;
            unique_item_name(&conn, item.project_id, name, Some(id))?;
        }
        if let Some(category) = &patch.category {
            patch.category = Some(normalize_category(category).to_owned());
        }
        patch_row(&conn, "characters", id, patch)?;
        get(&conn, "characters", id)
    }
    pub fn assets_for_item(&self, id: i64) -> Result<Vec<Asset>> {
        let conn = self.lock()?;
        require(&conn, "characters", id)?;
        rows(
            &conn,
            "SELECT * FROM assets WHERE character_id=? ORDER BY id",
            [id],
        )
    }
    pub fn assets_for_shot(&self, id: i64) -> Result<Vec<Asset>> {
        let conn = self.lock()?;
        require(&conn, "shots", id)?;
        rows(
            &conn,
            "SELECT * FROM assets WHERE shot_id=? ORDER BY id",
            [id],
        )
    }
    pub fn asset(&self, id: i64) -> Result<Asset> {
        get(&*self.lock()?, "assets", id)
    }
    pub fn select_shot_asset(&self, id: i64, asset_id: Option<i64>) -> Result<Shot> {
        let conn = self.lock()?;
        require(&conn, "shots", id)?;
        if let Some(a) = asset_id {
            let asset: Asset = get(&conn, "assets", a)?;
            if asset.shot_id != Some(id) {
                return Err(Error::Invalid("asset does not belong to this shot".into()));
            }
        }
        conn.execute(
            "UPDATE shots SET selected_asset_id=? WHERE id=?",
            params![asset_id, id],
        )?;
        get(&conn, "shots", id)
    }
    pub fn set_item_avatar(&self, id: i64, asset_id: Option<i64>) -> Result<AssetItem> {
        let conn = self.lock()?;
        require(&conn, "characters", id)?;
        if let Some(a) = asset_id {
            let asset: Asset = get(&conn, "assets", a)?;
            if asset.character_id != Some(id) {
                return Err(Error::Invalid(
                    "avatar does not belong to this asset item".into(),
                ));
            }
        }
        conn.execute(
            "UPDATE characters SET avatar_asset_id=? WHERE id=?",
            params![asset_id, id],
        )?;
        get(&conn, "characters", id)
    }
    pub fn set_asset_favorite(&self, id: i64, favorite: bool) -> Result<Asset> {
        let conn = self.lock()?;
        require(&conn, "assets", id)?;
        conn.execute(
            "UPDATE assets SET is_favorite=? WHERE id=?",
            params![favorite, id],
        )?;
        get(&conn, "assets", id)
    }
    pub fn events(&self, project_id: i64) -> Result<Vec<Event>> {
        let conn = self.lock()?;
        require(&conn, "projects", project_id)?;
        rows(
            &conn,
            "SELECT * FROM events WHERE project_id=? ORDER BY start_time_sort_key,id",
            [project_id],
        )
    }
    pub fn event(&self, id: i64) -> Result<Event> {
        get(&*self.lock()?, "events", id)
    }
    pub fn create_event(&self, project_id: i64, name: &str, color: &str) -> Result<Event> {
        nonempty(name, "event name")?;
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        require(&tx, "projects", project_id)?;
        let id = identity::allocate(&tx, "events")?;
        tx.execute(
            "INSERT INTO events(id,project_id,name,color,start_time_sort_key) VALUES(?,?,?,?,0)",
            params![id, project_id, name, color],
        )?;
        let row = get(&tx, "events", id)?;
        tx.commit()?;
        Ok(row)
    }
    pub fn update_event(&self, id: i64, patch: EventUpdate) -> Result<Event> {
        let conn = self.lock()?;
        patch_row(&conn, "events", id, patch)?;
        get(&conn, "events", id)
    }
    pub fn delete_event(&self, id: i64) -> Result<()> {
        let mut conn = self.lock()?;
        require(&conn, "events", id)?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM event_nodes WHERE event_id=?", [id])?;
        tx.execute("DELETE FROM events WHERE id=?", [id])?;
        tx.commit()?;
        Ok(())
    }
    pub fn upsert_event_node(
        &self,
        event_id: i64,
        target_type: &str,
        target_id: i64,
        description: &str,
    ) -> Result<EventNode> {
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let event: Event = get(&tx, "events", event_id)?;
        if target_project(&tx, target_type, target_id)? != event.project_id {
            return Err(Error::Invalid(
                "event and target belong to different projects".into(),
            ));
        }
        let existing=tx.query_row("SELECT id FROM event_nodes WHERE event_id=? AND target_type=? AND target_id=? LIMIT 1",params![event_id,target_type,target_id],|r|r.get::<_,i64>(0)).optional()?;
        let id = if let Some(id) = existing {
            tx.execute(
                "UPDATE event_nodes SET description=? WHERE id=?",
                params![description, id],
            )?;
            id
        } else {
            let id = identity::allocate(&tx, "event_nodes")?;
            tx.execute("INSERT INTO event_nodes(id,event_id,target_type,target_id,description) VALUES(?,?,?,?,?)",params![id,event_id,target_type,target_id,description])?;
            id
        };
        let row = get(&tx, "event_nodes", id)?;
        tx.commit()?;
        Ok(row)
    }
    pub fn delete_event_node(&self, id: i64) -> Result<()> {
        let conn = self.lock()?;
        require(&conn, "event_nodes", id)?;
        conn.execute("DELETE FROM event_nodes WHERE id=?", [id])?;
        Ok(())
    }
    pub fn event_matrix(&self, project_id: i64) -> Result<EventMatrix> {
        let conn = self.lock()?;
        require(&conn, "projects", project_id)?;
        Ok(EventMatrix {
            events: rows(
                &conn,
                "SELECT * FROM events WHERE project_id=? ORDER BY start_time_sort_key,id",
                [project_id],
            )?,
            nodes: rows(
                &conn,
                "SELECT n.* FROM event_nodes n JOIN events e ON n.event_id=e.id WHERE e.project_id=? ORDER BY n.id",
                [project_id],
            )?,
        })
    }
    pub fn ai_runs(
        &self,
        project_id: i64,
        episode_id: Option<i64>,
        action_key: Option<&str>,
        limit: usize,
    ) -> Result<Vec<AiActionRun>> {
        let conn = self.lock()?;
        require(&conn, "projects", project_id)?;
        rows(
            &conn,
            "SELECT * FROM ai_action_runs WHERE project_id=? AND (? IS NULL OR (target_type='episode' AND target_id=?)) AND (? IS NULL OR action_key=?) ORDER BY created_at DESC,id DESC LIMIT ?",
            params![
                project_id,
                episode_id,
                episode_id,
                action_key,
                action_key,
                limit.clamp(1, 500) as i64
            ],
        )
    }
    pub fn create_ai_run(
        &self,
        project_id: i64,
        episode_id: i64,
        action_key: &str,
        input_text: Option<&str>,
        output_text: &str,
        meta_data: Option<Value>,
    ) -> Result<AiActionRun> {
        nonempty(action_key, "action key")?;
        nonempty(output_text, "output text")?;
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        require(&tx, "projects", project_id)?;
        if target_project(&tx, "episode", episode_id)? != project_id {
            return Err(Error::Invalid("episode belongs to another project".into()));
        }
        let id = identity::allocate(&tx, "ai_action_runs")?;
        tx.execute("INSERT INTO ai_action_runs(id,project_id,target_type,target_id,action_key,input_text,output_text,meta_data,created_at) VALUES(?,?,'episode',?,?,?,?,?,CURRENT_TIMESTAMP)",params![id,project_id,episode_id,action_key,input_text,output_text,meta_data.map(|v|v.to_string())])?;
        let row = get(&tx, "ai_action_runs", id)?;
        tx.commit()?;
        Ok(row)
    }
    pub fn delete_ai_run(&self, id: i64) -> Result<()> {
        let conn = self.lock()?;
        require(&conn, "ai_action_runs", id)?;
        conn.execute("DELETE FROM ai_action_runs WHERE id=?", [id])?;
        Ok(())
    }
}
