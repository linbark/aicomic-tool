//! 语义抽取：调视觉模型把作品读成「手法 + 基元」。
//!
//! 只允许模型使用词表里已有的 term，未知 term 直接丢弃——防止视觉模型把
//! 自由标签带进库，这是标签汤最常见的入口。
//!
//! 密钥只从环境变量 `GEMINI_API_KEY` 读取，不写库、不落盘、不打印。

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const EXTRACTOR: &str = "vision-llm";

struct Term {
    id: i64,
    term: String,
    label: String,
    hint: String,
}

pub struct Opts<'a> {
    pub db: &'a str,
    pub root: &'a Path,
    pub model: &'a str,
    pub limit: Option<usize>,
    pub base_url: &'a str,
    pub thinking: u32,
    pub rpm: u32,
    pub shard: (usize, usize),
    pub finds_first: bool,
}

pub fn run(opts: &Opts<'_>) -> Result<()> {
    let Opts {
        db,
        root,
        model,
        limit,
        base_url,
        thinking,
        rpm,
        shard,
        finds_first,
    } = *opts;
    let key = std::env::var("GEMINI_API_KEY")
        .context("缺少环境变量 GEMINI_API_KEY（只在运行时读取，不写入任何文件）")?;
    let conn = Connection::open(db)?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    conn.busy_timeout(std::time::Duration::from_secs(30))?;

    let devices = load_terms(&conn, "device", "ui")?;
    let prims = load_terms(&conn, "primitive", "core")?;
    if devices.is_empty() || prims.is_empty() {
        return Err(anyhow!("词表为空，先应用 packs"));
    }
    let prompt = build_prompt(&devices, &prims);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()?;

    let order = if finds_first {
        "CASE WHEN w.collection = 'finds' THEN 0 ELSE 1 END, (a.id * 2654435761) % 997"
    } else {
        "(a.id * 2654435761) % 997"
    };
    let sql = format!(
        "SELECT a.id, a.work_id, a.local_path FROM asset a JOIN work w ON w.id = a.work_id
         WHERE a.local_path IS NOT NULL
           AND (?3 = 1 OR (a.id % ?4) = ?5)
           AND NOT EXISTS (SELECT 1 FROM observation o
                           WHERE o.asset_id = a.id AND o.extractor = ?1)
         ORDER BY {order}
         LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let jobs: Vec<(i64, i64, PathBuf)> = stmt
        .query_map(
            params![
                EXTRACTOR,
                limit.unwrap_or(1000) as i64,
                if shard.1 <= 1 { 1 } else { 0 },
                shard.1.max(1) as i64,
                shard.0 as i64
            ],
            |r| Ok((r.get(0)?, r.get(1)?, root.join(r.get::<_, String>(2)?))),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    println!(
        "待语义抽取：{}（模型 {model}，节流 {rpm}/min，thinking {thinking}）",
        jobs.len()
    );
    if jobs.is_empty() {
        return Ok(());
    }

    let dev_index: HashMap<&str, i64> = devices.iter().map(|d| (d.term.as_str(), d.id)).collect();
    let prim_index: HashMap<&str, i64> = prims.iter().map(|d| (d.term.as_str(), d.id)).collect();

    let now = crate::now_iso();
    let mut ok = 0usize;
    let mut failed = 0usize;
    let mut tokens = 0i64;
    let mut dev_rows = 0usize;
    let mut prim_rows = 0usize;
    let mut limited: Option<String> = None;
    let started = Instant::now();
    let interval = Duration::from_millis(60_000 / rpm.max(1) as u64);

    // 免费额度是「每模型每分钟 N 次」，所以串行 + 节流；并发只会换来 429。
    for (asset_id, work_id, path) in &jobs {
        if let Some(body) = &limited {
            println!("已停止：{body}");
            break;
        }
        std::thread::sleep(interval);
        let bytes = match prepare_image(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("读取/缩放失败 asset {asset_id}: {e}");
                failed += 1;
                continue;
            }
        };
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let body = request_body(&prompt, &b64, "image/jpeg", thinking);

        let mut attempt = 0;
        loop {
            match call(&client, base_url, model, &key, &body) {
                CallOutcome::Ok(json, used) => {
                    tokens += used;
                    let parsed = match parse(&json, &dev_index, &prim_index) {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("  ✗ 解析 asset {asset_id}: {e}");
                            failed += 1;
                            break;
                        }
                    };
                    let tx = conn.unchecked_transaction()?;
                    tx.execute(
                        "INSERT INTO observation(asset_id, extractor, extractor_version, payload_json, confidence, created_at)
                         VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                        params![asset_id, EXTRACTOR, model, serde_json::to_string(&json)?, parsed.confidence, now],
                    )?;
                    for (term, conf, evidence) in &parsed.devices {
                        if let Some(id) = dev_index.get(term.as_str()) {
                            tx.execute(
                                "INSERT OR IGNORE INTO device_usage(work_id, device_term_id, strength, note)
                                 VALUES(?1, ?2, ?3, ?4)",
                                params![work_id, id, conf, evidence],
                            )?;
                            dev_rows += 1;
                        }
                    }
                    for (term, score, conf) in &parsed.primitives {
                        if let Some(id) = prim_index.get(term.as_str()) {
                            tx.execute(
                                "INSERT INTO work_primitive(work_id, primitive_term_id, score, confidence, source, extractor_version, updated_at)
                                 VALUES(?1, ?2, ?3, ?4, 'vision', ?5, ?6)
                                 ON CONFLICT(work_id, primitive_term_id) DO UPDATE SET
                                   score = (work_primitive.score * work_primitive.confidence + excluded.score * excluded.confidence)
                                           / nullif(work_primitive.confidence + excluded.confidence, 0),
                                   confidence = min(0.95, max(work_primitive.confidence, excluded.confidence) * 1.15),
                                   source = CASE WHEN work_primitive.source = excluded.source THEN excluded.source ELSE 'mixed' END,
                                   extractor_version = excluded.extractor_version,
                                   updated_at = excluded.updated_at",
                                params![work_id, id, score, conf, model, now],
                            )?;
                            prim_rows += 1;
                        }
                    }
                    tx.commit()?;
                    ok += 1;
                    println!(
                        "  ✓ {ok}/{} asset {asset_id} 手法{} 基元{} 累计 {tokens} tok",
                        jobs.len(),
                        parsed.devices.len(),
                        parsed.primitives.len()
                    );
                    break;
                }
                CallOutcome::RateLimited(info) => {
                    if info.per_day {
                        limited = Some(info.body);
                        break;
                    }
                    attempt += 1;
                    if attempt > 5 {
                        limited = Some(format!("{}（重试 {} 次仍限流）", info.body, attempt));
                        break;
                    }
                    let wait = info.retry_after.max(5.0) + 1.0;
                    println!("  … 限流，等 {wait:.0}s 后重试（第 {attempt} 次）");
                    std::thread::sleep(Duration::from_secs_f32(wait));
                }
                CallOutcome::Failed(e) => {
                    failed += 1;
                    eprintln!("  ✗ asset {asset_id}: {e}");
                    break;
                }
            }
        }
    }

    println!(
        "完成：成功 {ok} 失败 {failed}，token {tokens}，用时 {:.1}s，手法 {dev_rows} 条、基元 {prim_rows} 条",
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

struct LimitInfo {
    body: String,
    retry_after: f32,
    per_day: bool,
}

fn parse_limit(text: &str) -> LimitInfo {
    let v: serde_json::Value = serde_json::from_str(text).unwrap_or(serde_json::Value::Null);
    let msg = v
        .pointer("/error/message")
        .and_then(|x| x.as_str())
        .unwrap_or(text);
    let retry_after = msg
        .split("Please retry in ")
        .nth(1)
        .and_then(|s| s.split('s').next())
        .and_then(|s| s.trim().parse::<f32>().ok())
        .or_else(|| {
            v.pointer("/error/details")
                .and_then(|d| d.as_array())
                .and_then(|a| {
                    a.iter().find_map(|e| {
                        e.get("retryDelay")
                            .and_then(|r| r.as_str())
                            .and_then(|s| s.trim_end_matches('s').parse::<f32>().ok())
                    })
                })
        })
        .unwrap_or(20.0);
    let per_day = msg.contains("PerDay") || msg.contains("per day");
    LimitInfo {
        body: msg.chars().take(300).collect(),
        retry_after,
        per_day,
    }
}

enum CallOutcome {
    Ok(serde_json::Value, i64),
    RateLimited(LimitInfo),
    Failed(String),
}

struct Parsed {
    devices: Vec<(String, f32, String)>,
    primitives: Vec<(String, f32, f32)>,
    confidence: f32,
}

fn parse(
    json: &serde_json::Value,
    dev_index: &HashMap<&str, i64>,
    prim_index: &HashMap<&str, i64>,
) -> Result<Parsed> {
    let mut devices = Vec::new();
    let mut prims = Vec::new();
    let mut confs = Vec::new();
    if let Some(arr) = json.get("devices").and_then(|v| v.as_array()) {
        for d in arr {
            let term = d.get("term").and_then(|v| v.as_str()).unwrap_or_default();
            if !dev_index.contains_key(term) {
                continue; // 词表外的 term 一律丢弃
            }
            let conf = d.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let ev = d
                .get("evidence")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            confs.push(conf);
            devices.push((term.to_string(), conf.clamp(0.0, 1.0), ev));
        }
    }
    if let Some(arr) = json.get("primitives").and_then(|v| v.as_array()) {
        for p in arr {
            let term = p.get("term").and_then(|v| v.as_str()).unwrap_or_default();
            if !prim_index.contains_key(term) {
                continue;
            }
            let score = p.get("score").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let conf = p.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            prims.push((
                term.to_string(),
                score.clamp(0.0, 1.0),
                conf.clamp(0.0, 1.0),
            ));
        }
    }
    let confidence = if confs.is_empty() {
        0.5
    } else {
        confs.iter().sum::<f32>() / confs.len() as f32
    };
    Ok(Parsed {
        devices,
        primitives: prims,
        confidence,
    })
}

fn load_terms(conn: &Connection, kind: &str, pack: &str) -> Result<Vec<Term>> {
    let mut stmt = conn.prepare(
        "SELECT id, term, label, definition FROM vocab_term
         WHERE kind=?1 AND status='active' AND pack_id=?2 ORDER BY id",
    )?;
    let rows = stmt
        .query_map(params![kind, pack], |r| {
            Ok(Term {
                id: r.get(0)?,
                term: r.get(1)?,
                label: r.get(2)?,
                hint: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn clip(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n {
        s.to_string()
    } else {
        format!("{}…", chars[..n].iter().collect::<String>())
    }
}

fn build_prompt(devices: &[Term], prims: &[Term]) -> String {
    let mut s = String::new();
    s.push_str(
        "你是设计分析器。看一张作品图，只做两件事：\n\
         1) 判断它用了哪些「手法」（device）；\n\
         2) 给它在各个「基元」（primitive）上的强度打分。\n\n\
         硬规则：\n\
         - term 只能从下面的列表里选，不得自造；\n\
         - 宁可少报：只报你有把握的，手法最多 6 个，基元最多 8 个；\n\
         - score 与 confidence 都是 0–1；\n\
         - evidence 用一句中文，指出图上的具体证据（位置、比例、颜色、元素），不要空话；\n\
         - summary 一句话概括版式，不要评价好坏。\n\n",
    );
    s.push_str("【可选手法】\n");
    for d in devices {
        s.push_str(&format!(
            "- {}「{}」{}\n",
            d.term,
            d.label,
            clip(&d.hint, 34)
        ));
    }
    s.push_str("\n【可选基元】\n");
    for p in prims {
        s.push_str(&format!(
            "- {}「{}」{}\n",
            p.term,
            p.label,
            clip(&p.hint, 32)
        ));
    }
    s
}

fn response_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "OBJECT",
        "properties": {
            "summary": {"type": "STRING"},
            "devices": {"type": "ARRAY", "items": {"type": "OBJECT", "properties": {
                "term": {"type": "STRING"},
                "confidence": {"type": "NUMBER"},
                "evidence": {"type": "STRING"}
            }, "required": ["term", "confidence", "evidence"]}},
            "primitives": {"type": "ARRAY", "items": {"type": "OBJECT", "properties": {
                "term": {"type": "STRING"},
                "score": {"type": "NUMBER"},
                "confidence": {"type": "NUMBER"}
            }, "required": ["term", "score", "confidence"]}}
        },
        "required": ["summary", "devices", "primitives"]
    })
}

fn request_body(prompt: &str, b64: &str, mime: &str, thinking: u32) -> serde_json::Value {
    serde_json::json!({
        "contents": [{"parts": [
            {"text": prompt},
            {"inline_data": {"mime_type": mime, "data": b64}}
        ]}],
        "generationConfig": {
            "responseMimeType": "application/json",
            "responseSchema": response_schema(),
            "temperature": 0.2,
            "maxOutputTokens": 1024,
            "thinkingConfig": {"thinkingBudget": thinking}
        }
    })
}

/// 缩到 768px 宽再发：原图 1200×675 约 1100 image token，768 宽约 300–400，
/// 版式判断不受影响，成本和延迟都降一档。
fn prepare_image(path: &Path) -> Result<Vec<u8>> {
    let img = image::ImageReader::open(path)?
        .with_guessed_format()?
        .decode()?;
    let (w, h) = (img.width(), img.height());
    let img = if w > 768 {
        let nh = ((h as f32) * 768.0 / w as f32).max(1.0) as u32;
        img.resize_exact(768, nh, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut buf = Vec::with_capacity(64 * 1024);
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 82);
    enc.encode_image(&img.to_rgb8())?;
    Ok(buf)
}

fn call(
    client: &reqwest::blocking::Client,
    base_url: &str,
    model: &str,
    key: &str,
    body: &serde_json::Value,
) -> CallOutcome {
    let url = format!("{base_url}/v1beta/models/{model}:generateContent");
    let mut last = String::new();
    for attempt in 0..4 {
        match client
            .post(&url)
            .header("x-goog-api-key", key)
            .json(body)
            .send()
        {
            Ok(r) => {
                let status = r.status();
                let text = r.text().unwrap_or_default();
                if status.is_success() {
                    let v: serde_json::Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(e) => return CallOutcome::Failed(format!("响应非 JSON: {e}")),
                    };
                    let used = v
                        .pointer("/usageMetadata/totalTokenCount")
                        .and_then(|x| x.as_i64())
                        .unwrap_or(0);
                    let content = match v
                        .pointer("/candidates/0/content/parts/0/text")
                        .and_then(|x| x.as_str())
                    {
                        Some(c) => c,
                        None => {
                            return CallOutcome::Failed(format!(
                                "响应里没有文本：{}",
                                &text[..text.len().min(200)]
                            ));
                        }
                    };
                    return match serde_json::from_str::<serde_json::Value>(content) {
                        Ok(p) => CallOutcome::Ok(p, used),
                        Err(e) => CallOutcome::Failed(format!(
                            "模型输出不是 JSON（{e}）：{}",
                            &content[..content.len().min(200)]
                        )),
                    };
                }
                if status.as_u16() == 429 {
                    return CallOutcome::RateLimited(parse_limit(&text));
                }
                last = format!("HTTP {status}: {}", &text[..text.len().min(200)]);
                if status.as_u16() >= 500 {
                    std::thread::sleep(Duration::from_secs(2 + attempt * 3));
                    continue;
                }
                break;
            }
            Err(e) => {
                last = e.to_string();
                std::thread::sleep(Duration::from_secs(1 + attempt));
            }
        }
    }
    CallOutcome::Failed(last)
}
