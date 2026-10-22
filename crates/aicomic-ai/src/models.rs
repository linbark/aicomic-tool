use crate::client::{base_url, google_version, http_client, request_json};
use crate::{AiError, AiProvider, AiSettings, LlmClient, Result};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub supported_generation_methods: Vec<String>,
}

impl ModelInfo {
    pub fn supports_chat(&self, provider: AiProvider) -> bool {
        match provider {
            AiProvider::OpenAi => true,
            AiProvider::Google => self
                .supported_generation_methods
                .iter()
                .any(|method| method == "generateContent"),
        }
    }
}

impl fmt::Display for ModelInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.id)?;
        if !self.display_name.is_empty() && self.display_name != self.id {
            write!(f, " · {}", self.display_name)?;
        }
        Ok(())
    }
}

impl LlmClient {
    /// Fetch every advertised model without requiring a selected chat model.
    /// Google capability metadata is retained, including non-chat models.
    pub async fn list_models(&self, settings: &AiSettings) -> Result<Vec<ModelInfo>> {
        settings.validate_request()?;
        if settings.api_key.trim().is_empty() {
            return Err(AiError::MissingApiKey);
        }
        let endpoint = models_endpoint(settings)?;
        let client = http_client(settings)?;
        let mut models: BTreeMap<String, ModelInfo> = BTreeMap::new();
        let mut cursors = HashSet::new();
        let mut cursor: Option<String> = None;
        for _ in 0..100 {
            let mut url = endpoint.clone();
            match settings.provider {
                AiProvider::Google => {
                    url.query_pairs_mut().append_pair("pageSize", "1000");
                    if let Some(cursor) = &cursor {
                        url.query_pairs_mut().append_pair("pageToken", cursor);
                    }
                }
                AiProvider::OpenAi => {
                    if let Some(cursor) = &cursor {
                        url.query_pairs_mut().append_pair("after", cursor);
                    }
                }
            }
            let data = request_json(&client, settings, reqwest::Method::GET, url, None).await?;
            let field = match settings.provider {
                AiProvider::OpenAi => "data",
                AiProvider::Google => "models",
            };
            let entries = data[field]
                .as_array()
                .ok_or_else(|| invalid_catalog("缺少模型数组"))?;
            let mut last_id = None;
            for entry in entries {
                let model = parse_model(entry, settings.provider)?;
                last_id = Some(model.id.clone());
                match models.get_mut(&model.id) {
                    Some(existing) => {
                        if existing.display_name.is_empty() {
                            existing.display_name = model.display_name;
                        }
                        for method in model.supported_generation_methods {
                            if !existing.supported_generation_methods.contains(&method) {
                                existing.supported_generation_methods.push(method);
                            }
                        }
                    }
                    None => {
                        models.insert(model.id.clone(), model);
                    }
                }
            }
            cursor = match settings.provider {
                AiProvider::Google => optional_text(&data, "nextPageToken")?
                    .filter(|token| !token.is_empty())
                    .map(str::to_owned),
                AiProvider::OpenAi => match data.get("has_more") {
                    None | Some(Value::Bool(false)) => None,
                    Some(Value::Bool(true)) => {
                        let next = optional_text(&data, "last_id")?
                            .filter(|id| !id.trim().is_empty())
                            .map(str::to_owned)
                            .or(last_id)
                            .ok_or_else(|| invalid_catalog("分页缺少后续游标"))?;
                        Some(next)
                    }
                    Some(_) => return Err(invalid_catalog("has_more 必须为布尔值")),
                },
            };
            let Some(next) = &cursor else {
                return Ok(models.into_values().collect());
            };
            if !cursors.insert(next.clone()) {
                return Err(invalid_catalog("分页游标重复，无法取得完整列表"));
            }
        }
        Err(invalid_catalog("分页超过 100 页，无法确认完整列表"))
    }
}

fn models_endpoint(settings: &AiSettings) -> Result<reqwest::Url> {
    let mut url = base_url(settings)?;
    let base = url.path().trim_end_matches('/');
    let path = match settings.provider {
        AiProvider::Google => format!("{}/models", google_version(base)?),
        AiProvider::OpenAi if base.ends_with("/chat/completions") => {
            format!("{}/models", base.trim_end_matches("/chat/completions"))
        }
        AiProvider::OpenAi if base.ends_with("/v1") => format!("{base}/models"),
        AiProvider::OpenAi => format!("{base}/v1/models"),
    };
    url.set_path(&path);
    Ok(url)
}

fn parse_model(entry: &Value, provider: AiProvider) -> Result<ModelInfo> {
    let (id_field, display_field, methods_field) = match provider {
        AiProvider::OpenAi => ("id", "display_name", "supported_generation_methods"),
        AiProvider::Google => ("name", "displayName", "supportedGenerationMethods"),
    };
    let raw_id = entry[id_field]
        .as_str()
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| invalid_catalog("模型条目缺少有效 ID"))?;
    let id = if provider == AiProvider::Google {
        raw_id.strip_prefix("models/").unwrap_or(raw_id)
    } else {
        raw_id
    };
    if id.trim().is_empty() || id.chars().any(char::is_whitespace) {
        return Err(invalid_catalog("模型条目 ID 无效"));
    }
    let display_name = optional_text(entry, display_field)?
        .unwrap_or("")
        .to_owned();
    let supported_generation_methods = match entry.get(methods_field) {
        None => Vec::new(),
        Some(Value::Array(methods)) => methods
            .iter()
            .map(|method| {
                method
                    .as_str()
                    .filter(|method| !method.trim().is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| invalid_catalog("模型能力列表含无效条目"))
            })
            .collect::<Result<Vec<_>>>()?,
        Some(_) => return Err(invalid_catalog("模型能力必须为数组")),
    };
    Ok(ModelInfo {
        id: id.into(),
        display_name,
        supported_generation_methods,
    })
}

fn optional_text<'a>(data: &'a Value, field: &str) -> Result<Option<&'a str>> {
    match data.get(field) {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(invalid_catalog("模型元数据字段格式无效")),
    }
}

fn invalid_catalog(message: &str) -> AiError {
    AiError::Protocol(format!("AI 模型列表无效：{message}"))
}
