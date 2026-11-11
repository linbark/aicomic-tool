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

pub struct BrewOpts<'a> {
    pub db: &'a str,
    pub recipe: &'a Path,
    pub allow_mixed: bool,
    pub force: bool,
    pub out_dir: Option<&'a Path>,
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

pub fn brew(o: &BrewOpts<'_>) -> Result<()> {
    let text = std::fs::read_to_string(o.recipe)
        .with_context(|| format!("读不到配方 {}", o.recipe.display()))?;
    let r: serde_json::Value = serde_json::from_str(&text).context("配方不是合法 JSON")?;
    let key = r["key"].as_str().context("配方缺 key")?.to_string();
    let name = r["name"].as_str().unwrap_or(&key).to_string();
    let goal = r["goal"].as_str().unwrap_or("").to_string();
    let template = r["combination_prompt"].as_str().unwrap_or("").to_string();
    let constraints = r
        .get("constraints")
        .cloned()
        .unwrap_or(serde_json::json!({}));
    let components = r["components"].as_array().cloned().unwrap_or_default();

    let conn = Connection::open(o.db)?;
    conn.busy_timeout(Duration::from_secs(10))?;

    // 配方入库（按 key upsert），便于后续统计与版本对比
    conn.execute(
        "INSERT INTO recipe(key,name,goal,combination_prompt,constraints_json,version,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(key) DO UPDATE SET name=excluded.name, goal=excluded.goal,
           combination_prompt=excluded.combination_prompt, constraints_json=excluded.constraints_json,
           version=excluded.version",
        params![
            key,
            name,
            goal,
            template,
            constraints.to_string(),
            r["version"].as_str().unwrap_or("1"),
            crate::now_iso()
        ],
    )?;
    let recipe_id: i64 =
        conn.query_row("SELECT id FROM recipe WHERE key=?1", params![key], |row| {
            row.get(0)
        })?;
    conn.execute(
        "DELETE FROM recipe_component WHERE recipe_id=?1",
        params![recipe_id],
    )?;
    for c in &components {
        conn.execute(
            "INSERT OR REPLACE INTO recipe_component(recipe_id,slot,dimension,ratio,required,fallback)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                recipe_id,
                c["slot"].as_str().unwrap_or("base"),
                c["dimension"].as_str().context("组件缺 dimension")?,
                c["ratio"].as_f64().unwrap_or(0.0),
                c["required"].as_bool().unwrap_or(true) as i64,
                c.get("fallback").and_then(|x| x.as_str())
            ],
        )?;
    }

    // 逐槽选药：同维度里质量最高的一瓶
    struct Picked {
        slot: String,
        dimension: String,
        ratio: f64,
        extract_id: i64,
        quality: f64,
        fire: String,
        material: String,
        content: serde_json::Value,
    }
    let mut picked: Vec<Picked> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for c in &components {
        let slot = c["slot"].as_str().unwrap_or("base").to_string();
        let dimension = c["dimension"].as_str().unwrap_or("").to_string();
        let ratio = c["ratio"].as_f64().unwrap_or(0.0);
        let required = c["required"].as_bool().unwrap_or(true);
        let row: Option<(i64, f64, String, String, String)> = conn
            .query_row(
                "SELECT e.id, e.quality, e.fire_model, coalesce(w.title,'?'), e.content_json
                 FROM inventory i JOIN extract e ON e.id=i.extract_id
                 LEFT JOIN work w ON w.id=e.work_id
                 WHERE e.dimension=?1 ORDER BY e.quality DESC, e.id DESC LIMIT 1",
                params![dimension],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .ok();
        match row {
            Some((id, q, fire, material, content)) => picked.push(Picked {
                slot,
                dimension,
                ratio,
                extract_id: id,
                quality: q,
                fire,
                material,
                content: serde_json::from_str(&content).unwrap_or(serde_json::json!({})),
            }),
            None => {
                if required {
                    missing.push(format!("{slot}/{dimension}"));
                }
            }
        }
    }
    if !missing.is_empty() {
        bail!("配方缺药：{}（先用 ie refine 炼出来）", missing.join("、"));
    }

    // 相冲检测
    let mut conflicts: Vec<serde_json::Value> = Vec::new();
    let fires: std::collections::BTreeSet<String> = picked
        .iter()
        .filter(|p| p.fire != "pixel-proxy")
        .map(|p| p.fire.clone())
        .collect();
    if fires.len() > 1 && !o.allow_mixed {
        conflicts.push(serde_json::json!({
            "rule": "fire.mixed", "severity": "error",
            "detail": format!("一炉混了 {} 种火候：{}", fires.len(), fires.iter().cloned().collect::<Vec<_>>().join("、"))
        }));
    }
    if let Some(p) = picked.iter().find(|p| p.dimension == "palette") {
        let contrast = p.content["stats"]["contrast"].as_f64().unwrap_or(0.0);
        let min = constraints["contrast_min"].as_f64().unwrap_or(4.5);
        if contrast > 0.0 && contrast < min {
            conflicts.push(serde_json::json!({
                "rule": "contrast.min", "severity": "error",
                "detail": format!("主色板明度对比 {contrast:.2}:1 低于配方要求 {min:.1}:1")
            }));
        }
    }
    for p in &picked {
        if p.quality < 0.5 {
            conflicts.push(serde_json::json!({
                "rule": "quality.low", "severity": "warn",
                "detail": format!("{} 药液质量仅 {:.2}", p.dimension, p.quality)
            }));
        }
    }
    let errors: Vec<&serde_json::Value> = conflicts
        .iter()
        .filter(|c| c["severity"] == "error")
        .collect();
    if !errors.is_empty() && !o.force {
        let lines: Vec<String> = errors
            .iter()
            .map(|c| format!("- {}", c["detail"].as_str().unwrap_or("")))
            .collect();
        bail!("相冲，拒绝炼丹：\n{}", lines.join("\n"));
    }

    // 渲染合成提示词
    let mut map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for p in &picked {
        let prefix = format!("{}.{}", p.slot, p.dimension);
        map.insert(
            format!("{prefix}.ratio"),
            format!("{:.0}%", p.ratio * 100.0),
        );
        map.insert(format!("{prefix}.content"), p.content.to_string());
        if let Some(obj) = p.content.as_object() {
            for (k, v) in obj {
                let s = match v {
                    serde_json::Value::Array(a) => a
                        .iter()
                        .map(|x| {
                            if let (Some(hex), Some(share)) = (
                                x.get("hex").and_then(|h| h.as_str()),
                                x.get("share").and_then(|s| s.as_f64()),
                            ) {
                                format!("{hex} {:.0}%", share * 100.0)
                            } else {
                                x.as_str()
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| x.to_string())
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("、"),
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                map.insert(format!("{prefix}.{k}"), s);
            }
        }
    }
    let mut rendered = template.clone();
    for (k, v) in &map {
        rendered = rendered.replace(&format!("{{{{{k}}}}}"), v);
    }

    // 组装丹药
    let mut md = String::new();
    md.push_str(&format!(
        "# 丹药 · {name}\n\n> 配方 {key} ｜ 目标：{goal}\n\n"
    ));
    md.push_str("## 合成提示词（可直接投喂）\n\n");
    md.push_str(&format!("> {rendered}\n\n"));
    md.push_str("## 各味药液\n\n| 槽位 | 维度 | 配比 | 药材 | 质量 | 火候 |\n| --- | --- | --- | --- | --- | --- |\n");
    for p in &picked {
        md.push_str(&format!(
            "| {} | {} | {:.0}% | {} | {:.2} | {} |\n",
            p.slot,
            p.dimension,
            p.ratio * 100.0,
            p.material,
            p.quality,
            p.fire
        ));
    }
    md.push_str("\n## 相冲检查\n\n");
    if conflicts.is_empty() {
        md.push_str("- 无冲突\n");
    } else {
        for c in &conflicts {
            md.push_str(&format!(
                "- [{}] {}：{}\n",
                c["severity"].as_str().unwrap_or("?"),
                c["rule"].as_str().unwrap_or("?"),
                c["detail"].as_str().unwrap_or("")
            ));
        }
    }

    let now = crate::now_iso();
    let output_json = serde_json::json!({
        "recipe": key,
        "components": picked.iter().map(|p| serde_json::json!({
            "slot": p.slot, "dimension": p.dimension, "ratio": p.ratio,
            "extract_id": p.extract_id, "material": p.material, "quality": p.quality, "fire": p.fire
        })).collect::<Vec<_>>(),
        "conflicts": conflicts,
        "prompt": rendered,
    });
    conn.execute(
        "INSERT INTO pill(recipe_id,output_md,output_json,score_json,cost_json,created_at)
         VALUES(?1,?2,?3,'{}','{\"tokens\":0}',?4)",
        params![recipe_id, md, output_json.to_string(), now],
    )?;
    let pill_id = conn.last_insert_rowid();

    if let Some(dir) = o.out_dir {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(format!("{pill_id}-{key}.md")), &md)?;
    }
    println!("{md}");
    println!("丹药 #{pill_id} 已入库");
    Ok(())
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
