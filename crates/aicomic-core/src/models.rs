use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Project {
    pub id: i64,
    pub uuid: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Episode {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub order: i64,
    pub description: Option<String>,
    pub action_text: Option<String>,
    pub prompt: Option<String>,
    pub script_locked: bool,
    pub script_locked_at: Option<String>,
    pub last_exec_run_id: Option<String>,
    pub exec_status: String,
    pub exec_artifacts: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EpisodeUpdate {
    pub title: Option<String>,
    pub order: Option<i64>,
    pub description: Option<String>,
    pub action_text: Option<String>,
    pub prompt: Option<String>,
    pub script_locked: Option<bool>,
    pub script_locked_at: Option<String>,
    pub last_exec_run_id: Option<String>,
    pub exec_status: Option<String>,
    pub exec_artifacts: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Scene {
    pub id: i64,
    pub episode_id: i64,
    pub sequence_number: i64,
    pub title: String,
    pub description: Option<String>,
    pub action_text: Option<String>,
    pub dialogue: Option<String>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SceneUpdate {
    pub title: Option<String>,
    pub sequence_number: Option<i64>,
    pub description: Option<String>,
    pub action_text: Option<String>,
    pub dialogue: Option<String>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Shot {
    pub id: i64,
    pub scene_id: i64,
    pub sequence_number: i64,
    pub title: String,
    pub action_text: Option<String>,
    pub dialogue: Option<String>,
    pub prompt: Option<String>,
    pub negative_prompt: Option<String>,
    pub selected_asset_id: Option<i64>,
    pub status: String,
    pub video_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShotUpdate {
    pub title: Option<String>,
    pub sequence_number: Option<i64>,
    pub action_text: Option<String>,
    pub dialogue: Option<String>,
    pub prompt: Option<String>,
    pub negative_prompt: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AssetItem {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub base_prompt: Option<String>,
    pub negative_prompt: Option<String>,
    pub category: String,
    pub avatar_asset_id: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssetItemUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub base_prompt: Option<String>,
    pub negative_prompt: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Asset {
    pub id: i64,
    pub character_id: Option<i64>,
    pub shot_id: Option<i64>,
    pub file_path: String,
    pub file_type: String,
    pub meta_data: Option<Value>,
    pub created_at: String,
    pub is_favorite: bool,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum AssetTarget {
    Item(i64),
    Shot(i64),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Event {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub color: String,
    pub start_time_sort_key: i64,
    pub description: Option<String>,
    pub graph_data: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventUpdate {
    pub name: Option<String>,
    pub color: Option<String>,
    pub start_time_sort_key: Option<i64>,
    pub description: Option<String>,
    pub graph_data: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EventNode {
    pub id: i64,
    pub event_id: i64,
    pub target_type: String,
    pub target_id: i64,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMatrix {
    pub events: Vec<Event>,
    pub nodes: Vec<EventNode>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AiActionRun {
    pub id: i64,
    pub project_id: i64,
    pub target_type: String,
    pub target_id: i64,
    pub action_key: String,
    pub input_text: Option<String>,
    pub output_text: String,
    pub meta_data: Option<Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeTree {
    #[serde(flatten)]
    pub episode: Episode,
    pub scenes: Vec<SceneTree>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneTree {
    #[serde(flatten)]
    pub scene: Scene,
    pub shots: Vec<ShotTree>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotTree {
    #[serde(flatten)]
    pub shot: Shot,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunFile {
    pub run_id: String,
    pub request: Option<Value>,
    pub response: Option<Value>,
    pub meta: Option<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageFile {
    pub name: String,
    pub preview: String,
    pub timestamp: u64,
}
