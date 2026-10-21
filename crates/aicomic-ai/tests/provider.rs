use aicomic_ai::{AiError, AiSettings, ChatMessage, ExecutionInput, LlmClient, WorkflowEngine};
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
