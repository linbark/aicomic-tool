use aicomic_ai::{AiError, AiProvider, AiSettings, LlmClient, ModelInfo};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
};

enum Reply {
    Json(u16, Value),
    Raw(u16, String),
    Disconnect,
    Redirect(String),
}

fn server(replies: Vec<Reply>) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        for reply in replies {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let n = stream.read(&mut buffer).unwrap();
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..n]);
                if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let _ = tx.send(String::from_utf8(bytes).unwrap());
            let (status, body, extra) = match reply {
                Reply::Disconnect => continue,
                Reply::Json(status, value) => (status, value.to_string(), String::new()),
                Reply::Raw(status, value) => (status, value, String::new()),
                Reply::Redirect(location) => {
                    (302, String::new(), format!("Location: {location}\r\n"))
                }
            };
            let header = format!(
                "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(body.as_bytes());
        }
    });
    (url, rx, handle)
}

fn settings(provider: AiProvider, base_url: String) -> AiSettings {
    AiSettings {
        provider,
        base_url,
        api_key: "catalog-fake-key".into(),
        model: String::new(),
        ..Default::default()
    }
}

#[tokio::test]
async fn google_lists_all_pages_sorted_and_deduplicated_with_capabilities_and_blank_selection() {
    let (url, requests, handle) = server(vec![
        Reply::Json(
            200,
            json!({
                "models":[
                    {"name":"models/gemini-z", "displayName":"Z model", "supportedGenerationMethods":["generateContent"]},
                    {"name":"models/embedding-a", "supportedGenerationMethods":["embedContent"]}
                ],
                "nextPageToken":"page two/+?"
            }),
        ),
        Reply::Json(
            200,
            json!({"models":[
                {"name":"models/gemini-z", "supportedGenerationMethods":["countTokens"]},
                {"name":"models/gemini-b", "displayName":"B model", "supportedGenerationMethods":["generateContent"]}
            ]}),
        ),
    ]);
    let models = LlmClient
        .list_models(&settings(AiProvider::Google, url))
        .await
        .unwrap();
    handle.join().unwrap();
    assert_eq!(
        models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        ["embedding-a", "gemini-b", "gemini-z"]
    );
    assert!(!models[0].supports_chat(AiProvider::Google));
    assert!(models[1].supports_chat(AiProvider::Google));
    assert_eq!(models[2].display_name, "Z model");
    assert_eq!(
        models[2].supported_generation_methods,
        ["generateContent", "countTokens"]
    );
    let requests: Vec<_> = requests.try_iter().collect();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("GET /v1beta/models?pageSize=1000 HTTP/1.1"));
    assert!(
        requests[1]
            .starts_with("GET /v1beta/models?pageSize=1000&pageToken=page+two%2F%2B%3F HTTP/1.1")
    );
    for request in requests {
        let headers = request.to_ascii_lowercase();
        assert!(headers.contains("x-goog-api-key: catalog-fake-key"));
        assert!(!headers.contains("authorization:"));
        assert!(!request.lines().next().unwrap().contains("catalog-fake-key"));
    }
}

#[tokio::test]
async fn google_model_list_accepts_origins_versions_and_full_generation_endpoints() {
    for (suffix, version) in [
        ("", "v1beta"),
        ("/", "v1beta"),
        ("/v1beta/", "v1beta"),
        ("/v1", "v1"),
        ("/v1beta/models/old:generateContent", "v1beta"),
        ("/v1/models/old:generateContent", "v1"),
    ] {
        let (url, requests, handle) = server(vec![Reply::Json(200, json!({"models":[]}))]);
        assert!(
            LlmClient
                .list_models(&settings(AiProvider::Google, format!("{url}{suffix}")))
                .await
                .unwrap()
                .is_empty()
        );
        handle.join().unwrap();
        assert!(
            requests
                .recv()
                .unwrap()
                .starts_with(&format!("GET /{version}/models?pageSize=1000 HTTP/1.1"))
        );
    }
}

#[tokio::test]
async fn openai_lists_with_bearer_auth_and_normalizes_endpoint_forms() {
    for (suffix, expected) in [
        ("", "/v1/models"),
        ("/v1/", "/v1/models"),
        ("/v1/chat/completions", "/v1/models"),
        ("/gateway/v1/chat/completions", "/gateway/v1/models"),
        ("/chat/completions", "/models"),
    ] {
        let (url, requests, handle) = server(vec![Reply::Json(
            200,
            json!({"data":[{"id":"z"},{"id":"a"},{"id":"a"}]}),
        )]);
        let models = LlmClient
            .list_models(&settings(AiProvider::OpenAi, format!("{url}{suffix}")))
            .await
            .unwrap();
        handle.join().unwrap();
        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "z"]
        );
        assert!(
            models
                .iter()
                .all(|model| model.supports_chat(AiProvider::OpenAi))
        );
        let request = requests.recv().unwrap();
        assert!(request.starts_with(&format!("GET {expected} HTTP/1.1")));
        let headers = request.to_ascii_lowercase();
        assert!(headers.contains("authorization: bearer catalog-fake-key"));
        assert!(!headers.contains("x-goog-api-key:"));
    }
}

#[tokio::test]
async fn openai_compatible_cursor_pagination_keeps_every_model() {
    let (url, requests, handle) = server(vec![
        Reply::Json(
            200,
            json!({"data":[{"id":"z"}],"has_more":true,"last_id":"cursor/+"}),
        ),
        Reply::Json(200, json!({"data":[{"id":"a"}],"has_more":true})),
        Reply::Json(
            200,
            json!({"data":[{"id":"b"}],"has_more":false,"last_id":null}),
        ),
    ]);
    let models = LlmClient
        .list_models(&settings(AiProvider::OpenAi, url))
        .await
        .unwrap();
    handle.join().unwrap();
    assert_eq!(models.len(), 3);
    let requests: Vec<_> = requests.try_iter().collect();
    assert!(requests[1].starts_with("GET /v1/models?after=cursor%2F%2B HTTP/1.1"));
    assert!(requests[2].starts_with("GET /v1/models?after=a HTTP/1.1"));
}

#[tokio::test]
async fn list_models_retries_a_disconnect_but_not_http_failures_and_redacts_keys() {
    let (url, requests, handle) = server(vec![
        Reply::Disconnect,
        Reply::Json(200, json!({"models":[]})),
    ]);
    LlmClient
        .list_models(&settings(AiProvider::Google, url))
        .await
        .unwrap();
    handle.join().unwrap();
    assert_eq!(requests.try_iter().count(), 2);
    let (url, requests, handle) = server(vec![
        Reply::Json(
            200,
            json!({"models":[{"name":"models/partial"}],"nextPageToken":"second"}),
        ),
        Reply::Json(403, json!({"error":"catalog-fake-key denied"})),
    ]);
    let error = LlmClient
        .list_models(&settings(AiProvider::Google, url))
        .await
        .unwrap_err();
    assert!(matches!(error, AiError::Http { status: 403, .. }));
    assert!(!error.to_string().contains("catalog-fake-key"));
    assert!(error.to_string().contains("***"));
    handle.join().unwrap();
    assert_eq!(requests.try_iter().count(), 2);
}

#[tokio::test]
async fn model_list_rejects_repeated_cursors_instead_of_returning_partial_success() {
    for provider in AiProvider::ALL {
        let reply = match provider {
            AiProvider::Google => json!({"models":[],"nextPageToken":"repeat"}),
            AiProvider::OpenAi => json!({"data":[{"id":"a"}],"has_more":true,"last_id":"repeat"}),
        };
        let (url, requests, handle) = server(vec![
            Reply::Json(200, reply.clone()),
            Reply::Json(200, reply),
        ]);
        let error = LlmClient
            .list_models(&settings(provider, url))
            .await
            .unwrap_err();
        assert!(matches!(error, AiError::Protocol(_)));
        assert!(error.to_string().contains("游标重复"));
        handle.join().unwrap();
        assert_eq!(requests.try_iter().count(), 2);
    }
}

#[tokio::test]
async fn model_list_bounds_the_number_of_pages_without_returning_partial_success() {
    let replies = (0..100)
        .map(|page| {
            Reply::Json(
                200,
                json!({"models":[],"nextPageToken":format!("page-{page}")}),
            )
        })
        .collect();
    let (url, requests, handle) = server(replies);
    let error = LlmClient
        .list_models(&settings(AiProvider::Google, url))
        .await
        .unwrap_err();
    assert!(matches!(error, AiError::Protocol(_)));
    assert!(error.to_string().contains("100 页"));
    handle.join().unwrap();
    assert_eq!(requests.try_iter().count(), 100);
}

#[tokio::test]
async fn malformed_catalogs_and_entries_are_protocol_errors() {
    for (provider, reply) in [
        (AiProvider::Google, json!({})),
        (AiProvider::Google, json!({"models":{}})),
        (AiProvider::Google, json!({"models":[{}]})),
        (AiProvider::Google, json!({"models":[{"name":"models/"}]})),
        (
            AiProvider::Google,
            json!({"models":[{"name":"models/a b"}]}),
        ),
        (
            AiProvider::Google,
            json!({"models":[{"name":"models/a","displayName":false}]}),
        ),
        (
            AiProvider::Google,
            json!({"models":[{"name":"models/a","supportedGenerationMethods":"generateContent"}]}),
        ),
        (
            AiProvider::Google,
            json!({"models":[{"name":"models/a","supportedGenerationMethods":[false]}]}),
        ),
        (
            AiProvider::Google,
            json!({"models":[],"nextPageToken":false}),
        ),
        (AiProvider::OpenAi, json!({"data":[{"id":null}]})),
        (AiProvider::OpenAi, json!({"data":[],"has_more":true})),
        (AiProvider::OpenAi, json!({"data":[],"has_more":"yes"})),
        (
            AiProvider::OpenAi,
            json!({"data":[{"id":"a"}],"has_more":true,"last_id":false}),
        ),
    ] {
        let (url, _, handle) = server(vec![Reply::Json(200, reply)]);
        assert!(matches!(
            LlmClient.list_models(&settings(provider, url)).await,
            Err(AiError::Protocol(_))
        ));
        handle.join().unwrap();
    }
}

#[tokio::test]
async fn model_list_rejects_unsafe_base_urls_and_missing_keys_before_network() {
    for provider in AiProvider::ALL {
        for url in [
            "https://catalog-fake-key@example.invalid/v1beta",
            "https://example.invalid/v1beta?key=catalog-fake-key",
            "https://example.invalid/v1beta#catalog-fake-key",
            "ftp://example.invalid/v1beta",
        ] {
            let error = LlmClient
                .list_models(&settings(provider, url.into()))
                .await
                .unwrap_err();
            assert!(matches!(error, AiError::InvalidInput(_)));
            assert!(!error.to_string().contains("catalog-fake-key"));
        }
        let mut config = settings(provider, "https://example.invalid".into());
        config.api_key = " ".into();
        assert!(matches!(
            LlmClient.list_models(&config).await,
            Err(AiError::MissingApiKey)
        ));
        config.api_key = "fake".into();
        config.timeout_seconds = f64::INFINITY;
        assert!(matches!(
            LlmClient.list_models(&config).await,
            Err(AiError::InvalidInput(_))
        ));
    }
}

#[tokio::test]
async fn model_listing_does_not_follow_redirects_or_accept_oversized_bodies() {
    let (url, requests, handle) = server(vec![Reply::Redirect(
        "http://example.invalid/key-leak".into(),
    )]);
    assert!(matches!(
        LlmClient
            .list_models(&settings(AiProvider::Google, url))
            .await,
        Err(AiError::Http { status: 302, .. })
    ));
    handle.join().unwrap();
    assert_eq!(requests.try_iter().count(), 1);
    let (url, _, handle) = server(vec![Reply::Raw(200, "x".repeat(16 * 1024 * 1024 + 1))]);
    let error = LlmClient
        .list_models(&settings(AiProvider::Google, url))
        .await
        .unwrap_err();
    assert!(matches!(error, AiError::Protocol(_)));
    assert!(error.to_string().contains("16 MiB"));
    handle.join().unwrap();
}

#[test]
fn model_labels_use_ids_and_optional_names_and_keep_capability_metadata() {
    let model = ModelInfo {
        id: "gemini-test".into(),
        display_name: "Gemini Test".into(),
        supported_generation_methods: vec!["embedContent".into()],
    };
    assert_eq!(model.to_string(), "gemini-test · Gemini Test");
    assert!(!model.supports_chat(AiProvider::Google));
    assert!(model.supports_chat(AiProvider::OpenAi));
    assert_eq!(
        ModelInfo {
            display_name: String::new(),
            ..model.clone()
        }
        .to_string(),
        "gemini-test"
    );
    assert_eq!(
        ModelInfo {
            display_name: model.id.clone(),
            ..model
        }
        .to_string(),
        "gemini-test"
    );
}
