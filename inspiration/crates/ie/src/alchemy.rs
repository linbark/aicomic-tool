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
    pub refresh: bool,
}

pub struct BrewOpts<'a> {
    pub db: &'a str,
    pub recipe: &'a Path,
    pub pinned: Option<&'a std::collections::HashMap<String, i64>>,
    pub formula_id: Option<i64>,
    pub allow_mixed: bool,
    pub force: bool,
    pub out_dir: Option<&'a Path>,
    pub quiet: bool,
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
    let work_domain: String = conn
        .query_row(
            "SELECT domain_id FROM work WHERE id=?1",
            params![o.work],
            |r| r.get(0),
        )
        .unwrap_or_default();
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
        let fire_hint = if t.params.contains("\"kind\":\"pixel\"") {
            "pixel-proxy".to_string()
        } else {
            o.model.to_string()
        };
        if !o.refresh
            && let Ok((eid, q)) = conn.query_row(
                "SELECT id, quality FROM extract
             WHERE work_id=?1 AND technique_id=?2 AND dimension=?3 AND fire_model=?4",
                params![o.work, t.id, t.dimension, fire_hint],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)),
            )
        {
            if o.to_bag {
                conn.execute(
                    "INSERT OR IGNORE INTO inventory(extract_id,added_at) VALUES(?1,?2)",
                    params![eid, now],
                )?;
            }
            println!(
                "  · {}「{}」已有同火候药液 #{}（质量 {:.2}），复用",
                t.key, t.title, eid, q
            );
            continue;
        }
        let (content, fire) = if t.params.contains("\"kind\":\"pixel\"") {
            let content = if t.dimension == "spacing" {
                spacing_extract(&m, &work_domain)
            } else {
                pixel_extract(&m)
            };
            (content, "pixel-proxy".to_string())
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

        // 图上确实没有该维度（例如无文字作品的「字」）：不入库，避免空药液污染背包
        let empty = content
            .as_object()
            .map(|o| {
                o.iter().all(|(k, v)| {
                    k == "confidence"
                        || v.as_str().map(|s| s.trim().is_empty()).unwrap_or(false)
                        || v.as_array().map(|a| a.is_empty()).unwrap_or(false)
                })
            })
            .unwrap_or(true);
        if empty && quality < 0.1 {
            println!("  · {}「{}」无内容，跳过", t.key, t.title);
            continue;
        }

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

/// 从实测色板推断角色：谁是底、谁是字、谁是线、谁是强调。
/// 纯计算，不调模型——颜色角色是最不该让模型"猜"的东西。
fn palette_roles(colors: &[(String, f32)], paper: &str, ink: &str) -> serde_json::Value {
    fn rgb(hex: &str) -> (f64, f64, f64) {
        let h = hex.trim_start_matches('#');
        let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f64 / 255.0;
        (v(0), v(2), v(4))
    }
    fn lum(hex: &str) -> f64 {
        let f = |c: f64| {
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, b) = rgb(hex);
        0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
    }
    fn contrast(a: &str, b: &str) -> f64 {
        let (l1, l2) = (lum(a), lum(b));
        let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
        (hi + 0.05) / (lo + 0.05)
    }
    fn sat(hex: &str) -> f64 {
        let (r, g, b) = rgb(hex);
        let mx = r.max(g).max(b);
        let mn = r.min(g).min(b);
        if mx <= 0.0 { 0.0 } else { (mx - mn) / mx }
    }
    let bg = if paper.is_empty() {
        colors
            .first()
            .map(|(h, _)| h.clone())
            .unwrap_or_else(|| "#FFFFFF".into())
    } else {
        paper.to_string()
    };
    let mut text = if ink.is_empty() {
        bg.clone()
    } else {
        ink.to_string()
    };
    let mut best = contrast(&bg, &text);
    for (hex, _) in colors {
        let c = contrast(&bg, hex);
        if c > best {
            best = c;
            text = hex.clone();
        }
    }
    let muted = colors
        .iter()
        .map(|(h, _)| (h.clone(), contrast(&bg, h)))
        .filter(|(h, c)| *c > 1.6 && *c < best * 0.75 && h != &text)
        .min_by(|a, b| (a.1 - 3.4).abs().partial_cmp(&(b.1 - 3.4).abs()).unwrap())
        .map(|(h, _)| h)
        .unwrap_or_else(|| text.clone());
    let border = colors
        .iter()
        .map(|(h, _)| (h.clone(), contrast(&bg, h)))
        .filter(|(h, c)| *c >= 1.2 && *c <= 2.6 && h != &text && h != &muted)
        .min_by(|a, b| (a.1 - 1.6).abs().partial_cmp(&(b.1 - 1.6).abs()).unwrap())
        .map(|(h, _)| h)
        .unwrap_or_else(|| muted.clone());
    let accent = colors
        .iter()
        .filter(|(h, _)| h != &bg && h != &text)
        .max_by(|a, b| sat(&a.0).partial_cmp(&sat(&b.0)).unwrap())
        .map(|(h, _)| h.clone())
        .unwrap_or_else(|| text.clone());
    serde_json::json!({
        "bg": bg, "text": text, "muted": muted, "border": border, "accent": accent,
        "text_contrast": (contrast(&bg, &text) * 100.0).round() / 100.0,
    })
}

fn spacing_extract(m: &metrics::Metrics, domain: &str) -> serde_json::Value {
    // 页边吸附到模数整数倍：测量出的 181 与模数 8 不自洽，会导致规格内部矛盾
    let base = m.spacing_base.max(1);
    let margin = ((m.page_margin + base / 2) / base) * base;
    serde_json::json!({
        "domain": domain,
        "base_px": m.spacing_base,
        "steps": m.spacing_steps,
        "page_margin_px": margin,
        "section_gap_px": m.section_gap,
        "confidence": 0.7,
        "method": "whitespace-bands"
    })
}

fn pixel_extract(m: &metrics::Metrics) -> serde_json::Value {
    serde_json::json!({
        "colors": m.palette.iter().map(|(hex, share)| serde_json::json!({"hex": hex, "share": share})).collect::<Vec<_>>(),
        "roles": palette_roles(&m.palette, &m.paper, &m.ink),
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
    let page_type = r
        .get("page_type")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let recipe_domain = r
        .get("domain")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
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
            "INSERT OR REPLACE INTO recipe_component(recipe_id,slot,dimension,ratio,required,fallback,polarity)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                recipe_id,
                c["slot"].as_str().unwrap_or("base"),
                c["dimension"].as_str().context("组件缺 dimension")?,
                c["ratio"].as_f64().unwrap_or(0.0),
                c["required"].as_bool().unwrap_or(true) as i64,
                c.get("fallback").and_then(|x| x.as_str()),
                c.get("polarity").and_then(|x| x.as_str()).unwrap_or("positive")
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
        polarity: String,
    }
    let mut picked: Vec<Picked> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for c in &components {
        let slot = c["slot"].as_str().unwrap_or("base").to_string();
        let dimension = c["dimension"].as_str().unwrap_or("").to_string();
        let ratio = c["ratio"].as_f64().unwrap_or(0.0);
        let required = c["required"].as_bool().unwrap_or(true);
        let polarity = c
            .get("polarity")
            .and_then(|x| x.as_str())
            .unwrap_or("positive")
            .to_string();
        if let Some(pin) = o.pinned.and_then(|m| m.get(&dimension)).copied() {
            let row: Option<(i64, f64, String, String, String)> = conn
                .query_row(
                    "SELECT e.id, e.quality, e.fire_model, coalesce(w.title,'?'), e.content_json
                     FROM extract e LEFT JOIN work w ON w.id=e.work_id WHERE e.id=?1",
                    params![pin],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .ok();
            if let Some((id, q, fire, material, content)) = row {
                picked.push(Picked {
                    slot,
                    dimension,
                    ratio,
                    extract_id: id,
                    quality: q,
                    fire,
                    material,
                    content: serde_json::from_str(&content).unwrap_or(serde_json::json!({})),
                    polarity,
                });
                continue;
            }
        }
        // 取候选（最多 20 瓶），palette 先按配方的对比度约束过滤，再按质量排
        let mut stmt = conn.prepare(
            "SELECT e.id, e.quality, e.fire_model, coalesce(w.title,'?'), e.content_json
             FROM inventory i JOIN extract e ON e.id=i.extract_id
             LEFT JOIN work w ON w.id=e.work_id
             WHERE e.dimension=?1 ORDER BY e.quality DESC, e.id DESC LIMIT 20",
        )?;
        let mut cands: Vec<(i64, f64, String, String, String)> = stmt
            .query_map(params![dimension], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        if dimension == "donts" && !page_type.is_empty() {
            // 优先选明确标注适用于本页面的药液；没有显式标注的（旧数据）作为兜底
            let explicit: Vec<_> = cands
                .iter()
                .filter(|c| {
                    let v: serde_json::Value = serde_json::from_str(&c.4).unwrap_or_default();
                    v["donts"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .any(|d| d["applies_to"].as_str() == Some(page_type.as_str()))
                        })
                        .unwrap_or(false)
                })
                .cloned()
                .collect();
            if !explicit.is_empty() {
                cands = explicit;
            }
        }
        if dimension == "spacing" && !recipe_domain.is_empty() {
            // 间距必须跟媒介：油画的页边距不能拿去当落地页的边距
            let matching: Vec<_> = cands
                .iter()
                .filter(|c| {
                    let v: serde_json::Value = serde_json::from_str(&c.4).unwrap_or_default();
                    v["domain"].as_str() == Some(recipe_domain.as_str())
                })
                .cloned()
                .collect();
            if !matching.is_empty() {
                cands = matching;
            }
        }
        if dimension == "palette" {
            let min = constraints["contrast_min"].as_f64().unwrap_or(0.0);
            if min > 0.0 {
                let passing: Vec<_> = cands
                    .iter()
                    .filter(|c| {
                        let v: serde_json::Value = serde_json::from_str(&c.4).unwrap_or_default();
                        v["stats"]["contrast"].as_f64().unwrap_or(0.0) >= min
                    })
                    .cloned()
                    .collect();
                if !passing.is_empty() {
                    cands = passing;
                }
            }
        }
        let row = cands.into_iter().next();
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
                polarity,
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

    // 规格自洽检查：不通过就拿不到丹药（除非 --force）
    let mut issues: Vec<serde_json::Value> = Vec::new();
    let ratio_sum: f64 = picked.iter().map(|p| p.ratio).sum();
    if (ratio_sum - 1.0).abs() > 0.02 {
        issues.push(serde_json::json!({"severity":"error","rule":"ratio.sum","detail":format!("配比之和 {ratio_sum:.2}，应为 1.0")}));
    }
    if let Some(p) = picked.iter().find(|p| p.dimension == "palette") {
        let c = p.content["roles"]["text_contrast"]
            .as_f64()
            .unwrap_or_else(|| p.content["stats"]["contrast"].as_f64().unwrap_or(0.0));
        if c > 0.0 && c < 4.5 {
            issues.push(serde_json::json!({"severity":"error","rule":"palette.contrast","detail":format!("正文对比 {c:.2}:1 < 4.5")}));
        }
    }
    if let Some(p) = picked.iter().find(|p| p.dimension == "spacing") {
        let base = p.content["base_px"].as_f64().unwrap_or(0.0);
        let margin = p.content["page_margin_px"].as_f64().unwrap_or(0.0);
        if base > 0.0 && margin > 0.0 && (margin % base).abs() > 0.01 {
            issues.push(serde_json::json!({"severity":"warn","rule":"spacing.modulus","detail":format!("页边 {margin:.0} 不是模数 {base:.0} 的整数倍")}));
        }
    }
    if let Some(p) = picked.iter().find(|p| p.dimension == "type") {
        let sizes: Vec<f64> = p.content["steps"]
            .as_array()
            .map(|a| a.iter().filter_map(|s| s["size_px"].as_f64()).collect())
            .unwrap_or_default();
        if sizes.len() >= 2 {
            let mx = sizes.iter().cloned().fold(0.0f64, f64::max);
            let mn = sizes.iter().cloned().fold(f64::MAX, f64::min);
            if mn > 0.0 && mx / mn < 2.0 {
                issues.push(serde_json::json!({"severity":"warn","rule":"type.hierarchy","detail":format!("最大/最小字号仅 {:.2}×，层级不足", mx / mn)}));
            }
        }
    }
    if !page_type.is_empty()
        && let Some(p) = picked.iter().find(|p| p.dimension == "donts")
    {
        let mismatched = p.content["donts"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|d| {
                        d["applies_to"]
                            .as_str()
                            .map(|t| t != page_type && t != "any")
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0);
        if mismatched > 0 {
            issues.push(serde_json::json!({"severity":"warn","rule":"donts.context","detail":format!("{mismatched} 条禁忌不适用于 {page_type}")}));
        }
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
                let s = stringify(v);
                let s = if p.polarity == "negative" {
                    s.split('、')
                        .map(|x| {
                            if x.starts_with('禁') || x.starts_with('不') {
                                x.to_string()
                            } else {
                                format!("禁止{x}")
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("；")
                } else {
                    s
                };
                map.insert(format!("{prefix}.{k}"), s);
                // 嵌套对象（如 palette.roles）摊平成 prefix.key.subkey
                if let Some(sub) = v.as_object() {
                    for (sk, sv) in sub {
                        // palette.roles 直接摊成 base.palette.bg，模板里才好写
                        let key = if k == "roles" {
                            format!("{prefix}.{sk}")
                        } else {
                            format!("{prefix}.{k}.{sk}")
                        };
                        map.insert(key, stringify(sv));
                    }
                }
            }
        }
    }
    let mut rendered = template.clone();
    for (k, v) in &map {
        rendered = rendered.replace(&format!("{{{{{k}}}}}"), v);
    }
    // 清掉没解析的占位符，避免把 {{base.palette.bg}} 原样交给生成器
    let mut unresolved = 0;
    while let Some(start) = rendered.find("{{") {
        let Some(end) = rendered[start..].find("}}") else {
            break;
        };
        rendered.replace_range(start..start + end + 2, "（未提供）");
        unresolved += 1;
    }
    if unresolved > 0 {
        issues.push(serde_json::json!({"severity":"error","rule":"prompt.unresolved","detail":format!("{unresolved} 个占位符没有对应字段")}));
    }
    for i in &issues {
        println!(
            "  [{}] {}：{}",
            i["severity"].as_str().unwrap_or("?"),
            i["rule"].as_str().unwrap_or("?"),
            i["detail"].as_str().unwrap_or("")
        );
    }
    let has_error = issues.iter().any(|i| i["severity"] == "error");
    if has_error && !o.force {
        bail!(
            "规格不自洽（{} 条 error），用 --force 可强行成丹",
            issues.iter().filter(|i| i["severity"] == "error").count()
        );
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
        "spec_issues": issues,
        "prompt": rendered,
    });
    conn.execute(
        "INSERT INTO pill(formula_id,recipe_id,output_md,output_json,score_json,cost_json,created_at)
         VALUES(?1,?2,?3,?4,'{}','{\"tokens\":0}',?5)",
        params![o.formula_id, recipe_id, md, output_json.to_string(), now],
    )?;
    let pill_id = conn.last_insert_rowid();
    let score = score_pill(&conn, pill_id)?;
    conn.execute(
        "UPDATE pill SET score_json=?1 WHERE id=?2",
        params![score.to_string(), pill_id],
    )?;

    if let Some(dir) = o.out_dir {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(format!("{pill_id}-{key}.md")), &md)?;
    }
    if o.quiet {
        println!(
            "  · #{} 预检分 {:.2}（{}）",
            pill_id,
            score["total"].as_f64().unwrap_or(0.0),
            key
        );
    } else {
        println!("{md}");
        println!(
            "丹药 #{pill_id} 已入库｜预检分 {}",
            score["total"].as_f64().unwrap_or(0.0)
        );
    }
    Ok(())
}

pub struct MutateOpts<'a> {
    pub db: &'a str,
    pub recipe: &'a Path,
    pub runs: usize,
    pub delta: f64,
    pub seed: u64,
}

pub fn mutate(o: &MutateOpts<'_>) -> Result<()> {
    let text = std::fs::read_to_string(o.recipe)?;
    let base: serde_json::Value = serde_json::from_str(&text).context("配方不是合法 JSON")?;
    let base_key = base["key"].as_str().unwrap_or("recipe").to_string();
    let mut results: Vec<(String, i64, f64, f64)> = Vec::new();
    let mut rng = o.seed.wrapping_mul(6364136223846793005).wrapping_add(1);

    for i in 0..o.runs {
        let mut v = base.clone();
        v["key"] = serde_json::json!(format!("{base_key}.v{i}"));
        let comps = v["components"].as_array_mut().cloned().unwrap_or_default();
        let mut total = 0.0f64;
        let mut mutated = Vec::new();
        for c in comps {
            let mut c = c.clone();
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let u = ((rng >> 11) % 2001) as f64 / 1000.0 - 1.0; // -1..1
            let r = c["ratio"].as_f64().unwrap_or(0.0) * (1.0 + o.delta * u);
            let r = r.max(0.01);
            c["ratio"] = serde_json::json!(r);
            total += r;
            mutated.push(c);
        }
        for c in &mut mutated {
            let r = c["ratio"].as_f64().unwrap_or(0.0) / total;
            c["ratio"] = serde_json::json!((r * 1000.0).round() / 1000.0);
        }
        v["components"] = serde_json::Value::Array(mutated);
        v["name"] = serde_json::json!(format!("{}（变异 v{i}）", v["name"].as_str().unwrap_or("")));

        let tmp = std::env::temp_dir().join(format!("ie-mutate-{}-{i}.json", std::process::id()));
        std::fs::write(&tmp, serde_json::to_string_pretty(&v)?)?;
        let before: i64 =
            Connection::open(o.db)?
                .query_row("SELECT coalesce(max(id),0) FROM pill", [], |r| r.get(0))?;
        brew(&BrewOpts {
            db: o.db,
            recipe: &tmp,
            pinned: None,
            formula_id: None,
            allow_mixed: true,
            force: true,
            out_dir: None,
            quiet: true,
        })?;
        let _ = std::fs::remove_file(&tmp);
        let conn = Connection::open(o.db)?;
        let (pid, score): (i64, String) = conn.query_row(
            "SELECT id, score_json FROM pill WHERE id > ?1 ORDER BY id DESC LIMIT 1",
            params![before],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let s: serde_json::Value = serde_json::from_str(&score).unwrap_or_default();
        let ratios: Vec<String> = v["components"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                format!(
                    "{}={:.2}",
                    c["dimension"].as_str().unwrap_or("?"),
                    c["ratio"].as_f64().unwrap_or(0.0)
                )
            })
            .collect();
        results.push((
            format!("v{i}"),
            pid,
            s["total"].as_f64().unwrap_or(0.0),
            s["quality"].as_f64().unwrap_or(0.0),
        ));
        println!("  v{i}: pill #{pid}  配比 {}", ratios.join(" "));
    }

    println!("\n{:<6} {:<8} {:>6} {:>6}", "变异", "丹药", "总分", "质量");
    let mut best = (String::new(), 0i64, -1.0);
    for (name, pid, total, quality) in &results {
        println!("{:<6} {:<8} {:>6.2} {:>6.2}", name, pid, total, quality);
        if *total > best.2 {
            best = (name.clone(), *pid, *total);
        }
    }
    if !best.0.is_empty() {
        println!("\n最佳变异：{}（丹药 #{}，{:.2}）", best.0, best.1, best.2);
    }
    Ok(())
}

pub struct MotionOpts<'a> {
    pub db: &'a str,
    pub video: &'a Path,
    pub provider: &'a str,
    pub model: &'a str,
    pub base_url: &'a str,
    pub thinking: u32,
}

/// 动效抽取：ffmpeg 抽 3 帧（10%/50%/90%），一次请求让模型比较变化。
pub fn refine_video(o: &MotionOpts<'_>) -> Result<()> {
    if !o.video.exists() {
        bail!("找不到视频 {}", o.video.display());
    }
    let dur_out = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(o.video)
        .output()
        .context("需要 ffprobe")?;
    let dur: f64 = String::from_utf8_lossy(&dur_out.stdout)
        .trim()
        .parse()
        .unwrap_or(3.0);

    let mut frames: Vec<Vec<u8>> = Vec::new();
    for (i, pct) in [0.1f64, 0.5, 0.9].iter().enumerate() {
        let ts = (dur * pct).max(0.0);
        let out = std::env::temp_dir().join(format!("ie-frame-{}-{i}.jpg", std::process::id()));
        let st = std::process::Command::new("ffmpeg")
            .args(["-y", "-ss", &format!("{ts:.2}"), "-i"])
            .arg(o.video)
            .args(["-frames:v", "1", "-q:v", "3"])
            .arg(&out)
            .output()
            .context("需要 ffmpeg")?;
        if !st.status.success() {
            bail!("抽帧失败：{}", String::from_utf8_lossy(&st.stderr));
        }
        let bytes = std::fs::read(&out)?;
        let _ = std::fs::remove_file(&out);
        if bytes.len() > 1000 {
            frames.push(bytes);
        }
    }
    if frames.len() < 2 {
        bail!("抽到的有效帧不足（{}）", frames.len());
    }

    let conn = Connection::open(o.db)?;
    let t = conn.query_row(
        "SELECT id, version, prompt_template FROM technique WHERE key='t.motion'",
        [],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        },
    )?;
    let (tid, tver, prompt) = t;

    let key = if o.provider == "openai" {
        std::env::var("IE_API_KEY").unwrap_or_default()
    } else {
        std::env::var("GEMINI_API_KEY").context("缺少 GEMINI_API_KEY")?
    };
    let body = if o.provider == "openai" {
        let mut content = vec![serde_json::json!({"type":"text","text":prompt})];
        for f in &frames {
            use base64::Engine;
            content.push(serde_json::json!({
                "type":"image_url",
                "image_url":{"url": format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(f))}
            }));
        }
        serde_json::json!({"model":o.model,"messages":[{"role":"user","content":content}],"temperature":0.2,"max_tokens":1200})
    } else {
        use base64::Engine;
        let mut parts = vec![serde_json::json!({"text": prompt})];
        for f in &frames {
            parts.push(serde_json::json!({
                "inline_data": {"mime_type":"image/jpeg","data": base64::engine::general_purpose::STANDARD.encode(f)}
            }));
        }
        serde_json::json!({
            "contents":[{"parts":parts}],
            "generationConfig":{"responseMimeType":"application/json","temperature":0.2,"maxOutputTokens":1200,"thinkingConfig":{"thinkingBudget":o.thinking}}
        })
    };

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()?;
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
    let quality = json["confidence"].as_f64().unwrap_or(0.55).clamp(0.0, 1.0) as f32;
    let hash = hash_of(&format!(
        "{}|t.motion|{}|{}|{}",
        o.video.display(),
        tver,
        o.model,
        json
    ));
    conn.execute(
        "INSERT INTO extract(work_id,technique_id,dimension,content_json,content_md,quality,fire_model,hash,created_at,technique_version)
         VALUES(NULL,?1,'motion',?2,NULL,?3,?4,?5,?6,?7)",
        params![tid, json.to_string(), quality, o.model, hash, crate::now_iso(), tver],
    )?;
    let eid = conn.last_insert_rowid();
    conn.execute(
        "INSERT OR IGNORE INTO inventory(extract_id,added_at) VALUES(?1,?2)",
        params![eid, crate::now_iso()],
    )?;
    println!(
        "  ✓ t.motion「观动」motion 质量 {quality:.2} 火候 {} → 药液 #{eid}",
        o.model
    );
    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

pub struct FormulaOpts<'a> {
    pub db: &'a str,
    pub formula: &'a Path,
    pub recipes_dir: &'a Path,
    pub root: &'a Path,
    pub provider: &'a str,
    pub model: &'a str,
    pub base_url: &'a str,
    pub thinking: u32,
}

pub fn formula_run(o: &FormulaOpts<'_>) -> Result<()> {
    let text = std::fs::read_to_string(o.formula)
        .with_context(|| format!("读不到丹方 {}", o.formula.display()))?;
    let f: serde_json::Value = serde_json::from_str(&text).context("丹方不是合法 JSON")?;
    let key = f["key"].as_str().context("丹方缺 key")?.to_string();
    let name = f["name"].as_str().unwrap_or(&key).to_string();
    let steps = f["steps"].as_array().cloned().unwrap_or_default();
    if steps.is_empty() {
        bail!("丹方没有步骤");
    }

    let conn = Connection::open(o.db)?;
    conn.busy_timeout(Duration::from_secs(10))?;
    conn.execute(
        "INSERT INTO formula(key,name,steps_json,created_at) VALUES(?1,?2,?3,?4)
         ON CONFLICT(key) DO UPDATE SET name=excluded.name, steps_json=excluded.steps_json",
        params![key, name, f["steps"].to_string(), crate::now_iso()],
    )?;
    let fid: i64 = conn.query_row("SELECT id FROM formula WHERE key=?1", params![key], |r| {
        r.get(0)
    })?;
    println!("丹方 {key}「{name}」共 {} 步", steps.len());

    let mut produced: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for (i, step) in steps.iter().enumerate() {
        let sid = step["id"].as_str().unwrap_or("?");
        let op = step["op"].as_str().unwrap_or("");
        println!("[{}/{}] {} {}", i + 1, steps.len(), sid, op);
        match op {
            "refine" => {
                let work = step["material"].as_i64().context("refine 缺 material")?;
                let technique = step["technique"].as_str().context("refine 缺 technique")?;
                refine(&RefineOpts {
                    db: o.db,
                    root: o.root,
                    work,
                    techniques: vec![technique.to_string()],
                    provider: o.provider,
                    model: o.model,
                    base_url: o.base_url,
                    thinking: o.thinking,
                    to_bag: true,
                    refresh: false,
                })?;
                let tid: i64 = conn.query_row(
                    "SELECT id FROM technique WHERE key=?1",
                    params![technique],
                    |r| r.get(0),
                )?;
                let eid: Option<i64> = conn
                    .query_row(
                        "SELECT id FROM extract WHERE work_id=?1 AND technique_id=?2 ORDER BY id DESC LIMIT 1",
                        params![work, tid],
                        |r| r.get(0),
                    )
                    .ok();
                if let Some(eid) = eid {
                    produced.insert(sid.to_string(), eid);
                } else {
                    println!(
                        "  ! {} 没炼出药液（该图无此维度），后续步骤会回落到背包选药",
                        sid
                    );
                }
            }
            "brew" => {
                let rkey = step["recipe"].as_str().context("brew 缺 recipe")?;
                let path = find_recipe(o.recipes_dir, rkey)?;
                let mut pinned = std::collections::HashMap::new();
                for input in step["inputs"].as_array().cloned().unwrap_or_default() {
                    if let Some(eid) = input.as_str().and_then(|s| produced.get(s)).copied() {
                        let dim: String = conn.query_row(
                            "SELECT dimension FROM extract WHERE id=?1",
                            params![eid],
                            |r| r.get(0),
                        )?;
                        pinned.insert(dim, eid);
                    }
                }
                brew(&BrewOpts {
                    db: o.db,
                    recipe: &path,
                    pinned: Some(&pinned),
                    formula_id: Some(fid),
                    allow_mixed: false,
                    force: false,
                    out_dir: None,
                    quiet: false,
                })?;
            }
            "render" => println!("  （brew 已输出 markdown，render 步骤先空转）"),
            other => bail!("未知步骤类型 {other}"),
        }
    }
    println!("丹方 {key} 执行完毕");
    Ok(())
}

fn find_recipe(dir: &Path, key: &str) -> Result<PathBuf> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("读不到 {}", dir.display()))? {
        let p = entry?.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(s) = std::fs::read_to_string(&p)
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&s)
            && v["key"].as_str() == Some(key)
        {
            return Ok(p);
        }
    }
    bail!("在 {} 里找不到 key={key} 的配方", dir.display())
}

/// 丹药预检评分：只衡量"这颗丹药是否合乎配方、材料是否够用"，不衡量美。
pub fn score_pill(conn: &Connection, pill_id: i64) -> Result<serde_json::Value> {
    let (recipe_id, output_json): (Option<i64>, String) = conn.query_row(
        "SELECT recipe_id, output_json FROM pill WHERE id=?1",
        params![pill_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let out: serde_json::Value = serde_json::from_str(&output_json).unwrap_or_default();
    let comps = out["components"].as_array().cloned().unwrap_or_default();
    let conflicts = out["conflicts"].as_array().cloned().unwrap_or_default();
    let prompt = out["prompt"].as_str().unwrap_or("");

    // 必需槽位是否齐全
    let (mut req_total, mut req_have) = (0usize, 0usize);
    if let Some(rid) = recipe_id {
        let mut stmt = conn.prepare(
            "SELECT slot, dimension FROM recipe_component WHERE recipe_id=?1 AND required=1",
        )?;
        let req: Vec<(String, String)> = stmt
            .query_map(params![rid], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        req_total = req.len();
        for (slot, dim) in &req {
            if comps
                .iter()
                .any(|c| c["slot"] == *slot && c["dimension"] == *dim)
            {
                req_have += 1;
            }
        }
    }
    let completeness = if req_total == 0 {
        1.0
    } else {
        req_have as f64 / req_total as f64
    };

    let weight_sum: f64 = comps.iter().filter_map(|c| c["ratio"].as_f64()).sum();
    let quality: f64 = if weight_sum > 0.0 {
        comps
            .iter()
            .filter_map(|c| Some(c["ratio"].as_f64()? * c["quality"].as_f64()?))
            .sum::<f64>()
            / weight_sum
    } else {
        0.0
    };

    let materials: std::collections::BTreeSet<&str> = comps
        .iter()
        .filter_map(|c| c["material"].as_str())
        .collect();
    let diversity = if comps.is_empty() {
        0.0
    } else {
        (materials.len() as f64 / comps.len() as f64).min(1.0)
    };

    let ratio_ok = (1.0 - (weight_sum - 1.0).abs()).clamp(0.0, 1.0);
    let prompt_ok = if !prompt.contains("{{") && prompt.chars().count() >= 40 {
        1.0
    } else {
        0.0
    };
    let errors = conflicts
        .iter()
        .filter(|c| c["severity"] == "error")
        .count() as f64;
    let warns = conflicts.iter().filter(|c| c["severity"] == "warn").count() as f64;

    let total = (0.25 * quality
        + 0.30 * completeness
        + 0.15 * diversity
        + 0.15 * ratio_ok
        + 0.15 * prompt_ok
        - 0.5 * errors
        - 0.1 * warns)
        .clamp(0.0, 1.0);

    Ok(serde_json::json!({
        "total": (total * 1000.0).round() / 1000.0,
        "quality": (quality * 1000.0).round() / 1000.0,
        "completeness": (completeness * 1000.0).round() / 1000.0,
        "diversity": (diversity * 1000.0).round() / 1000.0,
        "ratio_ok": (ratio_ok * 1000.0).round() / 1000.0,
        "prompt_ok": prompt_ok,
        "errors": errors as i64,
        "warns": warns as i64,
        "note": "预检分：衡量合规与材料可用性，不衡量美"
    }))
}

pub fn pills(db: &str, limit: usize) -> Result<()> {
    let conn = Connection::open(db)?;
    let mut stmt = conn.prepare(
        "SELECT p.id, coalesce(r.key,'-'), p.score_json, length(p.output_md), p.created_at
         FROM pill p LEFT JOIN recipe r ON r.id=p.recipe_id ORDER BY p.id DESC LIMIT ?1",
    )?;
    let rows: Vec<(i64, String, String, i64, String)> = stmt
        .query_map(params![limit as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    println!(
        "{:<5} {:<26} {:>6} {:>6} {:>6} {:>5} {:>5}",
        "id", "配方", "总分", "质量", "完整", "冲突", "长度"
    );
    for (id, recipe, score, len, _at) in rows {
        let s: serde_json::Value = serde_json::from_str(&score).unwrap_or_default();
        let g = |k: &str| s[k].as_f64().unwrap_or(0.0);
        println!(
            "{:<5} {:<26} {:>6.2} {:>6.2} {:>6.2} {:>6} {:>6}",
            id,
            recipe,
            g("total"),
            g("quality"),
            g("completeness"),
            s["errors"].as_i64().unwrap_or(0),
            len
        );
    }
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

fn stringify(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Array(a) => a
            .iter()
            .map(|x| {
                if let (Some(hex), Some(share)) = (
                    x.get("hex").and_then(|h| h.as_str()),
                    x.get("share").and_then(|s| s.as_f64()),
                ) {
                    format!("{hex} {:.0}%", share * 100.0)
                } else if let Some(name) = x.get("name").and_then(|n| n.as_str()) {
                    format!(
                        "{} {}px/{}",
                        name,
                        x["size_px"].as_f64().unwrap_or(0.0),
                        x["weight"].as_i64().unwrap_or(400)
                    )
                } else if let Some(text) = x.get("text").and_then(|n| n.as_str()) {
                    text.to_string()
                } else {
                    stringify(x)
                }
            })
            .collect::<Vec<_>>()
            .join("、"),
        serde_json::Value::Object(o) => o
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join("、"),
        other => other.to_string(),
    }
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
