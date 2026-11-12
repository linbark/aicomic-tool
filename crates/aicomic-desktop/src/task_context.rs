//! The desktop Adapter keeps current settings and task retrieval fresh for every
//! model request, including repair calls and resumed Runs, without AI depending
//! on the core or memory crates.

use crate::activity::{self, ProjectActivityLease};
use crate::integration;
use crate::session::EpisodeKey;
use aicomic_ai::{AiError, ExecutionInput, TaskContextProvider};
use aicomic_core::Store;
use aicomic_memory::MemoryService;
use serde_json::Value;
use std::{fmt, future::Future, pin::Pin};

#[derive(Clone)]
pub struct CurrentTaskContext {
    store: Store,
    memory: MemoryService,
    activity: Option<ProjectActivityLease>,
    #[cfg(test)]
    before_synchronize: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl CurrentTaskContext {
    pub fn new(store: Store, memory: MemoryService) -> Self {
        Self {
            store,
            memory,
            activity: None,
            #[cfg(test)]
            before_synchronize: None,
        }
    }

    pub fn with_activity(mut self, activity: ProjectActivityLease) -> Self {
        self.activity = Some(activity);
        self
    }
}

impl fmt::Debug for CurrentTaskContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CurrentTaskContext(<local project settings>)")
    }
}

pub async fn synchronize(
    store: Store,
    key: EpisodeKey,
    expected_visual: Option<Value>,
) -> Result<usize, String> {
    synchronize_with_activity(
        store,
        key,
        expected_visual,
        None,
        #[cfg(test)]
        None,
    )
    .await
}

async fn synchronize_with_activity(
    store: Store,
    key: EpisodeKey,
    expected_visual: Option<Value>,
    lease: Option<ProjectActivityLease>,
    #[cfg(test)] before: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
) -> Result<usize, String> {
    if lease
        .as_ref()
        .is_some_and(|lease| lease.project() != key.project)
    {
        return Err("后台任务租约与原项目不一致".into());
    }
    activity::run_blocking(lease, move || {
        #[cfg(test)]
        if let Some(before) = before {
            before();
        }
        integration::checked_episode(&store, key)?;
        if let Some(expected) = expected_visual {
            crate::production::validate_generation(&store, key, "workflow_storyboard")?;
            if integration::locked_visual_dna(&store, key.project)? != expected {
                return Err("已确认的视觉输入已变化，请重新确认生成".into());
            }
        }
        integration::sync_current_settings(&store, key.project)
    })
    .await
    .map_err(|error| error.to_string())?
}

impl TaskContextProvider for CurrentTaskContext {
    fn retrieve<'a>(
        &'a self,
        input: &'a ExecutionInput,
        _stage: &'a str,
    ) -> Pin<Box<dyn Future<Output = aicomic_ai::Result<Value>> + Send + 'a>> {
        Box::pin(async move {
            let project = input
                .project_id
                .parse::<i64>()
                .map_err(|_| AiError::InvalidInput("任务上下文必须使用原项目数字主键".into()))?;
            let key = EpisodeKey {
                project,
                episode: input.episode_id.unwrap_or(0),
            };
            let expected_visual = if input.action == "workflow_storyboard" {
                input.context.get("locked_visual_dna").cloned()
            } else {
                None
            };
            synchronize_with_activity(
                self.store.clone(),
                key,
                expected_visual,
                self.activity.clone(),
                #[cfg(test)]
                self.before_synchronize.clone(),
            )
            .await
            .map_err(AiError::InvalidState)?;
            let memory = self.memory.clone();
            let script = input.script.clone();
            let retrieval = async move { memory.task_context(project, script, None).await };
            let context = if let Some(lease) = self.activity.clone() {
                activity::finish(lease, retrieval)
                    .await
                    .map_err(AiError::InvalidState)?
            } else {
                retrieval.await
            }
            .map_err(|error| AiError::InvalidState(error.to_string()))?;
            serde_json::to_value(context).map_err(AiError::from)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aicomic_memory::EmbeddingSettings;
    use serde_json::json;

    #[test]
    fn current_adapter_keeps_the_actual_sync_worker_active_after_retrieve_is_aborted() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let project = store.create_project("写入租约", None).unwrap();
        let episode = store
            .create_episode(project.id, "第一集", Some("原文"))
            .unwrap();
        let tracker = crate::activity::ProjectActivityTracker::default();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = std::sync::Mutex::new(release_rx);
        let mut provider = CurrentTaskContext::new(
            store,
            MemoryService::new(root.path(), EmbeddingSettings::default()),
        )
        .with_activity(tracker.enter(project.id).unwrap());
        provider.before_synchronize = Some(std::sync::Arc::new(move || {
            started_tx.send(()).unwrap();
            release_rx.lock().unwrap().recv().unwrap();
        }));
        let input = ExecutionInput {
            project_id: project.id.to_string(),
            episode_id: Some(episode.id),
            script: "原文".into(),
            context: json!({}),
            action: "script_optimize".into(),
            run_id: "cancelled-context".into(),
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let task = runtime.spawn(async move { provider.retrieve(&input, "writer").await });
        started_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        task.abort();
        assert!(runtime.block_on(task).unwrap_err().is_cancelled());
        let still_active = tracker.is_active(project.id);
        release_tx.send(()).unwrap();
        runtime.shutdown_timeout(std::time::Duration::from_secs(5));
        assert!(
            still_active,
            "the worker, not the dropped Adapter, owns the lease"
        );
        assert!(!tracker.is_active(project.id));
    }

    #[test]
    fn activity_bound_adapter_rejects_another_projects_task_before_synchronizing() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let project = store.create_project("原项目", None).unwrap();
        let other = store.create_project("其它项目", None).unwrap();
        let tracker = crate::activity::ProjectActivityTracker::default();
        let provider = CurrentTaskContext::new(
            store,
            MemoryService::new(root.path(), EmbeddingSettings::default()),
        )
        .with_activity(tracker.enter(project.id).unwrap());
        let input = ExecutionInput {
            project_id: other.id.to_string(),
            episode_id: None,
            script: "不可跨项目".into(),
            context: json!({}),
            action: "project_outline_generate".into(),
            run_id: "wrong-project".into(),
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(
            runtime
                .block_on(provider.retrieve(&input, "architect"))
                .is_err()
        );
        assert!(tracker.is_active(project.id));
        assert!(!tracker.is_active(other.id));
        drop(provider);
        assert!(!tracker.any_active());
    }

    #[test]
    fn current_adapter_rejects_a_changed_frozen_visual_snapshot_before_the_next_request() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let project = store.create_project("冻结快照", None).unwrap();
        let episode = store
            .create_episode(project.id, "第一集", Some("原文"))
            .unwrap();
        let item = store
            .create_asset_item(project.id, "角色", "persona_visual", None)
            .unwrap();
        let dna = json!({"visual_dna":{"eyes":"blue"}});
        store
            .write_context(
                project.id,
                &format!("visual_dna.asset_item_{}.v1.json", item.id),
                &dna,
            )
            .unwrap();
        let input = ExecutionInput {
            project_id: project.id.to_string(),
            episode_id: Some(episode.id),
            script: "原文".into(),
            context: json!({"locked_visual_dna":[{"name":"角色","category":"persona_visual","visual_dna":dna}]}),
            action: "workflow_storyboard".into(),
            run_id: "frozen-source".into(),
        };
        store
            .patch_episode(
                episode.id,
                aicomic_core::EpisodeUpdate {
                    script_locked: Some(true),
                    exec_artifacts: Some(json!({"outline":{"logline":"结构"}})),
                    ..aicomic_core::EpisodeUpdate::default()
                },
            )
            .unwrap();
        let contexts = aicomic_ai::ContextStore::new(root.path().into());
        let mut production =
            crate::production::ProductionState::refresh(&store, project.id).unwrap();
        production.set(episode.id, crate::production::Gate::Structure, true);
        production.set(0, crate::production::Gate::Assets, true);
        production.save(&contexts, project.id).unwrap();
        let provider = CurrentTaskContext::new(
            store.clone(),
            MemoryService::new(root.path(), EmbeddingSettings::default()),
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(
            runtime
                .block_on(provider.retrieve(&input, "shot_spec"))
                .is_ok()
        );
        production.set(0, crate::production::Gate::Assets, false);
        production.save(&contexts, project.id).unwrap();
        assert!(
            runtime
                .block_on(provider.retrieve(&input, "prompt_translate"))
                .is_err(),
            "revoked production gates must also prevent the next request"
        );
        production.set(0, crate::production::Gate::Assets, true);
        production.save(&contexts, project.id).unwrap();
        store
            .write_context(
                project.id,
                &format!("visual_dna.asset_item_{}.v1.json", item.id),
                &json!({"visual_dna":{"eyes":"green"}}),
            )
            .unwrap();
        assert!(
            runtime
                .block_on(provider.retrieve(&input, "prompt_translate"))
                .is_err(),
            "a frozen input change must not silently mix a new memory projection into the old request"
        );
    }

    #[test]
    fn current_adapter_refreshes_between_requests_without_changing_the_original_input() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let project = store.create_project("动态上下文", None).unwrap();
        let episode = store
            .create_episode(project.id, "第一集", Some("继续创作"))
            .unwrap();
        let provider = CurrentTaskContext::new(
            store.clone(),
            MemoryService::new(root.path(), EmbeddingSettings::default()),
        );
        let input = ExecutionInput {
            project_id: project.id.to_string(),
            episode_id: Some(episode.id),
            script: "继续创作".into(),
            context: json!({"retrieved_memory":{"old":"不可变原输入"}}),
            action: "workflow_script".into(),
            run_id: "fresh-context".into(),
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        store
            .write_context(
                project.id,
                "series_bible.v1.json",
                &json!({"constraints":["暂时不得复活"]}),
            )
            .unwrap();
        let first = runtime
            .block_on(provider.retrieve(&input, "architect"))
            .unwrap();
        assert!(
            first["prompt_fragments"]["negative_constraints"]
                .as_str()
                .unwrap()
                .contains("暂时不得复活")
        );
        store
            .write_context(
                project.id,
                "series_bible.v1.json",
                &json!({"constraints":[]}),
            )
            .unwrap();
        let next = runtime
            .block_on(provider.retrieve(&input, "writer.repair"))
            .unwrap();
        assert!(
            next["layers"]["negative_constraints"]["records"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            input.context["retrieved_memory"],
            json!({"old":"不可变原输入"})
        );
        let other = store.create_project("其它项目", None).unwrap();
        let foreign = ExecutionInput {
            project_id: other.id.to_string(),
            ..input
        };
        assert!(
            runtime
                .block_on(provider.retrieve(&foreign, "writer"))
                .is_err()
        );
    }
}
