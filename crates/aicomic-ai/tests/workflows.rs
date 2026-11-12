use aicomic_ai::{
    AiError, AiSettings, ContextStore, ExecutionInput, ReviewDecision, TaskContextProvider,
    WorkflowEngine,
};
use serde_json::{Value, json};
use std::{
    future::Future,
    io::{Read, Write},
    net::TcpListener,
    pin::Pin,
    sync::{Arc, Mutex},
    thread,
};

fn provider(replies: Vec<Value>) -> (String, thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
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
            let request = String::from_utf8(data).unwrap();
            let (_, body) = request.split_once("\r\n\r\n").unwrap();
            requests.push(serde_json::from_str(body).unwrap());
            let text = reply
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("```json\n{reply}\n```"));
            let body = json!({"choices":[{"message":{"content":text},"finish_reason":"stop"}]})
                .to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
        requests
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

#[derive(Debug)]
struct CurrentTaskContext {
    store: ContextStore,
    stages: Mutex<Vec<String>>,
}
impl TaskContextProvider for CurrentTaskContext {
    fn retrieve<'a>(
        &'a self,
        input: &'a ExecutionInput,
        stage: &'a str,
    ) -> Pin<Box<dyn Future<Output = aicomic_ai::Result<Value>> + Send + 'a>> {
        Box::pin(async move {
            tokio::task::yield_now().await;
            let bible = self
                .store
                .get_context(&input.project_id, "series_bible", "v1")?;
            let mut stages = self.stages.lock().unwrap();
            stages.push(stage.into());
            Ok(json!({"fresh_call":stages.len(),"current_bible":bible}))
        })
    }
}
fn request_context(request: &Value) -> Value {
    let user = request["messages"][1]["content"].as_str().unwrap();
    serde_json::from_str(
        user.rsplit_once("\n\n### 创作上下文（遵循已确认设定）\n")
            .unwrap()
            .1,
    )
    .unwrap()
}

#[tokio::test]
async fn task_context_is_fresh_for_every_script_stage_and_json_repair() {
    let root = tempfile::tempdir().unwrap();
    let (url, handle) = provider(vec![
        json!({"series_bible":{"title":"Architect新设定"},"beat_sheet":[]}),
        json!("broken writer output"),
        json!({"script_fountain":"新剧本"}),
        json!({"issues":[]}),
    ]);
    let engine = WorkflowEngine::new(root.path().into(), settings(url));
    let saved_bible = json!({"title":"磁盘当前设定"});
    let saved_outline = json!({"title":"磁盘当前项目大纲"});
    engine
        .store()
        .put_context("12", "series_bible", "v1", &saved_bible)
        .unwrap();
    engine
        .store()
        .put_context("12", "project_outline", "v1", &saved_outline)
        .unwrap();
    let fresh = Arc::new(CurrentTaskContext {
        store: engine.store().clone(),
        stages: Mutex::new(vec![]),
    });
    let engine = engine.with_task_context(fresh.clone());
    let mut request = input("workflow_script", "fresh-script-run");
    request.context = json!({
        "retrieved_memory":{"stale_memory":"不得沿用"},
        "series_bible":{"title":"输入旧Bible"},
        "project_outline":{"title":"输入旧项目大纲"},
        "current_outline":{"title":"输入旧编辑大纲"},
        "artifacts":{
            "series_bible":{"title":"产物旧Bible"},
            "project_outline":{"title":"产物旧项目大纲"},
            "script":"需保留的创作产物"
        },
        "unknown_extension":{"keep":true}
    });
    let original = serde_json::to_value(&request).unwrap();
    engine.execute(request).await.unwrap();
    let requests = handle.join().unwrap();
    assert_eq!(
        *fresh.stages.lock().unwrap(),
        ["architect", "writer", "writer.repair", "qc.1"]
    );
    for (index, request) in requests.iter().enumerate() {
        let context = request_context(request);
        let expected_bible = if index == 0 {
            saved_bible.clone()
        } else {
            json!({"title":"Architect新设定"})
        };
        let provided = &context["provided"];
        assert_eq!(
            provided["retrieved_memory"],
            json!({"fresh_call":index+1,"current_bible":expected_bible})
        );
        assert_eq!(context["series_bible"], expected_bible);
        assert_eq!(provided["series_bible"], expected_bible);
        assert_eq!(provided["artifacts"]["series_bible"], expected_bible);
        assert_eq!(context["project_outline"], saved_outline);
        assert_eq!(provided["project_outline"], saved_outline);
        assert_eq!(provided["current_outline"], saved_outline);
        assert_eq!(provided["artifacts"]["project_outline"], saved_outline);
        assert_eq!(provided["artifacts"]["script"], "需保留的创作产物");
        assert_eq!(provided["unknown_extension"], json!({"keep":true}));
        assert!(!context.to_string().contains("不得沿用"));
        let stage = ["architect", "writer", "writer.repair", "qc.1"][index];
        assert_eq!(
            engine
                .store()
                .read_stage("12", "fresh-script-run", &format!("{stage}.context"))
                .unwrap()
                .unwrap(),
            context
        );
    }
    assert_eq!(
        engine
            .store()
            .read_run("12", "fresh-script-run")
            .unwrap()
            .unwrap()["request"],
        original
    );
}

#[tokio::test]
async fn task_context_replaces_visual_dna_only_from_an_owned_current_source() {
    let root = tempfile::tempdir().unwrap();
    let cases = [
        json!({"item_id":7}),
        json!({"item_id":7,"version":"v2"}),
        json!({"item_id":8}),
        json!({}),
        json!({"item_id":0}),
        json!({"item_id":-1}),
        json!({"item_id":"7"}),
        json!({"item_id":7}), // No hook: preserve the legacy supplied copies.
    ];
    let (url, handle) = provider(vec![json!("生成结果"); cases.len()]);
    let engine = WorkflowEngine::new(root.path().into(), settings(url));
    let current = json!({"hair":"当前黑发"});
    let versioned = json!({"hair":"当前白发"});
    engine
        .store()
        .put_context("12", "visual_dna.asset_item_7", "v1", &current)
        .unwrap();
    engine
        .store()
        .put_context("12", "visual_dna.asset_item_7", "v2", &versioned)
        .unwrap();
    engine
        .store()
        .put_context(
            "13",
            "visual_dna.asset_item_8",
            "v1",
            &json!({"hair":"其它项目设定"}),
        )
        .unwrap();
    let refreshed = engine
        .clone()
        .with_task_context(Arc::new(CurrentTaskContext {
            store: engine.store().clone(),
            stages: Mutex::new(vec![]),
        }));
    for (index, mut context) in cases.into_iter().enumerate() {
        context["visual_dna"] = json!({"hair":"输入过期DNA"});
        context["artifacts"] = json!({"visual_dna":{"hair":"产物过期DNA"},"script":"保留创作产物"});
        context["locked_visual_dna"] = json!({"hair":"用户确认的冻结输入"});
        context["unknown_extension"] = json!({"keep":true});
        let mut request = input("script_optimize", &format!("fresh-dna-{index}"));
        request.context = context;
        let original = serde_json::to_value(&request).unwrap();
        let selected = if index == 7 { &engine } else { &refreshed };
        selected.execute(request).await.unwrap();
        assert_eq!(
            selected
                .store()
                .read_run("12", &format!("fresh-dna-{index}"))
                .unwrap()
                .unwrap()["request"],
            original
        );
    }
    for (index, request) in handle.join().unwrap().iter().enumerate() {
        let provided = &request_context(request)["provided"];
        match index {
            0 | 1 => {
                let expected = if index == 0 { &current } else { &versioned };
                assert_eq!(&provided["visual_dna"], expected);
                assert_eq!(&provided["artifacts"]["visual_dna"], expected);
            }
            7 => {
                assert_eq!(provided["visual_dna"]["hair"], "输入过期DNA");
                assert_eq!(provided["artifacts"]["visual_dna"]["hair"], "产物过期DNA");
            }
            _ => {
                assert!(provided.get("visual_dna").is_none());
                assert!(provided["artifacts"].get("visual_dna").is_none());
            }
        }
        assert_eq!(
            provided["locked_visual_dna"],
            json!({"hair":"用户确认的冻结输入"})
        );
        assert_eq!(provided["artifacts"]["script"], "保留创作产物");
        assert_eq!(provided["unknown_extension"], json!({"keep":true}));
    }
}

#[tokio::test]
async fn restarted_resume_refreshes_old_memory_and_audits_without_rewriting_input() {
    let root = tempfile::tempdir().unwrap();
    let changeset = json!({"schema_version":"changeset.v0","entities":[]});
    let (url, handle) = provider(vec![changeset.clone(), changeset]);
    let engine = WorkflowEngine::new(root.path().into(), settings(url.clone()));
    let mut request = input("memory_extract_changeset", "fresh-resume-run");
    request.context = json!({"retrieved_memory":{"old_memory":"保留于历史输入"},"series_bible":{"title":"过期输入设定"}});
    let original = serde_json::to_value(&request).unwrap();
    engine.execute(request).await.unwrap();
    assert!(
        engine
            .store()
            .read_stage("12", "fresh-resume-run", "changeset.extract.context")
            .unwrap()
            .is_none()
    );
    let request_path = engine
        .store()
        .run_dir("12", "fresh-resume-run")
        .unwrap()
        .join("request.json");
    let original_bytes = std::fs::read(&request_path).unwrap();
    engine
        .store()
        .put_context("12", "series_bible", "v1", &json!({"title":"重启前新设定"}))
        .unwrap();
    drop(engine);

    let restarted = WorkflowEngine::new(root.path().into(), settings(url));
    let fresh = Arc::new(CurrentTaskContext {
        store: restarted.store().clone(),
        stages: Mutex::new(vec![]),
    });
    let restarted = restarted.with_task_context(fresh.clone());
    let result = restarted
        .resume("12", "fresh-resume-run", ReviewDecision::Regenerate, None)
        .await
        .unwrap();
    assert_eq!(result.waiting_for.as_deref(), Some("review_changeset"));
    let requests = handle.join().unwrap();
    assert_eq!(
        request_context(&requests[0])["provided"]["retrieved_memory"],
        original["context"]["retrieved_memory"]
    );
    let context = request_context(&requests[1]);
    assert_eq!(*fresh.stages.lock().unwrap(), ["changeset.extract"]);
    assert_eq!(
        context["provided"]["retrieved_memory"],
        json!({"fresh_call":1,"current_bible":{"title":"重启前新设定"}})
    );
    assert!(!context.to_string().contains("保留于历史输入"));
    assert_eq!(
        context["provided"]["series_bible"],
        json!({"title":"重启前新设定"})
    );
    let audit = restarted
        .store()
        .read_stage("12", "fresh-resume-run", "changeset.extract.context")
        .unwrap()
        .unwrap();
    assert_eq!(audit["provided"], context["provided"]);
    assert_eq!(audit["series_bible"], context["series_bible"]);
    assert_eq!(std::fs::read(request_path).unwrap(), original_bytes);
    assert_eq!(
        restarted
            .store()
            .read_stage("12", "fresh-resume-run", "chat.interrupt")
            .unwrap()
            .unwrap()["resume_state"]["input"],
        original
    );
}

#[derive(Debug)]
struct UnavailableTaskContext {
    invalid: bool,
    stages: Mutex<Vec<String>>,
}
impl TaskContextProvider for UnavailableTaskContext {
    fn retrieve<'a>(
        &'a self,
        _input: &'a ExecutionInput,
        stage: &'a str,
    ) -> Pin<Box<dyn Future<Output = aicomic_ai::Result<Value>> + Send + 'a>> {
        Box::pin(async move {
            self.stages.lock().unwrap().push(stage.into());
            if self.invalid {
                Ok(json!([]))
            } else {
                Err(AiError::InvalidState(
                    "private adapter error mock-only".into(),
                ))
            }
        })
    }
}

#[tokio::test]
async fn unavailable_task_context_blocks_models_and_keeps_failed_runs_auditable() {
    let root = tempfile::tempdir().unwrap();
    let (url, handle) = provider(vec![json!({"schema_version":"changeset.v0","entities":[]})]);
    let original_engine = WorkflowEngine::new(root.path().into(), settings(url));
    let mut old_input = input("memory_extract_changeset", "context-failed-resume");
    old_input.context = json!({"retrieved_memory":{"stale_memory":"不得回退"}});
    original_engine.execute(old_input.clone()).await.unwrap();
    handle.join().unwrap();
    let request_path = original_engine
        .store()
        .run_dir("12", "context-failed-resume")
        .unwrap()
        .join("request.json");
    let original_bytes = std::fs::read(&request_path).unwrap();
    drop(original_engine);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    for invalid in [false, true] {
        let context_provider = Arc::new(UnavailableTaskContext {
            invalid,
            stages: Mutex::new(vec![]),
        });
        let engine = WorkflowEngine::new(
            root.path().into(),
            settings(format!("http://{}", listener.local_addr().unwrap())),
        )
        .with_task_context(context_provider.clone());
        let (run, stage, original, result) = if invalid {
            (
                "context-failed-resume",
                "changeset.extract",
                serde_json::to_value(&old_input).unwrap(),
                engine
                    .resume(
                        "12",
                        "context-failed-resume",
                        ReviewDecision::Regenerate,
                        None,
                    )
                    .await,
            )
        } else {
            let mut request = input("outline_generate", "context-failed-execute");
            request.context = old_input.context.clone();
            let original = serde_json::to_value(&request).unwrap();
            (
                "context-failed-execute",
                "outline_generate",
                original,
                engine.execute(request).await,
            )
        };
        let error = result.unwrap_err().to_string();
        assert!(error.contains("任务上下文"));
        assert!(!error.contains("private adapter error"));
        assert!(!error.contains("mock-only"));
        assert_eq!(*context_provider.stages.lock().unwrap(), [stage]);
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        let snapshot = engine.store().read_run("12", run).unwrap().unwrap();
        assert_eq!(snapshot["request"], original);
        assert_eq!(snapshot["response"]["status"], "error");
        assert!(!snapshot["response"].to_string().contains("mock-only"));
        let audit = engine
            .store()
            .read_stage("12", run, &format!("{stage}.context"))
            .unwrap()
            .unwrap();
        assert_eq!(audit["status"], "error");
        assert_eq!(
            audit["error"],
            if invalid {
                "task_context_invalid"
            } else {
                "task_context_unavailable"
            }
        );
        assert!(!audit.to_string().contains("不得回退"));
        assert!(
            !engine
                .store()
                .read_stage("12", run, "chat.error")
                .unwrap()
                .unwrap()
                .to_string()
                .contains("mock-only")
        );
    }
    assert_eq!(std::fs::read(request_path).unwrap(), original_bytes);
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
    let unavailable = Arc::new(UnavailableTaskContext {
        invalid: false,
        stages: Mutex::new(vec![]),
    });
    let engine = WorkflowEngine::new(root.path().to_path_buf(), settings(url))
        .with_task_context(unavailable.clone());
    assert!(engine.test_connection().await.unwrap().contains("成功"));
    assert!(unavailable.stages.lock().unwrap().is_empty());
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
async fn project_outline_optimization_uses_explicit_edits_instead_of_the_saved_outline() {
    let root = tempfile::tempdir().unwrap();
    let (url, handle) = provider(vec![
        json!("broken output"),
        json!({"project_outline":{"title":"优化后的大纲","episode_outlines":[]},"changes_summary":"保留本轮编辑"}),
    ]);
    let engine = WorkflowEngine::new(root.path().into(), settings(url));
    engine
        .store()
        .put_context(
            "12",
            "project_outline",
            "v1",
            &json!({"title":"磁盘上的旧大纲"}),
        )
        .unwrap();
    let edited =
        json!({"title":"表单中尚未保存的新大纲","episode_outlines":[{"summary":"新增悬念"}]});
    let mut request = input("project_outline_optimize", "edited-outline-run");
    request.context["current_outline"] = edited.clone();
    let result = engine.execute(request).await.unwrap();
    let requests = handle.join().unwrap();
    assert_eq!(requests.len(), 2);
    for (index, request) in requests.iter().enumerate() {
        let user_message = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "user")
            .unwrap()["content"]
            .as_str()
            .unwrap();
        if index == 0 {
            assert!(user_message.contains(&format!("当前项目大纲：{edited}")));
        }
        assert!(user_message.contains("表单中尚未保存的新大纲"));
        assert!(!user_message.contains("磁盘上的旧大纲"));
    }
    assert_eq!(result.artifacts["project_outline"]["title"], "优化后的大纲");
}

#[tokio::test]
async fn invalid_explicit_outline_is_rejected_without_falling_back_to_saved_content() {
    let root = tempfile::tempdir().unwrap();
    let engine = WorkflowEngine::new(root.path().into(), AiSettings::default());
    let saved = json!({"title":"应当保留的旧大纲"});
    engine
        .store()
        .put_context("12", "project_outline", "v1", &saved)
        .unwrap();
    for (index, invalid) in [Value::Null, json!("普通文本"), json!([]), json!(12)]
        .into_iter()
        .enumerate()
    {
        let mut request = input(
            "project_outline_optimize",
            &format!("invalid-outline-{index}"),
        );
        request.context["current_outline"] = invalid;
        assert!(matches!(
            engine.execute(request).await,
            Err(AiError::InvalidInput(_))
        ));
        assert_eq!(
            engine
                .store()
                .get_context("12", "project_outline", "v1")
                .unwrap(),
            Some(saved.clone())
        );
    }
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
