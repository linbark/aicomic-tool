//! Durable reconciliation between an AI snapshot and its application writes.
//! A Run can pause more than once: status alone is not a snapshot identity.

#[cfg(test)]
use crate::production::Gate;
use crate::production::ProductionState;
use crate::session::EpisodeKey;
use aicomic_ai::ContextStore;
use aicomic_core::{EpisodeUpdate, Store};
use serde_json::{Value, json};

const STAGE: &str = "desktop.materialization";

#[derive(Clone)]
pub struct PendingCommit {
    pub run_id: String,
    pub workflow_status: String,
    pub artifacts: Value,
    pub waiting_for: Option<String>,
}

impl PendingCommit {
    fn snapshot(&self) -> Value {
        json!({"status":self.workflow_status,"artifacts":self.artifacts,"waiting_for":self.waiting_for})
    }

    fn marker(&self, key: EpisodeKey, status: &str) -> Value {
        json!({"schema_version":2,"status":status,"episode_id":key.episode,
            "workflow_status":self.workflow_status,"artifacts":self.artifacts,
            "snapshot":self.snapshot()})
    }

    fn matches(&self, marker: &Value) -> bool {
        marker["snapshot"] == self.snapshot()
    }

    fn committable(&self) -> bool {
        self.artifacts.is_object()
            && matches!(
                self.workflow_status.as_str(),
                "done" | "paused" | "rejected" | "needs_clarification"
            )
    }
}

fn numeric_id(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

fn recovery_target(
    project: &aicomic_core::Project,
    run: &str,
    episodes: &[aicomic_core::Episode],
    sources: &[&Value],
    action: Option<&str>,
    legacy_episode: bool,
) -> Option<EpisodeKey> {
    let mut declared_episode = None;
    for source in sources {
        if !source.is_object() {
            return None;
        }
        let legacy_project = source.get("project_id_pk").map(numeric_id);
        if legacy_project.is_some_and(|id| id != Some(project.id)) {
            return None;
        }
        if let Some(id) = source.get("project_id") {
            if let Some(id) = numeric_id(id) {
                if id != project.id {
                    return None;
                }
            } else if !id.as_str().is_some_and(|id| {
                !id.is_empty() && (id == project.uuid || legacy_project == Some(Some(project.id)))
            }) {
                return None;
            }
        }
        if source
            .get("run_id")
            .is_some_and(|id| id.as_str() != Some(run))
        {
            return None;
        }
        if let Some(id) = source.get("episode_id") {
            let id = if id.is_null() { 0 } else { numeric_id(id)? };
            if id < 0 || declared_episode.is_some_and(|declared| declared != id) {
                return None;
            }
            declared_episode = Some(id);
        }
    }
    let mut referenced = episodes
        .iter()
        .filter(|episode| episode.last_exec_run_id.as_deref() == Some(run));
    let referenced_episode = referenced.next().map(|episode| episode.id);
    if referenced.next().is_some()
        || referenced_episode
            .is_some_and(|id| declared_episode.is_some_and(|declared| id != declared))
    {
        return None;
    }
    let project_action = action.is_some_and(|action| {
        action.starts_with("project_outline_")
            || matches!(action, "visual_dna" | "visual_dna_ingest")
    });
    let episode = declared_episode
        .or(referenced_episode)
        .or(project_action.then_some(0))?;
    if episode == 0 && (legacy_episode || action.is_some_and(|_| !project_action)) {
        return None;
    }
    if episode > 0 && !episodes.iter().any(|candidate| candidate.id == episode) {
        return None;
    }
    Some(EpisodeKey {
        project: project.id,
        episode,
    })
}

/// Recover incomplete application writes without replaying historical completed
/// snapshots or accepting stale responses from a cancelled/failed execution.
/// The caller may show one pending result per target; unshown results stay on disk.
pub fn recover(store: &Store, project: i64) -> Result<Vec<(EpisodeKey, PendingCommit)>, String> {
    let expected_project = store.project(project).map_err(|e| e.to_string())?;
    let episodes = store.episodes(project).map_err(|e| e.to_string())?;
    let contexts = ContextStore::new(store.paths().root.clone());
    // Core lists directories even if the process exited before writing meta.json.
    let mut runs = store.list_run_files(project).map_err(|e| e.to_string())?;
    runs.sort_by(|a, b| {
        b.meta
            .as_ref()
            .and_then(|m| m["created_at_ms"].as_u64())
            .unwrap_or(0)
            .cmp(
                &a.meta
                    .as_ref()
                    .and_then(|m| m["created_at_ms"].as_u64())
                    .unwrap_or(0),
            )
            .then_with(|| a.run_id.cmp(&b.run_id))
    });
    let mut recovered = Vec::new();
    for run in runs {
        let marker = contexts
            .read_stage(&project.to_string(), &run.run_id, STAGE)
            .map_err(|e| e.to_string())?;
        let response = run.response.as_ref();
        let request = run.request.as_ref();
        let chat = contexts
            .read_stage(&project.to_string(), &run.run_id, "chat.status")
            .map_err(|e| e.to_string())?;
        let interrupt = if chat.as_ref().is_some_and(|s| s["status"] == "paused") {
            contexts
                .read_stage(&project.to_string(), &run.run_id, "chat.interrupt")
                .map_err(|e| e.to_string())?
        } else {
            None
        };
        let resume = interrupt.as_ref().map(|i| &i["resume_state"]);
        let resume_input = resume.map(|s| s.get("input").unwrap_or(s));
        let sources = [
            request,
            run.meta.as_ref(),
            response,
            marker.as_ref(),
            interrupt.as_ref(),
            resume,
            resume_input,
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        let action = request
            .and_then(|r| r["action"].as_str())
            .or_else(|| resume_input.and_then(|s| s["action"].as_str()));
        let legacy_episode = resume.is_some_and(|state| state.get("input").is_none());
        let Some(key) = recovery_target(
            &expected_project,
            &run.run_id,
            &episodes,
            &sources,
            action,
            legacy_episode,
        ) else {
            continue;
        };
        let episode = key.episode;
        let current_episode = episodes.iter().find(|e| e.id == episode);
        let is_current =
            current_episode.is_some_and(|e| e.last_exec_run_id.as_deref() == Some(&run.run_id));
        let managed = marker.is_some()
            || is_current
            || request.is_some_and(|r| r["context"]["preflight"].is_object());
        if !managed {
            continue;
        }
        if chat.as_ref().is_some_and(|s| {
            matches!(
                s["status"].as_str(),
                Some("cancelled" | "error" | "running")
            )
        }) {
            continue;
        }
        let pending = if let Some(response) = response.filter(|r| r["artifacts"].is_object()) {
            let status = response["status"].as_str().unwrap_or("");
            if !matches!(
                status,
                "done" | "paused" | "rejected" | "needs_clarification"
            ) {
                continue;
            }
            if response
                .get("run_id")
                .is_some_and(|id| id != &json!(run.run_id))
            {
                continue;
            }
            if let Some(chat) = &chat {
                if chat["status"].as_str().is_some_and(|s| s != status) {
                    continue;
                }
                if status == "paused"
                    && chat.get("waiting_for").is_some()
                    && chat["waiting_for"] != response["waiting_for"]
                {
                    continue;
                }
            }
            PendingCommit {
                run_id: run.run_id.clone(),
                workflow_status: status.into(),
                artifacts: response["artifacts"].clone(),
                waiting_for: response["waiting_for"].as_str().map(str::to_owned),
            }
        } else if response.is_none() && resume.is_some_and(|s| s["artifacts"].is_object()) {
            let kind = interrupt
                .as_ref()
                .and_then(|i| i["kind"].as_str())
                .unwrap_or("");
            if !matches!(
                kind,
                "confirm_outline"
                    | "confirm_assets"
                    | "confirm_split"
                    | "confirm_ingest"
                    | "review_changeset"
            ) || chat
                .as_ref()
                .is_some_and(|s| s.get("waiting_for").is_some_and(|waiting| waiting != kind))
            {
                continue;
            }
            PendingCommit {
                run_id: run.run_id.clone(),
                workflow_status: "paused".into(),
                artifacts: resume.unwrap()["artifacts"].clone(),
                waiting_for: Some(kind.into()),
            }
        } else if let Some(error) = marker.as_ref().filter(|m| {
            response.is_none()
                && matches!(m["status"].as_str(), Some("error" | "applying"))
                && m["artifacts"].is_object()
        }) {
            // An interrupted retry may have replaced an old error marker with
            // applying before any response.json ever existed.
            if let Some(snapshot) = error.get("snapshot") {
                PendingCommit {
                    run_id: run.run_id.clone(),
                    workflow_status: snapshot["status"].as_str().unwrap_or("").into(),
                    artifacts: snapshot["artifacts"].clone(),
                    waiting_for: snapshot["waiting_for"].as_str().map(str::to_owned),
                }
            } else {
                PendingCommit {
                    run_id: run.run_id.clone(),
                    workflow_status: error["workflow_status"].as_str().unwrap_or("").into(),
                    artifacts: error["artifacts"].clone(),
                    waiting_for: chat
                        .as_ref()
                        .and_then(|s| s["waiting_for"].as_str())
                        .map(str::to_owned),
                }
            }
        } else {
            continue;
        };
        if !pending.committable() {
            continue;
        }
        if let Some(chat) = &chat
            && (chat["status"].as_str() != Some(pending.workflow_status.as_str())
                || (pending.workflow_status == "paused"
                    && chat.get("waiting_for").is_some()
                    && chat["waiting_for"].as_str() != pending.waiting_for.as_deref()))
        {
            continue;
        }
        if let Some(marker) = &marker
            && marker["status"] == "done"
        {
            if pending.matches(marker) {
                continue;
            }
            // Old desktop completed SQL, memory and gates before writing this
            // marker. A matching SQL snapshot safely recognizes that commit.
            if marker.get("snapshot").is_none()
                && marker["workflow_status"] == pending.workflow_status
                && current_episode.is_some_and(|e| {
                    is_current
                        && e.exec_status == pending.workflow_status
                        && e.exec_artifacts.as_ref() == Some(&pending.artifacts)
                })
            {
                continue;
            }
            // Project-level legacy done markers have no SQL snapshot. Do not
            // revoke later human confirmations merely for upgrading the app.
            if episode == 0
                && marker.get("snapshot").is_none()
                && marker["workflow_status"] == pending.workflow_status
            {
                continue;
            }
        }
        // Unmarked old Runs not referenced by the episode are audit history, not
        // evidence that a current application submission was interrupted.
        if episode > 0
            && !is_current
            && marker
                .as_ref()
                .is_none_or(|m| !matches!(m["status"].as_str(), Some("error" | "applying")))
        {
            continue;
        }
        recovered.push((key, pending));
    }
    Ok(recovered)
}

/// Complete all application writes for this exact snapshot and original target.
/// The done marker is last; every preceding operation is safe to retry.
pub fn apply(store: &Store, key: EpisodeKey, pending: &PendingCommit) -> Result<(), String> {
    if !pending.committable() {
        return Err("运行快照状态或产物不允许提交".into());
    }
    let project = store.project(key.project).map_err(|e| e.to_string())?;
    let episode = if key.episode > 0 {
        let episode = store.episode(key.episode).map_err(|e| e.to_string())?;
        if episode.project_id != key.project {
            return Err("运行分集不属于原项目".into());
        }
        Some(episode)
    } else if key.episode == 0 {
        None
    } else {
        return Err("无效的运行分集".into());
    };
    let contexts = ContextStore::new(store.paths().root.clone());
    let marker = contexts
        .read_stage(&key.project.to_string(), &pending.run_id, STAGE)
        .map_err(|e| e.to_string())?;
    let persisted = contexts
        .read_run(&key.project.to_string(), &pending.run_id)
        .map_err(|e| e.to_string())?;
    let expected =
        json!({"project_id":key.project,"episode_id":key.episode,"run_id":pending.run_id});
    let mut sources = vec![&expected];
    sources.extend(marker.as_ref());
    if let Some(run) = &persisted {
        sources.extend([&run["request"], &run["meta"], &run["response"]]);
    }
    let episodes = store.episodes(key.project).map_err(|e| e.to_string())?;
    let action = persisted
        .as_ref()
        .and_then(|run| run["request"]["action"].as_str());
    if recovery_target(
        &project,
        &pending.run_id,
        &episodes,
        &sources,
        action,
        false,
    ) != Some(key)
    {
        return Err("运行快照声明与提交目标不一致".into());
    }
    if marker
        .as_ref()
        .is_some_and(|m| m["status"] == "done" && pending.matches(m))
    {
        return Ok(());
    }
    ProductionState::refresh(store, key.project)?;
    contexts
        .write_stage(
            &key.project.to_string(),
            &pending.run_id,
            STAGE,
            &pending.marker(key, "applying"),
        )
        .map_err(|e| e.to_string())?;
    if key.episode > 0 {
        store
            .apply_workflow_artifacts(
                key.project,
                key.episode,
                &pending.run_id,
                &pending.artifacts,
            )
            .map_err(|e| e.to_string())?;
    }
    crate::integration::apply_memory(
        store,
        key,
        &pending.run_id,
        &pending.artifacts,
        &pending.workflow_status,
    )?;
    // Recovery of an older failed Run must not replace the current Run pointer.
    if episode.as_ref().is_some_and(|e| {
        e.last_exec_run_id
            .as_deref()
            .is_none_or(|id| id == pending.run_id)
    }) {
        store
            .patch_episode(
                key.episode,
                EpisodeUpdate {
                    last_exec_run_id: Some(pending.run_id.clone()),
                    exec_status: Some(pending.workflow_status.clone()),
                    exec_artifacts: Some(pending.artifacts.clone()),
                    ..EpisodeUpdate::default()
                },
            )
            .map_err(|e| e.to_string())?;
    }
    ProductionState::refresh(store, key.project)?;
    contexts
        .write_stage(
            &key.project.to_string(),
            &pending.run_id,
            STAGE,
            &pending.marker(key, "done"),
        )
        .map_err(|e| e.to_string())
}

pub fn record_failure(
    store: &Store,
    key: EpisodeKey,
    pending: &PendingCommit,
    error: &str,
) -> Result<(), String> {
    let mut marker = pending.marker(key, "error");
    marker["error"] = json!(error);
    ContextStore::new(store.paths().root.clone())
        .write_stage(&key.project.to_string(), &pending.run_id, STAGE, &marker)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_current_paused_artifacts_from_native_and_legacy_stage_only_runs() {
        for native in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let store = Store::open(root.path()).unwrap();
            let project = store.create_project("阶段恢复", None).unwrap();
            let episode = store
                .create_episode(project.id, "第一集", Some("原文"))
                .unwrap();
            let run = "stage-only-paused";
            store
                .patch_episode(
                    episode.id,
                    EpisodeUpdate {
                        last_exec_run_id: Some(run.into()),
                        ..EpisodeUpdate::default()
                    },
                )
                .unwrap();
            let artifacts =
                json!({"split_episodes":{"episodes":[{"title":"原暂停产物","script":"原文"}]}});
            let state = if native {
                json!({"schema_version":1,"input":{"project_id":project.id.to_string(),"episode_id":episode.id,"run_id":run,"action":"episode_execute","script":"原文","context":{}},"next_step":3,"steps":[{"action_key":"episode_outline_generate"},{"action_key":"episode_assets_visual_dna"},{"action_key":"episode_split_episodes"},{"action_key":"episode_asset_ingest"}],"artifacts":artifacts,"kind":"confirm_split"})
            } else {
                json!({"project_id_pk":project.id,"project_id":"old-project-uuid","episode_id":episode.id,"run_id":run,"script_text":"原文","step_index":3,"artifacts":artifacts})
            };
            let contexts = ContextStore::new(root.path().into());
            contexts
                .write_stage(
                    &project.id.to_string(),
                    run,
                    "chat.status",
                    &json!({"status":"paused","waiting_for":"confirm_split"}),
                )
                .unwrap();
            contexts
                .write_stage(
                    &project.id.to_string(),
                    run,
                    "chat.interrupt",
                    &json!({"kind":"confirm_split","resume_state":state}),
                )
                .unwrap();
            let recovered = recover(&store, project.id).unwrap();
            assert_eq!(recovered.len(), 1, "native format: {native}");
            assert_eq!(
                recovered[0].0,
                EpisodeKey {
                    project: project.id,
                    episode: episode.id
                }
            );
            assert_eq!(recovered[0].1.artifacts, artifacts);
            assert_eq!(recovered[0].1.waiting_for.as_deref(), Some("confirm_split"));
        }
    }

    #[test]
    fn rejects_conflicting_project_run_and_episode_declarations_from_every_snapshot_source() {
        for source in ["request", "meta", "response", "marker", "interrupt"] {
            for field in ["project_id", "episode_id", "run_id"] {
                let root = tempfile::tempdir().unwrap();
                let store = Store::open(root.path()).unwrap();
                let project = store.create_project("归属项目", None).unwrap();
                let episode = store
                    .create_episode(project.id, "第一集", Some("原文"))
                    .unwrap();
                let other_episode = store
                    .create_episode(project.id, "另一集", Some("另一原文"))
                    .unwrap();
                let other_project = store.create_project("其它项目", None).unwrap();
                let run = "owned-paused-run";
                store
                    .patch_episode(
                        episode.id,
                        EpisodeUpdate {
                            last_exec_run_id: Some(run.into()),
                            ..EpisodeUpdate::default()
                        },
                    )
                    .unwrap();
                let artifacts = json!({"outline":{"title":"候选产物"}});
                let mut request = json!({"project_id":project.id.to_string(),"episode_id":episode.id,"run_id":run,"action":"episode_execute","context":{"preflight":{}}});
                let mut response = json!({"project_id":project.id,"episode_id":episode.id,"run_id":run,"status":"paused","artifacts":artifacts,"waiting_for":"confirm_split"});
                let mut meta =
                    json!({"project_id":project.id,"episode_id":episode.id,"run_id":run});
                let mut marker = json!({"project_id":project.id,"episode_id":episode.id,"run_id":run,"status":"error","workflow_status":"paused","artifacts":artifacts});
                let mut interrupt = json!({"kind":"confirm_split","resume_state":{"schema_version":1,"input":request,"next_step":1,"steps":[{"action_key":"episode_split_episodes"}],"artifacts":artifacts,"kind":"confirm_split"}});
                let mismatch = match field {
                    "project_id" => json!(other_project.id),
                    "episode_id" => json!(other_episode.id),
                    _ => json!("different-run"),
                };
                match source {
                    "request" => request[field] = mismatch,
                    "meta" => meta[field] = mismatch,
                    "response" => response[field] = mismatch,
                    "marker" => marker[field] = mismatch,
                    _ => interrupt["resume_state"]["input"][field] = mismatch,
                };
                let contexts = ContextStore::new(root.path().into());
                contexts
                    .snapshot_run(&project.id.to_string(), run, &request, &response, &meta)
                    .unwrap();
                contexts
                    .write_stage(&project.id.to_string(), run, STAGE, &marker)
                    .unwrap();
                contexts
                    .write_stage(
                        &project.id.to_string(),
                        run,
                        "chat.status",
                        &json!({"status":"paused","waiting_for":"confirm_split"}),
                    )
                    .unwrap();
                contexts
                    .write_stage(&project.id.to_string(), run, "chat.interrupt", &interrupt)
                    .unwrap();
                assert!(
                    recover(&store, project.id).unwrap().is_empty(),
                    "accepted {source}.{field} conflict"
                );
            }
        }
    }

    #[test]
    fn resumes_applying_and_error_marker_snapshots_without_a_response_file() {
        for marker_status in ["applying", "error"] {
            let root = tempfile::tempdir().unwrap();
            let store = Store::open(root.path()).unwrap();
            let project = store.create_project("提交恢复", None).unwrap();
            let episode = store
                .create_episode(project.id, "第一集", Some("原文"))
                .unwrap();
            let run = "marker-only";
            store
                .patch_episode(
                    episode.id,
                    EpisodeUpdate {
                        last_exec_run_id: Some(run.into()),
                        ..EpisodeUpdate::default()
                    },
                )
                .unwrap();
            let artifacts = json!({"approved_split_episodes":{"episodes":[{"title":"已批准分集","script":"原文"}]}});
            let contexts = ContextStore::new(root.path().into());
            contexts
                .write_stage(
                    &project.id.to_string(),
                    run,
                    "chat.status",
                    &json!({"status":"paused","waiting_for":"confirm_ingest"}),
                )
                .unwrap();
            contexts.write_stage(&project.id.to_string(),run,STAGE,&json!({"schema_version":2,"status":marker_status,"episode_id":episode.id,"workflow_status":"paused","artifacts":artifacts,"snapshot":{"status":"paused","artifacts":artifacts,"waiting_for":"confirm_ingest"}})).unwrap();
            let recovered = recover(&store, project.id).unwrap();
            assert_eq!(recovered.len(), 1, "marker status: {marker_status}");
            assert_eq!(
                recovered[0].0,
                EpisodeKey {
                    project: project.id,
                    episode: episode.id
                }
            );
            assert_eq!(recovered[0].1.artifacts, artifacts);
            assert_eq!(
                recovered[0].1.waiting_for.as_deref(),
                Some("confirm_ingest")
            );
        }
    }

    #[test]
    fn never_recovers_failed_cancelled_running_or_unknown_marker_payloads() {
        for status in ["error", "cancelled", "running", "future_status", ""] {
            for snapshot in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let store = Store::open(root.path()).unwrap();
                let project = store.create_project("未批准状态", None).unwrap();
                let episode = store
                    .create_episode(project.id, "第一集", Some("原文"))
                    .unwrap();
                let run = "invalid-marker-status";
                let artifacts = json!({"approved_asset_ingest_plan":{"items":[{"name":"不得写入","kind":"prop"}]}});
                let mut marker = json!({"status":"error","episode_id":episode.id,"workflow_status":status,"artifacts":artifacts});
                if status.is_empty() {
                    marker.as_object_mut().unwrap().remove("workflow_status");
                }
                if snapshot {
                    marker["snapshot"] =
                        json!({"status":status,"artifacts":artifacts,"waiting_for":null});
                }
                ContextStore::new(root.path().into())
                    .write_stage(&project.id.to_string(), run, STAGE, &marker)
                    .unwrap();
                assert!(
                    recover(&store, project.id).unwrap().is_empty(),
                    "accepted status {status:?}, snapshot {snapshot}"
                );
                assert!(store.asset_items(project.id).unwrap().is_empty());
            }
        }
    }

    #[test]
    fn apply_rejects_noncommittable_snapshots_before_writing_any_domain_data() {
        for status in ["cancelled", "error", "running", "future_status"] {
            let root = tempfile::tempdir().unwrap();
            let store = Store::open(root.path()).unwrap();
            let project = store.create_project("禁止提交", None).unwrap();
            let episode = store
                .create_episode(project.id, "第一集", Some("原文"))
                .unwrap();
            let pending = PendingCommit {
                run_id: "not-committable".into(),
                workflow_status: status.into(),
                artifacts: json!({"approved_asset_ingest_plan":{"items":[{"name":"不得写入","kind":"prop","category":"prop"}]}}),
                waiting_for: None,
            };
            assert!(
                apply(
                    &store,
                    EpisodeKey {
                        project: project.id,
                        episode: episode.id
                    },
                    &pending
                )
                .is_err(),
                "accepted {status}"
            );
            assert!(store.asset_items(project.id).unwrap().is_empty());
            assert!(
                store
                    .episode(episode.id)
                    .unwrap()
                    .last_exec_run_id
                    .is_none()
            );
        }
    }

    #[test]
    fn apply_rejects_a_different_declared_episode_before_done_marker_shortcut() {
        for source in ["marker", "request", "meta", "response"] {
            let root = tempfile::tempdir().unwrap();
            let store = Store::open(root.path()).unwrap();
            let project = store.create_project("目标核验", None).unwrap();
            let origin = store
                .create_episode(project.id, "原分集", Some("原文"))
                .unwrap();
            let other = store
                .create_episode(project.id, "其它分集", Some("其它原文"))
                .unwrap();
            let run = "original-episode-run";
            let artifacts = json!({"approved_asset_ingest_plan":{"items":[{"name":"不可错写","kind":"prop","category":"prop"}]}});
            let pending = PendingCommit {
                run_id: run.into(),
                workflow_status: "done".into(),
                artifacts: artifacts.clone(),
                waiting_for: None,
            };
            if source == "marker" {
                ContextStore::new(root.path().into()).write_stage(&project.id.to_string(),run,STAGE,&json!({"status":"done","episode_id":origin.id,"workflow_status":"done","artifacts":artifacts,"snapshot":{"status":"done","artifacts":artifacts,"waiting_for":null}})).unwrap();
            } else {
                store.write_run(project.id,run,&format!("{source}.json"),&json!({"project_id":project.id,"episode_id":origin.id,"run_id":run,"status":"done","artifacts":artifacts})).unwrap();
            }
            assert!(
                apply(
                    &store,
                    EpisodeKey {
                        project: project.id,
                        episode: other.id
                    },
                    &pending
                )
                .is_err(),
                "accepted wrong target in {source}"
            );
            assert!(store.asset_items(project.id).unwrap().is_empty());
            assert!(store.episode(other.id).unwrap().last_exec_run_id.is_none());
        }
    }

    #[test]
    fn missing_episode_is_only_project_scoped_for_an_explicit_independent_action() {
        for action in ["episode_execute", "project_outline_generate"] {
            let root = tempfile::tempdir().unwrap();
            let store = Store::open(root.path()).unwrap();
            let project = store.create_project("作用域", None).unwrap();
            let run = "missing-episode";
            ContextStore::new(root.path().into()).snapshot_run(&project.id.to_string(),run,
                &json!({"project_id":project.id,"run_id":run,"action":action,"context":{"preflight":{}}}),
                &json!({"run_id":run,"status":"done","artifacts":{"project_outline":{"title":"大纲"}}}),&json!({})).unwrap();
            let recovered = recover(&store, project.id).unwrap();
            if action == "episode_execute" {
                assert!(recovered.is_empty());
            } else {
                assert_eq!(recovered.len(), 1);
                assert_eq!(recovered[0].0.episode, 0);
            }
        }
    }

    #[test]
    fn legacy_completed_snapshots_do_not_replay_or_revoke_later_confirmations() {
        for current in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let store = Store::open(root.path()).unwrap();
            let project = store.create_project("已提交历史", None).unwrap();
            let episode = store
                .create_episode(project.id, "第一集", Some("原文"))
                .unwrap();
            let run = "legacy-completed";
            let artifacts = json!({"outline":{"title":"已提交产物"}});
            store
                .patch_episode(
                    episode.id,
                    EpisodeUpdate {
                        last_exec_run_id: Some(if current { run } else { "newer-run" }.into()),
                        exec_status: Some("paused".into()),
                        exec_artifacts: Some(artifacts.clone()),
                        ..EpisodeUpdate::default()
                    },
                )
                .unwrap();
            let contexts = ContextStore::new(root.path().into());
            contexts.snapshot_run(&project.id.to_string(),run,&json!({"project_id":project.id,"episode_id":episode.id,"run_id":run,"action":"episode_execute","context":{"preflight":{}}}),&json!({"run_id":run,"status":"paused","artifacts":artifacts,"waiting_for":"confirm_ingest"}),&json!({"episode_id":episode.id})).unwrap();
            contexts
                .write_stage(
                    &project.id.to_string(),
                    run,
                    STAGE,
                    &json!({"status":"done","episode_id":episode.id,"workflow_status":"paused"}),
                )
                .unwrap();
            contexts
                .write_stage(
                    &project.id.to_string(),
                    run,
                    "chat.status",
                    &json!({"status":"paused","waiting_for":"confirm_ingest"}),
                )
                .unwrap();
            let mut production = ProductionState::default();
            production.set(episode.id, Gate::Structure, true);
            production.save(&contexts, project.id).unwrap();
            assert!(
                recover(&store, project.id).unwrap().is_empty(),
                "replayed legacy done, current: {current}"
            );
            assert!(
                ProductionState::load(&contexts, project.id).confirmed(episode.id, Gate::Structure)
            );
            assert_eq!(
                store.episode(episode.id).unwrap().exec_artifacts,
                Some(artifacts)
            );
        }
    }
}
