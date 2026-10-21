use crate::store::{read_json, write_json};
use crate::{AiError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub key: String,
    pub title: String,
    pub category: String,
    pub prompt: String,
    pub variables: Vec<String>,
    pub is_builtin: bool,
    pub is_modified: bool,
}
#[derive(Clone, Debug)]
pub struct PromptRegistry {
    root: PathBuf,
}
impl PromptRegistry {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    fn defaults(&self) -> Result<BTreeMap<String, Value>> {
        Ok(serde_json::from_str(include_str!(
            "../assets/default_prompts.json"
        ))?)
    }
    fn raw(&self) -> Result<Value> {
        let mut raw =
            read_json(&self.root.join("prompt_templates.json"))?.unwrap_or(json!({"templates":{}}));
        if !raw.is_object() {
            raw = json!({"templates":{}});
        }
        if !raw["templates"].is_object() {
            raw["templates"] = json!({});
        }
        Ok(raw)
    }
    pub fn list(&self) -> Result<Vec<PromptTemplate>> {
        let mut effective = self.defaults()?;
        for (key, value) in self.raw()?["templates"]
            .as_object()
            .expect("templates object")
        {
            if value.is_object() {
                effective.insert(key.clone(), value.clone());
            }
        }
        effective.keys().map(|key| self.get(key)).collect()
    }
    pub fn get(&self, key: &str) -> Result<PromptTemplate> {
        let key = key.trim();
        let defaults = self.defaults()?;
        let raw = self.raw()?;
        let base = defaults.get(key);
        let effective = raw["templates"]
            .get(key)
            .filter(|v| v.is_object())
            .or(base)
            .ok_or_else(|| AiError::InvalidInput(format!("Prompt 不存在：{key}")))?;
        let text = |field: &str, fallback: &str| {
            effective[field]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(fallback)
                .to_owned()
        };
        let variables = effective
            .get("variables")
            .filter(|v| v.is_array())
            .or_else(|| base.and_then(|v| v.get("variables")))
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let modified = base
            .map(|b| {
                ["title", "category", "prompt"]
                    .iter()
                    .any(|k| effective[*k] != b[*k])
            })
            .unwrap_or(false);
        Ok(PromptTemplate {
            key: key.into(),
            title: text("title", key),
            category: text("category", "misc"),
            prompt: text("prompt", ""),
            variables,
            is_builtin: base.is_some(),
            is_modified: modified,
        })
    }
    pub fn upsert(
        &self,
        key: &str,
        title: &str,
        category: &str,
        prompt: &str,
    ) -> Result<PromptTemplate> {
        let key = key.trim();
        if key.is_empty() {
            return Err(AiError::InvalidInput("Prompt key 不能为空".into()));
        }
        let variables = self.get(key).map(|t| t.variables).unwrap_or_default();
        let mut raw = self.raw()?;
        raw["templates"][key] =
            json!({"title":title,"category":category,"prompt":prompt,"variables":variables});
        write_json(&self.root.join("prompt_templates.json"), &raw)?;
        self.get(key)
    }
    pub fn create(
        &self,
        key: &str,
        title: &str,
        category: &str,
        prompt: &str,
    ) -> Result<PromptTemplate> {
        if self.get(key).is_ok() {
            return Err(AiError::InvalidInput("Prompt key 已存在".into()));
        }
        self.upsert(key, title, category, prompt)
    }
    pub fn reset(&self, key: &str) -> Result<PromptTemplate> {
        if !self.defaults()?.contains_key(key) {
            return Err(AiError::InvalidInput(
                "该 key 不是内置模板，无法重置".into(),
            ));
        }
        self.remove_override(key)?;
        self.get(key)
    }
    fn remove_override(&self, key: &str) -> Result<()> {
        let mut raw = self.raw()?;
        raw["templates"]
            .as_object_mut()
            .expect("object")
            .remove(key);
        write_json(&self.root.join("prompt_templates.json"), &raw)
    }
    pub fn delete(&self, key: &str) -> Result<()> {
        self.get(key)?;
        self.remove_override(key)
    }
    pub fn render(&self, key: &str, variables: &Value) -> Result<String> {
        let tpl = self.get(key)?;
        let mut rendered = tpl.prompt;
        for name in tpl.variables {
            let value = variables
                .get(&name)
                .ok_or_else(|| AiError::InvalidInput(format!("Prompt 缺少变量：{name}")))?;
            rendered = rendered.replace(
                &format!("{{{name}}}"),
                &value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            );
        }
        Ok(rendered)
    }
}
