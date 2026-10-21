use aicomic_ai::{
    AiError, AiProvider, AiSettings, ChatMessage, ExecutionInput, LlmClient, WorkflowEngine,
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
};

fn server(
    responses: Vec<Option<(u16, Value)>>,
) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = vec![];
            let mut buffer = [0; 4096];
            loop {
                let n = stream.read(&mut buffer).unwrap();
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..n]);
                let text = String::from_utf8_lossy(&bytes);
                if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                    let len = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|s| s.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if body.len() >= len {
                        break;
                    }
                }
            }
            tx.send(String::from_utf8(bytes).unwrap()).unwrap();
            if let Some((status, value)) = response {
                let body = value.to_string();
                write!(stream,"HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        }
    });
    (url, rx, handle)
}
fn messages() -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: "user".into(),
        content: "你好".into(),
    }]
}
fn completion(text: &str, finish: &str) -> Value {
    json!({"choices":[{"message":{"content":text},"finish_reason":finish}]})
}

#[tokio::test]
async fn provider_retries_a_disconnected_request_and_keeps_v1_endpoint_and_token_bound() {
    let (url, rx, handle) = server(vec![None, Some((200, completion("完成", "stop")))]);
    let settings = AiSettings {
        api_key: "fake-test-key".into(),
        base_url: format!("{url}/v1"),
        max_tokens: Some(99999),
        ..Default::default()
    };
    let result = LlmClient.chat(&settings, &messages()).await.unwrap();
    assert_eq!(result, "完成");
    handle.join().unwrap();
    let requests: Vec<_> = rx.try_iter().collect();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("POST /v1/chat/completions HTTP/1.1"));
    let body: Value = serde_json::from_str(requests[1].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body["max_tokens"], 8192);
    assert_eq!(body["messages"][0]["content"], "你好");
}

#[tokio::test]
async fn provider_reports_output_token_truncation_instead_of_accepting_partial_json() {
    let (url, _rx, handle) = server(vec![Some((200, completion("{\"partial\":", "length")))]);
    let settings = AiSettings {
        api_key: "fake".into(),
        base_url: url,
        ..Default::default()
    };
    assert!(matches!(
        LlmClient.chat(&settings, &messages()).await,
        Err(AiError::Truncated { .. })
    ));
    handle.join().unwrap();
}

#[tokio::test]
async fn provider_rejects_auth_error_without_retrying_and_does_not_echo_api_key() {
    let (url, _rx, handle) = server(vec![Some((401, json!({"error":"fake-test-key denied"})))]);
    let settings = AiSettings {
        api_key: "fake-test-key".into(),
        base_url: url,
        ..Default::default()
    };
    let error = LlmClient.chat(&settings, &messages()).await.unwrap_err();
    assert!(matches!(error, AiError::Http { status: 401, .. }));
    assert!(!error.to_string().contains("fake-test-key"));
    handle.join().unwrap();
}

#[tokio::test]
async fn retrieved_negative_constraints_reach_architect_writer_and_qc_requests() {
    let replies = [
        json!({"series_bible":{},"beat_sheet":[]}),
        json!({"script_fountain":"小明走进茶馆。"}),
        json!({"issues":[]}),
    ];
    let (url, rx, handle) = server(
        replies
            .into_iter()
            .map(|v| Some((200, completion(&v.to_string(), "stop"))))
            .collect(),
    );
    let root = tempfile::tempdir().unwrap();
    let engine = WorkflowEngine::new(
        root.path().to_path_buf(),
        AiSettings {
            api_key: "fake-key".into(),
            base_url: url,
            ..Default::default()
        },
    );
    engine
        .execute(ExecutionInput {
            project_id: "12".into(),
            episode_id: Some(1),
            script: "写茶馆重逢故事".into(),
            context: json!({"retrieved_memory":{"negative_constraints":["不允许现代枪械"]}}),
            action: "workflow_script".into(),
            run_id: "memory-context-run".into(),
        })
        .await
        .unwrap();
    handle.join().unwrap();
    let requests: Vec<_> = rx.try_iter().collect();
    assert_eq!(requests.len(), 3);
    for (step, request) in requests.iter().enumerate() {
        let payload: Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert!(
            payload["messages"][1]["content"]
                .as_str()
                .unwrap()
                .contains("不允许现代枪械"),
            "retrieved constraints missing from model step {step}"
        );
    }
}

fn google_settings(base_url: String) -> AiSettings {
    AiSettings {
        provider: AiProvider::Google,
        api_key: "fake-google-key".into(),
        base_url,
        model: "gemini-flash-latest".into(),
        ..Default::default()
    }
}

fn google_completion(text: &str, finish: &str) -> Value {
    json!({"candidates":[{"content":{"role":"model","parts":[{"text":text}]},"finishReason":finish}]})
}

#[tokio::test]
async fn google_uses_native_auth_payload_roles_and_generation_config() {
    let reply = json!({"candidates":[{
        "content":{"parts":[
            {"text":"private reasoning", "thought":true},
            {"text":" {\"script\":"},
            {"text":"\"完成\"} "}
        ]},
        "finishReason":"STOP"
    }]});
    let (url, rx, handle) = server(vec![Some((200, reply))]);
    let mut settings = google_settings(format!("{url}/v1beta"));
    settings.temperature = 0.7;
    settings.max_tokens = Some(4096);
    let messages: Vec<_> = [
        ("system", "创作总则"),
        ("developer", "输出约束"),
        ("user", "第一轮"),
        ("assistant", "上一轮回复"),
        ("user", "继续写"),
    ]
    .into_iter()
    .map(|(role, content)| ChatMessage {
        role: role.into(),
        content: content.into(),
    })
    .collect();
    assert_eq!(
        LlmClient.chat(&settings, &messages).await.unwrap(),
        "{\"script\":\"完成\"}"
    );
    handle.join().unwrap();
    let request = rx.recv().unwrap();
    let (headers, body) = request.split_once("\r\n\r\n").unwrap();
    assert!(
        headers.starts_with("POST /v1beta/models/gemini-flash-latest:generateContent HTTP/1.1")
    );
    let headers = headers.to_ascii_lowercase();
    assert!(headers.contains("x-goog-api-key: fake-google-key"));
    assert!(headers.contains("content-type: application/json"));
    assert!(!headers.contains("authorization:"));
    assert!(!headers.lines().next().unwrap().contains("fake-google-key"));
    let payload: Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        payload["systemInstruction"],
        json!({"parts":[{"text":"创作总则"},{"text":"输出约束"}]})
    );
    assert_eq!(
        payload["contents"],
        json!([
            {"role":"user","parts":[{"text":"第一轮"}]},
            {"role":"model","parts":[{"text":"上一轮回复"}]},
            {"role":"user","parts":[{"text":"继续写"}]}
        ])
    );
    assert_eq!(payload["generationConfig"]["temperature"], 0.7);
    assert_eq!(payload["generationConfig"]["maxOutputTokens"], 4096);
    assert!(payload.get("model").is_none());
    assert!(payload.get("messages").is_none());
    assert!(!body.contains("fake-google-key"));
}

#[tokio::test]
async fn google_accepts_api_roots_and_rebuilds_full_endpoint_from_selected_model() {
    for (suffix, version) in [
        ("", "v1beta"),
        ("/", "v1beta"),
        ("/v1beta/", "v1beta"),
        ("/v1", "v1"),
        ("/v1beta/models/old-model:generateContent", "v1beta"),
        ("/v1/models/old-model:generateContent", "v1"),
    ] {
        let (url, rx, handle) = server(vec![Some((200, google_completion("完成", "STOP")))]);
        let mut settings = google_settings(format!("{url}{suffix}"));
        settings.model = "models/gemini-flash-latest".into();
        assert_eq!(
            LlmClient.chat(&settings, &messages()).await.unwrap(),
            "完成"
        );
        handle.join().unwrap();
        let request = rx.recv().unwrap();
        assert!(request.starts_with(&format!(
            "POST /{version}/models/gemini-flash-latest:generateContent HTTP/1.1"
        )));
        let payload: Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert!(payload.get("systemInstruction").is_none());
    }
}

#[tokio::test]
async fn google_retries_disconnections_and_preserves_token_limit() {
    let (url, rx, handle) = server(vec![None, Some((200, google_completion("完成", "STOP")))]);
    let mut settings = google_settings(url);
    settings.max_tokens = Some(99999);
    assert_eq!(
        LlmClient.chat(&settings, &messages()).await.unwrap(),
        "完成"
    );
    handle.join().unwrap();
    let requests: Vec<_> = rx.try_iter().collect();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    let payload: Value =
        serde_json::from_str(requests[1].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(payload["generationConfig"]["maxOutputTokens"], 8192);
}

#[tokio::test]
async fn google_token_limit_with_or_without_text_is_truncation() {
    for reply in [
        google_completion("{\"partial\":", "MAX_TOKENS"),
        json!({"candidates":[{"finishReason":"MAX_TOKENS"}]}),
    ] {
        let (url, _rx, handle) = server(vec![Some((200, reply))]);
        let error = LlmClient
            .chat(&google_settings(url), &messages())
            .await
            .unwrap_err();
        assert!(
            matches!(error, AiError::Truncated { finish_reason } if finish_reason == "MAX_TOKENS")
        );
        handle.join().unwrap();
    }
}

#[tokio::test]
async fn google_rejects_blocked_empty_and_abnormal_responses() {
    for reply in [
        json!({"promptFeedback":{"blockReason":"SAFETY"}}),
        json!({"candidates":[]}),
        google_completion("必须拒绝的部分输出", "SAFETY"),
        google_completion("部分输出", "RECITATION"),
        google_completion("部分输出", "FINISH_REASON_UNSPECIFIED"),
        google_completion("部分输出", ""),
        google_completion("部分输出", "UNKNOWN_FUTURE_REASON"),
        google_completion("", "STOP"),
        json!({"candidates":[{"finishReason":"STOP"}]}),
        json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":"推理","thought":true}]}}]}),
        json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":"拦截内容"}]},"safetyRatings":[{"blocked":true}]}]}),
    ] {
        let (url, _rx, handle) = server(vec![Some((200, reply))]);
        assert!(matches!(
            LlmClient.chat(&google_settings(url), &messages()).await,
            Err(AiError::Protocol(_))
        ));
        handle.join().unwrap();
    }
}

#[tokio::test]
async fn google_http_error_redacts_reflected_key_without_retrying() {
    let (url, rx, handle) = server(vec![Some((
        403,
        json!({"error":{"message":"fake-google-key denied"}}),
    ))]);
    let error = LlmClient
        .chat(&google_settings(url), &messages())
        .await
        .unwrap_err();
    assert!(matches!(error, AiError::Http { status: 403, .. }));
    assert!(!error.to_string().contains("fake-google-key"));
    assert!(error.to_string().contains("***"));
    handle.join().unwrap();
    assert_eq!(rx.try_iter().count(), 1);
}

#[tokio::test]
async fn google_rejects_ambiguous_urls_models_and_message_roles_before_network() {
    for url in [
        "https://fake-google-key@example.invalid/v1beta",
        "https://example.invalid/v1beta?key=fake-google-key",
        "https://example.invalid/v1beta#fake-google-key",
        "ftp://example.invalid/v1beta",
        "https://example.invalid/v1beta/chat/completions",
    ] {
        let error = LlmClient
            .chat(&google_settings(url.into()), &messages())
            .await
            .unwrap_err();
        assert!(matches!(error, AiError::InvalidInput(_)));
        assert!(!error.to_string().contains("fake-google-key"));
    }
    for model in [
        "",
        "models/",
        "../other",
        "gemini?key=fake-google-key",
        "..",
    ] {
        let mut settings = google_settings("https://example.invalid/v1beta".into());
        settings.model = model.into();
        assert!(matches!(
            LlmClient.chat(&settings, &messages()).await,
            Err(AiError::InvalidInput(_))
        ));
    }
    for messages in [
        vec![],
        vec![ChatMessage {
            role: "system".into(),
            content: "instruction without contents".into(),
        }],
        vec![ChatMessage {
            role: "tool".into(),
            content: "unsupported role".into(),
        }],
    ] {
        assert!(matches!(
            LlmClient
                .chat(
                    &google_settings("https://example.invalid/v1beta".into()),
                    &messages
                )
                .await,
            Err(AiError::InvalidInput(_))
        ));
    }
}

#[tokio::test]
async fn google_workflow_and_connection_test_use_native_requests() {
    let replies = [
        "OK".to_owned(),
        json!({"series_bible":{},"beat_sheet":[]}).to_string(),
        json!({"script_fountain":"小明走进茶馆。"}).to_string(),
        json!({"issues":[]}).to_string(),
    ];
    let (url, rx, handle) = server(
        replies
            .into_iter()
            .map(|v| Some((200, google_completion(&v, "STOP"))))
            .collect(),
    );
    let root = tempfile::tempdir().unwrap();
    let engine = WorkflowEngine::new(root.path().to_path_buf(), google_settings(url));
    assert_eq!(engine.test_connection().await.unwrap(), "连接成功");
    let result = engine
        .execute(ExecutionInput {
            project_id: "12".into(),
            episode_id: Some(1),
            script: "写茶馆重逢故事".into(),
            context: json!({"retrieved_memory":{"negative_constraints":["不允许现代枪械"]}}),
            action: "workflow_script".into(),
            run_id: "google-workflow-run".into(),
        })
        .await
        .unwrap();
    assert_eq!(result.artifacts["script_fountain"], "小明走进茶馆。");
    handle.join().unwrap();
    let requests: Vec<_> = rx.try_iter().collect();
    assert_eq!(requests.len(), 4);
    for (step, request) in requests.iter().enumerate() {
        assert!(
            request.starts_with("POST /v1beta/models/gemini-flash-latest:generateContent HTTP/1.1")
        );
        let payload: Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert!(payload["systemInstruction"]["parts"][0]["text"].is_string());
        if step == 0 {
            assert_eq!(payload["generationConfig"]["maxOutputTokens"], 1024);
        } else {
            assert!(
                payload["contents"][0]["parts"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("不允许现代枪械"),
                "retrieved constraints missing from Google step {step}"
            );
        }
    }
}
