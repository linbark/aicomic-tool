use crate::{AiError, AiProvider, AiSettings, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}
#[derive(Clone, Debug, Default)]
pub struct LlmClient;
impl LlmClient {
    pub async fn chat(&self, settings: &AiSettings, messages: &[ChatMessage]) -> Result<String> {
        settings.validate()?;
        if settings.api_key.trim().is_empty() {
            return Err(AiError::MissingApiKey);
        }
        let url = endpoint(settings)?;
        let timeout = Duration::from_secs_f64(settings.timeout_seconds);
        let client = reqwest::Client::builder()
            .connect_timeout(timeout.min(Duration::from_secs(10)))
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| AiError::Network(e.to_string()))?;
        let payload = match settings.provider {
            AiProvider::OpenAi => {
                json!({"model":settings.model,"messages":messages,"temperature":settings.temperature,"max_tokens":settings.normalized_max_tokens()})
            }
            AiProvider::Google => google_payload(settings, messages)?,
        };
        for attempt in 0..3 {
            let result = async {
                let request = client
                    .post(url.clone())
                    .header("Accept", "application/json");
                let request = match settings.provider {
                    AiProvider::OpenAi => request.bearer_auth(&settings.api_key),
                    AiProvider::Google => request.header("x-goog-api-key", &settings.api_key),
                };
                let mut response = request.json(&payload).send().await?;
                let status = response.status().as_u16();
                let mut bytes = Vec::new();
                while let Some(chunk) = response.chunk().await? {
                    bytes.extend_from_slice(&chunk);
                    if bytes.len() > 16 * 1024 * 1024 {
                        break;
                    }
                }
                Ok::<_, reqwest::Error>((status, bytes))
            }
            .await;
            let (status, bytes) = match result {
                Ok(result) => result,
                Err(e) => {
                    if attempt < 2
                        && (e.is_timeout() || e.is_connect() || e.is_request() || e.is_body())
                    {
                        tokio::time::sleep(Duration::from_millis(350 * (1 << attempt))).await;
                        continue;
                    }
                    let message = e.to_string().replace(&settings.api_key, "***");
                    return Err(AiError::Network(format!("AI 请求失败：{message}")));
                }
            };
            if bytes.len() > 16 * 1024 * 1024 {
                return Err(AiError::Protocol("AI 响应超过 16 MiB 限制".into()));
            }
            if !(200..300).contains(&status) {
                let message: String = String::from_utf8_lossy(&bytes)
                    .replace(&settings.api_key, "***")
                    .chars()
                    .take(500)
                    .collect();
                return Err(AiError::Http { status, message });
            }
            let data: Value = serde_json::from_slice(&bytes)?;
            return match settings.provider {
                AiProvider::OpenAi => openai_content(&data),
                AiProvider::Google => google_content(&data),
            };
        }
        Err(AiError::Network("AI 重试次数已耗尽".into()))
    }
}

fn endpoint(settings: &AiSettings) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(settings.base_url.trim())
        .map_err(|_| AiError::InvalidInput("AI Base URL 无效".into()))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AiError::InvalidInput(
            "AI Base URL 必须为 HTTP/HTTPS，且不含登录凭证、查询参数或片段".into(),
        ));
    }
    let base = url.path().trim_end_matches('/');
    let path = match settings.provider {
        AiProvider::OpenAi if base.ends_with("/chat/completions") => base.to_owned(),
        AiProvider::OpenAi if base.ends_with("/v1") => format!("{base}/chat/completions"),
        AiProvider::OpenAi => format!("{base}/v1/chat/completions"),
        AiProvider::Google => {
            let model = settings.model.trim();
            let model = model.strip_prefix("models/").unwrap_or(model);
            if model.is_empty()
                || !model
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
                || matches!(model, "." | "..")
            {
                return Err(AiError::InvalidInput("Google AI 模型名称无效".into()));
            }
            let version = match base {
                "" => "/v1beta",
                "/v1" | "/v1beta" => base,
                _ => base
                    .split_once("/models/")
                    .filter(|(root, tail)| {
                        matches!(*root, "/v1" | "/v1beta")
                            && !tail.contains('/')
                            && tail.ends_with(":generateContent")
                    })
                    .map(|(root, _)| root)
                    .ok_or_else(|| {
                        AiError::InvalidInput(
                            "Google AI 地址应为 API 域名、/v1beta、/v1 或 generateContent 地址"
                                .into(),
                        )
                    })?,
            };
            format!("{version}/models/{model}:generateContent")
        }
    };
    url.set_path(&path);
    Ok(url)
}

fn google_payload(settings: &AiSettings, messages: &[ChatMessage]) -> Result<Value> {
    let mut instructions = Vec::new();
    let mut contents = Vec::new();
    for message in messages {
        let role = match message.role.as_str() {
            "system" | "developer" => {
                instructions.push(json!({"text": message.content}));
                continue;
            }
            "assistant" => "model",
            "user" => "user",
            _ => return Err(AiError::InvalidInput("Google AI 消息角色无效".into())),
        };
        contents.push(json!({"role": role, "parts": [{"text": message.content}]}));
    }
    if contents.is_empty() {
        return Err(AiError::InvalidInput("Google AI 请求缺少对话内容".into()));
    }
    let mut payload = json!({
        "contents": contents,
        "generationConfig": {
            "temperature": settings.temperature,
            "maxOutputTokens": settings.normalized_max_tokens()
        }
    });
    if !instructions.is_empty() {
        payload["systemInstruction"] = json!({"parts": instructions});
    }
    Ok(payload)
}

fn openai_content(data: &Value) -> Result<String> {
    let choice = data["choices"]
        .as_array()
        .and_then(|c| c.first())
        .ok_or_else(|| AiError::Protocol("AI 响应缺少 choices".into()))?;
    let content = choice["message"]["content"]
        .as_str()
        .ok_or_else(|| AiError::Protocol("AI 响应缺少文本 message.content".into()))?;
    let finish = choice["finish_reason"].as_str().unwrap_or("");
    checked_content(content, finish, matches!(finish, "length" | "max_tokens"))
}

fn google_content(data: &Value) -> Result<String> {
    if data["promptFeedback"]["blockReason"]
        .as_str()
        .is_some_and(|reason| !matches!(reason, "" | "BLOCK_REASON_UNSPECIFIED"))
    {
        return Err(AiError::Protocol("Google AI 拒绝生成此内容".into()));
    }
    let candidate = data["candidates"]
        .as_array()
        .and_then(|c| c.first())
        .ok_or_else(|| AiError::Protocol("Google AI 响应缺少 candidates".into()))?;
    let finish = candidate["finishReason"].as_str().unwrap_or("");
    if finish == "MAX_TOKENS" {
        return Err(AiError::Truncated {
            finish_reason: finish.into(),
        });
    }
    if finish != "STOP"
        || candidate["safetyRatings"]
            .as_array()
            .is_some_and(|ratings| {
                ratings
                    .iter()
                    .any(|rating| rating["blocked"].as_bool() == Some(true))
            })
    {
        return Err(AiError::Protocol(
            "Google AI 未正常完成生成，内容可能被拦截".into(),
        ));
    }
    let content: String = candidate["content"]["parts"]
        .as_array()
        .ok_or_else(|| AiError::Protocol("Google AI 响应缺少文本 parts".into()))?
        .iter()
        .filter(|part| part["thought"].as_bool() != Some(true))
        .filter_map(|part| part["text"].as_str())
        .collect();
    checked_content(&content, finish, false)
}

fn checked_content(content: &str, finish: &str, truncated: bool) -> Result<String> {
    let content = content.trim();
    let stripped = content
        .strip_prefix("```json")
        .or_else(|| content.strip_prefix("```JSON"))
        .unwrap_or(content)
        .trim()
        .trim_end_matches("```")
        .trim();
    let incomplete = (stripped.starts_with('{') && !stripped.ends_with('}'))
        || (stripped.starts_with('[') && !stripped.ends_with(']'));
    if truncated || incomplete {
        return Err(AiError::Truncated {
            finish_reason: if finish.is_empty() {
                "incomplete_json".into()
            } else {
                finish.into()
            },
        });
    }
    if content.is_empty() {
        return Err(AiError::Protocol("AI 返回空文本".into()));
    }
    Ok(content.to_owned())
}

pub fn extract_json(text: &str) -> Result<Value> {
    let text = text.trim();
    if let Ok(value) = serde_json::from_str(text) {
        return Ok(value);
    }
    // Parse the first complete object/array, preserving braces and escapes in JSON strings.
    for (start, _) in text.char_indices().filter(|(_, c)| matches!(c, '{' | '[')) {
        let mut stream = serde_json::Deserializer::from_str(&text[start..]).into_iter::<Value>();
        if let Some(Ok(value)) = stream.next() {
            return Ok(value);
        }
    }
    Err(AiError::Protocol("AI 返回无法解析为 JSON".into()))
}
