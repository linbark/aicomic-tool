use aicomic_ai::{AiSettings, ContextStore, ExecutionInput, ReviewDecision, WorkflowEngine};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

fn provider(replies: Vec<Value>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        for reply in replies {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            let mut data = vec![];
            let mut buf = [0; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                data.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&data);
                if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                    let len = headers
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|s| s.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if body.len() >= len {
                        break;
                    }
                }
            }
            let text = reply
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("```json\n{reply}\n```"));
            let body = json!({"choices":[{"message":{"content":text},"finish_reason":"stop"}]})
                .to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    (url, handle)
}
fn input(action: &str, run: &str) -> ExecutionInput {
    ExecutionInput {
        project_id: "12".into(),
        episode_id: Some(8),
        script: "小明走进茶馆，发现失踪的父亲。".into(),
        context: json!({}),
        action: action.into(),
        run_id: run.into(),
    }
}
fn settings(url: String) -> AiSettings {
    AiSettings {
        api_key: "mock-only".into(),
        base_url: url,
        ..Default::default()
    }
}

#[tokio::test]
async fn episode_exec_persists_outline_assets_and_split_then_resumes_both_human_reviews() {
    let root = tempfile::tempdir().unwrap();
    let (url, handle) = provider(vec![
        json!({"logline":"重逢","characters":[],"act_1":"茶馆","act_2":"相认","act_3":"离开","key_beats":[]}),
        json!({"characters":[{"name":"小明","description":"少年","visual_dna":{"hair":"黑"}}],"props":[],"locations":[{"name":"茶馆"}],"series_style":{}}),
        json!({"episodes":[{"title":"重逢","script":"小明走进茶馆","outline":"相认"}]}),
    ]);
    let engine = WorkflowEngine::new(root.path().to_path_buf(), settings(url));
    let first = engine
        .execute(input("episode_execute", "episode-run"))
        .await
        .unwrap();
    assert_eq!(first.status, "paused");
    assert_eq!(first.waiting_for.as_deref(), Some("confirm_split"));
    assert_eq!(first.artifacts["outline"]["logline"], "重逢");
    let store = ContextStore::new(root.path().to_path_buf());
    assert!(
        store
            .read_stage("12", "episode-run", "episode_assets_visual_dna.raw")
            .unwrap()
            .is_some()
    );
    // Resume with a newly constructed engine: restart recovery must not need an in-memory task.
    let restarted = WorkflowEngine::new(root.path().to_path_buf(), AiSettings::default());
    let next = restarted
        .resume("12", "episode-run", ReviewDecision::Confirmed, None)
        .await
        .unwrap();
    assert_eq!(next.waiting_for.as_deref(), Some("confirm_ingest"));
    assert_eq!(
        next.artifacts["approved_split_episodes"]["episodes"][0]["title"],
        "重逢"
    );
    let done = restarted
        .resume("12", "episode-run", ReviewDecision::Confirmed, None)
        .await
        .unwrap();
    assert_eq!(done.status, "done");
    assert_eq!(
        done.artifacts["approved_asset_ingest_plan"]["items"][0]["name"],
        "小明"
    );
    assert!(
        restarted
            .resume("12", "episode-run", ReviewDecision::Confirmed, None)
            .await
            .is_err()
    );
    assert_eq!(
        store.read_run("12", "episode-run").unwrap().unwrap()["response"]["status"],
        "done"
    );
    handle.join().unwrap();
}

#[tokio::test]
async fn invalid_json_is_repaired_once_before_a_structured_action_is_saved() {
    let root = tempfile::tempdir().unwrap();
    let (url, handle) = provider(vec![
        json!("broken output"),
        json!({"characters":[],"props":[],"locations":[],"series_style":{}}),
    ]);
    let engine = WorkflowEngine::new(root.path().to_path_buf(), settings(url));
    let done = engine
        .execute(input("episode_assets_visual_dna", "repair-run"))
        .await
        .unwrap();
    assert_eq!(done.status, "done");
    assert!(done.artifacts["assets_visual_dna"]["characters"].is_array());
    handle.join().unwrap();
}

#[tokio::test]
async fn chat_plan_chains_outline_into_script_with_auditable_step_stages() {
    let root = tempfile::tempdir().unwrap();
    let (url, handle) = provider(vec![
        json!({"steps":[{"action_key":"outline_generate"},{"action_key":"generate_script"}]}),
        json!("故事大纲"),
        json!("INT. 茶馆 - 日\n小明推门。"),
    ]);
    let engine = WorkflowEngine::new(root.path().to_path_buf(), settings(url));
    let done = engine.execute(input("chat", "chat-run")).await.unwrap();
    assert_eq!(done.output_text, "INT. 茶馆 - 日\n小明推门。");
    assert_eq!(done.artifacts["outline"], "故事大纲");
    assert!(
        ContextStore::new(root.path().to_path_buf())
            .read_stage("12", "chat-run", "chat.step.1.end")
            .unwrap()
            .is_some()
    );
    handle.join().unwrap();
}

#[tokio::test]
async fn failed_provider_writes_error_status_and_never_fabricates_output() {
    let root = tempfile::tempdir().unwrap();
    let engine = WorkflowEngine::new(root.path().to_path_buf(), AiSettings::default());
    assert!(
        engine
            .execute(input("episode_outline_generate", "failed-run"))
            .await
            .is_err()
    );
    let store = ContextStore::new(root.path().to_path_buf());
    assert_eq!(
        store
            .read_stage("12", "failed-run", "chat.status")
            .unwrap()
            .unwrap()["status"],
        "error"
    );
    assert!(
        store
            .read_stage("12", "failed-run", "episode_outline_generate.raw")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn connection_test_only_sends_a_tiny_user_initiated_request() {
    let root = tempfile::tempdir().unwrap();
    let (url, handle) = provider(vec![json!("OK")]);
    let engine = WorkflowEngine::new(root.path().to_path_buf(), settings(url));
    assert!(engine.test_connection().await.unwrap().contains("成功"));
    handle.join().unwrap();
}

#[tokio::test]
async fn project_outline_generation_and_optimization_write_compatible_context() {
    let root = tempfile::tempdir().unwrap();
    let (url, handle) = provider(vec![
        json!({"title":"初稿","total_episodes":2,"episode_outlines":[]}),
        json!({"project_outline":{"title":"改稿","total_episodes":2,"episode_outlines":[]},"changes_summary":"加强悬念"}),
    ]);
    let engine = WorkflowEngine::new(root.path().to_path_buf(), settings(url));
    engine
        .execute(input("project_outline_generate", "outline-run"))
        .await
        .unwrap();
    let result = engine
        .execute(input("project_outline_optimize", "optimize-run"))
        .await
        .unwrap();
    assert_eq!(result.artifacts["changes_summary"], "加强悬念");
    assert_eq!(
        engine
            .store()
            .get_context("12", "project_outline", "v1")
            .unwrap()
            .unwrap()["title"],
        "改稿"
    );
    handle.join().unwrap();
}

#[tokio::test]
async fn visual_dna_is_explicitly_text_based_and_saves_per_asset_context() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("data")).unwrap();
    std::fs::write(root.path().join("data/portrait.png"), b"mock file").unwrap();
    let (url, handle) = provider(vec![
        json!({"character_core":{"visual_dna":{"hair_style":"black"}},"technical_specs":{},"stable_diffusion_tags":"portrait"}),
    ]);
    let engine = WorkflowEngine::new(root.path().to_path_buf(), settings(url));
    let mut req = input("visual_dna", "dna-run");
    req.context = json!({"item_id":7,"asset_file_path":"portrait.png"});
    let result = engine.execute(req).await.unwrap();
    assert_eq!(result.artifacts["visual_dna_source"], "filename_text_only");
    assert!(
        engine
            .store()
            .get_context("12", "visual_dna.asset_item_7", "v1")
            .unwrap()
            .is_some()
    );
    handle.join().unwrap();
}

#[tokio::test]
async fn script_workflow_runs_architect_writer_qc_and_uses_the_qc_revision() {
    let root = tempfile::tempdir().unwrap();
    let (url, handle) = provider(vec![
        json!({"series_bible":{"title":"茶馆"},"beat_sheet":[{"beat":"相认"}]}),
        json!({"script_fountain":"初稿"}),
        json!({"issues":[],"revised_script_fountain":"修订稿"}),
    ]);
    let engine = WorkflowEngine::new(root.path().to_path_buf(), settings(url));
    let result = engine
        .execute(input("workflow_script", "writing-run"))
        .await
        .unwrap();
    assert_eq!(result.artifacts["script_fountain"], "修订稿");
    assert!(result.artifacts["qc_report"].is_object());
    assert_eq!(
        engine
            .store()
            .get_context("12", "series_bible", "v1")
            .unwrap()
            .unwrap()["title"],
        "茶馆"
    );
    handle.join().unwrap();
}

#[tokio::test]
async fn memory_extraction_requires_human_review_and_returns_approved_payload() {
    let root = tempfile::tempdir().unwrap();
    let (url, handle) = provider(vec![
        json!({"schema_version":"changeset.v0","project_id":12,"entities":[],"evidences":[],"snapshots":[],"time_constraints":[],"time_blocks":[]}),
    ]);
    let engine = WorkflowEngine::new(root.path().to_path_buf(), settings(url));
    let result = engine
        .execute(input("memory_extract_changeset", "memory-run"))
        .await
        .unwrap();
    assert_eq!(result.waiting_for.as_deref(), Some("review_changeset"));
    let done = engine
        .resume("12", "memory-run", ReviewDecision::Approve, None)
        .await
        .unwrap();
    assert_eq!(
        done.artifacts["approved_changeset"]["schema_version"],
        "changeset.v0"
    );
    handle.join().unwrap();
}

#[tokio::test]
async fn storyboard_generates_controlled_shots_then_merges_the_prompt_pairs() {
    let root = tempfile::tempdir().unwrap();
    let (url, handle) = provider(vec![
        json!([{"title":"推门","action_text":"小明推门","shot_size":"MS","camera_angle":"EYE","lighting_style":"SOFT"}]),
        json!([{"prompt":"young man, teahouse","negative_prompt":"blurry"}]),
    ]);
    let engine = WorkflowEngine::new(root.path().to_path_buf(), settings(url));
    let result = engine
        .execute(input("workflow_storyboard", "storyboard-run"))
        .await
        .unwrap();
    assert_eq!(
        result.artifacts["shots"][0]["prompt"],
        "young man, teahouse"
    );
    assert_eq!(result.artifacts["shots"][0]["action_text"], "小明推门");
    handle.join().unwrap();
}

#[tokio::test]
async fn split_scenes_renders_template_variables_and_enforces_the_requested_limit() {
    let root = tempfile::tempdir().unwrap();
    let (url, handle) = provider(vec![
        json!([{"title":"一","description":"茶馆"},{"title":"二","description":"街道"}]),
    ]);
    let engine = WorkflowEngine::new(root.path().to_path_buf(), settings(url));
    let mut req = input("split_scenes", "scene-run");
    req.context = json!({"max_scenes":1});
    let result = engine.execute(req).await.unwrap();
    assert_eq!(result.artifacts["scenes"].as_array().unwrap().len(), 1);
    handle.join().unwrap();
}

#[tokio::test]
async fn failed_later_step_keeps_earlier_output_in_the_run_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let (url, handle) = provider(vec![
        json!({"logline":"已生成"}),
        json!("unparseable"),
        json!("still broken"),
    ]);
    let engine = WorkflowEngine::new(root.path().to_path_buf(), settings(url));
    assert!(
        engine
            .execute(input("episode_execute", "partial-run"))
            .await
            .is_err()
    );
    let snapshot = engine
        .store()
        .read_run("12", "partial-run")
        .unwrap()
        .unwrap();
    assert_eq!(
        snapshot["response"]["artifacts"]["outline"]["logline"],
        "已生成"
    );
    handle.join().unwrap();
}

#[tokio::test]
async fn old_python_episode_interrupt_can_resume_using_the_original_numeric_project_folder() {
    let root = tempfile::tempdir().unwrap();
    let store = ContextStore::new(root.path().to_path_buf());
    store
        .write_stage(
            "12",
            "legacy-run",
            "chat.status",
            &json!({"status":"paused","waiting_for":"confirm_split"}),
        )
        .unwrap();
    store.write_stage("12","legacy-run","chat.interrupt",&json!({"kind":"confirm_split","resume_state":{"project_id_pk":12,"project_id":"old-uuid","episode_id":8,"run_id":"legacy-run","script_text":"小明推门","step_index":3,"artifacts":{"outline":{},"assets_visual_dna":{"characters":[{"name":"小明"}],"props":[],"locations":[],"series_style":{}},"split_episodes":{"episodes":[{"title":"第一集","script":"小明推门"}]}}}})).unwrap();
    let engine = WorkflowEngine::new(root.path().to_path_buf(), AiSettings::default());
    let result = engine
        .resume("12", "legacy-run", ReviewDecision::Approve, None)
        .await
        .unwrap();
    assert_eq!(result.waiting_for.as_deref(), Some("confirm_ingest"));
    assert_eq!(
        result.artifacts["approved_split_episodes"]["episodes"][0]["title"],
        "第一集"
    );
}

#[tokio::test]
async fn script_workflow_can_derive_scenes_when_the_option_is_enabled() {
    let root = tempfile::tempdir().unwrap();
    let (url, handle) = provider(vec![
        json!({"series_bible":{},"beat_sheet":[]}),
        json!({"script_fountain":"INT. 茶馆 - 日"}),
        json!([{"title":"茶馆","description":"小明推门"}]),
    ]);
    let engine = WorkflowEngine::new(root.path().to_path_buf(), settings(url));
    let mut req = input("workflow_script", "derived-run");
    req.context = json!({"qc_loops":0,"derived_split_scenes":true,"max_scenes":1});
    let result = engine.execute(req).await.unwrap();
    assert_eq!(result.artifacts["derived"]["scenes"][0]["title"], "茶馆");
    handle.join().unwrap();
}

#[tokio::test]
async fn corrupted_review_artifacts_are_rejected_without_panicking_or_marking_done() {
    let root = tempfile::tempdir().unwrap();
    let store = ContextStore::new(root.path().to_path_buf());
    store
        .write_stage(
            "12",
            "corrupt-run",
            "chat.status",
            &json!({"status":"paused"}),
        )
        .unwrap();
    store.write_stage("12","corrupt-run","chat.interrupt",&json!({"kind":"confirm_ingest","resume_state":{"schema_version":1,"input":input("episode_execute","corrupt-run"),"next_step":1,"steps":[{"action_key":"episode_asset_ingest"}],"artifacts":[],"kind":"confirm_ingest"}})).unwrap();
    let engine = WorkflowEngine::new(root.path().to_path_buf(), AiSettings::default());
    assert!(
        engine
            .resume("12", "corrupt-run", ReviewDecision::Approve, None)
            .await
            .is_err()
    );
    assert_eq!(
        store
            .read_stage("12", "corrupt-run", "chat.status")
            .unwrap()
            .unwrap()["status"],
        "paused"
    );
}

#[tokio::test]
async fn a_new_non_review_run_reuses_creative_outputs_without_replaying_old_approvals() {
    let root = tempfile::tempdir().unwrap();
    let (url, handle) = provider(vec![
        json!({"logline":"旧Run大纲","characters":[]}),
        json!({"characters":[{"name":"小明"}],"props":[],"locations":[],"series_style":{}}),
        json!({"episodes":[{"title":"旧分集","script":"小明走进茶馆。"}]}),
        json!("小明推开茶馆木门。"),
    ]);
    let engine = WorkflowEngine::new(root.path().to_path_buf(), settings(url));
    engine
        .execute(input("episode_execute", "old-approved-run"))
        .await
        .unwrap();
    engine
        .resume("12", "old-approved-run", ReviewDecision::Approve, None)
        .await
        .unwrap();
    let completed = engine
        .resume("12", "old-approved-run", ReviewDecision::Approve, None)
        .await
        .unwrap();
    assert!(completed.artifacts["approved_split_episodes"].is_object());
    assert!(completed.artifacts["approved_asset_ingest_plan"].is_object());

    let mut previous = completed.artifacts.clone();
    previous["approved_changeset"] = json!({"schema_version":"changeset.v0"});
    previous["approved_future_operation"] = json!({"permission":"old-run-only"});
    previous["changeset_id"] = json!("old-change-id");
    previous["changeset"] = json!({"schema_version":"changeset.v0","project_id":12});
    previous["plan"] = json!({"steps":[{"action_key":"episode_asset_ingest"}]});
    previous["ingest_preview"] = json!({"create_count":1});
    previous["status"] = json!("done");
    previous["run_id"] = json!("old-approved-run");
    previous["_execution"] = json!({"committed":true});
    previous["custom_unknown_marker"] = json!(true);
    let mut next = input("script_optimize", "new-writing-run");
    next.context = json!({"artifacts":previous});
    let result = engine.execute(next).await.unwrap();

    assert_eq!(result.status, "done");
    assert_eq!(result.artifacts["script"], "小明推开茶馆木门。");
    assert_eq!(result.artifacts["outline"]["logline"], "旧Run大纲");
    assert_eq!(
        result.artifacts["split_episodes"]["episodes"][0]["title"],
        "旧分集"
    );
    assert!(
        !result
            .artifacts
            .as_object()
            .unwrap()
            .keys()
            .any(|key| key.starts_with("approved_")),
        "a new Run must never inherit permission to materialize old output"
    );
    for marker in [
        "asset_ingest_plan",
        "changeset_id",
        "changeset",
        "plan",
        "ingest_preview",
        "status",
        "run_id",
        "_execution",
        "custom_unknown_marker",
    ] {
        assert!(
            result.artifacts.get(marker).is_none(),
            "stale execution marker survived: {marker}"
        );
    }
    let new_snapshot = engine
        .store()
        .read_run("12", "new-writing-run")
        .unwrap()
        .unwrap();
    assert!(
        new_snapshot["request"]["context"]["artifacts"]
            .get("approved_split_episodes")
            .is_none()
    );
    let old_snapshot = engine
        .store()
        .read_run("12", "old-approved-run")
        .unwrap()
        .unwrap();
    assert!(
        old_snapshot["response"]["artifacts"]["approved_split_episodes"].is_object(),
        "starting a new Run must not erase the old Run's approval state"
    );
    handle.join().unwrap();
}
