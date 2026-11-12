//! 多裁判合议：同一批丹药，多个裁判各自成对比较，再按规则仲裁。
//!
//! 规则（先定规则，再写代码）：
//! 1. 每对比较按裁判权重加权平均，得到聚合胜率；
//! 2. 若两个裁判方向相反（一个判 1 赢、一个判 2 赢）→ 该对标记「分歧」；
//! 3. 存在分歧，或聚合胜率落在 0.4–0.6 之间 → 涉及条目转人工；
//! 4. 其余情况用 Bradley-Terry 拟合相对强弱，正常出分。
//!
//! 这条规则不假装能消解分歧：它只负责把「裁判打架」变成「人工任务」。

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::time::Duration;

use crate::vision::{CallOutcome, call, call_openai, openai_text_body};

#[derive(Clone)]
struct Judge {
    name: String,
    provider: String,
    model: String,
    base_url: String,
    rpm: u32,
    weight: f64,
}

pub struct PanelOpts<'a> {
    pub db: &'a str,
    pub pills: Vec<i64>,
    pub judges: Vec<String>,
    pub cfg_path: &'a std::path::Path,
    pub repeats: usize,
    pub dimension: String,
    pub seed: u64,
}

struct Cand {
    pill_id: i64,
    label: String,
    precheck: f64,
    prompt: String,
}

pub fn run(o: &PanelOpts<'_>) -> Result<()> {
    if o.pills.len() < 2 {
        bail!("至少两颗丹药");
    }
    let cfg_text = std::fs::read_to_string(o.cfg_path)
        .with_context(|| format!("读不到裁判配置 {}", o.cfg_path.display()))?;
    let all: Vec<Value> = serde_json::from_str(&cfg_text).context("裁判配置不是 JSON 数组")?;
    let mut judges: Vec<Judge> = Vec::new();
    for want in &o.judges {
        let v = all
            .iter()
            .find(|j| j["name"].as_str() == Some(want.as_str()))
            .with_context(|| format!("配置里没有裁判 {want}"))?;
        judges.push(Judge {
            name: want.clone(),
            provider: v["provider"].as_str().unwrap_or("gemini").to_string(),
            model: v["model"].as_str().unwrap_or("").to_string(),
            base_url: v["base_url"].as_str().unwrap_or("").to_string(),
            rpm: v["rpm"].as_u64().unwrap_or(0) as u32,
            weight: v["weight"].as_f64().unwrap_or(1.0),
        });
    }
    if judges.is_empty() {
        bail!("没有可用裁判");
    }

    // 载入候选并抹掉材料名（盲评）
    let conn = Connection::open(o.db)?;
    conn.busy_timeout(Duration::from_secs(10))?;

    // 若人工校准过，用库里的权重覆盖配置
    for j in &mut judges {
        if let Ok(w) = conn.query_row(
            "SELECT weight FROM judge_model WHERE name=?1",
            params![j.name],
            |r| r.get::<_, f64>(0),
        ) {
            j.weight = w;
        }
    }
    let mut cands: Vec<Cand> = Vec::new();
    let mut names: std::collections::BTreeSet<String> = Default::default();
    for pid in &o.pills {
        let (score_json, out_json): (String, String) = conn
            .query_row(
                "SELECT score_json, output_json FROM pill WHERE id=?1",
                params![pid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .with_context(|| format!("没有丹药 #{pid}"))?;
        let s: Value = serde_json::from_str(&score_json).unwrap_or_default();
        let out: Value = serde_json::from_str(&out_json).unwrap_or_default();
        if s["errors"].as_i64().unwrap_or(0) > 0 {
            println!("  跳过 #{pid}：预检有 error");
            continue;
        }
        for c in out["components"].as_array().cloned().unwrap_or_default() {
            if let Some(m) = c["material"].as_str()
                && m != "?"
            {
                names.insert(m.to_string());
            }
        }
        cands.push(Cand {
            pill_id: *pid,
            label: String::new(),
            precheck: s["total"].as_f64().unwrap_or(0.0),
            prompt: out["prompt"].as_str().unwrap_or("").to_string(),
        });
    }
    for c in &mut cands {
        for n in &names {
            c.prompt = c.prompt.replace(n.as_str(), "（某作品）");
        }
    }
    if cands.len() < 2 {
        bail!("通过预检的丹药不足两颗");
    }

    let mut rng = o.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let mut order: Vec<usize> = (0..cands.len()).collect();
    for i in (1..order.len()).rev() {
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = ((rng >> 11) as usize) % (i + 1);
        order.swap(i, j);
    }
    let labels = ["A", "B", "C", "D", "E", "F"];
    for (pos, idx) in order.iter().enumerate() {
        cands[*idx].label = labels[pos].to_string();
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()?;
    let criterion = match o.dimension.as_str() {
        "hierarchy" => "只看层级是否清楚",
        "consistency" => "只看体系是否自洽",
        "restraint" => "只看是否克制、有没有堆砌",
        "executability" => "只看能否被直接执行",
        _ => "综合层级、自洽、克制、可执行",
    };

    // 每对 × 每裁判 → 加权胜率
    struct PairVote {
        i: usize,
        j: usize,
        votes: Vec<(String, f64)>, // (裁判名, 对 i 的胜率)
        disputed: bool,
        combined: f64,
    }
    let mut pairs: Vec<PairVote> = Vec::new();
    for i in 0..cands.len() {
        for j in (i + 1)..cands.len() {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let flip = (rng >> 11) % 2 == 1;
            let (left, right) = if flip { (j, i) } else { (i, j) };
            let prompt = format!(
                "你是设计评审。下面两份设计规格，只有文字。\n\n【候选 1】\n{}\n\n【候选 2】\n{}\n\n问题：{}，哪一份更好？\n只输出 JSON：{{\"winner\":\"1\"|\"2\"|\"tie\",\"reason\":\"一句话\"}}，不要解释。",
                cands[left].prompt, cands[right].prompt, criterion
            );
            let mut votes = Vec::new();
            for jd in &judges {
                let key = if jd.provider == "openai" {
                    std::env::var("IE_API_KEY").unwrap_or_default()
                } else {
                    std::env::var("GEMINI_API_KEY").unwrap_or_default()
                };
                let mut wins = 0.0;
                for _ in 0..o.repeats.max(1) {
                    if jd.rpm > 0 {
                        std::thread::sleep(Duration::from_millis(60_000 / jd.rpm as u64));
                    }
                    let body = if jd.provider == "openai" {
                        openai_text_body(&prompt, &jd.model, 300)
                    } else {
                        json!({
                            "contents":[{"parts":[{"text": prompt}]}],
                            "generationConfig":{"responseMimeType":"application/json","temperature":0.2,"maxOutputTokens":300,"thinkingConfig":{"thinkingBudget":0}}
                        })
                    };
                    let outcome = if jd.provider == "openai" {
                        call_openai(&client, &jd.base_url, &key, &body)
                    } else {
                        call(&client, &jd.base_url, &jd.model, &key, &body)
                    };
                    let w = match outcome {
                        CallOutcome::Ok(v, _) => match v["winner"].as_str().unwrap_or("tie") {
                            "1" => 1.0,
                            "2" => 0.0,
                            _ => 0.5,
                        },
                        CallOutcome::RateLimited(i) => bail!("{} 限流：{}", jd.name, i.body),
                        CallOutcome::Failed(e) => bail!("{} 调用失败：{e}", jd.name),
                    };
                    wins += if flip { 1.0 - w } else { w };
                }
                votes.push((jd.name.clone(), wins / o.repeats.max(1) as f64));
            }
            let wsum: f64 = judges.iter().map(|j| j.weight).sum();
            let combined: f64 = votes
                .iter()
                .zip(&judges)
                .map(|((_, v), j)| v * j.weight)
                .sum::<f64>()
                / wsum;
            // 裁决分三档：赢 / 平 / 输。只要裁判之间的裁决档位不一致，就算分歧。
            // 硬分歧 = 有人判赢有人判输；软分歧 = 有人判平有人判赢（或输）。
            let verdict = |v: f64| -> i8 {
                if v > 0.5 + 1e-9 {
                    1
                } else if v < 0.5 - 1e-9 {
                    -1
                } else {
                    0
                }
            };
            let verdicts: Vec<i8> = votes.iter().map(|(_, v)| verdict(*v)).collect();
            let disputed = !verdicts.iter().all(|x| *x == verdicts[0]);
            pairs.push(PairVote {
                i,
                j,
                votes,
                disputed,
                combined,
            });
        }
    }

    // Bradley-Terry（聚合胜率）
    let n = cands.len();
    let flat: Vec<(usize, usize, f64)> = pairs.iter().map(|p| (p.i, p.j, p.combined)).collect();
    let mut s = vec![1.0f64; n];
    for _ in 0..300 {
        let mut next = vec![0.0f64; n];
        for (k, slot) in next.iter_mut().enumerate() {
            let (mut w, mut denom) = (0.5f64, 1.0 / (s[k] + 1.0));
            for (a, b, wa) in &flat {
                if *a == k {
                    w += wa;
                    denom += 1.0 / (s[*a] + s[*b]);
                } else if *b == k {
                    w += 1.0 - wa;
                    denom += 1.0 / (s[*a] + s[*b]);
                }
            }
            *slot = if denom > 0.0 { w / denom } else { 1.0 };
        }
        let mx = next.iter().cloned().fold(0.0f64, f64::max).max(1e-9);
        s = next.iter().map(|x| x / mx).collect();
    }
    let totals: Vec<f64> = s.iter().map(|x| (x * 10000.0).round() / 100.0).collect();

    let now = crate::now_iso();
    let judge_json = json!({
        "method": "panel+bt",
        "judges": judges.iter().map(|j| json!({"name": j.name, "weight": j.weight})).collect::<Vec<_>>(),
        "pairs": pairs.iter().map(|p| json!({
            "i": cands[p.i].label, "j": cands[p.j].label,
            "votes": p.votes, "combined": p.combined, "disputed": p.disputed
        })).collect::<Vec<_>>(),
        "strengths": totals,
    });
    conn.execute(
        "INSERT INTO judge_run(status,stage,input_json,precheck_json,judge_json,created_at,updated_at)
         VALUES('judging','panel',?1,?2,?3,?4,?4)",
        params![
            json!({"pills": o.pills, "seed": o.seed, "judges": o.judges}).to_string(),
            json!(cands.iter().map(|c| json!({"pill": c.pill_id, "precheck": c.precheck})).collect::<Vec<_>>()).to_string(),
            judge_json.to_string(),
            now
        ],
    )?;
    let run_id = conn.last_insert_rowid();

    let mut disputed_ids: std::collections::BTreeSet<i64> = Default::default();
    for p in &pairs {
        if p.disputed || (p.combined > 0.4 && p.combined < 0.6) {
            disputed_ids.insert(cands[p.i].pill_id);
            disputed_ids.insert(cands[p.j].pill_id);
        }
    }

    for (idx, c) in cands.iter().enumerate() {
        let total = totals[idx];
        let needs_human = disputed_ids.contains(&c.pill_id) || total < 40.0;
        let status = if needs_human {
            "human_needed"
        } else {
            "machine_done"
        };
        conn.execute(
            "INSERT INTO judge_item(run_id,pill_id,label,display_order,precheck,machine_json,machine_total,status)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                run_id,
                c.pill_id,
                c.label,
                idx as i64,
                c.precheck,
                json!({"strength": s[idx], "total": total}).to_string(),
                total,
                status
            ],
        )?;
    }
    let pending: i64 = conn.query_row(
        "SELECT count(*) FROM judge_item WHERE run_id=?1 AND status='human_needed'",
        params![run_id],
        |r| r.get(0),
    )?;
    let status = if pending > 0 {
        "awaiting_human"
    } else {
        "done"
    };
    conn.execute(
        "UPDATE judge_run SET status=?1, updated_at=?2 WHERE id=?3",
        params![status, crate::now_iso(), run_id],
    )?;
    crate::judge::recompute(&conn, run_id)?;

    println!(
        "合议 run #{run_id}（{} 位裁判：{}，seed {}）",
        judges.len(),
        judges
            .iter()
            .map(|j| format!("{}×{:.2}", j.name, j.weight))
            .collect::<Vec<_>>()
            .join("、"),
        o.seed
    );
    for p in &pairs {
        let votes: Vec<String> = p.votes.iter().map(|(n, v)| format!("{n}={v:.2}")).collect();
        println!(
            "  {} vs {} → 聚合 {:.2}  {}{}",
            cands[p.i].label,
            cands[p.j].label,
            p.combined,
            votes.join(" "),
            if p.disputed { "  ⚠ 裁判分歧" } else { "" }
        );
    }
    println!("{:<6} {:<6} {:>8} {:>7}", "丹药", "标签", "合议分", "预检");
    for (idx, c) in cands.iter().enumerate() {
        println!(
            "{:<6} {:<6} {:>8.1} {:>7.3}",
            c.pill_id, c.label, totals[idx], c.precheck
        );
    }
    println!(
        "分歧/低分条目：{} 条（ie judge review --run {run_id}）",
        pending
    );
    Ok(())
}
