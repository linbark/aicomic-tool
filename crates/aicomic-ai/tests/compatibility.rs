use aicomic_ai::{AiSettings, ContextStore, PromptRegistry, extract_json};
use serde_json::json;

#[test]
fn old_settings_load_defaults_and_never_debug_print_the_key() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("ai_settings.json"),
        r#"{"api_key":"secret-key","max_tokens":0,"temperature":0}"#,
    )
    .unwrap();
    let settings = AiSettings::load(root.path()).unwrap();
    assert_eq!(settings.api_key, "secret-key");
    assert_eq!(settings.temperature, 0.0);
    assert_eq!(settings.normalized_max_tokens(), 8192);
    assert_eq!(settings.base_url, "https://api.deepseek.com");
    assert!(!format!("{settings:?}").contains("secret-key"));
    assert_eq!(settings.masked()["has_api_key"], true);
    assert!(settings.masked().get("api_key").is_none());
}

#[test]
fn prompt_override_reset_and_custom_delete_keep_the_legacy_envelope() {
    let root = tempfile::tempdir().unwrap();
    let registry = PromptRegistry::new(root.path().to_path_buf());
    let original = registry.get("episode_assets_visual_dna_system").unwrap();
    assert!(original.prompt.contains("characters"));
    registry
        .upsert(
            "episode_assets_visual_dna_system",
            "新标题",
            "workflow",
            "新提示",
        )
        .unwrap();
    assert!(
        registry
            .get("episode_assets_visual_dna_system")
            .unwrap()
            .is_modified
    );
    registry.reset("episode_assets_visual_dna_system").unwrap();
    assert_eq!(
        registry
            .get("episode_assets_visual_dna_system")
            .unwrap()
            .prompt,
        original.prompt
    );
    registry
        .create("my_prompt", "自定义", "writing", "你好")
        .unwrap();
    assert!(
        registry
            .create("my_prompt", "重复", "writing", "x")
            .is_err()
    );
    let raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("prompt_templates.json")).unwrap())
            .unwrap();
    assert_eq!(raw["templates"]["my_prompt"]["prompt"], "你好");
    registry.delete("my_prompt").unwrap();
    assert!(registry.get("my_prompt").is_err());
}

#[test]
fn context_and_stages_use_old_paths_and_reject_traversal() {
    let root = tempfile::tempdir().unwrap();
    let store = ContextStore::new(root.path().to_path_buf());
    store
        .put_context("12", "series_bible", "v1", &json!({"title":"中文"}))
        .unwrap();
    assert_eq!(
        store
            .get_context("12", "series_bible", "v1")
            .unwrap()
            .unwrap()["title"],
        "中文"
    );
    assert!(
        root.path()
            .join("projects/12/context/series_bible.v1.json")
            .exists()
    );
    store
        .write_stage("12", "run1", "chat.status", &json!({"status":"paused"}))
        .unwrap();
    assert_eq!(
        store
            .read_stage("12", "run1", "chat.status")
            .unwrap()
            .unwrap()["status"],
        "paused"
    );
    assert_eq!(
        store.list_stages("12", "run1").unwrap()[0].name,
        "chat.status"
    );
    assert!(
        store
            .put_context("../escape", "series_bible", "v1", &json!({}))
            .is_err()
    );
    assert!(
        store
            .write_stage("12", "../escape", "chat.status", &json!({}))
            .is_err()
    );
}

#[test]
fn llm_json_extraction_handles_fences_prose_arrays_and_braces_inside_strings() {
    assert_eq!(
        extract_json("```json\n{\"中文\":\"a } b\"}\n```").unwrap(),
        json!({"中文":"a } b"})
    );
    assert_eq!(
        extract_json("以下是结果：[{\"name\":\"甲\"},{\"name\":\"乙\"}] 完成").unwrap(),
        json!([{"name":"甲"},{"name":"乙"}])
    );
    assert_eq!(
        extract_json("分析：{\"a\":1}\n{\"b\":2}").unwrap(),
        json!({"a":1})
    );
    assert!(extract_json("{\"unfinished\":").is_err());
}
