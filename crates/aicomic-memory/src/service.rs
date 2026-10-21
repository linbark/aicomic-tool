use crate::*;
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Semantic retrieval is opt-in; original facts and review need no model.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct EmbeddingSettings {
    pub mode: String,
    pub model_dir: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: usize,
}
impl Default for EmbeddingSettings {
    fn default() -> Self {
        Self {
            mode: "disabled".into(),
            model_dir: String::new(),
            base_url: "https://api.openai.com/v1".into(),
            api_key: String::new(),
            model: "text-embedding-3-small".into(),
            max_tokens: 512,
        }
    }
}
impl EmbeddingSettings {
    pub fn load(root: &Path) -> Result<Self> {
        match std::fs::read(root.join("embedding_settings.json")) {
            Ok(bytes) => {
                let settings: Self = serde_json::from_slice(&bytes)?;
                settings.validate()?;
                Ok(settings)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        self.validate()?;
        std::fs::create_dir_all(root)?;
        // tempfile creates private files on Unix and persist replaces atomically on Windows.
        let mut file = tempfile::NamedTempFile::new_in(root)?;
        file.write_all(&serde_json::to_vec_pretty(self)?)?;
        file.as_file().sync_all()?;
        file.persist(root.join("embedding_settings.json"))
            .map_err(|e| Error::Io(e.error))?;
        Ok(())
    }
    fn validate(&self) -> Result<()> {
        match self.mode.as_str() {
            "disabled" => Ok(()),
            "local" if !self.model_dir.trim().is_empty() && self.max_tokens >= 2 => Ok(()),
            "local" => Err(invalid("本地 Embedding 需要模型目录和有效的最大 token 数")),
            "http" => {
                let url = reqwest::Url::parse(&self.base_url)
                    .map_err(|_| invalid("Embedding URL 无效"))?;
                if !["http", "https"].contains(&url.scheme())
                    || url.host_str().is_none()
                    || self.model.trim().is_empty()
                {
                    Err(invalid("HTTP Embedding 需要 HTTP(S) URL 和模型名"))
                } else {
                    Ok(())
                }
            }
            _ => Err(invalid("Embedding mode 必须为 disabled、local 或 http")),
        }
    }
    fn provider(&self) -> Result<Arc<dyn EmbeddingProvider>> {
        self.validate()?;
        match self.mode.as_str() {
            "disabled" => Err(Error::EmbeddingNotConfigured),
            "http" => Ok(Arc::new(OpenAICompatibleEmbedding::new(
                &self.base_url,
                &self.api_key,
                &self.model,
            )?)),
            "local" => {
                #[cfg(feature = "local-bge")]
                {
                    Ok(Arc::new(LocalBgeM3::from_dir(
                        Path::new(&self.model_dir),
                        self.max_tokens,
                    )?))
                }
                #[cfg(not(feature = "local-bge"))]
                {
                    Err(invalid("当前构建未启用 local-bge，无法运行本地 BGE-M3"))
                }
            }
            _ => Err(invalid("Embedding mode 无效")),
        }
    }
}

/// Clone this service to share its lazily loaded provider. Create a new instance
/// after changing settings, so model identity and vector caches stay isolated.
#[derive(Clone)]
pub struct MemoryService {
    root: PathBuf,
    settings: EmbeddingSettings,
    provider: Arc<Mutex<Option<Arc<dyn EmbeddingProvider>>>>,
}
impl MemoryService {
    pub fn new(root: impl AsRef<Path>, settings: EmbeddingSettings) -> Self {
        Self {
            root: root.as_ref().to_owned(),
            settings,
            provider: Arc::new(Mutex::new(None)),
        }
    }
    pub fn settings(&self) -> &EmbeddingSettings {
        &self.settings
    }
    fn loaded_provider(&self) -> Result<Arc<dyn EmbeddingProvider>> {
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| invalid("Embedding 模型锁不可用"))?;
        if let Some(loaded) = provider.as_ref() {
            return Ok(Arc::clone(loaded));
        }
        let loaded = self.settings.provider()?;
        *provider = Some(Arc::clone(&loaded));
        Ok(loaded)
    }
    pub async fn rebuild(&self, project_id: i64) -> Result<usize> {
        let service = self.clone();
        tokio::task::spawn_blocking(move || {
            let provider = service.loaded_provider()?;
            MemoryStore::open_root(&service.root)?.rebuild_vectors(project_id, provider.as_ref())
        })
        .await
        .map_err(|e| invalid(format!("Embedding 后台任务失败: {e}")))?
    }
    pub async fn semantic_search(
        &self,
        project_id: i64,
        query: impl Into<String>,
        top_k: usize,
    ) -> Result<Vec<ScoredMemoryRecord>> {
        let query = query.into();
        let service = self.clone();
        tokio::task::spawn_blocking(move || {
            let provider = service.loaded_provider()?;
            let mut q = MemoryQuery::new(project_id);
            q.query_text = Some(query);
            q.top_k = top_k.clamp(1, 100);
            let store = MemoryStore::open_root(&service.root)?;
            store.refresh_vectors(project_id, provider.as_ref())?;
            let result = store.retrieve_semantic(&q, provider.as_ref())?;
            Ok(result
                .records
                .into_iter()
                .zip(result.scores)
                .map(|(record, score)| ScoredMemoryRecord { record, score })
                .collect())
        })
        .await
        .map_err(|e| invalid(format!("Embedding 后台任务失败: {e}")))?
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScoredMemoryRecord {
    pub record: MemoryRecord,
    pub score: f32,
}
impl MemoryService {
    pub async fn task_context(
        &self,
        project_id: i64,
        task: impl Into<String>,
        entity: Option<String>,
    ) -> Result<TaskMemoryContext> {
        let task = task.into();
        let service = self.clone();
        tokio::task::spawn_blocking(move || {
            let provider = if service.settings.mode == "disabled" {
                None
            } else {
                Some(service.loaded_provider()?)
            };
            let store = MemoryStore::open_root(&service.root)?;
            if let Some(provider) = provider.as_ref() {
                store.refresh_vectors(project_id, provider.as_ref())?;
            }
            store.retrieve_for_task(project_id, &task, entity.as_deref(), provider.as_deref())
        })
        .await
        .map_err(|e| invalid(format!("记忆上下文后台任务失败: {e}")))?
    }
}
impl MemoryService {
    pub async fn ensure_changeset_for_run(
        &self,
        project_id: i64,
        episode_id: Option<i64>,
        run_id: impl Into<String>,
        payload: Value,
    ) -> Result<String> {
        let run_id = run_id.into();
        let service = self.clone();
        tokio::task::spawn_blocking(move || {
            let store = MemoryStore::open_root(&service.root)?;
            if let Some(id) = store.existing_run_changeset(project_id, &run_id, &payload)? {
                return Ok(id);
            }
            if service.settings.mode == "disabled" {
                store.ensure_changeset_for_run(project_id, episode_id, &run_id, &payload)
            } else {
                let provider = service.loaded_provider()?;
                store.refresh_vectors(project_id, provider.as_ref())?;
                store.ensure_changeset_for_run_with_provider(
                    project_id,
                    episode_id,
                    &run_id,
                    &payload,
                    provider.as_ref(),
                )
            }
        })
        .await
        .map_err(|e| invalid(format!("变更集准备后台任务失败: {e}")))?
    }
}
