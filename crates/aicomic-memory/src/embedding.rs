use crate::{Error, Result};
pub trait EmbeddingProvider: Send + Sync {
    /// Stable identity of model, endpoint and model revision. Used to isolate caches.
    fn identifier(&self) -> String;
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}
pub struct UnconfiguredEmbedding;
impl EmbeddingProvider for UnconfiguredEmbedding {
    fn identifier(&self) -> String {
        "unconfigured".into()
    }
    fn embed(&self, _: &[String]) -> Result<Vec<Vec<f32>>> {
        Err(Error::EmbeddingNotConfigured)
    }
}
/// OpenAI-compatible embeddings endpoint, called directly from Rust.
pub struct OpenAICompatibleEmbedding {
    client: reqwest::blocking::Client,
    url: String,
    api_key: String,
    model: String,
}
impl OpenAICompatibleEmbedding {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Result<Self> {
        let mut url =
            reqwest::Url::parse(base_url).map_err(|_| crate::invalid("Embedding URL 无效"))?;
        if !["http", "https"].contains(&url.scheme())
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(crate::invalid(
                "Embedding URL 必须为 HTTP(S)，凭据请使用 API Key",
            ));
        }
        if model.trim().is_empty() {
            return Err(Error::EmbeddingNotConfigured);
        }
        let path = url.path().trim_end_matches('/');
        let path = if path.ends_with("/embeddings") {
            path.to_owned()
        } else {
            format!("{path}/embeddings")
        };
        url.set_path(&path);
        url.set_query(None);
        url.set_fragment(None);
        Ok(Self {
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()?,
            url: url.to_string(),
            api_key: api_key.into(),
            model: model.into(),
        })
    }
}
impl EmbeddingProvider for OpenAICompatibleEmbedding {
    fn identifier(&self) -> String {
        use sha2::Digest;
        format!(
            "openai-compatible:{:x}",
            sha2::Sha256::digest(format!("{}\n{}", self.url, self.model).as_bytes())
        )
    }
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let mut request = self
            .client
            .post(&self.url)
            .json(&serde_json::json!({"model":self.model,"input":texts,"encoding_format":"float"}));
        if !self.api_key.is_empty() {
            request = request.bearer_auth(&self.api_key)
        }
        let response = request
            .send()?
            .error_for_status()?
            .json::<serde_json::Value>()?;
        let rows = response["data"]
            .as_array()
            .ok_or_else(|| crate::invalid("Embedding 响应缺少 data"))?;
        if rows.len() != texts.len() {
            return Err(crate::invalid("Embedding 响应数量与输入不一致"));
        }
        let mut out = vec![None; texts.len()];
        let mut dimension = None;
        for row in rows {
            let index = row["index"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| *n < texts.len())
                .ok_or_else(|| crate::invalid("Embedding 响应 index 无效"))?;
            if out[index].is_some() {
                return Err(crate::invalid("Embedding 响应 index 重复"));
            }
            let vector: Vec<f32> = serde_json::from_value(row["embedding"].clone())?;
            if vector.is_empty()
                || vector.iter().any(|v| !v.is_finite())
                || dimension.is_some_and(|d| d != vector.len())
            {
                return Err(crate::invalid("Embedding 响应向量为空、非有限或维度不一致"));
            }
            dimension = Some(vector.len());
            out[index] = Some(vector);
        }
        out.into_iter()
            .map(|v| v.ok_or_else(|| crate::invalid("Embedding 响应不完整")))
            .collect()
    }
}
#[cfg(feature = "local-bge")]
/// Dense BGE-M3 inference using Candle's XLM-RoBERTa on CPU.
/// Weights, tokenizer and config must already exist in an explicitly chosen folder.
/// Sparse and ColBERT heads are not used by the legacy dense memory search.
pub struct LocalBgeM3 {
    model: candle_transformers::models::xlm_roberta::XLMRobertaModel,
    tokenizer: tokenizers::Tokenizer,
    key: String,
}
#[cfg(feature = "local-bge")]
impl LocalBgeM3 {
    pub fn from_dir(directory: &std::path::Path, max_tokens: usize) -> Result<Self> {
        use candle_core::{DType, Device};
        use candle_nn::VarBuilder;
        use candle_transformers::models::xlm_roberta::{Config, XLMRobertaModel};
        use sha2::Digest;
        let read = |name: &str| {
            std::fs::read(directory.join(name))
                .map_err(|e| crate::invalid(format!("本地 BGE-M3 缺少或无法读取 {name}: {e}")))
        };
        let config_bytes = read("config.json")?;
        let value: serde_json::Value = serde_json::from_slice(&config_bytes)?;
        if value["model_type"] != "xlm-roberta" {
            return Err(crate::invalid("本地模型必须为 BGE-M3 / XLM-RoBERTa 架构"));
        }
        let config: Config = serde_json::from_value(value)?;
        let limit = config
            .max_position_embeddings
            .saturating_sub(config.pad_token_id as usize + 1);
        if max_tokens < 2 || max_tokens > limit {
            return Err(crate::invalid(format!(
                "BGE-M3 max_tokens 必须在 2 至 {limit} 之间"
            )));
        }
        let tokenizer_bytes = read("tokenizer.json")?;
        let mut tokenizer = tokenizers::Tokenizer::from_bytes(&tokenizer_bytes)
            .map_err(|e| crate::invalid(format!("BGE-M3 tokenizer.json 无效: {e}")))?;
        tokenizer
            .with_truncation(Some(tokenizers::utils::truncation::TruncationParams {
                max_length: max_tokens,
                ..Default::default()
            }))
            .map_err(|e| crate::invalid(format!("BGE-M3 分词截断设置失败: {e}")))?;
        tokenizer.with_padding(None);
        let weights = read("model.safetensors")?;
        let mut digest = sha2::Sha256::new();
        digest.update(&config_bytes);
        digest.update(&tokenizer_bytes);
        digest.update(&weights);
        digest.update(max_tokens.to_le_bytes());
        let key = format!("local-bge-m3:{:x}", digest.finalize());
        // Safe owned safetensors, with no Python runtime, model download or child process.
        let vb = VarBuilder::from_buffered_safetensors(weights, DType::F32, &Device::Cpu)
            .map_err(|e| crate::invalid(format!("BGE-M3 权重加载失败: {e}")))?;
        let vb = if vb.contains_tensor("embeddings.word_embeddings.weight") {
            vb
        } else {
            vb.pp("roberta")
        };
        let model = XLMRobertaModel::new(&config, vb)
            .map_err(|e| crate::invalid(format!("BGE-M3 模型加载失败: {e}")))?;
        Ok(Self {
            model,
            tokenizer,
            key,
        })
    }
}
#[cfg(feature = "local-bge")]
impl EmbeddingProvider for LocalBgeM3 {
    fn identifier(&self) -> String {
        self.key.clone()
    }
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        use candle_core::{DType, Device, IndexOp, Tensor};
        let mut vectors = Vec::with_capacity(texts.len());
        for text in texts {
            let tokens = self
                .tokenizer
                .encode(text.as_str(), true)
                .map_err(|e| crate::invalid(format!("BGE-M3 分词失败: {e}")))?;
            if tokens.get_ids().first() != Some(&0) {
                return Err(crate::invalid(
                    "BGE-M3 tokenizer 必须在首位添加 <s> / CLS token",
                ));
            }
            let run = || -> candle_core::Result<Vec<f32>> {
                let input = Tensor::new(tokens.get_ids(), &Device::Cpu)?.unsqueeze(0)?;
                let attention =
                    Tensor::new(tokens.get_attention_mask(), &Device::Cpu)?.unsqueeze(0)?;
                let types = Tensor::zeros(input.shape(), DType::U32, &Device::Cpu)?;
                self.model
                    .forward(&input, &attention, &types, None, None, None)?
                    .i((0, 0, ..))?
                    .to_vec1::<f32>()
            };
            let mut vector =
                run().map_err(|e| crate::invalid(format!("BGE-M3 Rust 推理失败: {e}")))?;
            let norm = vector
                .iter()
                .map(|n| f64::from(*n).powi(2))
                .sum::<f64>()
                .sqrt();
            if !norm.is_finite() || norm == 0.0 {
                return Err(crate::invalid("BGE-M3 输出无效向量"));
            }
            for n in &mut vector {
                *n = (f64::from(*n) / norm) as f32
            }
            vectors.push(vector);
        }
        Ok(vectors)
    }
}
