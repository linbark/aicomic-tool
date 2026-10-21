use crate::{AiError, AiSettings, Result};
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
        let base = settings.base_url.trim_end_matches('/');
        let endpoint = if base.ends_with("/chat/completions") {
            base.to_owned()
        } else if base.ends_with("/v1") {
            format!("{base}/chat/completions")
        } else {
            format!("{base}/v1/chat/completions")
        };
        let url = reqwest::Url::parse(&endpoint)
            .map_err(|_| AiError::InvalidInput("AI Base URL 无效".into()))?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(AiError::InvalidInput(
                "AI Base URL 必须为 HTTP/HTTPS 且不含登录凭证".into(),
            ));
        }
        let timeout = Duration::from_secs_f64(settings.timeout_seconds);
        let client = reqwest::Client::builder()
            .connect_timeout(timeout.min(Duration::from_secs(10)))
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| AiError::Network(e.to_string()))?;
        let payload = json!({"model":settings.model,"messages":messages,"temperature":settings.temperature,"max_tokens":settings.normalized_max_tokens()});
        for attempt in 0..3 {
            let result = async {
                let mut response = client
                    .post(url.clone())
                    .bearer_auth(&settings.api_key)
                    .header("Accept", "application/json")
                    .json(&payload)
                    .send()
                    .await?;
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
            let choice = data["choices"]
                .as_array()
                .and_then(|c| c.first())
                .ok_or_else(|| AiError::Protocol("AI 响应缺少 choices".into()))?;
            let content = choice["message"]["content"]
                .as_str()
                .ok_or_else(|| AiError::Protocol("AI 响应缺少文本 message.content".into()))?
                .trim();
            let finish = choice["finish_reason"].as_str().unwrap_or("");
            let stripped = content
                .strip_prefix("```json")
                .or_else(|| content.strip_prefix("```JSON"))
                .unwrap_or(content)
                .trim()
                .trim_end_matches("```")
                .trim();
            let incomplete = (stripped.starts_with('{') && !stripped.ends_with('}'))
                || (stripped.starts_with('[') && !stripped.ends_with(']'));
            if matches!(finish, "length" | "max_tokens") || incomplete {
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
            return Ok(content.to_owned());
        }
        Err(AiError::Network("AI 重试次数已耗尽".into()))
    }
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
