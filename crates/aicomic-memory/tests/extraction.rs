use aicomic_memory::{EvidenceIngestOptions, MemoryStore, normalize_extracted_changeset};
use serde_json::json;
#[test]
fn generated_entity_ids_allow_the_same_character_name_in_two_projects() {
    let root = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(root.path()).unwrap();
    for project in [1, 2] {
        let payload = json!({"entities":[{"entity_type":"character","canonical_name":"林青"}]});
        let changeset = store
            .ensure_changeset_for_run(project, None, "same-run-name", &payload)
            .unwrap();
        store.apply_changeset(&changeset, "desktop", None).unwrap();
    }
    let first = normalize_extracted_changeset(
        1,
        None,
        "",
        &[],
        &json!({"entities":[{"entity_type":"character","canonical_name":"林青"}]}),
    )
    .unwrap();
    let second = normalize_extracted_changeset(
        2,
        None,
        "",
        &[],
        &json!({"entities":[{"entity_type":"character","canonical_name":"林青"}]}),
    )
    .unwrap();
    assert_ne!(
        first["entities"][0]["entity_id"],
        second["entities"][0]["entity_id"]
    );
}
#[test]
fn extracted_changesets_enforce_evidence_whitelist_and_fill_stable_ids() {
    let payload = json!({"entities":[{"entity_type":"character","canonical_name":"林青","created_from_evidence_id":"invented"}],"snapshots":[{"canonical_name":"林青","fields":{"visual":{"eyes":"blue"},"invented_namespace":"wrong"},"evidence_ids":["known","invented"]},{"canonical_name":"林青","fields":{},"evidence_ids":["invented"]}],"events":[{"summary":"林青获得玉剑","participants":["林青"],"evidence_ids":["known"]}],"time_constraints":[{"relation":"before","evidence_ids":["known","invented"]}]});
    let ids = vec!["known".into()];
    let out = normalize_extracted_changeset(7, Some(3), "CH03", &ids, &payload).unwrap();
    assert_eq!(out["schema_version"], "changeset.v0");
    assert_eq!(out["entities"][0]["project_id"], 7);
    let entity_id = out["entities"][0]["entity_id"].as_str().unwrap();
    assert!(entity_id.starts_with("character:林青:"));
    assert_eq!(out["snapshots"].as_array().unwrap().len(), 1);
    assert_eq!(out["snapshots"][0]["entity_id"], entity_id);
    assert_eq!(out["snapshots"][0]["evidence_ids"], json!(["known"]));
    assert!(
        out["snapshots"][0]["fields"]
            .get("invented_namespace")
            .is_none()
    );
    assert_eq!(out["snapshots"][0]["valid_from_story_order"], "CH03.E0001");
    assert_eq!(out["conflicts"][0]["conflict_type"], "invalid_evidence_ref");
    assert_eq!(out["events"][0]["participants"][0], entity_id);
    assert_eq!(out["time_constraints"][0]["project_id"], 7);
    let second = normalize_extracted_changeset(7, Some(3), "CH03", &ids, &payload).unwrap();
    assert_eq!(second["entities"][0]["entity_id"], entity_id);
}
#[test]
fn evidence_options_keep_scene_and_original_tags() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let options = EvidenceIngestOptions {
        episode_id: Some(2),
        scene_id: Some(3),
        max_quote_chars: 3,
        tags: vec!["chapter-original".into()],
    };
    let ids = store
        .ingest_with_options(1, "天地玄黄宇宙洪荒", &options)
        .unwrap();
    let evidence = store.list_evidences(1, &ids).unwrap();
    assert_eq!(evidence[0]["scene_id"], 3);
    assert_eq!(
        evidence[0]["tags"],
        json!(["chapter-original", "chunk:1/3"])
    );
}
#[test]
fn resolver_remaps_payload_duplicates_and_reports_type_collisions() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let payload = json!({"entities":[{"entity_id":"a","entity_type":"character","canonical_name":"林青"},{"entity_id":"b","entity_type":"character","canonical_name":"林 青"},{"entity_id":"c","entity_type":"organization","canonical_name":"林青"}],"snapshots":[{"entity_id":"b"}]});
    let out = store.resolve_changeset_entities(1, &payload).unwrap();
    assert_eq!(out["entities"].as_array().unwrap().len(), 2);
    assert_eq!(out["snapshots"][0]["entity_id"], "a");
    let types = out["conflicts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["conflict_type"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(types.contains(&"duplicate_in_payload"));
    assert!(types.contains(&"name_type_collision"));
}

#[test]
fn run_changeset_creation_is_idempotent_across_normalization_and_restart() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let evidence = store.ingest_text(1, Some(2), "林青获得玉剑", 600).unwrap();
    let payload = json!({"evidence_ids":evidence,"entities":[{"entity_type":"character","canonical_name":"林青"}],"snapshots":[{"canonical_name":"林青","fields":{"items":["玉剑"]},"evidence_ids":evidence}]});
    let id = store
        .ensure_changeset_for_run(1, Some(2), "run-1", &payload)
        .unwrap();
    let normalized = store.get_changeset(&id).unwrap().unwrap()["payload"].clone();
    assert_eq!(
        store
            .ensure_changeset_for_run(1, Some(2), "run-1", &payload)
            .unwrap(),
        id
    );
    drop(store);
    let store = MemoryStore::open_root(dir.path()).unwrap();
    assert_eq!(
        store
            .ensure_changeset_for_run(1, Some(2), "run-1", &payload)
            .unwrap(),
        id
    );
    assert_eq!(
        store.get_changeset(&id).unwrap().unwrap()["payload"],
        normalized
    );
    store.apply_changeset(&id, "human", None).unwrap();
    assert_eq!(
        store
            .ensure_changeset_for_run(1, Some(2), "run-1", &payload)
            .unwrap(),
        id
    );
    let mut changed = payload.clone();
    changed["snapshots"][0]["fields"]["items"] = json!(["断剑"]);
    assert!(
        store
            .ensure_changeset_for_run(1, Some(2), "run-1", &changed)
            .unwrap_err()
            .to_string()
            .contains("已审核")
    );
    assert_eq!(store.list_changesets(1, None, 10).unwrap().len(), 1);
}
#[test]
fn pending_run_changeset_can_be_revised_without_duplicate_records() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let id = store
        .ensure_changeset_for_run(1, None, "run-2", &json!({"entities":[],"notes":"first"}))
        .unwrap();
    assert_eq!(
        store
            .ensure_changeset_for_run(1, None, "run-2", &json!({"entities":[],"notes":"edited"}))
            .unwrap(),
        id
    );
    assert_eq!(
        store.get_changeset(&id).unwrap().unwrap()["payload"]["notes"],
        "edited"
    );
}
#[test]
fn resolver_preserves_type_hint_and_semantic_candidate_review() {
    use aicomic_memory::{EmbeddingProvider, MemoryRecord};
    struct Provider;
    impl EmbeddingProvider for Provider {
        fn identifier(&self) -> String {
            "resolver-model".into()
        }
        fn embed(&self, t: &[String]) -> aicomic_memory::Result<Vec<Vec<f32>>> {
            Ok(t.iter().map(|_| vec![1., 0.]).collect())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    store.upsert_entity(&json!({"entity_id":"location","project_id":1,"entity_type":"location","canonical_name":"青云山城"})).unwrap();
    store.rebuild_vectors(1, &Provider).unwrap();
    let payload = json!({"entities":[{"entity_id":"candidate","entity_type":"organization","canonical_name":"苍云山城"}]});
    let out = store
        .resolve_changeset_entities_with_provider(1, &payload, &Provider)
        .unwrap();
    let types = out["conflicts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["conflict_type"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(types.contains(&"type_hint_mismatch"));
    assert!(types.contains(&"entity_similar"));
    assert_eq!(out["entities"][0]["entity_id"], "candidate");
    let _: Option<MemoryRecord> = None;
}
#[test]
fn duplicate_payload_references_follow_final_canonical_id() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    store.upsert_entity(&json!({"entity_id":"canonical","project_id":1,"entity_type":"character","canonical_name":"林青"})).unwrap();
    let payload = json!({"entities":[{"entity_id":"a","entity_type":"character","canonical_name":"林青"},{"entity_id":"b","entity_type":"character","canonical_name":"林青"}],"snapshots":[{"entity_id":"b"}]});
    let out = store.resolve_changeset_entities(1, &payload).unwrap();
    assert_eq!(out["snapshots"][0]["entity_id"], "canonical");
}
