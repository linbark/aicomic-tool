use aicomic_memory::{EmbeddingSettings, MemoryService};
#[test]
fn settings_roundtrip_and_disabled_service_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = EmbeddingSettings::load(dir.path()).unwrap();
    assert_eq!(settings.mode, "disabled");
    settings.mode = "http".into();
    settings.base_url = "https://embedding.example/v1".into();
    settings.api_key = "test-secret".into();
    settings.model = "bge-m3".into();
    settings.max_tokens = 1024;
    settings.save(dir.path()).unwrap();
    let loaded = EmbeddingSettings::load(dir.path()).unwrap();
    assert_eq!(loaded.api_key, "test-secret");
    assert_eq!(loaded.max_tokens, 1024);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let service = MemoryService::new(dir.path(), EmbeddingSettings::default());
    assert!(
        runtime
            .block_on(service.rebuild(1))
            .unwrap_err()
            .to_string()
            .contains("未配置")
    );
    assert!(
        runtime
            .block_on(service.semantic_search(1, "剑", 10))
            .unwrap_err()
            .to_string()
            .contains("未配置")
    );
}
#[test]
fn model_identity_and_dimension_mismatch_require_rebuild() {
    use aicomic_memory::{EmbeddingProvider, MemoryQuery, MemoryRecord, MemoryStore};
    struct Provider(&'static str, usize);
    impl EmbeddingProvider for Provider {
        fn identifier(&self) -> String {
            self.0.into()
        }
        fn embed(&self, t: &[String]) -> aicomic_memory::Result<Vec<Vec<f32>>> {
            Ok(t.iter().map(|_| vec![1.; self.1]).collect())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    store
        .write(&MemoryRecord::new(1, "static_bible", "prop", "青剑"), false)
        .unwrap();
    store.rebuild_vectors(1, &Provider("model-a", 2)).unwrap();
    let mut q = MemoryQuery::new(1);
    q.query_text = Some("剑".into());
    assert!(
        store
            .retrieve_semantic(&q, &Provider("model-b", 2))
            .unwrap_err()
            .to_string()
            .contains("重建")
    );
    assert!(
        store
            .retrieve_semantic(&q, &Provider("model-a", 3))
            .unwrap_err()
            .to_string()
            .contains("重建")
    );
}
#[test]
fn disabled_context_exposes_structured_mode_without_loading_model() {
    use aicomic_memory::{MemoryRecord, MemoryStore};
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open_root(dir.path()).unwrap();
    store
        .write(
            &MemoryRecord::new(
                1,
                "world_rules_negative",
                "negative_constraint",
                "禁止超自然复活",
            ),
            false,
        )
        .unwrap();
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
    assert_eq!(context.layers["negative_constraints"].records.len(), 1);
}
#[test]
fn asynchronous_run_changeset_service_preserves_single_review_record() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let service = MemoryService::new(dir.path(), EmbeddingSettings::default());
    let payload = serde_json::json!({"entities":[],"notes":"draft"});
    let first = runtime
        .block_on(service.ensure_changeset_for_run(1, None, "service-run", payload.clone()))
        .unwrap();
    let second = runtime
        .block_on(service.ensure_changeset_for_run(1, None, "service-run", payload))
        .unwrap();
    assert_eq!(first, second);
}
#[test]
fn existing_run_changeset_does_not_require_reloading_unavailable_model() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let payload = serde_json::json!({"entities":[],"notes":"already saved"});
    let service = MemoryService::new(dir.path(), EmbeddingSettings::default());
    let id = runtime
        .block_on(service.ensure_changeset_for_run(1, None, "saved-run", payload.clone()))
        .unwrap();
    let settings = EmbeddingSettings {
        mode: "local".into(),
        model_dir: dir
            .path()
            .join("model-not-present")
            .to_string_lossy()
            .into_owned(),
        ..Default::default()
    };
    let reopened = MemoryService::new(dir.path(), settings);
    assert_eq!(
        runtime
            .block_on(reopened.ensure_changeset_for_run(1, None, "saved-run", payload))
            .unwrap(),
        id
    );
}
