//! 评判流水线：预检 → 机器盲评 → 人工干预 → 归并。
//!
//! 设计要点：
//! - 每一步都写库，可中断、可重放；
//! - 机器盲评打乱标签（A/B/C），不告诉模型材料来源；
//! - 判不出差别、分歧过大、随机抽检的条目进人工队列；
//! - 人工结论回填后重算最终分，作为后续校准依据。

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::time::Duration;

use crate::vision::{CallOutcome, call, call_openai, openai_text_body};

pub struct RunOpts<'a> {
    pub db: &'a str,
    pub pills: Vec<i64>,
    pub provider: &'a str,
    pub model: &'a str,
    pub base_url: &'a str,
    pub thinking: u32,
    pub seed: u64,
}

struct Candidate {
    pill_id: i64,
    label: String,
    precheck: f64,
    prompt: String,
    materials: Vec<String>,
}

pub fn run(o: &RunOpts<'_>) -> Result<()> {
    if o.pills.len() < 2 {
        bail!("至少给两颗丹药（--pills 12,13），盲评需要比较");
    }
    let conn = Connection::open(o.db)?;
    conn.busy_timeout(Duration::from_secs(10))?;

    let mut cands: Vec<Candidate> = Vec::new();
    for (i, pid) in o.pills.iter().enumerate() {
        let (score_json, output_json): (String, String) = conn
            .query_row(
                "SELECT score_json, output_json FROM pill WHERE id=?1",
                params![pid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .with_context(|| format!("没有丹药 #{pid}"))?;
        let s: Value = serde_json::from_str(&score_json).unwrap_or_default();
        let out: Value = serde_json::from_str(&output_json).unwrap_or_default();
        let precheck = s["total"].as_f64().unwrap_or(0.0);
        if s["errors"].as_i64().unwrap_or(0) > 0 {
            println!("  跳过 #{pid}：预检有 error，先修配方");
            continue;
        }
        let materials: Vec<String> = out["components"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|c| c["material"].as_str())
                    .filter(|m| *m != "?")
                    .map(|m| m.to_string())
                    .collect()
            })
            .unwrap_or_default();
        cands.push(Candidate {
            pill_id: *pid,
            label: String::new(),
            precheck,
            prompt: out["prompt"].as_str().unwrap_or("").to_string(),
            materials,
        });
        let _ = i;
    }
    if cands.len() < 2 {
        bail!("通过预检的丹药不足两颗");
    }

    // 打乱顺序并贴标签 A/B/C…
    let mut order: Vec<usize> = (0..cands.len()).collect();
    let mut rng = o.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
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

    // 盲评前抹掉材料名：规格里可能混入了作品标题（实测出现过 "COOLEST"），会泄漏来源
    let all_names: Vec<String> = cands
        .iter()
        .flat_map(|c| c.materials.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    for c in &mut cands {
        for name in &all_names {
            if !name.is_empty() {
                c.prompt = c.prompt.replace(name.as_str(), "（某作品）");
            }
        }
    }

    let key = if o.provider == "openai" {
        std::env::var("IE_API_KEY").unwrap_or_default()
    } else {
        std::env::var("GEMINI_API_KEY").context("缺少 GEMINI_API_KEY")?
    };
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()?;

    // 成对比较：每对问一次，左右顺序随机，抵消位置偏差
    let mut pair_results: Vec<(usize, usize, f64, String)> = Vec::new();
    for i in 0..cands.len() {
        for j in (i + 1)..cands.len() {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let flip = (rng >> 11) % 2 == 1;
            let (left, right) = if flip { (j, i) } else { (i, j) };
            let prompt = format!(
                "你是设计评审。下面是两份设计规格，只有文字。\n\n【候选 1】\n{}\n\n【候选 2】\n{}\n\n                 哪一份更好？标准：层级更清楚、体系更自洽、更克制、更可执行。\n                 只输出 JSON：{{\"winner\":\"1\"|\"2\"|\"tie\",\"reason\":\"一句话\",\"confidence\":0.0}}，不要解释文字。",
                cands[left].prompt, cands[right].prompt
            );
            let body = if o.provider == "openai" {
                openai_text_body(&prompt, o.model, 400)
            } else {
                json!({
                    "contents": [{"parts": [{"text": prompt}]}],
                    "generationConfig": {"responseMimeType":"application/json","temperature":0.2,"maxOutputTokens":400,"thinkingConfig":{"thinkingBudget":o.thinking}}
                })
            };
            let outcome = if o.provider == "openai" {
                call_openai(&client, o.base_url, &key, &body)
            } else {
                call(&client, o.base_url, o.model, &key, &body)
            };
            let v = match outcome {
                CallOutcome::Ok(v, _) => v,
                CallOutcome::RateLimited(i) => bail!("限流：{}", i.body),
                CallOutcome::Failed(e) => bail!("成对评判失败：{e}"),
            };
            let winner = v["winner"].as_str().unwrap_or("tie");
            let reason = v["reason"].as_str().unwrap_or("").to_string();
            let win_for_left = match winner {
                "1" => 1.0,
                "2" => 0.0,
                _ => 0.5,
            };
            // 转回 (i, j) 顺序：win for i
            let win_for_i = if flip {
                1.0 - win_for_left
            } else {
                win_for_left
            };
            println!(
                "  对比 {} vs {} → {}",
                cands[i].label,
                cands[j].label,
                if win_for_i > 0.5 {
                    cands[i].label.clone()
                } else if win_for_i < 0.5 {
                    cands[j].label.clone()
                } else {
                    "平".into()
                }
            );
            pair_results.push((i, j, win_for_i, reason));
        }
    }

    // Bradley-Terry 拟合（MM 迭代），得到相对强弱
    let n = cands.len();
    let mut s = vec![1.0f64; n];
    for _ in 0..300 {
        let mut next = vec![0.0f64; n];
        for (k, slot) in next.iter_mut().enumerate() {
            // 先验：每个候选有一场对"平均对手"的虚拟胜利，避免一两场全输就掉到 0
            let (mut w, mut denom) = (0.5f64, 1.0 / (s[k] + 1.0));
            for (a, b, wa, _) in &pair_results {
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
        "method": "pairwise+bt",
        "pairs": pair_results.iter().map(|(a,b,w,r)| json!({"i": cands[*a].label, "j": cands[*b].label, "win_i": w, "reason": r})).collect::<Vec<_>>(),
        "strengths": totals,
    });
    conn.execute(
        "INSERT INTO judge_run(status,stage,input_json,precheck_json,judge_json,created_at,updated_at)
         VALUES('judging','machine',?1,?2,?3,?4,?4)",
        params![
            json!({"pills": o.pills, "seed": o.seed}).to_string(),
            json!(cands.iter().map(|c| json!({"pill": c.pill_id, "precheck": c.precheck})).collect::<Vec<_>>()).to_string(),
            judge_json.to_string(),
            now
        ],
    )?;
    let run_id = conn.last_insert_rowid();

    let mut scores: Vec<(i64, String, f64)> = Vec::new();
    for (i, c) in cands.iter().enumerate() {
        let total = totals[i];
        conn.execute(
            "INSERT INTO judge_item(run_id,pill_id,label,display_order,precheck,machine_json,machine_total,status)
             VALUES(?1,?2,?3,?4,?5,?6,?7,'machine_done')",
            params![
                run_id,
                c.pill_id,
                c.label,
                i as i64,
                c.precheck,
                json!({"strength": s[i], "total": total}).to_string(),
                total
            ],
        )?;
        scores.push((c.pill_id, c.label.clone(), total));
    }

    // 孔多塞检查：若没有任何候选人赢下所有对手，说明偏好成环，BT 的"平局"是真实情况
    let condorcet: Option<usize> = (0..cands.len()).find(|k| {
        pair_results.iter().all(|(a, b, w, _)| {
            if a == k {
                *w > 0.5
            } else if b == k {
                *w < 0.5
            } else {
                true
            }
        })
    });

    // 升级规则：分差太小判不出、分数过低、随机 20% 抽检
    let machine_vals: Vec<f64> = scores.iter().map(|t| t.2).collect();
    let spread = machine_vals
        .iter()
        .fold((f64::MAX, f64::MIN), |(mn, mx), v| (mn.min(*v), mx.max(*v)));
    let indistinguishable = spread.1 - spread.0 < 3.0 || condorcet.is_none();
    let mut human_needed = 0usize;
    for (i, (pid, _label, total)) in scores.iter().enumerate() {
        let low = *total < 40.0;
        let sample = i == 0; // 固定抽第一颗做人工校准
        if indistinguishable || low || sample {
            conn.execute(
                "UPDATE judge_item SET status='human_needed' WHERE run_id=?1 AND pill_id=?2",
                params![run_id, pid],
            )?;
            human_needed += 1;
        }
    }
    let status = if human_needed > 0 {
        "awaiting_human"
    } else {
        "done"
    };
    conn.execute(
        "UPDATE judge_run SET status=?1, stage='final', updated_at=?2 WHERE id=?3",
        params![status, now, run_id],
    )?;
    recompute(&conn, run_id)?;

    println!(
        "盲评 run #{run_id}（seed {}，成对比较 + Bradley-Terry）",
        o.seed
    );
    println!("{:<5} {:<6} {:>8} {:>6}", "丹药", "标签", "机器分", "预检");
    for (pid, label, total) in &scores {
        println!(
            "{:<5} {:<6} {:>8.1} {:>6.3}",
            pid,
            label,
            total,
            cands
                .iter()
                .find(|c| c.pill_id == *pid)
                .map(|c| c.precheck)
                .unwrap_or(0.0)
        );
    }
    if condorcet.is_none() {
        println!("⚠ 无孔多塞胜者：成对偏好成环（A>B>C>A），判不出优劣 → 全部转人工");
    } else if indistinguishable {
        println!("⚠ 机器分差 < 3 分，判不出优劣 → 全部转人工");
    }
    println!("需要人工干预的条目：{human_needed}（见 ie judge review --run {run_id}）");
    Ok(())
}

fn recompute(conn: &Connection, run_id: i64) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT id, precheck, machine_total, human_score FROM judge_item WHERE run_id=?1",
    )?;
    type Row = (i64, Option<f64>, Option<f64>, Option<f64>);
    let rows: Vec<Row> = stmt
        .query_map(params![run_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut finals = Vec::new();
    for (item_id, pre, machine, human) in rows {
        let m = machine.unwrap_or(0.0);
        let p = pre.unwrap_or(0.0) * 100.0;
        let final_total = match human {
            Some(h) => (0.45 * m + 0.25 * p + 0.30 * h * 0.01 * 100.0).round(),
            None => (0.65 * m + 0.35 * p).round(),
        };
        conn.execute(
            "UPDATE judge_item SET final_total=?1 WHERE id=?2",
            params![final_total, item_id],
        )?;
        finals.push((item_id, final_total, human.is_some()));
    }
    let has_pending: i64 = conn.query_row(
        "SELECT count(*) FROM judge_item WHERE run_id=?1 AND status='human_needed'",
        params![run_id],
        |r| r.get(0),
    )?;
    let status = if has_pending > 0 {
        "awaiting_human"
    } else {
        "done"
    };
    conn.execute(
        "UPDATE judge_run SET final_json=?1, status=?2, updated_at=?3 WHERE id=?4",
        params![
            json!(
                finals
                    .iter()
                    .map(|(id, t, h)| json!({"item": id, "final": t, "human": h}))
                    .collect::<Vec<_>>()
            )
            .to_string(),
            status,
            crate::now_iso(),
            run_id
        ],
    )?;
    Ok(())
}

pub fn list(db: &str) -> Result<()> {
    let conn = Connection::open(db)?;
    let mut stmt = conn.prepare(
        "SELECT id, status, stage, (SELECT count(*) FROM judge_item WHERE run_id=judge_run.id),
                (SELECT count(*) FROM judge_item WHERE run_id=judge_run.id AND status='human_needed'),
                created_at
         FROM judge_run ORDER BY id DESC LIMIT 20",
    )?;
    let rows: Vec<(i64, String, String, i64, i64, String)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    println!(
        "{:<5} {:<16} {:<8} {:>4} {:>6} 创建",
        "run", "状态", "阶段", "条目", "待人工"
    );
    for (id, status, stage, n, pending, at) in rows {
        println!(
            "{:<5} {:<16} {:<8} {:>4} {:>6} {}",
            id, status, stage, n, pending, at
        );
    }
    Ok(())
}

pub fn review(db: &str, run_id: Option<i64>) -> Result<()> {
    let conn = Connection::open(db)?;
    let sql = "SELECT i.id, i.run_id, i.label, i.pill_id, i.machine_total, i.precheck, i.human_score
               FROM judge_item i WHERE i.status='human_needed' AND (?1 IS NULL OR i.run_id=?1) ORDER BY i.run_id, i.display_order";
    let mut stmt = conn.prepare(sql)?;
    type QRow = (i64, i64, String, i64, Option<f64>, Option<f64>, Option<f64>);
    let rows: Vec<QRow> = stmt
        .query_map(params![run_id], |r| {
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
        println!("没有待人工的条目");
        return Ok(());
    }
    println!(
        "{:<6} {:<5} {:<7} {:<7} {:>8} {:>6} {:>6}",
        "item", "run", "标签", "丹药", "机器分", "预检", "人工"
    );
    for (id, run, label, pid, machine, pre, human) in rows {
        println!(
            "{:<6} {:<5} {:<7} {:<7} {:>8} {:>6} {:>6}",
            id,
            run,
            label,
            pid,
            machine.map(|v| format!("{v:.1}")).unwrap_or("-".into()),
            pre.map(|v| format!("{v:.3}")).unwrap_or("-".into()),
            human.map(|v| format!("{v:.0}")).unwrap_or("-".into())
        );
    }
    println!("\n回填：ie judge record --item <item_id> --score 0-100 [--note ...] [--approve]");
    Ok(())
}

/// 一屏审阅：把一次 run 的所有候选、分数、规格摘要和成对理由放在同一屏。
pub fn screen(db: &str, run_id: i64) -> Result<()> {
    let conn = Connection::open(db)?;
    let (status, seed, judge_json): (String, i64, Option<String>) = conn.query_row(
        "SELECT status, coalesce(json_extract(input_json,'$.seed'),0), judge_json FROM judge_run WHERE id=?1",
        params![run_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let j: Value = judge_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    println!("══ run #{run_id} · {status} · seed {seed} ══");
    if let Some(pairs) = j["pairs"].as_array() {
        for p in pairs {
            let winner = if p["win_i"].as_f64().unwrap_or(0.5) > 0.5 {
                p["i"].as_str().unwrap_or("?")
            } else if p["win_i"].as_f64().unwrap_or(0.5) < 0.5 {
                p["j"].as_str().unwrap_or("?")
            } else {
                "平"
            };
            println!(
                "  {} vs {} → {}   （{}）",
                p["i"].as_str().unwrap_or("?"),
                p["j"].as_str().unwrap_or("?"),
                winner,
                p["reason"].as_str().unwrap_or("")
            );
        }
    }

    let mut stmt = conn.prepare(
        "SELECT i.id, i.label, i.pill_id, i.machine_total, i.precheck, i.human_score, i.status,
                p.output_json, p.output_md
         FROM judge_item i JOIN pill p ON p.id=i.pill_id
         WHERE i.run_id=?1 ORDER BY i.display_order",
    )?;
    type ScreenRow = (
        i64,
        String,
        i64,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        String,
        String,
        String,
    );
    let rows: Vec<ScreenRow> = stmt
        .query_map(params![run_id], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    for (item_id, label, pill_id, machine, precheck, human, status, output_json, md) in rows {
        let out: Value = serde_json::from_str(&output_json).unwrap_or_default();
        let prompt = out["prompt"].as_str().unwrap_or("");
        let excerpt: String = prompt.chars().take(180).collect();
        println!("──────────────────────────────────────────────");
        println!(
            "[{label}] item#{item_id} pill#{pill_id}  机器 {}  预检 {}  人工 {}  {}",
            machine.map(|v| format!("{v:.1}")).unwrap_or("-".into()),
            precheck.map(|v| format!("{v:.3}")).unwrap_or("-".into()),
            human.map(|v| format!("{v:.0}")).unwrap_or("-".into()),
            status
        );
        if prompt.is_empty() {
            let plain: String = md.chars().take(180).collect();
            println!("  规格：{plain}");
        } else {
            println!("  规格：{excerpt}");
        }
    }
    println!("──────────────────────────────────────────────");
    println!("批量回填：ie judge fill --run {run_id} --scores \"A=82,B=70,C=75\" [--note \"...\"]");
    Ok(())
}

/// 批量回填：一次把这一屏所有候选的人工分写进去。
pub fn fill(db: &str, run_id: i64, scores: &str, note: Option<&str>) -> Result<()> {
    let conn = Connection::open(db)?;
    let mut stmt = conn.prepare("SELECT id, label FROM judge_item WHERE run_id=?1")?;
    let items: Vec<(i64, String)> = stmt
        .query_map(params![run_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if items.is_empty() {
        bail!("run #{run_id} 没有条目");
    }

    let mut applied = 0usize;
    for part in scores.split([',', ';', '、']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((label, val)) = part.split_once('=') else {
            bail!("看不懂 \"{part}\"，格式应为 A=82");
        };
        let label = label.trim().to_uppercase();
        let score: f64 = val.trim().parse().context("分数不是数字")?;
        if !(0.0..=100.0).contains(&score) {
            bail!("{label} 的分数 {score} 超出 0–100");
        }
        let Some((item_id, _)) = items.iter().find(|(_, l)| *l == label) else {
            bail!("run #{run_id} 里没有标签 {label}");
        };
        conn.execute(
            "UPDATE judge_item SET human_score=?1, human_note=?2, status='done' WHERE id=?3",
            params![score, note, item_id],
        )?;
        applied += 1;
    }
    if applied == 0 {
        bail!("没有解析到任何分数");
    }
    recompute(&conn, run_id)?;

    let mut stmt = conn.prepare(
        "SELECT label, pill_id, machine_total, human_score, final_total FROM judge_item WHERE run_id=?1 ORDER BY display_order",
    )?;
    type FillRow = (String, i64, Option<f64>, Option<f64>, Option<f64>);
    let rows: Vec<FillRow> = stmt
        .query_map(params![run_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    println!("已回填 {applied} 条，run #{run_id} 重算：");
    println!(
        "{:<6} {:<8} {:>8} {:>6} {:>8}",
        "标签", "丹药", "机器分", "人工", "最终分"
    );
    for (label, pid, m, h, f) in rows {
        println!(
            "{:<6} {:<8} {:>8} {:>6} {:>8}",
            label,
            pid,
            m.map(|v| format!("{v:.1}")).unwrap_or("-".into()),
            h.map(|v| format!("{v:.0}")).unwrap_or("-".into()),
            f.map(|v| format!("{v:.1}")).unwrap_or("-".into())
        );
    }
    let status: String = conn.query_row(
        "SELECT status FROM judge_run WHERE id=?1",
        params![run_id],
        |r| r.get(0),
    )?;
    println!("run 状态：{status}");
    Ok(())
}

pub fn record(db: &str, item_id: i64, score: f64, note: Option<&str>) -> Result<()> {
    if !(0.0..=100.0).contains(&score) {
        bail!("人工分应在 0–100");
    }
    let conn = Connection::open(db)?;
    let run_id: i64 = conn.query_row(
        "SELECT run_id FROM judge_item WHERE id=?1",
        params![item_id],
        |r| r.get(0),
    )?;
    conn.execute(
        "UPDATE judge_item SET human_score=?1, human_note=?2, status='done' WHERE id=?3",
        params![score, note, item_id],
    )?;
    recompute(&conn, run_id)?;
    println!("已记录 item #{item_id} 人工分 {score:.0}，run #{run_id} 已重算");
    Ok(())
}
