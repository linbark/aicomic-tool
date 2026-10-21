#![cfg(feature = "local-bge")]
use aicomic_memory::{EmbeddingProvider, LocalBgeM3};
#[test]
fn local_model_never_downloads_missing_weights() {
    let dir = tempfile::tempdir().unwrap();
    let error = LocalBgeM3::from_dir(dir.path(), 512).err().unwrap();
    assert!(error.to_string().contains("config.json"));
}
#[test]
fn local_xlm_roberta_runs_real_rust_tensor_path_with_explicit_weights() {
    use candle_core::{DType, Device};
    use candle_nn::{VarBuilder, VarMap};
    use candle_transformers::models::xlm_roberta::{Config, XLMRobertaModel};
    use tokenizers::{
        Tokenizer, models::wordlevel::WordLevel, processors::template::TemplateProcessing,
    };
    let dir = tempfile::tempdir().unwrap();
    let config = serde_json::json!({"model_type":"xlm-roberta","hidden_size":4,"layer_norm_eps":0.00001,"attention_probs_dropout_prob":0.,"hidden_dropout_prob":0.,"num_attention_heads":2,"position_embedding_type":"absolute","intermediate_size":8,"hidden_act":"gelu","num_hidden_layers":1,"vocab_size":8,"max_position_embeddings":16,"type_vocab_size":1,"pad_token_id":1});
    std::fs::write(dir.path().join("config.json"), config.to_string()).unwrap();
    let cfg: Config = serde_json::from_value(config).unwrap();
    let vars = VarMap::new();
    let vb = VarBuilder::from_varmap(&vars, DType::F32, &Device::Cpu);
    let _model = XLMRobertaModel::new(&cfg, vb).unwrap();
    vars.save(dir.path().join("model.safetensors")).unwrap();
    let model = WordLevel::builder()
        .vocab(
            [
                ("<s>".into(), 0),
                ("<pad>".into(), 1),
                ("</s>".into(), 2),
                ("<unk>".into(), 3),
                ("hello".into(), 4),
            ]
            .into_iter()
            .collect(),
        )
        .unk_token("<unk>".into())
        .build()
        .unwrap();
    let mut tokenizer = Tokenizer::new(model);
    tokenizer.with_post_processor(Some(
        TemplateProcessing::builder()
            .try_single("<s> $A </s>")
            .unwrap()
            .special_tokens(vec![("<s>", 0), ("</s>", 2)])
            .build()
            .unwrap(),
    ));
    tokenizer
        .save(dir.path().join("tokenizer.json"), false)
        .unwrap();
    let provider = LocalBgeM3::from_dir(dir.path(), 8).unwrap();
    let vectors = provider.embed(&["hello".into(), "hello".into()]).unwrap();
    assert_eq!(vectors.len(), 2);
    assert_eq!(vectors[0].len(), 4);
    assert_eq!(vectors[0], vectors[1]);
    let norm: f32 = vectors[0].iter().map(|v| v * v).sum();
    assert!((norm - 1.).abs() < 0.0001);
}
