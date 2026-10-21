use aicomic_memory::{MemoryQuery, MemoryRecord, MemoryStore, UnconfiguredEmbedding};
use serde_json::json;

#[test]
fn legacy_database_roundtrip_and_project_isolation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory_store.db");
    // Exactly the old pre-canonical schema. Opening must add columns, preserve IDs.
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE memory_records(id TEXT PRIMARY KEY, project_id INTEGER NOT NULL, namespace TEXT NOT NULL, type TEXT NOT NULL, entity TEXT, content TEXT NOT NULL, payload_json TEXT, source_ref TEXT, time_index TEXT, hash TEXT, created_at_ms INTEGER, updated_at_ms INTEGER); INSERT INTO memory_records(id,project_id,namespace,type,content) VALUES('001122aabb',7,'static_bible','world_rule','旧设定');").unwrap();
    drop(db);
    let store = MemoryStore::open(&path).unwrap();
    assert_eq!(
        store.list_records(&MemoryQuery::new(7)).unwrap()[0].id,
        "001122aabb"
    );
    assert!(store.list_records(&MemoryQuery::new(8)).unwrap().is_empty());
    store
        .write(
            &MemoryRecord::new(7, "dynamic_plot", "event", "角色获得玉剑"),
            true,
        )
        .unwrap();
    drop(store);
    assert_eq!(
        MemoryStore::open_root(dir.path())
            .unwrap()
            .list_records(&MemoryQuery::new(7))
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn approval_is_atomic_and_materializes_latest_confirmed_profile() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let payload = json!({"entities":[{"entity_id":"char:1","project_id":2,"entity_type":"character","canonical_name":"林青","aliases":["阿青"]}],"snapshots":[{"snapshot_id":"s1","project_id":2,"entity_id":"char:1","valid_from_story_order":"CH01","fields":{"items":["剑"]}},{"snapshot_id":"s2","project_id":2,"entity_id":"char:1","valid_from_story_order":"CH02","fields":{"injuries":["左臂受伤"]}}]});
    let id = store.create_changeset(2, None, &payload).unwrap();
    store.apply_changeset(&id, "human", Some("已核对")).unwrap();
    let cs = store.get_changeset(&id).unwrap().unwrap();
    assert_eq!(cs["review_status"], "approved");
    assert_eq!(cs["review_log"][0]["note"], "已核对");
    let mut q = MemoryQuery::new(2);
    q.namespace = Some("static_bible".into());
    let records = store.list_records(&q).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "profile::char:1");
    assert_eq!(
        records[0].payload_json.as_ref().unwrap()["current_snapshot_id"],
        "s2"
    );
    let bad=store.create_changeset(2,None,&json!({"entities":[{"entity_id":"bad","project_id":2,"entity_type":"character","canonical_name":"幽灵"}],"snapshots":[{"snapshot_id":"broken","project_id":99,"entity_id":"bad","fields":{}}]})).unwrap();
    assert!(store.apply_changeset(&bad, "human", None).is_err());
    assert!(store.find_entities(2, "幽灵").unwrap().is_empty());
    assert_eq!(
        store.get_changeset(&bad).unwrap().unwrap()["review_status"],
        "pending_review"
    );
}

#[test]
fn chinese_evidence_chunks_preserve_raw_text_and_hex_ids() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let ids = store
        .ingest_text(1, Some(9), "白云苍狗天地悠悠\n\n春风又绿江南岸", 4)
        .unwrap();
    assert_eq!(ids.len(), 4);
    assert!(ids.iter().all(|id| id.len() == 32));
    let evidence = store.list_evidences(1, &ids).unwrap();
    let quotes: Vec<_> = evidence
        .iter()
        .map(|e| e["quote"].as_str().unwrap())
        .collect();
    assert_eq!(quotes, vec!["白云苍狗", "天地悠悠", "春风又绿", "江南岸"]);
    assert_eq!(evidence[2]["span"]["paragraph_index"], 1);
}

#[test]
fn semantic_search_requires_configured_embeddings_but_raw_facts_work() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    store
        .write(
            &MemoryRecord::new(1, "static_bible", "world_rule", "无月之夜禁止出城"),
            true,
        )
        .unwrap();
    assert_eq!(store.list_records(&MemoryQuery::new(1)).unwrap().len(), 1);
    assert!(
        store
            .rebuild_vectors(1, &UnconfiguredEmbedding)
            .unwrap_err()
            .to_string()
            .contains("未配置")
    );
}

#[test]
fn conflicts_can_be_audited_and_resolved_without_provider() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let id=store.create_conflict(1,&json!({"conflict_type":"entity_ambiguous","entity_id":"a","old_claim":{"name":"旧"},"new_claim":{"name":"新"}})).unwrap();
    assert_eq!(store.list_conflicts(1, Some("open"), 10).unwrap().len(), 1);
    store
        .resolve_conflict(&id, "editor", Some("另有其人"))
        .unwrap();
    assert!(
        store
            .list_conflicts(1, Some("open"), 10)
            .unwrap()
            .is_empty()
    );
    let conflicts = store.list_conflicts(1, Some("resolved"), 10).unwrap();
    assert_eq!(conflicts[0]["resolution_note"], "另有其人");
    assert_eq!(
        store
            .get_record(1, &id)
            .unwrap()
            .unwrap()
            .payload_json
            .unwrap()["status"],
        "resolved"
    );
}

#[test]
fn time_records_keep_original_contract_defaults_and_lists() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let id=store.upsert_time_constraint(&json!({"project_id":1,"relation":"flashback_of","from_event_id":"e1","to_event_id":"e2"})).unwrap();
    let records = store.list_time_constraints(1, None, 20).unwrap();
    assert_eq!(records[0]["id"], id);
    assert_eq!(records[0]["status"], "hypothesis");
    assert_eq!(records[0]["confidence"], 0.5);
    store.upsert_time_block(&json!({"id":"b1","project_id":1,"name":"十年前","constraint_ids":[id],"event_ids":["e1"]})).unwrap();
    assert_eq!(
        store.list_time_blocks(1, None, 10).unwrap()[0]["name"],
        "十年前"
    );
}

#[test]
fn exact_alias_resolves_only_unique_same_type_entities() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    store.upsert_entity(&json!({"entity_id":"old","project_id":1,"entity_type":"character","canonical_name":"林青","aliases":["阿青"]})).unwrap();
    let payload = json!({"entities":[{"entity_id":"new","project_id":1,"entity_type":"character","canonical_name":"阿青"}],"snapshots":[{"snapshot_id":"s","project_id":1,"entity_id":"new","fields":{}}]});
    let out = store.resolve_changeset_entities(1, &payload).unwrap();
    assert_eq!(out["entities"][0]["entity_id"], "old");
    assert_eq!(out["snapshots"][0]["entity_id"], "old");
    store.upsert_entity(&json!({"entity_id":"other","project_id":1,"entity_type":"character","canonical_name":"阿青"})).unwrap();
    let out = store.resolve_changeset_entities(1, &payload).unwrap();
    assert_eq!(out["entities"][0]["entity_id"], "new");
    assert_eq!(out["conflicts"][0]["conflict_type"], "entity_ambiguous");
}

#[test]
fn series_bible_and_visual_dna_materialize_original_namespaces() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let bible = json!({"world_rules":{"magic":"禁用"},"characters":{"林青":{"visual_dna_ref":"asset1","personality":"沉静"}},"glossary":{"仙城":"主城"},"constraints":["不得复活"]});
    assert_eq!(store.index_series_bible(1, &bible).unwrap().len(), 5);
    assert_eq!(store.index_series_bible(1, &bible).unwrap().len(), 5);
    assert_eq!(store.list_records(&MemoryQuery::new(1)).unwrap().len(), 5);
    assert_eq!(store.index_visual_dna(1,9,&json!({"character_core":{"visual_dna":{"eyes":"blue"},"attire":{"coat":"black"}},"technical_specs":{"ratio":"16:9"},"stable_diffusion_tags":"black coat"})).unwrap().len(),4);
}

#[test]
fn persistent_vectors_rank_filter_and_invalidate_changed_content() {
    struct Embeddings;
    impl aicomic_memory::EmbeddingProvider for Embeddings {
        fn identifier(&self) -> String {
            "test-v1".into()
        }
        fn embed(&self, texts: &[String]) -> aicomic_memory::Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|t| {
                    if t.contains("剑") {
                        vec![1., 0.]
                    } else {
                        vec![0., 1.]
                    }
                })
                .collect())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let mut sword = MemoryRecord::new(1, "static_bible", "prop", "玉剑");
    let id = store.write(&sword, false).unwrap();
    store
        .write(
            &MemoryRecord::new(1, "static_bible", "location", "青城"),
            false,
        )
        .unwrap();
    assert_eq!(store.rebuild_vectors(1, &Embeddings).unwrap(), 2);
    drop(store);
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let mut q = MemoryQuery::new(1);
    q.query_text = Some("剑".into());
    q.top_k = 1;
    assert_eq!(
        store.retrieve_semantic(&q, &Embeddings).unwrap().records[0].id,
        id
    );
    sword.content = "城门".into();
    store.write(&sword, false).unwrap();
    let err = store
        .retrieve_semantic(&q, &Embeddings)
        .unwrap_err()
        .to_string();
    assert!(err.contains("重建"));
}

#[test]
fn approved_changeset_never_regresses_profile_or_overwrites_existing_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    for (order, description) in [("CH03", "新状态"), ("CH01", "旧状态")] {
        let id=store.create_changeset(1,None,&json!({"entities":[{"entity_id":"hero","project_id":1,"entity_type":"character","canonical_name":"主角"}],"snapshots":[{"snapshot_id":order,"project_id":1,"entity_id":"hero","valid_from_story_order":order,"fields":{"goals":description}}]})).unwrap();
        store.apply_changeset(&id, "editor", None).unwrap();
    }
    assert_eq!(
        store
            .get_record(1, "profile::hero")
            .unwrap()
            .unwrap()
            .payload_json
            .unwrap()["current_snapshot_id"],
        "CH03"
    );
}

#[test]
fn http_embeddings_obey_wire_contract_and_response_index_order() {
    use aicomic_memory::{EmbeddingProvider, OpenAICompatibleEmbedding};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buf = [0; 4096];
        loop {
            let n = socket.read(&mut buf).unwrap();
            request.extend_from_slice(&buf[..n]);
            if request.windows(4).any(|w| w == b"\r\n\r\n") && request.ends_with(b"}") {
                break;
            }
        }
        let request = String::from_utf8(request).unwrap();
        assert!(request.starts_with("POST /v1/embeddings "));
        assert!(request.contains("Bearer secret"));
        assert!(request.contains("\"model\":\"bge\""));
        let body = r#"{"data":[{"index":1,"embedding":[0,1]},{"index":0,"embedding":[1,0]}]}"#;
        write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    let provider =
        OpenAICompatibleEmbedding::new(&format!("http://{address}/v1"), "secret", "bge").unwrap();
    assert_eq!(
        provider.embed(&["a".into(), "b".into()]).unwrap(),
        vec![vec![1., 0.], vec![0., 1.]]
    );
    server.join().unwrap();
}

#[test]
fn canonical_write_cannot_leave_partial_row_after_id_collision() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let mut occupied = MemoryRecord::new(1, "static_bible", "world_rule", "原项目事实");
    occupied.id = "occupied".into();
    store.write(&occupied, false).unwrap();
    assert!(store.upsert_entity(&json!({"entity_id":"occupied","project_id":2,"entity_type":"character","canonical_name":"其他项目"})).is_err());
    assert!(store.find_entities(2, "其他项目").unwrap().is_empty());
}

#[test]
fn structured_storyboard_and_qc_outputs_write_episodic_and_dynamic_facts() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let ids=store.extract_structured_output(1,&json!({"beat_sheet":[{"title":"初遇","beat_type":"setup"}],"shots":[{"action_text":"林青捡起玉剑，左臂受伤"},{"action_text":"天空晴朗"}]}),Some(2),Some(3),Some("run.writer")).unwrap();
    assert_eq!(ids.len(), 3);
    let mut q = MemoryQuery::new(1);
    q.namespace = Some("episodic".into());
    let records = store.list_records(&q).unwrap();
    assert_eq!(records.len(), 2);
    let shot = records.iter().find(|r| r.content.contains("镜头")).unwrap();
    assert_eq!(
        shot.payload_json.as_ref().unwrap()["state_changes"]["prop_acquired"],
        true
    );
    assert_eq!(shot.time_index.as_deref(), Some("ep2_sc3_bt0"));
    assert_eq!(store.extract_qc_report(1,&json!({"issues":[{"type":"continuity","message":"道具不连续"}],"revised_script_fountain":"修订内容"}),Some(2),None,Some("run.qc")).unwrap().len(),2);
}
#[test]
fn task_retrieval_keeps_confirmed_negative_constraints_and_detects_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let mut rule = MemoryRecord::new(1, "world_rules_negative", "negative_constraint", "禁止复活");
    store.write(&rule, false).unwrap();
    rule.id = "unconfirmed".into();
    rule.status = "hypothesis".into();
    store.write(&rule, false).unwrap();
    let mut character = MemoryRecord::new(1, "static_bible", "character_fact", "林青双眼蓝色");
    character.entity = Some("林青".into());
    store.write(&character, false).unwrap();
    character.id = "changed".into();
    character.content = "林青双眼黑色".into();
    store.write(&character, false).unwrap();
    let context = store
        .retrieve_for_task(1, "角色视觉规则", Some("林青"), None)
        .unwrap();
    assert_eq!(context.layers["negative_constraints"].records.len(), 1);
    assert_eq!(context.layers["L2_static"].records.len(), 2);
    assert_eq!(context.conflicts.len(), 1);
    assert!(context.prompt_fragments["negative_constraints"].contains("禁止复活"));
    assert_eq!(context.retrieval_mode, "structured");
}
#[test]
fn dirty_vector_refresh_embeds_only_changed_and_new_facts() {
    use aicomic_memory::EmbeddingProvider;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Provider(AtomicUsize);
    impl EmbeddingProvider for Provider {
        fn identifier(&self) -> String {
            "incremental-model".into()
        }
        fn embed(&self, t: &[String]) -> aicomic_memory::Result<Vec<Vec<f32>>> {
            self.0.fetch_add(t.len(), Ordering::Relaxed);
            Ok(t.iter()
                .map(|s| {
                    if s.contains("剑") {
                        vec![1., 0.]
                    } else {
                        vec![0., 1.]
                    }
                })
                .collect())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    let provider = Provider(AtomicUsize::new(0));
    let mut record = MemoryRecord::new(1, "static_bible", "prop", "玉剑");
    store.write(&record, false).unwrap();
    store
        .write(
            &MemoryRecord::new(1, "static_bible", "location", "城门"),
            false,
        )
        .unwrap();
    assert!(
        store
            .refresh_vectors(1, &provider)
            .unwrap_err()
            .to_string()
            .contains("重建")
    );
    store.rebuild_vectors(1, &provider).unwrap();
    assert_eq!(provider.0.load(Ordering::Relaxed), 2);
    record.content = "断剑".into();
    store.write(&record, false).unwrap();
    store
        .write(&MemoryRecord::new(1, "static_bible", "prop", "铜剑"), false)
        .unwrap();
    assert_eq!(store.refresh_vectors(1, &provider).unwrap(), 2);
    assert_eq!(provider.0.load(Ordering::Relaxed), 4);
    assert_eq!(store.refresh_vectors(1, &provider).unwrap(), 0);
    assert_eq!(provider.0.load(Ordering::Relaxed), 4);
    let mut q = MemoryQuery::new(1);
    q.query_text = Some("剑".into());
    assert_eq!(
        store
            .retrieve_semantic(&q, &provider)
            .unwrap()
            .records
            .len(),
        3
    );
}
