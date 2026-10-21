use crate::*;

impl Store {
    pub fn read_json(&self, relative: impl AsRef<Path>) -> Result<Option<Value>> {
        paths::read_json(&self.paths.resolve(relative)?)
    }
    pub fn write_json(&self, relative: impl AsRef<Path>, value: &Value) -> Result<()> {
        paths::write_json(&self.paths.resolve(relative)?, value)
    }
    pub fn read_context(&self, project_id: i64, filename: &str) -> Result<Option<Value>> {
        paths::component(filename)?;
        self.project(project_id)?;
        paths::read_json(&paths::safe_join(
            &self.paths.context_dir(project_id)?,
            filename,
        )?)
    }
    pub fn write_context(&self, project_id: i64, filename: &str, value: &Value) -> Result<()> {
        paths::component(filename)?;
        self.project(project_id)?;
        paths::write_json(
            &paths::safe_join(&self.paths.context_dir(project_id)?, filename)?,
            value,
        )
    }
    pub fn read_run(&self, project_id: i64, run_id: &str, filename: &str) -> Result<Option<Value>> {
        paths::component(run_id)?;
        self.project(project_id)?;
        paths::read_json(&paths::safe_join(
            &self.paths.runs_dir(project_id)?,
            format!("{run_id}/{filename}"),
        )?)
    }
    pub fn write_run(
        &self,
        project_id: i64,
        run_id: &str,
        filename: &str,
        value: &Value,
    ) -> Result<()> {
        paths::component(run_id)?;
        self.project(project_id)?;
        paths::write_json(
            &paths::safe_join(
                &self.paths.runs_dir(project_id)?,
                format!("{run_id}/{filename}"),
            )?,
            value,
        )
    }
    pub fn snapshot_run(
        &self,
        project_id: i64,
        run_id: &str,
        request: &Value,
        response: &Value,
        meta: &Value,
    ) -> Result<()> {
        self.write_run(project_id, run_id, "request.json", request)?;
        self.write_run(project_id, run_id, "response.json", response)?;
        self.write_run(project_id, run_id, "meta.json", meta)
    }
    pub fn snapshot_stage(
        &self,
        project_id: i64,
        run_id: &str,
        stage_name: &str,
        value: &Value,
    ) -> Result<()> {
        paths::component(stage_name)?;
        self.write_run(
            project_id,
            run_id,
            &format!("stages/{stage_name}.json"),
            value,
        )
    }
    pub fn list_run_files(&self, project_id: i64) -> Result<Vec<RunFile>> {
        self.project(project_id)?;
        let path = self.paths.runs_dir(project_id)?;
        if !path.exists() {
            return Ok(vec![]);
        }
        let mut output = vec![];
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let id = entry.file_name().to_string_lossy().into_owned();
                output.push(RunFile {
                    request: self.read_run(project_id, &id, "request.json")?,
                    response: self.read_run(project_id, &id, "response.json")?,
                    meta: self.read_run(project_id, &id, "meta.json")?,
                    run_id: id,
                });
            }
        }
        output.sort_by(|a, b| b.run_id.cmp(&a.run_id));
        Ok(output)
    }
    pub fn list_stages(&self, project_id: i64, run_id: &str) -> Result<Vec<StageFile>> {
        paths::component(run_id)?;
        self.project(project_id)?;
        let path = paths::safe_join(
            &self.paths.runs_dir(project_id)?,
            format!("{run_id}/stages"),
        )?;
        if !path.exists() {
            return Ok(vec![]);
        }
        let mut output = vec![];
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|e| e == "json")
            {
                let filename = entry.file_name().to_string_lossy().into_owned();
                let value = self
                    .read_run(project_id, run_id, &format!("stages/{filename}"))?
                    .unwrap_or(Value::Null);
                let text = value
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string());
                let preview = text.chars().take(300).collect();
                let timestamp = entry
                    .metadata()?
                    .modified()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                output.push(StageFile {
                    name: filename.trim_end_matches(".json").into(),
                    preview,
                    timestamp,
                });
            }
        }
        output.sort_by(|a, b| a.timestamp.cmp(&b.timestamp).then(a.name.cmp(&b.name)));
        Ok(output)
    }
}
