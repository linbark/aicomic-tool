//! Production gates that live above the SQLite schema.
//!
//! Confirmations are stored as one project context document instead of new
//! columns, so old databases keep opening without a migration. The desktop UI
//! derives stage states from real artifacts; gates only record the human
//! "confirmed" step that artifacts alone cannot express.

use aicomic_ai::ContextStore;
use aicomic_core::Store;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const CONTEXT_KIND: &str = "production_state";
pub const CONTEXT_VERSION: &str = "v1";
pub const SCHEMA_VERSION: &str = "production.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Outline,
    Assets,
    Structure,
    Storyboard,
}

impl Gate {
    pub fn label(self) -> &'static str {
        match self {
            Gate::Outline => "确认大纲",
            Gate::Assets => "冻结资产",
            Gate::Structure => "确认结构",
            Gate::Storyboard => "确认定稿",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageState {
    Done,
    Review,
    Active,
    Empty,
}

impl StageState {
    pub fn label(self) -> &'static str {
        match self {
            StageState::Done => "已完成",
            StageState::Review => "待确认",
            StageState::Active => "进行中",
            StageState::Empty => "未开始",
        }
    }

    pub fn tone(self) -> crate::ui::Tone {
        match self {
            StageState::Done => crate::ui::Tone::Mint,
            StageState::Review => crate::ui::Tone::Gold,
            StageState::Active => crate::ui::Tone::Indigo,
            StageState::Empty => crate::ui::Tone::Muted,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct EpisodeGates {
    pub structure_approved: bool,
    pub storyboard_approved: bool,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProductionState {
    pub schema_version: String,
    pub outline_approved: bool,
    pub assets_frozen: bool,
    pub episodes: BTreeMap<String, EpisodeGates>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_versions: Option<InputVersions>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct InputVersions {
    bible: String,
    outline: String,
    assets: String,
    episodes: BTreeMap<String, EpisodeVersions>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct EpisodeVersions {
    script: String,
    structure: String,
    storyboard: String,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

fn revision(value: impl Serialize) -> Result<String, String> {
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

impl InputVersions {
    fn read(store: &Store, project: i64) -> Result<Self, String> {
        let contexts = ContextStore::new(store.paths().root.clone());
        let context = |kind: &str| {
            contexts
                .get_context(&project.to_string(), kind, "v1")
                .map_err(|error| error.to_string())
        };
        let mut assets = Vec::new();
        for item in store
            .asset_items(project)
            .map_err(|error| error.to_string())?
        {
            let media = store
                .assets_for_item(item.id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|asset| {
                    json!({"id":asset.id,"path":asset.file_path,
                    "type":asset.file_type,"metadata":asset.meta_data})
                })
                .collect::<Vec<_>>();
            let dna = context(&format!("visual_dna.asset_item_{}", item.id))?
                .unwrap_or_else(|| json!({}));
            assets.push(
                json!({"id":item.id,"name":item.name,"category":item.category,
                "description":item.description.as_deref().unwrap_or(""),
                "base_prompt":item.base_prompt.as_deref().unwrap_or(""),
                "negative_prompt":item.negative_prompt.as_deref().unwrap_or(""),
                "avatar":item.avatar_asset_id,"media":media,"visual_dna":dna}),
            );
        }
        let mut episodes = BTreeMap::new();
        for episode in store.episodes(project).map_err(|error| error.to_string())? {
            let artifacts = episode
                .exec_artifacts
                .as_ref()
                .cloned()
                .unwrap_or_else(|| json!({}));
            episodes.insert(episode.id.to_string(),EpisodeVersions {
                script:revision(episode.description.as_deref().unwrap_or(""))?,
                structure:revision(json!({"outline":artifacts.get("outline"),"beat_sheet":artifacts.get("beat_sheet")}))?,
                storyboard:revision(artifacts.get("shots"))?,
                extra:BTreeMap::new(),
            });
        }
        Ok(Self {
            bible: revision(context("series_bible")?.unwrap_or_else(|| json!({})))?,
            outline: revision(context("project_outline")?.unwrap_or_else(|| json!({})))?,
            assets: revision(assets)?,
            episodes,
            extra: BTreeMap::new(),
        })
    }
}

impl Default for ProductionState {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION.into(),
            outline_approved: false,
            assets_frozen: false,
            episodes: BTreeMap::new(),
            input_versions: None,
            extra: BTreeMap::new(),
        }
    }
}

impl ProductionState {
    #[cfg(test)]
    pub fn load(contexts: &ContextStore, project: i64) -> Self {
        Self::try_load(contexts, project).unwrap_or_default()
    }

    pub fn try_load(contexts: &ContextStore, project: i64) -> Result<Self, String> {
        let state: Self = contexts
            .get_context(&project.to_string(), CONTEXT_KIND, CONTEXT_VERSION)
            .map_err(|error| error.to_string())?
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        if state.schema_version != SCHEMA_VERSION {
            return Err("制作状态版本不受支持，未覆盖原文件".into());
        }
        Ok(state)
    }

    /// Bind legacy confirmations on their first read, then revoke confirmations
    /// only when their real persisted inputs change, regardless of the UI path.
    pub fn refresh(store: &Store, project: i64) -> Result<Self, String> {
        let contexts = ContextStore::new(store.paths().root.clone());
        let mut state = Self::try_load(&contexts, project)?;
        let previous = state.clone();
        let mut next = InputVersions::read(store, project)?;
        if let Some(old) = state.input_versions.clone() {
            next.extra = old.extra.clone();
            for (episode, versions) in &mut next.episodes {
                if let Some(previous) = old.episodes.get(episode) {
                    versions.extra = previous.extra.clone();
                }
            }
            if old.bible != next.bible {
                state.outline_approved = false;
                state.assets_frozen = false;
                state.invalidate_structures();
            }
            if old.outline != next.outline {
                state.outline_approved = false;
                state.invalidate_structures();
            }
            if old.assets != next.assets {
                state.assets_frozen = false;
                state.invalidate_storyboards();
            }
            for (episode, sources) in &next.episodes {
                if let Some(old) = old.episodes.get(episode) {
                    let gates = state.episodes.entry(episode.clone()).or_default();
                    if old.script != sources.script || old.structure != sources.structure {
                        gates.structure_approved = false;
                        gates.storyboard_approved = false;
                    }
                    if old.storyboard != sources.storyboard {
                        gates.storyboard_approved = false;
                    }
                }
            }
        }
        state.input_versions = Some(next);
        if state != previous {
            state.save(&contexts, project)?;
        }
        Ok(state)
    }

    fn invalidate_structures(&mut self) {
        for gates in self.episodes.values_mut() {
            gates.structure_approved = false;
            gates.storyboard_approved = false;
        }
    }

    fn invalidate_storyboards(&mut self) {
        for gates in self.episodes.values_mut() {
            gates.storyboard_approved = false;
        }
    }

    pub fn save(&self, contexts: &ContextStore, project: i64) -> Result<(), String> {
        contexts
            .put_context(
                &project.to_string(),
                CONTEXT_KIND,
                CONTEXT_VERSION,
                &json!(self),
            )
            .map_err(|error| error.to_string())
    }

    pub fn episode(&self, episode: i64) -> EpisodeGates {
        self.episodes
            .get(&episode.to_string())
            .cloned()
            .unwrap_or_default()
    }

    pub fn episode_mut(&mut self, episode: i64) -> &mut EpisodeGates {
        self.episodes.entry(episode.to_string()).or_default()
    }

    pub fn confirmed(&self, episode: i64, gate: Gate) -> bool {
        match gate {
            Gate::Outline => self.outline_approved,
            Gate::Assets => self.assets_frozen,
            Gate::Structure => self.episode(episode).structure_approved,
            Gate::Storyboard => self.episode(episode).storyboard_approved,
        }
    }

    pub fn set(&mut self, episode: i64, gate: Gate, confirmed: bool) {
        match gate {
            Gate::Outline => self.outline_approved = confirmed,
            Gate::Assets => self.assets_frozen = confirmed,
            Gate::Structure => self.episode_mut(episode).structure_approved = confirmed,
            Gate::Storyboard => self.episode_mut(episode).storyboard_approved = confirmed,
        }
    }

    /// Drop the confirmation for a gate because its underlying artifact changed.
    pub fn invalidate(&mut self, episode: i64, gate: Gate) {
        if self.confirmed(episode, gate) {
            self.set(episode, gate, false);
        }
    }
}

pub fn has_structure(artifacts: &Value) -> bool {
    artifacts["outline"]
        .as_object()
        .is_some_and(|value| !value.is_empty())
        || artifacts["outline"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty())
        || artifacts["beat_sheet"]
            .as_array()
            .is_some_and(|value| !value.is_empty())
}

/// Writing/revision actions remain available before locking. Production actions
/// consume saved, confirmed inputs and validate them again at confirmation time.
pub fn validate_generation(
    store: &Store,
    key: crate::session::EpisodeKey,
    action: &str,
) -> Result<(), String> {
    let needs_script = matches!(
        action,
        "episode_outline_generate"
            | "episode_assets_visual_dna"
            | "workflow_storyboard"
            | "episode_execute"
    );
    if !needs_script {
        return Ok(());
    }
    let episode = store
        .episode(key.episode)
        .map_err(|error| error.to_string())?;
    if episode.project_id != key.project {
        return Err("生成分集不属于原项目".into());
    }
    if !episode.script_locked
        || episode
            .description
            .as_deref()
            .is_none_or(|script| script.trim().is_empty())
    {
        return Err("请先保存并锁定本集剧本，再开始制作生成".into());
    }
    let production = ProductionState::refresh(store, key.project)?;
    if matches!(action, "episode_assets_visual_dna" | "workflow_storyboard") {
        let artifacts = episode
            .exec_artifacts
            .as_ref()
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !has_structure(&artifacts) || !production.confirmed(key.episode, Gate::Structure) {
            return Err("请先完成结构拆解并确认本集结构".into());
        }
    }
    if action == "workflow_storyboard"
        && (!production.assets_frozen
            || store
                .asset_items(key.project)
                .map_err(|error| error.to_string())?
                .is_empty())
    {
        return Err("请先准备并冻结项目资产，再生成分镜".into());
    }
    Ok(())
}

pub struct ScriptSave {
    pub unlocked: bool,
}

/// Every editor save path uses the same original-target validation, atomic SQL
/// script/unlock write and input-version reconciliation. Failed saves stay dirty.
pub fn save_script(
    store: &Store,
    key: crate::session::EpisodeKey,
    title: &str,
    script: &str,
) -> Result<ScriptSave, String> {
    let previous = store
        .episode(key.episode)
        .map_err(|error| error.to_string())?;
    if previous.project_id != key.project {
        return Err("保存分集不属于原项目".into());
    }
    ProductionState::refresh(store, key.project)?;
    let saved = store
        .update_episode(key.episode, title, Some(script))
        .map_err(|error| error.to_string())?;
    ProductionState::refresh(store, key.project)?;
    Ok(ScriptSave {
        unlocked: previous.script_locked && !saved.script_locked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_unconfirmed_and_round_trips_episode_gates() {
        let mut state = ProductionState::default();
        assert!(!state.outline_approved);
        assert!(!state.assets_frozen);
        assert!(!state.confirmed(7, Gate::Structure));

        state.set(7, Gate::Outline, true);
        state.set(7, Gate::Assets, true);
        state.set(7, Gate::Structure, true);
        state.set(7, Gate::Storyboard, true);
        assert!(state.confirmed(7, Gate::Structure));
        assert!(!state.confirmed(8, Gate::Structure));

        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["schema_version"], SCHEMA_VERSION);
        assert_eq!(json["episodes"]["7"]["structure_approved"], true);
        let restored: ProductionState = serde_json::from_value(json).unwrap();
        assert_eq!(restored, state);
    }

    #[test]
    fn missing_or_older_documents_fall_back_to_unconfirmed() {
        let state: ProductionState = serde_json::from_value(json!({})).unwrap();
        assert_eq!(state, ProductionState::default());
        assert_eq!(state.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn persisted_input_versions_invalidate_only_their_real_dependents_and_keep_unknown_fields() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let project = store.create_project("版本对账", None).unwrap();
        let episodes = [
            store
                .create_episode(project.id, "第一集", Some("正文1"))
                .unwrap()
                .id,
            store
                .create_episode(project.id, "第二集", Some("正文2"))
                .unwrap()
                .id,
        ];
        let item = store
            .create_asset_item(project.id, "角色", "persona_visual", Some("主角"))
            .unwrap();
        let contexts = ContextStore::new(root.path().into());
        contexts
            .put_context(
                &project.id.to_string(),
                CONTEXT_KIND,
                CONTEXT_VERSION,
                &json!({"schema_version":SCHEMA_VERSION,"unknown_future":{"preserve":true},
                "episodes":{episodes[0].to_string():{"custom_annotation":{"keep":"nested"}}}}),
            )
            .unwrap();
        let confirm_all = |mut state: ProductionState| {
            state.set(0, Gate::Outline, true);
            state.set(0, Gate::Assets, true);
            for episode in episodes {
                state.set(episode, Gate::Structure, true);
                state.set(episode, Gate::Storyboard, true);
            }
            state.save(&contexts, project.id).unwrap();
            state
        };
        let state = confirm_all(ProductionState::refresh(&store, project.id).unwrap());
        assert_eq!(ProductionState::refresh(&store, project.id).unwrap(), state);
        let mut raw = contexts
            .get_context(&project.id.to_string(), CONTEXT_KIND, CONTEXT_VERSION)
            .unwrap()
            .unwrap();
        raw["input_versions"]["custom_version_note"] = json!({"keep":"version extension"});
        raw["input_versions"]["episodes"][episodes[0].to_string()]["custom_episode_note"] =
            json!(["keep", "episode extension"]);
        contexts
            .put_context(&project.id.to_string(), CONTEXT_KIND, CONTEXT_VERSION, &raw)
            .unwrap();
        contexts
            .put_context(
                &project.id.to_string(),
                "project_outline",
                "v1",
                &json!({"title":"新版大纲"}),
            )
            .unwrap();
        let state = ProductionState::refresh(&store, project.id).unwrap();
        assert!(!state.outline_approved);
        assert!(state.assets_frozen);
        assert!(
            episodes
                .into_iter()
                .all(|episode| !state.confirmed(episode, Gate::Structure)
                    && !state.confirmed(episode, Gate::Storyboard))
        );
        let state = confirm_all(state);
        contexts
            .put_context(
                &project.id.to_string(),
                "project_outline",
                "v1",
                &json!({"title":"新版大纲"}),
            )
            .unwrap();
        assert_eq!(
            ProductionState::refresh(&store, project.id).unwrap(),
            state,
            "same-value saves preserve confirmations"
        );
        contexts
            .put_context(
                &project.id.to_string(),
                &format!("visual_dna.asset_item_{}", item.id),
                "v1",
                &json!({"visual_dna":{"hair":"black"}}),
            )
            .unwrap();
        let state = ProductionState::refresh(&store, project.id).unwrap();
        assert!(state.outline_approved);
        assert!(!state.assets_frozen);
        assert!(
            episodes
                .into_iter()
                .all(|episode| state.confirmed(episode, Gate::Structure)
                    && !state.confirmed(episode, Gate::Storyboard))
        );
        confirm_all(state);
        contexts
            .put_context(
                &project.id.to_string(),
                "series_bible",
                "v1",
                &json!({"world_rules":{"magic":"允许"}}),
            )
            .unwrap();
        let state = ProductionState::refresh(&store, project.id).unwrap();
        assert!(!state.outline_approved && !state.assets_frozen);
        assert!(
            episodes
                .into_iter()
                .all(|episode| !state.confirmed(episode, Gate::Structure)
                    && !state.confirmed(episode, Gate::Storyboard))
        );
        assert_eq!(
            contexts
                .get_context(&project.id.to_string(), CONTEXT_KIND, CONTEXT_VERSION)
                .unwrap()
                .unwrap()["unknown_future"],
            json!({"preserve":true})
        );
        let raw = contexts
            .get_context(&project.id.to_string(), CONTEXT_KIND, CONTEXT_VERSION)
            .unwrap()
            .unwrap();
        assert_eq!(
            raw["episodes"][episodes[0].to_string()]["custom_annotation"],
            json!({"keep":"nested"})
        );
        assert_eq!(
            raw["input_versions"]["custom_version_note"],
            json!({"keep":"version extension"})
        );
        assert_eq!(
            raw["input_versions"]["episodes"][episodes[0].to_string()]["custom_episode_note"],
            json!(["keep", "episode extension"])
        );
    }

    #[test]
    fn invalidating_a_gate_only_clears_that_episode() {
        let mut state = ProductionState::default();
        state.set(1, Gate::Structure, true);
        state.set(2, Gate::Structure, true);
        state.set(1, Gate::Storyboard, true);
        state.invalidate(1, Gate::Structure);
        assert!(!state.confirmed(1, Gate::Structure));
        assert!(state.confirmed(1, Gate::Storyboard));
        assert!(state.confirmed(2, Gate::Structure));
    }
}
