//! Automatic memory effects of reviewed native workflow artifacts.
use crate::session::EpisodeKey;
use aicomic_core::Store;
use aicomic_memory::{MemoryStore, stable_hex_id};
use serde_json::{Value, json};
pub fn source_evidences(store: &Store, key: EpisodeKey, ids: &[String]) -> Result<Value, String> {
    checked_episode(store, key)?;
    let memory = MemoryStore::open_root(&store.paths().root).map_err(|e| e.to_string())?;
    Ok(Value::Array(
        memory
            .list_evidences(key.project, ids)
            .map_err(|e| e.to_string())?,
    ))
}
fn checked_episode(store: &Store, key: EpisodeKey) -> Result<(), String> {
    store.project(key.project).map_err(|e| e.to_string())?;
    if key.episode == 0 {
        return Ok(());
    }
    if key.episode < 0 {
        return Err("分集 ID 无效".into());
    }
    let episode = store.episode(key.episode).map_err(|e| e.to_string())?;
    if episode.project_id != key.project {
        return Err("分集不属于当前项目，未写入记忆".into());
    }
    Ok(())
}
fn episode_id(key: EpisodeKey) -> Option<i64> {
    (key.episode > 0).then_some(key.episode)
}
pub fn prepare_memory(store: &Store, key: EpisodeKey, script: &str) -> Result<Vec<String>, String> {
    checked_episode(store, key)?;
    let memory = MemoryStore::open_root(&store.paths().root).map_err(|e| e.to_string())?;
    let project = key.project.to_string();
    let episode = key.episode.to_string();
    let text_hash = stable_hex_id(&[script]);
    let normalized = script.replace("\r\n", "\n");
    let mut paragraphs = vec![];
    let mut lines = vec![];
    for line in normalized.lines() {
        if line.trim().is_empty() {
            if !lines.is_empty() {
                paragraphs.push(lines.join("\n"));
                lines.clear();
            }
        } else {
            lines.push(line.trim_end().to_owned());
        }
    }
    if !lines.is_empty() {
        paragraphs.push(lines.join("\n"));
    }
    let mut ids = vec![];
    for (paragraph_index, paragraph) in paragraphs.iter().enumerate() {
        let chars = paragraph.trim().chars().collect::<Vec<_>>();
        let chunks = chars
            .chunks(600)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>();
        for (chunk_index, quote) in chunks.iter().enumerate() {
            let quote = quote.trim();
            if quote.is_empty() {
                continue;
            }
            let id = stable_hex_id(&[
                "evidence",
                &project,
                &episode,
                &text_hash,
                &paragraph_index.to_string(),
                &chunk_index.to_string(),
            ])[..32]
                .to_owned();
            let mut tags = vec![
                if key.episode == 0 {
                    "project_source"
                } else {
                    "episode_script"
                }
                .to_owned(),
            ];
            if chunks.len() > 1 {
                tags.push(format!("chunk:{}/{}", chunk_index + 1, chunks.len()))
            }
            memory.upsert_evidence(&json!({"evidence_id":id,"project_id":key.project,"episode_id":episode_id(key),"quote":quote,"span":{"paragraph_index":paragraph_index},"tags":tags})).map_err(|e|e.to_string())?;
            ids.push(id);
        }
    }
    Ok(ids)
}
fn review(memory: &MemoryStore, id: &str, approve: bool) -> Result<(), String> {
    let changeset = memory
        .get_changeset(id)
        .map_err(|e| e.to_string())?
        .ok_or("待审核记忆变更集不存在")?;
    let current = changeset["review_status"].as_str().unwrap_or("");
    match (approve, current) {
        (true, "approved") | (false, "rejected") => Ok(()),
        (_, "pending_review") => if approve {
            memory.apply_changeset(id, "desktop", Some("已确认工作流产物"))
        } else {
            memory.reject_changeset(id, "desktop", Some("工作流审核拒绝"))
        }
        .map_err(|e| e.to_string()),
        _ => Err("记忆变更集已存在相反审核结果，请创建新的运行".into()),
    }
}
// A Run's result also carries creative context inherited from earlier Runs.
// Compare against its immutable request, not the previous resume/completion,
// so retries and later approvals still materialize this Run's own output.
fn produced_artifact<'a>(artifacts: &'a Value, inherited: &Value, name: &str) -> Option<&'a Value> {
    artifacts
        .get(name)
        .filter(|value| inherited.get(name) != Some(*value))
}
fn produced_structure(artifacts: &Value, inherited: &Value) -> Value {
    let fields = ["beat_sheet", "shots"]
        .into_iter()
        .filter_map(|name| {
            produced_artifact(artifacts, inherited, name)
                .map(|value| (name.to_owned(), value.clone()))
        })
        .collect();
    Value::Object(fields)
}
pub fn apply_memory(
    store: &Store,
    key: EpisodeKey,
    run: &str,
    artifacts: &Value,
    status: &str,
) -> Result<(), String> {
    checked_episode(store, key)?;
    if !artifacts.is_object() {
        return Err("工作流记忆产物必须为 JSON 对象".into());
    }
    let request = store
        .read_run(key.project, run, "request.json")
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| json!({}));
    let inherited = &request["context"]["artifacts"];
    let memory = MemoryStore::open_root(&store.paths().root).map_err(|e| e.to_string())?;
    let requested = request["context"]["source_evidence_ids"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let requested = if requested.is_empty() {
        if let Some(script) = request["script"].as_str().filter(|s| !s.trim().is_empty()) {
            prepare_memory(store, key, script)?
        } else {
            requested
        }
    } else {
        requested
    };
    let source_ids = memory
        .list_evidences(key.project, &requested)
        .map_err(|e| e.to_string())?
        .iter()
        .filter_map(|e| e["evidence_id"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    memory
        .extract_structured_output(
            key.project,
            &produced_structure(artifacts, inherited),
            episode_id(key),
            None,
            Some(&format!("{run}.structured")),
        )
        .map_err(|e| e.to_string())?;
    if let Some(data) = artifacts.get("structured_output").filter(|v| v.is_object()) {
        memory
            .extract_structured_output(
                key.project,
                &produced_structure(data, &inherited["structured_output"]),
                episode_id(key),
                None,
                Some(&format!("{run}.structured")),
            )
            .map_err(|e| e.to_string())?;
    }
    if let Some(qc) = produced_artifact(artifacts, inherited, "qc_report").filter(|v| v.is_object())
    {
        memory
            .extract_qc_report(
                key.project,
                qc,
                episode_id(key),
                None,
                Some(&format!("{run}.qc")),
            )
            .map_err(|e| e.to_string())?;
    }
    let bible = if let Some(bible) =
        produced_artifact(artifacts, inherited, "series_bible").filter(|v| v.is_object())
    {
        Some(bible.clone())
    } else {
        store
            .read_context(key.project, "series_bible.v1.json")
            .map_err(|e| e.to_string())?
    };
    if let Some(bible) = bible {
        memory
            .index_series_bible(key.project, &bible)
            .map_err(|e| e.to_string())?;
    }
    // Identical DNA can be newly generated for a different item. That action's
    // explicit target takes precedence over equality with its creative context.
    let explicit_visual = matches!(
        request["action"].as_str(),
        Some("visual_dna" | "visual_dna_ingest")
    );
    if let Some(dna) = produced_artifact(artifacts, inherited, "visual_dna")
        .or_else(|| {
            explicit_visual
                .then(|| artifacts.get("visual_dna"))
                .flatten()
        })
        .filter(|v| v.is_object())
    {
        let item_id = request["context"]["item_id"]
            .as_i64()
            .filter(|id| *id > 0)
            .ok_or("VisualDNA 运行缺少真实素材 item_id")?;
        let item = store.asset_item(item_id).map_err(|e| e.to_string())?;
        if item.project_id != key.project {
            return Err("VisualDNA 素材不属于当前项目，未写入记忆".into());
        }
        let dna = if dna["character_core"].is_object() {
            dna.clone()
        } else {
            json!({"character_core":{"visual_dna":dna.get("visual_dna").unwrap_or(dna)},"technical_specs":dna["technical_specs"],"stable_diffusion_tags":dna["stable_diffusion_tags"]})
        };
        memory
            .index_visual_dna(key.project, item_id, &dna)
            .map_err(|e| e.to_string())?;
    }
    let approved = artifacts
        .get("approved_changeset")
        .filter(|v| v.is_object());
    let pending = artifacts.get("changeset").filter(|v| v.is_object());
    if let Some(payload) = approved.or(pending) {
        let mut payload = payload.clone();
        payload["evidence_ids"] = json!(source_ids);
        let id = memory
            .ensure_changeset_for_run(key.project, episode_id(key), run, &payload)
            .map_err(|e| e.to_string())?;
        if approved.is_some() {
            review(&memory, &id, true)?
        } else if status == "rejected" {
            review(&memory, &id, false)?
        }
    }
    if let Some(plan) = artifacts
        .get("approved_asset_ingest_plan")
        .filter(|v| v.is_object())
    {
        let items = plan["items"]
            .as_array()
            .ok_or("已批准素材入库计划缺少 items 数组")?;
        let existing = store.asset_items(key.project).map_err(|e| e.to_string())?;
        let mut entities = vec![];
        let mut snapshots = vec![];
        for item in items {
            let name = item["name"].as_str().unwrap_or("").trim();
            let kind = item["kind"].as_str().unwrap_or("");
            if name.is_empty() || !["character", "location", "prop", "organization"].contains(&kind)
            {
                return Err("已批准素材入库计划包含缺失名称或未知 kind".into());
            }
            let asset = existing
                .iter()
                .find(|asset| asset.name.trim() == name)
                .ok_or_else(|| format!("主库尚未写入素材 {name}，未伪造素材 ID"))?;
            let entity_id = stable_hex_id(&["entity", &key.project.to_string(), kind, name]);
            entities.push(json!({"entity_id":entity_id,"project_id":key.project,"entity_type":kind,"canonical_name":name,"aliases":[],"status":"confirmed","confidence":1.0,"source_kind":"system","created_from_evidence_id":source_ids.first()}));
            if kind == "character" {
                if source_ids.is_empty() {
                    return Err("素材视觉记忆缺少本次运行的原文证据，不能确认无出处的快照".into());
                }
                let dna = item
                    .get("visual_dna")
                    .filter(|v| v.is_object())
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let tags = item["stable_diffusion_tags"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or_else(|| item["base_prompt"].as_str())
                    .unwrap_or("");
                snapshots.push(json!({"snapshot_id":stable_hex_id(&["snapshot",&entity_id,"visual_v1"]),"project_id":key.project,"entity_id":entity_id,"valid_from_story_order":if key.episode==0{format!("project:{}:asset_ingest",key.project)}else{format!("episode:{}:asset_ingest",key.episode)},"fields":{"visual":{"description":item["description"],"stable_diffusion_tags":tags,"visual_dna":dna}},"why":"assets_visual_dna","evidence_ids":source_ids.iter().take(20).collect::<Vec<_>>(),"status":"confirmed","confidence":1.0,"source_kind":"system"}));
                let stored = store
                    .read_context(
                        key.project,
                        &format!("visual_dna.asset_item_{}.v1.json", asset.id),
                    )
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("素材 {} 的 VisualDNA 文件尚未写入主库", asset.id))?;
                let normalized = if stored["character_core"].is_object() {
                    stored
                } else {
                    json!({"character_core":{"visual_dna":stored.get("visual_dna").cloned().unwrap_or(dna),"attire":stored["attire"]},"technical_specs":stored["technical_specs"],"stable_diffusion_tags":stored["stable_diffusion_tags"].as_str().filter(|s|!s.is_empty()).unwrap_or(tags)})
                };
                memory
                    .index_visual_dna(key.project, asset.id, &normalized)
                    .map_err(|e| e.to_string())?;
            }
        }
        let payload = json!({"schema_version":"changeset.v0","project_id":key.project,"episode_id":episode_id(key),"story_order_base":if key.episode==0{format!("project:{}:asset_ingest",key.project)}else{format!("episode:{}:asset_ingest",key.episode)},"evidence_ids":source_ids,"entities":entities,"snapshots":snapshots,"materialize":{"write_static_bible":true}});
        let id = memory
            .ensure_changeset_for_run(
                key.project,
                episode_id(key),
                &format!("{run}:asset_ingest"),
                &payload,
            )
            .map_err(|e| e.to_string())?;
        review(&memory, &id, true)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aicomic_memory::{MemoryQuery, MemoryStore};
    use serde_json::json;
    fn fixture() -> (tempfile::TempDir, Store, EpisodeKey) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let project = store.create_project("测试项目", None).unwrap();
        let episode = store
            .create_episode(project.id, "第一集", Some("林青推开城门，捡起玉剑。"))
            .unwrap();
        let key = EpisodeKey {
            project: project.id,
            episode: episode.id,
        };
        (dir, store, key)
    }
    fn source(store: &Store, key: EpisodeKey, run: &str) -> Vec<String> {
        let ids = prepare_memory(store, key, "林青推开城门，捡起玉剑。").unwrap();
        store
            .write_run(
                key.project,
                run,
                "request.json",
                &json!({"context":{"source_evidence_ids":ids}}),
            )
            .unwrap();
        ids
    }
    #[test]
    fn prepare_evidence_is_stable_after_repeat_and_restart() {
        let (dir, store, key) = fixture();
        let first = prepare_memory(&store, key, "林青推开城门，捡起玉剑。").unwrap();
        assert_eq!(
            prepare_memory(&store, key, "林青推开城门，捡起玉剑。").unwrap(),
            first
        );
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(
            prepare_memory(&store, key, "林青推开城门，捡起玉剑。").unwrap(),
            first
        );
        let mut q = MemoryQuery::new(key.project);
        q.memory_type = Some("evidence".into());
        assert_eq!(
            MemoryStore::open_root(dir.path())
                .unwrap()
                .list_records(&q)
                .unwrap()
                .len(),
            first.len()
        );
    }
    #[test]
    fn workflow_memory_replays_without_duplicates() {
        let (dir, store, key) = fixture();
        source(&store, key, "writer-run");
        let artifacts = json!({"series_bible":{"world_rules":{"life":"不能复活"},"constraints":["不允许现代枪械"]},"beat_sheet":[{"title":"初遇","beat_type":"setup"}],"shots":[{"action_text":"林青捡起玉剑并受伤"}],"qc_report":{"issues":[{"type":"continuity","message":"道具检查"}],"revised_script_fountain":"修订完成"}});
        apply_memory(&store, key, "writer-run", &artifacts, "done").unwrap();
        let before = MemoryStore::open_root(dir.path())
            .unwrap()
            .list_records(&MemoryQuery::new(key.project))
            .unwrap()
            .len();
        apply_memory(&store, key, "writer-run", &artifacts, "done").unwrap();
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        apply_memory(&store, key, "writer-run", &artifacts, "done").unwrap();
        let records = MemoryStore::open_root(dir.path())
            .unwrap()
            .list_records(&MemoryQuery::new(key.project))
            .unwrap();
        assert_eq!(records.len(), before);
        assert!(records.iter().any(|r| r.namespace == "episodic"));
        assert!(records.iter().any(|r| r.namespace == "dynamic_plot"));
        assert!(
            records
                .iter()
                .any(|r| r.namespace == "world_rules_negative")
        );
    }
    #[test]
    fn changeset_pause_approve_and_reject_use_source_whitelist() {
        let (dir, store, key) = fixture();
        let evidence = source(&store, key, "memory-run");
        let payload = json!({"schema_version":"changeset.v0","entities":[{"entity_type":"character","canonical_name":"林青"}],"snapshots":[{"canonical_name":"林青","fields":{"visual":{"eyes":"blue"}},"evidence_ids":[evidence[0],"invented"]}]});
        apply_memory(
            &store,
            key,
            "memory-run",
            &json!({"changeset":payload}),
            "paused",
        )
        .unwrap();
        let memory = MemoryStore::open_root(dir.path()).unwrap();
        let pending = memory
            .list_changesets(key.project, Some("pending_review"), 10)
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending[0]["payload"]["snapshots"][0]["evidence_ids"],
            json!([evidence[0]])
        );
        let id = pending[0]["changeset_id"].as_str().unwrap().to_owned();
        drop(memory);
        apply_memory(
            &store,
            key,
            "memory-run",
            &json!({"changeset":payload,"approved_changeset":payload}),
            "done",
        )
        .unwrap();
        apply_memory(
            &store,
            key,
            "memory-run",
            &json!({"approved_changeset":payload}),
            "done",
        )
        .unwrap();
        let memory = MemoryStore::open_root(dir.path()).unwrap();
        assert_eq!(
            memory.get_changeset(&id).unwrap().unwrap()["review_status"],
            "approved"
        );
        assert_eq!(
            memory.list_changesets(key.project, None, 10).unwrap().len(),
            1
        );
        source(&store, key, "reject-run");
        apply_memory(
            &store,
            key,
            "reject-run",
            &json!({"changeset":{"entities":[]}}),
            "rejected",
        )
        .unwrap();
        assert_eq!(
            memory
                .list_changesets(key.project, Some("rejected"), 10)
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn approved_ingest_adds_canonical_visual_memory_without_recreating_assets() {
        let (dir, store, key) = fixture();
        source(&store, key, "asset-run");
        let plan = json!({"items":[{"kind":"character","category":"persona_visual","name":"林青","description":"蓝眼少年","base_prompt":"blue eyes","visual_dna":{"eyes":"blue"}},{"kind":"location","category":"background","name":"青云城","description":"主城"}]});
        let artifacts = json!({"approved_asset_ingest_plan":plan});
        store
            .apply_workflow_artifacts(key.project, key.episode, "asset-run", &artifacts)
            .unwrap();
        let before = store.asset_items(key.project).unwrap();
        apply_memory(&store, key, "asset-run", &artifacts, "done").unwrap();
        apply_memory(&store, key, "asset-run", &artifacts, "done").unwrap();
        assert_eq!(store.asset_items(key.project).unwrap().len(), before.len());
        let memory = MemoryStore::open_root(dir.path()).unwrap();
        assert_eq!(memory.find_entities(key.project, "林青").unwrap().len(), 1);
        assert_eq!(
            memory.find_entities(key.project, "青云城").unwrap().len(),
            1
        );
        let mut q = MemoryQuery::new(key.project);
        q.namespace = Some("static_bible".into());
        let records = memory.list_records(&q).unwrap();
        assert!(records.iter().any(|r| {
            r.payload_json
                .as_ref()
                .is_some_and(|p| p["fields"]["visual"]["visual_dna"]["eyes"] == "blue")
        }));
        assert!(
            records
                .iter()
                .any(|r| r.entity == Some(format!("item_{}", before[0].id)))
        );
    }
}
#[cfg(test)]
mod project_run_tests {
    use super::*;
    use aicomic_memory::MemoryQuery;
    #[test]
    fn project_context_and_visual_dna_runs_need_no_episode() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let project = store.create_project("纯项目", None).unwrap();
        let item = store
            .create_asset_item(project.id, "林青", "persona_visual", None)
            .unwrap();
        let key = EpisodeKey {
            project: project.id,
            episode: 0,
        };
        let ids = prepare_memory(&store, key, "人物既有设定：蓝色双眼").unwrap();
        let evidence = source_evidences(&store, key, &ids).unwrap();
        assert_eq!(evidence[0]["episode_id"], Value::Null);
        store
            .write_run(
                project.id,
                "project-run",
                "request.json",
                &json!({"context":{"source_evidence_ids":ids,"item_id":item.id}}),
            )
            .unwrap();
        let dna = json!({"character_core":{"visual_dna":{"eyes":"blue"}},"stable_diffusion_tags":"blue eyes"});
        let artifacts =
            json!({"series_bible":{"world_rules":{"life":"不可复活"}},"visual_dna":dna});
        apply_memory(&store, key, "project-run", &artifacts, "done").unwrap();
        apply_memory(&store, key, "project-run", &artifacts, "done").unwrap();
        assert!(store.episodes(project.id).unwrap().is_empty());
        let records = MemoryStore::open_root(dir.path())
            .unwrap()
            .list_records(&MemoryQuery::new(project.id))
            .unwrap();
        assert!(
            records
                .iter()
                .any(|r| r.entity == Some(format!("item_{}", item.id)))
        );
        assert!(records.iter().any(|r| r.memory_type == "world_rule"));
    }
}

#[cfg(test)]
mod incremental_tests {
    use super::*;
    use aicomic_memory::MemoryQuery;

    #[test]
    fn project_outline_does_not_materialize_visual_dna_inherited_from_another_run() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let project = store.create_project("项目", None).unwrap();
        let key = EpisodeKey {
            project: project.id,
            episode: 0,
        };
        let inherited = json!({"visual_dna":{"character_core":{"visual_dna":{"eyes":"blue"}}}});
        store
            .write_run(
                project.id,
                "outline-after-dna",
                "request.json",
                &json!({
                    "action":"project_outline_generate", "context":{"artifacts":inherited}
                }),
            )
            .unwrap();
        let mut artifacts = inherited;
        artifacts["project_outline"] = json!({"episodes":[{"title":"新故事"}]});

        apply_memory(&store, key, "outline-after-dna", &artifacts, "done").unwrap();
        assert!(
            MemoryStore::open_root(dir.path())
                .unwrap()
                .list_records(&MemoryQuery::new(project.id))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn only_changed_structured_fields_and_qc_are_materialized_in_a_new_run() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let project = store.create_project("项目", None).unwrap();
        let episode = store.create_episode(project.id, "第一集", None).unwrap();
        let key = EpisodeKey {
            project: project.id,
            episode: episode.id,
        };
        let inherited = json!({
            "beat_sheet":[{"title":"旧节拍"}],
            "shots":[{"action_text":"旧镜头：林青受伤"}],
            "structured_output":{"beat_sheet":[{"title":"旧嵌套节拍"}],"shots":[{"action_text":"旧嵌套镜头：林青丢失玉剑"}]},
            "qc_report":{"issues":[{"type":"continuity","message":"旧检查问题"}],"revised_script_fountain":"旧修订"}
        });
        store
            .write_run(
                project.id,
                "new-structure",
                "request.json",
                &json!({
                    "context":{"artifacts":inherited}
                }),
            )
            .unwrap();
        let mut artifacts = inherited;
        artifacts["beat_sheet"] = json!([{"title":"新节拍"}]);
        artifacts["structured_output"]["beat_sheet"] = json!([{"title":"新嵌套节拍"}]);

        apply_memory(&store, key, "new-structure", &artifacts, "done").unwrap();
        let records = MemoryStore::open_root(dir.path())
            .unwrap()
            .list_records(&MemoryQuery::new(project.id))
            .unwrap();
        assert!(records.iter().any(|r| r.content.contains("新节拍")));
        assert!(records.iter().any(|r| r.content.contains("新嵌套节拍")));
        assert!(
            records.iter().all(|r| !r.content.contains('旧')),
            "inherited creative output must not create this run's memory: {records:?}"
        );
    }

    #[test]
    fn an_explicit_visual_dna_run_can_index_identical_dna_for_a_new_item() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let project = store.create_project("项目", None).unwrap();
        let item = store
            .create_asset_item(project.id, "同样蓝眼睛的另一角色", "persona_visual", None)
            .unwrap();
        let key = EpisodeKey {
            project: project.id,
            episode: 0,
        };
        let artifacts = json!({"visual_dna":{"character_core":{"visual_dna":{"eyes":"blue"}}}});
        store.write_run(project.id, "new-item-same-dna", "request.json", &json!({
            "action":"visual_dna_ingest", "context":{"item_id":item.id,"artifacts":artifacts}
        })).unwrap();

        apply_memory(&store, key, "new-item-same-dna", &artifacts, "done").unwrap();
        let records = MemoryStore::open_root(dir.path())
            .unwrap()
            .list_records(&MemoryQuery::new(project.id))
            .unwrap();
        assert!(
            records
                .iter()
                .any(|r| r.entity == Some(format!("item_{}", item.id)))
        );
    }
}
