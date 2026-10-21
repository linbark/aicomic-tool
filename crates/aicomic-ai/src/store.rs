use crate::{AiError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
static FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub(crate) fn component(s: &str) -> Result<&str> {
    if s.is_empty()
        || s == "."
        || s == ".."
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(AiError::InvalidInput(format!("无效的文件标识：{s}")));
    }
    Ok(s)
}
pub(crate) fn read_json(path: &Path) -> Result<Option<Value>> {
    match fs::read(path) {
        Ok(data) => Ok(Some(serde_json::from_slice(&data)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub(crate) fn write_json(path: &Path, value: &Value) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AiError::InvalidInput("缺少文件目录".into()))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".write-{}-{}-{}.tmp",
        std::process::id(),
        now_ms(),
        FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[derive(Clone, Debug)]
pub struct ContextStore {
    root: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StageMeta {
    pub name: String,
    pub timestamp: f64,
    pub preview: String,
}
impl ContextStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    fn project_dir(&self, project: &str) -> Result<PathBuf> {
        Ok(self.root.join("projects").join(component(project)?))
    }
    pub fn run_dir(&self, project: &str, run: &str) -> Result<PathBuf> {
        Ok(self
            .project_dir(project)?
            .join("runs")
            .join(component(run)?))
    }
    pub fn context_path(&self, project: &str, kind: &str, version: &str) -> Result<PathBuf> {
        Ok(self.project_dir(project)?.join("context").join(format!(
            "{}.{}.json",
            component(kind)?,
            component(version)?
        )))
    }
    pub fn put_context(
        &self,
        project: &str,
        kind: &str,
        version: &str,
        data: &Value,
    ) -> Result<()> {
        if !data.is_object() {
            return Err(AiError::InvalidInput("Context 必须为 JSON object".into()));
        }
        write_json(&self.context_path(project, kind, version)?, data)
    }
    pub fn get_context(&self, project: &str, kind: &str, version: &str) -> Result<Option<Value>> {
        read_json(&self.context_path(project, kind, version)?)
    }
    pub fn stage_path(&self, project: &str, run: &str, stage: &str) -> Result<PathBuf> {
        let safe: String = stage
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
            .collect();
        Ok(self.run_dir(project, run)?.join("stages").join(format!(
            "{}.json",
            if safe.is_empty() { "stage" } else { &safe }
        )))
    }
    pub fn write_stage(&self, project: &str, run: &str, stage: &str, data: &Value) -> Result<()> {
        write_json(&self.stage_path(project, run, stage)?, data)
    }
    pub fn read_stage(&self, project: &str, run: &str, stage: &str) -> Result<Option<Value>> {
        read_json(&self.stage_path(project, run, stage)?)
    }
    pub fn list_stages(&self, project: &str, run: &str) -> Result<Vec<StageMeta>> {
        let dir = self.run_dir(project, run)?.join("stages");
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut stages = Vec::new();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let modified = fs::metadata(&path)?
                .modified()?
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            let data = read_json(&path)?.unwrap_or(Value::Null);
            stages.push(StageMeta {
                name: path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                timestamp: modified,
                preview: data.to_string().chars().take(300).collect(),
            });
        }
        stages.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(stages)
    }
    pub fn snapshot_run(
        &self,
        project: &str,
        run: &str,
        request: &Value,
        response: &Value,
        meta: &Value,
    ) -> Result<()> {
        let dir = self.run_dir(project, run)?;
        write_json(&dir.join("request.json"), request)?;
        write_json(&dir.join("response.json"), response)?;
        let old = read_json(&dir.join("meta.json"))?;
        let project_value = project
            .parse::<i64>()
            .map(|n| json!(n))
            .unwrap_or(json!(project));
        let mut final_meta = json!({"project_id":project_value, "run_id":run,"created_at_ms": old.as_ref().and_then(|v| v["created_at_ms"].as_u64()).unwrap_or_else(now_ms)});
        if let Some(object) = meta.as_object() {
            for (key, value) in object {
                final_meta[key] = value.clone();
            }
        }
        write_json(&dir.join("meta.json"), &final_meta)
    }
    pub fn read_run(&self, project: &str, run: &str) -> Result<Option<Value>> {
        let dir = self.run_dir(project, run)?;
        if !dir.exists() {
            return Ok(None);
        }
        Ok(Some(
            json!({"request":read_json(&dir.join("request.json"))?.unwrap_or(json!({})), "response":read_json(&dir.join("response.json"))?.unwrap_or(json!({})),"meta":read_json(&dir.join("meta.json"))?.unwrap_or(json!({}))}),
        ))
    }
    pub fn list_runs(&self, project: &str) -> Result<Vec<Value>> {
        let dir = self.project_dir(project)?.join("runs");
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut runs = Vec::new();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir()
                && let Some(meta) = read_json(&path.join("meta.json"))?
            {
                runs.push(meta);
            }
        }
        runs.sort_by_key(|v| std::cmp::Reverse(v["created_at_ms"].as_u64().unwrap_or(0)));
        Ok(runs)
    }
}
