//! 炼丹 D0：炼药（refine）与背包（bag）。
//!
//! 药材 = work；手法 = technique 表里的模板；药液 = extract；背包 = inventory。
//! 药液按 (work, technique, dimension, fire_model) 唯一，重复炼不会产生副本。

use anyhow::{Context, Result, bail};
use base64::Engine;
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::metrics;
use crate::vision::{CallOutcome, call, call_openai, openai_body, prepare_image};

pub struct RefineOpts<'a> {
    pub db: &'a str,
    pub root: &'a Path,
    pub work: i64,
    pub techniques: Vec<String>,
    pub provider: &'a str,
    pub model: &'a str,
    pub base_url: &'a str,
    pub thinking: u32,
    pub to_bag: bool,
}

pub struct BagOpts<'a> {
    pub db: &'a str,
    pub dimension: Option<&'a str>,
    pub limit: usize,
}

struct Technique {
    id: i64,
    key: String,
    title: String,
    dimension: String,
    prompt: String,
    params: String,
    version: String,
}

pub fn refine(o: &RefineOpts<'_>) -> Result<()> {
    let conn = Connection::open(o.db)?;
    conn.busy_timeout(Duration::from_secs(10))?;
    let (asset_path, title, _collection): (String, String, String) = conn
        .query_row(
            "SELECT a.local_path, w.title, w.collection FROM asset a JOIN work w ON w.id=a.work_id
             WHERE a.work_id=?1 AND a.local_path IS NOT NULL ORDER BY a.id LIMIT 1",
            params![o.work],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .with_context(|| format!("work {} 没有可用素材", o.work))?;
    let path = PathBuf::from(o.root).join(asset_path);
    let m = metrics::compute(&path)?;

    let mut techniques = Vec::new();
    for key in &o.techniques {
        let t = conn
            .query_row(
                "SELECT id,key,title,dimension,prompt_template,params_json,version FROM technique WHERE key=?1 AND status='active'",
                params![key],
                |r| {
                    Ok(Technique {
                        id: r.get(0)?,
                        key: r.get(1)?,
                        title: r.get(2)?,
                        dimension: r.get(3)?,
                        prompt: r.get(4)?,
                        params: r.get(5)?,
                        version: r.get(6)?,
                    })
                },
            )
            .with_context(|| format!("没有手法 {key}"))?;
        techniques.push(t);
    }

    // 只有需要模型的手法才要求密钥
    let needs_llm = techniques
        .iter()
        .any(|t| !t.params.contains("\"kind\":\"pixel\""));
    let key = if !needs_llm {
        String::new()
    } else if o.provider == "openai" {
        std::env::var("IE_API_KEY").unwrap_or_default()
    } else {
        std::env::var("GEMINI_API_KEY").context("缺少 GEMINI_API_KEY")?
    };
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()?;

    let now = crate::now_iso();
    for t in &techniques {
        let (content, fire) = if t.params.contains("\"kind\":\"pixel\"") {
            (pixel_extract(&m), "pixel-proxy".to_string())
        } else {
            let bytes = prepare_image(&path)?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let body = request_body(o.provider, &t.prompt, &b64, o.model, o.thinking);
            let outcome = if o.provider == "openai" {
                call_openai(&client, o.base_url, &key, &body)
            } else {
                call(&client, o.base_url, o.model, &key, &body)
            };
            let json = match outcome {
                CallOutcome::Ok(v, _) => v,
                CallOutcome::RateLimited(i) => bail!("限流：{}", i.body),
                CallOutcome::Failed(e) => bail!("调用失败：{e}"),
            };
            (json, o.model.to_string())
        };

        let quality = content
            .get("confidence")
            .and_then(|x| x.as_f64())
            .map(|c| c.clamp(0.0, 1.0) as f32)
            .unwrap_or(if fire == "pixel-proxy" { 0.85 } else { 0.6 });

        let hash = hash_of(&format!(
            "{}|{}|{}|{}|{}",
            o.work, t.key, t.version, fire, content
        ));
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO extract(work_id,technique_id,dimension,content_json,content_md,quality,fire_model,hash,created_at,technique_version)
             VALUES(?1,?2,?3,?4,NULL,?5,?6,?7,?8,?9)
             ON CONFLICT(work_id,technique_id,dimension,fire_model) DO UPDATE SET
               content_json=excluded.content_json, quality=excluded.quality,
               hash=excluded.hash, created_at=excluded.created_at",
            params![
                o.work,
                t.id,
                t.dimension,
                content.to_string(),
                quality,
                fire,
                hash,
                now,
                t.version
            ],
        )?;
        let eid: i64 = tx.query_row(
            "SELECT id FROM extract WHERE work_id=?1 AND technique_id=?2 AND dimension=?3 AND fire_model=?4",
            params![o.work, t.id, t.dimension, fire],
            |r| r.get(0),
        )?;
        if o.to_bag {
            tx.execute(
                "INSERT OR IGNORE INTO inventory(extract_id,added_at) VALUES(?1,?2)",
                params![eid, now],
            )?;
        }
        tx.commit()?;
        println!(
            "  ✓ {}「{}」{} 质量 {:.2} 火候 {} → 药液 #{}",
            t.key, t.title, t.dimension, quality, fire, eid
        );
    }
    println!(
        "炼药完成：{}「{}」共 {} 瓶{}",
        o.work,
        title,
        techniques.len(),
        if o.to_bag { "，已入背包" } else { "" }
    );
    Ok(())
}

fn request_body(
    provider: &str,
    prompt: &str,
    b64: &str,
    model: &str,
    thinking: u32,
) -> serde_json::Value {
    if provider == "openai" {
        let mut b = openai_body(prompt, b64, model, thinking, 2048);
        b["generationConfig"] = serde_json::Value::Null;
        b
    } else {
        serde_json::json!({
            "contents": [{"parts": [
                {"text": prompt},
                {"inline_data": {"mime_type": "image/jpeg", "data": b64}}
            ]}],
            "generationConfig": {
                "responseMimeType": "application/json",
                "temperature": 0.2,
                "maxOutputTokens": 2048,
                "thinkingConfig": {"thinkingBudget": thinking}
            }
        })
    }
}

fn pixel_extract(m: &metrics::Metrics) -> serde_json::Value {
    serde_json::json!({
        "colors": m.palette.iter().map(|(hex, share)| serde_json::json!({"hex": hex, "share": share})).collect::<Vec<_>>(),
        "stats": {
            "whitespace": m.whitespace,
            "contrast": m.contrast,
            "edge_density": m.edge_density,
            "symmetry": m.symmetry,
            "alignment": m.alignment,
        },
        "confidence": 0.85
    })
}

pub fn bag(o: &BagOpts<'_>) -> Result<()> {
    let conn = Connection::open(o.db)?;
    let sql = "SELECT extract_id, dimension, dimension_label, round(quality,2), fire_model, coalesce(material,'?'), technique
               FROM v_bag WHERE (?1 IS NULL OR dimension = ?1) ORDER BY extract_id DESC LIMIT ?2";
    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<(i64, String, String, f64, String, String, String)> = stmt
        .query_map(params![o.dimension, o.limit as i64], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if rows.is_empty() {
        println!("背包是空的：先 `ie refine --work <id> --technique t.palette`");
        return Ok(());
    }
    println!(
        "{:<6} {:<10} {:<12} {:<6} {:<22} {:<26} 手法",
        "id", "维度", "标签", "质量", "火候", "药材"
    );
    for (id, dim, label, q, fire, material, tech) in rows {
        println!(
            "{:<6} {:<10} {:<12} {:<6} {:<22} {:<26} {}",
            id,
            dim,
            label,
            format!("{q:.2}"),
            fire,
            truncate(&material, 24),
            tech
        );
    }
    Ok(())
}

fn hash_of(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

fn truncate(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n {
        s.to_string()
    } else {
        format!("{}…", chars[..n].iter().collect::<String>())
    }
}
