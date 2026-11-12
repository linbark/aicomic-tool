use aicomic_memory::{
    EmbeddingProvider, EmbeddingSettings, MemoryQuery, MemoryRecord, MemoryService, MemoryStore,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct MockEmbedding(AtomicUsize);
impl EmbeddingProvider for MockEmbedding {
    fn identifier(&self) -> String {
        "projection-test-model".into()
    }
    fn embed(&self, texts: &[String]) -> aicomic_memory::Result<Vec<Vec<f32>>> {
        self.0.fetch_add(texts.len(), Ordering::Relaxed);
        Ok(texts.iter().map(|_| vec![1., 0.]).collect())
    }
}

#[test]
fn bible_replacement_retires_deleted_rules_before_task_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let old = store
        .index_series_bible(
            1,
            &json!({"world_rules":{"magic":"旧法术"},"constraints":["禁止复活"]}),
        )
        .unwrap();
    let current = store
        .index_series_bible(1, &json!({"world_rules":{"magic":"新法术"}}))
        .unwrap();
    assert_eq!(current.len(), 1);
    for id in &old {
        assert_eq!(
            store.get_record(1, id).unwrap().unwrap().status,
            "retconned"
        );
    }
    let context = store.retrieve_for_task(1, "世界规则", None, None).unwrap();
    assert!(context.layers["negative_constraints"].records.is_empty());
    assert!(
        !context
            .prompt_fragments
            .contains_key("negative_constraints")
    );
    assert!(context.prompt_fragments["L2_static"].contains("新法术"));
    assert!(!context.prompt_fragments["L2_static"].contains("旧法术"));
    assert_eq!(store.list_records(&MemoryQuery::new(1)).unwrap().len(), 1);
    let mut history = MemoryQuery::new(1);
    history.status = Some("retconned".into());
    assert_eq!(store.list_records(&history).unwrap().len(), 2);
    assert!(store.index_series_bible(1, &json!({})).unwrap().is_empty());
    assert!(store.list_records(&MemoryQuery::new(1)).unwrap().is_empty());
}

#[test]
fn legacy_duplicates_reuse_only_their_source_and_dna_replays_stay_stable() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let mut manual =
        MemoryRecord::new(1, "static_bible", "world_rule", "世界规则: magic. 同源内容");
    manual.id = "manual-rule".into();
    manual.source_kind = "manual".into();
    manual.source_ref = Some("canonical-manual".into());
    store.write(&manual, false).unwrap();
    let mut legacy = manual.clone();
    legacy.id = "legacy-borrowed".into();
    legacy.source_kind = "system".into();
    legacy.source_ref = Some("series_bible.v1".into());
    legacy.payload_json = Some(json!({"key":"magic","value":"同源内容"}));
    // Reproduce the old cross-source deduplication that lost the Bible owner.
    assert_eq!(store.write(&legacy, true).unwrap(), manual.id);
    let bible = json!({"world_rules":{"magic":"同源内容"},"constraints":["禁止枪械","禁止枪械"]});
    let bible_ids = store.index_series_bible(1, &bible).unwrap();
    assert_eq!(bible_ids.len(), 2);
    assert!(!bible_ids.contains(&manual.id));
    assert_eq!(store.index_series_bible(1, &bible).unwrap(), bible_ids);

    let dna = json!({"character_core":{"visual_dna":{"eyes":"blue"}}});
    let mut old = MemoryRecord::new(
        1,
        "static_bible",
        "character_design",
        "角色视觉 DNA: {\"eyes\":\"blue\"}",
    );
    old.id = "legacy-dna-a".into();
    old.entity = Some("item_9".into());
    old.source_ref = Some("visual_dna.asset_item_9.v1".into());
    old.payload_json = Some(json!({"item_id":9,"visual_dna":{"eyes":"blue"}}));
    store.write(&old, false).unwrap();
    old.id = "legacy-dna-b".into();
    store.write(&old, false).unwrap();
    let ids = store.index_visual_dna(1, 9, &dna).unwrap();
    assert_eq!(ids, vec!["legacy-dna-a"]);
    assert_eq!(store.index_visual_dna(1, 9, &dna).unwrap(), ids);
    assert_eq!(
        store.get_record(1, "legacy-dna-b").unwrap().unwrap().status,
        "retconned"
    );
    let other_ids = store.index_visual_dna(1, 10, &dna).unwrap();
    assert_ne!(other_ids, ids);
    store.index_visual_dna(1, 9, &json!({})).unwrap();
    assert_eq!(
        store.get_record(1, &other_ids[0]).unwrap().unwrap().status,
        "confirmed"
    );
    assert_eq!(
        store.get_record(1, &manual.id).unwrap().unwrap().status,
        "confirmed"
    );
}

#[test]
fn complete_snapshot_removes_missing_sources_without_touching_projects_or_manual_facts() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let bible = json!({"constraints":["旧项目禁忌"]});
    let dna = json!({"character_core":{"visual_dna":{"eyes":"blue"}}});
    let old = store.index_series_bible(1, &bible).unwrap();
    let removed_dna = store.index_visual_dna(1, 9, &dna).unwrap();
    let other_project = store.index_visual_dna(2, 9, &dna).unwrap();
    let mut manual = MemoryRecord::new(
        1,
        "world_rules_negative",
        "negative_constraint",
        "人工确认禁忌",
    );
    manual.source_kind = "manual".into();
    // Even a reserved source_ref must not grant authority over hand-written facts.
    manual.source_ref = Some("series_bible.v1".into());
    manual.payload_json = Some(json!({"constraint":"人工确认禁忌"}));
    let manual_id = store.write(&manual, false).unwrap();
    store
        .upsert_evidence(&json!({"evidence_id":"raw","project_id":1,"quote":"原文不得丢失"}))
        .unwrap();
    let changeset = store.create_changeset(1, None, &json!({
        "entities":[{"entity_id":"hero","project_id":1,"entity_type":"character","canonical_name":"人工角色","source_kind":"manual"}],
        "snapshots":[{"snapshot_id":"snapshot","project_id":1,"entity_id":"hero","fields":{"eyes":"black"},"source_kind":"manual"}]
    })).unwrap();
    store.apply_changeset(&changeset, "human", None).unwrap();

    assert_eq!(
        store
            .sync_current_settings(1, &json!({}), &[(10, dna.clone())])
            .unwrap(),
        1
    );
    for id in old.iter().chain(&removed_dna) {
        assert_eq!(
            store.get_record(1, id).unwrap().unwrap().status,
            "retconned"
        );
    }
    for id in [
        manual_id.as_str(),
        "raw",
        "hero",
        "snapshot",
        "profile::hero",
    ] {
        assert_eq!(
            store.get_record(1, id).unwrap().unwrap().status,
            "confirmed"
        );
    }
    assert_eq!(
        store
            .get_record(2, &other_project[0])
            .unwrap()
            .unwrap()
            .status,
        "confirmed"
    );
    assert_eq!(
        store
            .sync_current_settings(1, &json!({}), &[(10, dna.clone()), (10, dna)])
            .unwrap(),
        1
    );
    assert_eq!(store.sync_current_settings(1, &json!({}), &[]).unwrap(), 0);
    let context = store.retrieve_for_task(1, "世界规则", None, None).unwrap();
    assert_eq!(context.layers["negative_constraints"].records.len(), 1);
    assert_eq!(
        context.layers["negative_constraints"].records[0].id,
        manual_id
    );
}

#[test]
fn restart_noop_and_failed_snapshot_preserve_facts_vectors_and_disabled_prompts() {
    let dir = tempfile::tempdir().unwrap();
    let bible = json!({"constraints":["旧禁忌"]});
    let dna = json!({"character_core":{"visual_dna":{"eyes":"blue"}}});
    let old = {
        let store = MemoryStore::open_root(dir.path()).unwrap();
        store
            .sync_current_settings(1, &bible, &[(9, dna.clone())])
            .unwrap();
        store.rebuild_vectors(1, &MockEmbedding::default()).unwrap();
        store.index_series_bible(1, &bible).unwrap()
    };
    let db = rusqlite::Connection::open(dir.path().join("memory_store.db")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_noop BEFORE UPDATE ON memory_records BEGIN SELECT RAISE(ABORT,'unchanged rows must not be rewritten'); END;").unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    assert_eq!(
        store
            .sync_current_settings(1, &bible, &[(9, dna.clone())])
            .unwrap(),
        2
    );
    assert_eq!(store.index_series_bible(1, &bible).unwrap(), old);
    db.execute_batch("DROP TRIGGER reject_noop; CREATE TRIGGER fail_later_source BEFORE INSERT ON memory_records WHEN NEW.source_ref='visual_dna.asset_item_9.v1' BEGIN SELECT RAISE(ABORT,'injected later source failure'); END;").unwrap();
    let new_bible = json!({"constraints":["新禁忌"]});
    let new_dna = json!({"character_core":{"visual_dna":{"eyes":"black"}}});
    assert!(
        store
            .sync_current_settings(1, &new_bible, &[(9, new_dna.clone())])
            .is_err()
    );
    assert_eq!(
        store.get_record(1, &old[0]).unwrap().unwrap().status,
        "confirmed"
    );
    let mut query = MemoryQuery::new(1);
    query.query_text = Some("禁忌".into());
    let after_failure = store
        .retrieve_semantic(&query, &MockEmbedding::default())
        .unwrap();
    assert_eq!(after_failure.records.len(), 2);
    assert!(
        after_failure
            .records
            .iter()
            .all(|r| !r.content.contains("新禁忌") && !r.content.contains("black"))
    );
    db.execute_batch("DROP TRIGGER fail_later_source;").unwrap();
    assert_eq!(
        store
            .sync_current_settings(1, &new_bible, &[(9, new_dna)])
            .unwrap(),
        2
    );
    drop(store);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let service = MemoryService::new(dir.path(), EmbeddingSettings::default());
    let context = runtime
        .block_on(service.task_context(1, "世界规则", None))
        .unwrap();
    assert_eq!(context.retrieval_mode, "structured");
    assert!(context.prompt_fragments["negative_constraints"].contains("新禁忌"));
    assert!(!context.prompt_fragments["negative_constraints"].contains("旧禁忌"));
}

#[test]
fn vectors_exclude_history_incrementally_and_late_embeddings_cannot_revive_it() {
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    struct RetiringEmbedding {
        root: PathBuf,
        retire: AtomicBool,
    }
    impl EmbeddingProvider for RetiringEmbedding {
        fn identifier(&self) -> String {
            "projection-test-model".into()
        }
        fn embed(&self, texts: &[String]) -> aicomic_memory::Result<Vec<Vec<f32>>> {
            if self.retire.swap(false, Ordering::SeqCst) {
                // Simulate a source sync finishing while embeddings are in flight.
                MemoryStore::open_root(&self.root)?.sync_current_settings(1, &json!({}), &[])?;
            }
            Ok(texts.iter().map(|_| vec![1., 0.]).collect())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let provider = MockEmbedding::default();
    store
        .index_series_bible(1, &json!({"constraints":["旧禁忌"]}))
        .unwrap();
    assert_eq!(store.rebuild_vectors(1, &provider).unwrap(), 1);
    let bible = json!({"constraints":["新禁忌"]});
    let current = store.index_series_bible(1, &bible).unwrap();
    assert_eq!(store.refresh_vectors(1, &provider).unwrap(), 1);
    assert_eq!(provider.0.load(Ordering::Relaxed), 2);
    store.index_series_bible(1, &bible).unwrap();
    assert_eq!(store.refresh_vectors(1, &provider).unwrap(), 0);
    assert_eq!(provider.0.load(Ordering::Relaxed), 2);
    let mut query = MemoryQuery::new(1);
    query.query_text = Some("禁忌".into());
    assert_eq!(
        store.retrieve_semantic(&query, &provider).unwrap().records[0].id,
        current[0]
    );
    assert_eq!(store.rebuild_vectors(1, &provider).unwrap(), 1);

    store
        .index_series_bible(1, &json!({"constraints":["更改禁忌"]}))
        .unwrap();
    let late = RetiringEmbedding {
        root: dir.path().into(),
        retire: AtomicBool::new(true),
    };
    assert_eq!(store.refresh_vectors(1, &late).unwrap(), 0);
    let db = rusqlite::Connection::open(dir.path().join("memory_store.db")).unwrap();
    let vector_count = || {
        db.query_row("SELECT count(*) FROM rust_memory_vectors", [], |row| {
            row.get::<_, usize>(0)
        })
        .unwrap()
    };
    assert_eq!(vector_count(), 0);
    assert!(
        store
            .retrieve_semantic(&query, &provider)
            .unwrap()
            .records
            .is_empty()
    );
    store.index_series_bible(1, &bible).unwrap();
    late.retire.store(true, Ordering::SeqCst);
    assert_eq!(store.rebuild_vectors(1, &late).unwrap(), 0);
    assert_eq!(vector_count(), 0);
    let mut history = MemoryQuery::new(1);
    history.status = Some("retconned".into());
    assert_eq!(store.list_records(&history).unwrap().len(), 3);
}

#[test]
fn damaged_or_ambiguous_snapshots_never_mean_source_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let bible = json!({"constraints":["必须保留"]});
    let dna = json!({"character_core":{"visual_dna":{"eyes":"blue"}}});
    store
        .sync_current_settings(1, &bible, &[(9, dna.clone())])
        .unwrap();
    let before = serde_json::to_value(store.list_records(&MemoryQuery::new(1)).unwrap()).unwrap();
    for damaged in [json!(null), json!([]), json!("bad"), json!(false)] {
        assert!(store.index_series_bible(1, &damaged).is_err());
        assert!(store.index_visual_dna(1, 9, &damaged).is_err());
        assert!(
            store
                .sync_current_settings(1, &json!({}), &[(9, dna.clone()), (10, damaged)])
                .is_err()
        );
    }
    for damaged in [
        json!({"constraints":"broken"}),
        json!({"world_rules":[]}),
        json!({"glossary":"broken"}),
        json!({"characters":{"林青":"broken"}}),
    ] {
        assert!(store.index_series_bible(1, &damaged).is_err());
    }
    assert!(
        store
            .index_visual_dna(1, 9, &json!({"character_core":[]}))
            .is_err()
    );
    assert!(
        store
            .sync_current_settings(1, &json!({}), &[(9, dna.clone()), (9, json!({}))])
            .is_err()
    );
    assert!(
        store
            .sync_current_settings(1, &json!({}), &[(-1, dna)])
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(store.list_records(&MemoryQuery::new(1)).unwrap()).unwrap(),
        before
    );
}
