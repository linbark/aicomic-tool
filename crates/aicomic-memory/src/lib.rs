//! Canonical memory storage for native desktop applications.
//!
//! SQLite remains the source of truth. Embedding is optional and never blocks
//! viewing, editing or reviewing original facts. Qdrant local data is not opened;
//! the native vector cache is rebuilt explicitly from SQLite records.
mod embedding;
mod extraction;
mod indexer;
mod service;
mod store;
mod workflow;
pub use embedding::*;
pub use extraction::normalize_extracted_changeset;
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use service::*;
use std::time::{SystemTime, UNIX_EPOCH};
pub use store::MemoryStore;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("记忆数据库错误: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("记忆数据格式错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("文件错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("语义检索未配置 embedding 模型；原始证据和审核仍可使用")]
    EmbeddingNotConfigured,
    #[error("Embedding 请求失败: {0}")]
    Http(#[from] reqwest::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
/// Legacy deterministic canonical identity (SHA1 of trimmed nonempty parts).
pub fn stable_hex_id(parts: &[&str]) -> String {
    use sha1::Digest;
    let raw = parts
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("|");
    format!("{:x}", sha1::Sha1::digest(raw.as_bytes()))
}
fn confirmed() -> String {
    "confirmed".into()
}
fn system() -> String {
    "system".into()
}
fn one() -> f64 {
    1.0
}
fn ten() -> usize {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    #[serde(default = "new_id")]
    pub id: String,
    pub project_id: i64,
    pub namespace: String,
    #[serde(rename = "type")]
    pub memory_type: String,
    #[serde(default)]
    pub entity: Option<String>,
    pub content: String,
    #[serde(default)]
    pub payload_json: Option<Value>,
    #[serde(default)]
    pub source_ref: Option<String>,
    #[serde(default)]
    pub time_index: Option<String>,
    #[serde(default = "confirmed")]
    pub status: String,
    #[serde(default = "one")]
    pub confidence: f64,
    #[serde(default = "system")]
    pub source_kind: String,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub story_order: Option<String>,
    #[serde(default)]
    pub story_time: Option<Value>,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub created_at_ms: Option<i64>,
}
impl MemoryRecord {
    pub fn new(project_id: i64, namespace: &str, memory_type: &str, content: &str) -> Self {
        Self {
            id: new_id(),
            project_id,
            namespace: namespace.into(),
            memory_type: memory_type.into(),
            entity: None,
            content: content.into(),
            payload_json: None,
            source_ref: None,
            time_index: None,
            status: confirmed(),
            confidence: 1.0,
            source_kind: system(),
            evidence_ids: vec![],
            story_order: None,
            story_time: None,
            hash: None,
            created_at_ms: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryQuery {
    pub project_id: i64,
    #[serde(default)]
    pub query_text: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default, rename = "type")]
    pub memory_type: Option<String>,
    #[serde(default)]
    pub entity: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub min_confidence: Option<f64>,
    #[serde(default)]
    pub time_index_from: Option<String>,
    #[serde(default)]
    pub time_index_to: Option<String>,
    #[serde(default = "ten")]
    pub top_k: usize,
    #[serde(default)]
    pub min_score: Option<f32>,
}
impl MemoryQuery {
    pub fn new(project_id: i64) -> Self {
        Self {
            project_id,
            query_text: None,
            namespace: None,
            memory_type: None,
            entity: None,
            status: None,
            source_kind: None,
            min_confidence: None,
            time_index_from: None,
            time_index_to: None,
            top_k: 100,
            min_score: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRetrievalResult {
    pub records: Vec<MemoryRecord>,
    pub scores: Vec<f32>,
    pub total: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMemoryContext {
    pub retrieval_mode: String,
    pub layers: std::collections::BTreeMap<String, MemoryRetrievalResult>,
    pub conflicts: Vec<Value>,
    pub prompt_fragments: std::collections::BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceIngestOptions {
    pub episode_id: Option<i64>,
    pub scene_id: Option<i64>,
    pub max_quote_chars: usize,
    pub tags: Vec<String>,
}
impl Default for EvidenceIngestOptions {
    fn default() -> Self {
        Self {
            episode_id: None,
            scene_id: None,
            max_quote_chars: 600,
            tags: vec![],
        }
    }
}
