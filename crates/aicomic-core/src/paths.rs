use crate::{Error, Result};
use serde_json::Value;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub data_dir: PathBuf,
    pub database_path: PathBuf,
}

impl AppPaths {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        std::fs::create_dir_all(root.as_ref())?;
        let root = root.as_ref().canonicalize()?;
        let data_dir = safe_join(&root, "data")?;
        let database_path = safe_join(&root, "database.db")?;
        std::fs::create_dir_all(&data_dir)?;
        Ok(Self {
            root,
            data_dir,
            database_path,
        })
    }
    pub fn settings_path(&self) -> PathBuf {
        self.root.join("ai_settings.json")
    }
    pub fn prompts_path(&self) -> PathBuf {
        self.root.join("prompt_templates.json")
    }
    pub fn memory_path(&self) -> PathBuf {
        self.root.join("memory_store.db")
    }
    pub fn project_dir(&self, id: i64) -> Result<PathBuf> {
        safe_join(&self.root, format!("projects/{id}"))
    }
    pub fn context_dir(&self, id: i64) -> Result<PathBuf> {
        safe_join(&self.root, format!("projects/{id}/context"))
    }
    pub fn runs_dir(&self, id: i64) -> Result<PathBuf> {
        safe_join(&self.root, format!("projects/{id}/runs"))
    }
    pub fn resolve(&self, relative: impl AsRef<Path>) -> Result<PathBuf> {
        safe_join(&self.root, relative)
    }
}

pub(crate) fn safe_join(base: &Path, relative: impl AsRef<Path>) -> Result<PathBuf> {
    let raw = relative
        .as_ref()
        .to_str()
        .ok_or_else(|| Error::Invalid("path is not UTF-8".into()))?;
    let normalized = raw.replace('\\', "/");
    if normalized.is_empty() || normalized.contains(':') || normalized.contains('\0') {
        return Err(Error::UnsafePath(raw.into()));
    }
    let rel = Path::new(&normalized);
    let mut result = base.to_path_buf();
    for part in rel.components() {
        match part {
            Component::Normal(p) => result.push(p),
            _ => return Err(Error::UnsafePath(raw.into())),
        }
        if let Ok(meta) = std::fs::symlink_metadata(&result)
            && meta.file_type().is_symlink()
        {
            return Err(Error::UnsafePath(raw.into()));
        }
    }
    Ok(result)
}

pub(crate) fn component(value: &str) -> Result<()> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value
            .chars()
            .any(|c| c == '/' || c == '\\' || c == ':' || c == '\0')
    {
        return Err(Error::UnsafePath(value.into()));
    }
    Ok(())
}

pub(crate) fn read_json(path: &Path) -> Result<Option<Value>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub(crate) fn write_json(path: &Path, value: &Value) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::UnsafePath(path.display().to_string()))?;
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temp, value)?;
    temp.write_all(b"\n")?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| Error::Io(e.error))?;
    Ok(())
}
