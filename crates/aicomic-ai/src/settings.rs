use crate::store::{read_json, write_json};
use crate::{AiError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fmt, path::Path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AiProvider {
    #[default]
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "google")]
    Google,
}
impl AiProvider {
    pub const ALL: [Self; 2] = [Self::OpenAi, Self::Google];

    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.deepseek.com",
            Self::Google => "https://generativelanguage.googleapis.com/v1beta",
        }
    }
    pub fn default_model(self) -> &'static str {
        match self {
            Self::OpenAi => "deepseek-chat",
            Self::Google => "gemini-flash-latest",
        }
    }
}
impl fmt::Display for AiProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OpenAi => "OpenAI 兼容",
            Self::Google => "Google AI / Gemini",
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    pub provider: AiProvider,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub temperature: f64,
    pub max_tokens: Option<u32>,
    pub timeout_seconds: f64,
}
impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: AiProvider::default(),
            api_key: String::new(),
            base_url: AiProvider::default().default_base_url().into(),
            model: AiProvider::default().default_model().into(),
            temperature: 0.2,
            max_tokens: None,
            timeout_seconds: 120.0,
        }
    }
}
impl fmt::Debug for AiSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AiSettings")
            .field("provider", &self.provider)
            .field("api_key", &if self.api_key.is_empty() { "" } else { "***" })
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .field("max_tokens", &self.max_tokens)
            .field("timeout_seconds", &self.timeout_seconds)
            .finish()
    }
}
impl AiSettings {
    pub fn load(root: &Path) -> Result<Self> {
        let raw = read_json(&root.join("ai_settings.json"))?.unwrap_or(json!({}));
        let defaults = Self::default();
        let provider = raw
            .get("provider")
            .map(|value| serde_json::from_value::<AiProvider>(value.clone()))
            .transpose()?
            .unwrap_or_default();
        let text = |key: &str, fallback: String| {
            raw[key]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or(fallback)
        };
        Ok(Self {
            provider,
            api_key: text("api_key", String::new()),
            base_url: text("base_url", provider.default_base_url().into()),
            model: text("model", provider.default_model().into()),
            temperature: raw["temperature"].as_f64().unwrap_or(defaults.temperature),
            max_tokens: raw["max_tokens"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0),
            timeout_seconds: raw["timeout_seconds"]
                .as_f64()
                .filter(|n| n.is_finite() && *n > 0.0)
                .unwrap_or(defaults.timeout_seconds),
        })
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        self.validate()?;
        let path = root.join("ai_settings.json");
        let mut raw = read_json(&path)?.unwrap_or(json!({}));
        if !raw.is_object() {
            raw = json!({});
        }
        for (key, value) in serde_json::to_value(self)?
            .as_object()
            .expect("settings object")
        {
            raw[key] = value.clone();
        }
        write_json(&path, &raw)
    }
    pub fn normalized_max_tokens(&self) -> u32 {
        self.max_tokens.filter(|n| *n > 0).unwrap_or(8192).min(8192)
    }
    pub fn masked(&self) -> Value {
        json!({"provider": self.provider, "has_api_key": !self.api_key.trim().is_empty(), "base_url": self.base_url, "model": self.model, "temperature": self.temperature, "max_tokens": self.max_tokens, "timeout_seconds": self.timeout_seconds})
    }
    pub fn validate(&self) -> Result<()> {
        if self.model.trim().is_empty() {
            return Err(AiError::InvalidInput("AI 模型名称不能为空".into()));
        }
        if !self.timeout_seconds.is_finite()
            || self.timeout_seconds <= 0.0
            || self.timeout_seconds > 86400.0
        {
            return Err(AiError::InvalidInput(
                "timeout_seconds 必须在 0 至 86400 秒之间".into(),
            ));
        }
        if !self.temperature.is_finite() || !(0.0..=2.0).contains(&self.temperature) {
            return Err(AiError::InvalidInput(
                "temperature 必须在 0 至 2 之间".into(),
            ));
        }
        Ok(())
    }
}
