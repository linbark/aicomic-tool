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
        cands.push(Candidate {
            pill_id: *pid,
            label: String::new(),
            precheck,
            prompt: out["prompt"].as_str().unwrap_or("").to_string(),
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
    let mut prompt = String::from(
        "你是设计评审。下面是几份**设计规格**（只有文字，没有图）。请按四项各打 0–10 分：\
         hierarchy（层级是否清楚）、consistency（体系是否自洽）、restraint（克制，有没有堆砌）、executability（能否被直接执行）。\
         只输出一个 JSON 对象：{\"items\":[{\"label\":\"A\",\"hierarchy\":0,\"consistency\":0,\"restraint\":0,\"executability\":0,\"reason\":\"一句话\"}],\"confidence\":0.0}。\
         不要解释，不要 markdown。\n\n",
    );
    for (pos, idx) in order.iter().enumerate() {
        cands[*idx].label = labels[pos].to_string();
        prompt.push_str(&format!(
            "【候选 {}】\n{}\n\n",
            labels[pos], cands[*idx].prompt
        ));
    }

    let key = if o.provider == "openai" {
        std::env::var("IE_API_KEY").unwrap_or_default()
    } else {
        std::env::var("GEMINI_API_KEY").context("缺少 GEMINI_API_KEY")?
    };
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()?;
    let body = if o.provider == "openai" {
        openai_text_body(&prompt, o.model, 1200)
    } else {
        json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {
                "responseMimeType": "application/json",
                "temperature": 0.2,
                "maxOutputTokens": 1200,
                "thinkingConfig": {"thinkingBudget": o.thinking}
            }
        })
    };
    let outcome = if o.provider == "openai" {
        call_openai(&client, o.base_url, &key, &body)
    } else {
        call(&client, o.base_url, o.model, &key, &body)
    };
    let judged = match outcome {
        CallOutcome::Ok(v, _) => v,
        CallOutcome::RateLimited(i) => bail!("限流：{}", i.body),
        CallOutcome::Failed(e) => bail!("评判调用失败：{e}"),
    };

    let now = crate::now_iso();
    conn.execute(
        "INSERT INTO judge_run(status,stage,input_json,precheck_json,judge_json,created_at,updated_at)
         VALUES('judging','machine',?1,?2,?3,?4,?4)",
        params![
            json!({"pills": o.pills, "seed": o.seed}).to_string(),
            json!(cands.iter().map(|c| json!({"pill": c.pill_id, "precheck": c.precheck})).collect::<Vec<_>>()).to_string(),
            judged.to_string(),
            now
        ],
    )?;
    let run_id = conn.last_insert_rowid();

    let items = judged["items"].as_array().cloned().unwrap_or_default();
    let mut totals: Vec<(i64, String, f64)> = Vec::new();
    for c in &cands {
        let item = items
            .iter()
            .find(|x| x["label"].as_str() == Some(c.label.as_str()));
        let (machine, total) = match item {
            Some(x) => {
                // 模型有时按 0–1 打分，有时按 0–10；按最大值判断量纲
                let vals: Vec<f64> = ["hierarchy", "consistency", "restraint", "executability"]
                    .iter()
                    .filter_map(|k| x[*k].as_f64())
                    .collect();
                let max = vals.iter().cloned().fold(0.0f64, f64::max);
                let scale = if max <= 1.0 { 10.0 } else { 1.0 };
                let sum: f64 = vals.iter().sum::<f64>() * scale;
                (x.clone(), (sum / 40.0 * 100.0 * 100.0).round() / 100.0)
            }
            None => (json!({}), 0.0),
        };
        conn.execute(
            "INSERT INTO judge_item(run_id,pill_id,label,display_order,precheck,machine_json,machine_total,status)
             VALUES(?1,?2,?3,?4,?5,?6,?7,'machine_done')",
            params![
                run_id,
                c.pill_id,
                c.label,
                c.pill_id as i32,
                c.precheck,
                machine.to_string(),
                total
            ],
        )?;
        totals.push((c.pill_id, c.label.clone(), total));
    }

    // 升级规则：分差太小判不出、分数过低、随机 20% 抽检
    let machine_vals: Vec<f64> = totals.iter().map(|t| t.2).collect();
    let spread = machine_vals
        .iter()
        .fold((f64::MAX, f64::MIN), |(mn, mx), v| (mn.min(*v), mx.max(*v)));
    let indistinguishable = spread.1 - spread.0 < 3.0;
    let mut human_needed = 0usize;
    for (i, (pid, _label, total)) in totals.iter().enumerate() {
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

    println!("盲评 run #{run_id}（seed {}）", o.seed);
    println!("{:<5} {:<6} {:>8} {:>6}", "丹药", "标签", "机器分", "预检");
    for (pid, label, total) in &totals {
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
    if indistinguishable {
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
