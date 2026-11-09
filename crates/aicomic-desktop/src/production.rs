//! Production gates that live above the SQLite schema.
//!
//! Confirmations are stored as one project context document instead of new
//! columns, so old databases keep opening without a migration. The desktop UI
//! derives stage states from real artifacts; gates only record the human
//! "confirmed" step that artifacts alone cannot express.

use aicomic_ai::ContextStore;
use serde::{Deserialize, Serialize};
use serde_json::json;
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProductionState {
    pub schema_version: String,
    pub outline_approved: bool,
    pub assets_frozen: bool,
    pub episodes: BTreeMap<String, EpisodeGates>,
}

impl Default for ProductionState {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION.into(),
            outline_approved: false,
            assets_frozen: false,
            episodes: BTreeMap::new(),
        }
    }
}

impl ProductionState {
    pub fn load(contexts: &ContextStore, project: i64) -> Self {
        contexts
            .get_context(&project.to_string(), CONTEXT_KIND, CONTEXT_VERSION)
            .ok()
            .flatten()
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default()
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
