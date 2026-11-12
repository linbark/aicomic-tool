//! 评判流水线：预检 → 机器盲评 → 人工干预 → 归并。
//!
//! 设计要点：
//! - 机器评判完成后落库；机器调用目前没有断点恢复能力；
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
    pub repeats: usize,
    pub dimension: String,
    pub rpm: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewSource {
    Human,
    Ai,
    Demo,
    Legacy,
}

impl ReviewSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Ai => "ai",
            Self::Demo => "demo",
            Self::Legacy => "legacy",
        }
    }
}

impl std::str::FromStr for ReviewSource {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "human" => Ok(Self::Human),
            "ai" => Ok(Self::Ai),
            "demo" => Ok(Self::Demo),
            "legacy" => Ok(Self::Legacy),
            _ => bail!("评分来源应为 human/ai/demo/legacy"),
        }
    }
}

pub struct ReviewProvenance<'a> {
    pub source: ReviewSource,
    pub reviewer: Option<&'a str>,
    pub dimension: Option<&'a str>,
}

impl ReviewProvenance<'_> {
    #[allow(
        dead_code,
        reason = "兼容旧 fill/record 包装器；CLI 使用显式 provenance"
    )]
    pub fn demo() -> Self {
        Self {
            source: ReviewSource::Demo,
            reviewer: None,
            dimension: None,
        }
    }

    fn validate(&self) -> Result<()> {
        if self.source == ReviewSource::Human && self.reviewer.is_none_or(|r| r.trim().is_empty()) {
            bail!("human 评分必须显式指定非空 reviewer；AI/演示评分请用 ai/demo");
        }
        if self.dimension.is_some_and(|d| d.trim().is_empty()) {
            bail!("评分维度不能是空字符串");
        }
        Ok(())
    }
}

pub struct FillOpts<'a> {
    pub db: &'a str,
    pub run_id: i64,
    pub scores: &'a str,
    pub note: Option<&'a str>,
    pub provenance: ReviewProvenance<'a>,
}

struct Candidate {
    pill_id: i64,
    label: String,
    precheck: f64,
    prompt: String,
    materials: Vec<String>,
}

/// 持久化的运行输入快照，不含密钥。
pub fn input_metadata(o: &RunOpts<'_>) -> Value {
    json!({"pills": o.pills, "seed": o.seed, "dimension":o.dimension,
        "provider":o.provider, "model":o.model, "repeats":o.repeats.max(1)})
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
            let criterion = match o.dimension.as_str() {
                "hierarchy" => "只看层级是否清楚",
                "consistency" => "只看体系是否自洽",
                "restraint" => "只看是否克制、有没有堆砌",
                "executability" => "只看能否被直接执行",
                _ => "综合层级、自洽、克制、可执行",
            };
            let prompt = format!(
                "你是设计评审。下面是两份设计规格，只有文字。\n\n【候选 1】\n{}\n\n【候选 2】\n{}\n\n问题：{}，哪一份更好？\n只输出 JSON：{{\"winner\":\"1\"|\"2\"|\"tie\",\"reason\":\"一句话\",\"confidence\":0.0}}，不要解释文字。",
                cands[left].prompt, cands[right].prompt, criterion
            );
            let body = if o.provider == "openai" {
                openai_text_body(&prompt, o.model, 400)
            } else {
                json!({
                    "contents": [{"parts": [{"text": prompt}]}],
                    "generationConfig": {"responseMimeType":"application/json","temperature":0.2,"maxOutputTokens":400,"thinkingConfig":{"thinkingBudget":o.thinking}}
                })
            };
            let mut ll: f64 = 0.0;
            let mut reason = String::new();
            for _ in 0..o.repeats.max(1) {
                // 免费层每分钟额度很低，按 rpm 节流；0 表示不限速
                if o.rpm > 0 {
                    std::thread::sleep(Duration::from_millis(60_000 / o.rpm as u64));
                }
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
                ll += match winner {
                    "1" => 1.0,
                    "2" => 0.0,
                    _ => 0.5,
                };
                if reason.is_empty() {
                    reason = v["reason"].as_str().unwrap_or("").to_string();
                }
            }
            let win_for_left = ll / o.repeats.max(1) as f64;
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
            input_metadata(o).to_string(),
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

pub(crate) fn recompute(conn: &Connection, run_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE judge_item SET status='human_needed' WHERE run_id=?1
        AND status='done' AND human_score IS NOT NULL
        AND NOT (human_source='human' AND length(trim(coalesce(human_reviewer,'')))>0)",
        [run_id],
    )?;
    let mut stmt = conn.prepare(
        "SELECT id, precheck, machine_total,
                CASE WHEN human_source='human' AND length(trim(coalesce(human_reviewer,'')))>0
                     THEN human_score ELSE NULL END
         FROM judge_item WHERE run_id=?1",
    )?;
    type Row = (i64, Option<f64>, Option<f64>, Option<f64>);
    let rows: Vec<Row> = stmt
        .query_map(params![run_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut finals = Vec::new();
    for (item_id, pre, machine, human) in rows {
        let human = human.filter(|h| h.is_finite() && (0.0..=100.0).contains(h));
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

pub const MIN_CALIBRATION_SAMPLES: i64 = 20;

/// 仅用真实、同审核人的成对评分校准；少样本仅展示，不自动应用。
pub fn calibrate(db: &str) -> Result<()> {
    let mut connection = Connection::open(db)?;
    connection.busy_timeout(Duration::from_secs(10))?;
    let conn = connection.transaction()?;
    let mut stmt = conn.prepare(
        "SELECT id, judge_json FROM judge_run WHERE stage='panel' AND judge_json IS NOT NULL",
    )?;
    let runs: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    // 裁判名 -> (样本数, 一致数)
    let mut stats: std::collections::BTreeMap<String, (i64, i64)> = Default::default();
    let mut used_pairs = 0usize;
    for (run_id, judge_json) in runs {
        let j: Value = serde_json::from_str(&judge_json).unwrap_or_default();
        let mut labels: std::collections::HashMap<String, (String, Option<f64>)> =
            Default::default();
        {
            let mut s = conn.prepare(
                "SELECT label, human_reviewer, human_score FROM judge_item WHERE run_id=?1
                    AND human_source='human' AND human_reviewer IS NOT NULL
                    AND length(trim(human_reviewer)) > 0",
            )?;
            for row in s.query_map(params![run_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<f64>>(2)?,
                ))
            })? {
                let (label, reviewer, human) = row?;
                labels.insert(label, (reviewer, human));
            }
        }
        for p in j["pairs"].as_array().cloned().unwrap_or_default() {
            let (Some(li), Some(lj)) = (p["i"].as_str(), p["j"].as_str()) else {
                continue;
            };
            let (Some((ri, Some(hi))), Some((rj, Some(hj)))) = (labels.get(li), labels.get(lj))
            else {
                continue;
            };
            if ri.trim().is_empty()
                || ri.trim() != rj.trim()
                || !hi.is_finite()
                || !hj.is_finite()
                || !(0.0..=100.0).contains(hi)
                || !(0.0..=100.0).contains(hj)
            {
                continue;
            }
            // 人工结论：分差 >3 才算分出胜负
            let human = if (hi - hj).abs() <= 3.0 {
                None
            } else if hi > hj {
                Some(true)
            } else {
                Some(false)
            };
            let verdict = |v: f64| -> Option<bool> {
                if v > 0.5 + 1e-9 {
                    Some(true)
                } else if v < 0.5 - 1e-9 {
                    Some(false)
                } else {
                    None
                }
            };
            for vote in p["votes"].as_array().cloned().unwrap_or_default() {
                let (Some(name), Some(v)) = (vote[0].as_str(), vote[1].as_f64()) else {
                    continue;
                };
                let got = verdict(v);
                let agree = match (human, got) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a == b,
                    _ => false,
                };
                let e = stats.entry(name.to_string()).or_insert((0, 0));
                e.0 += 1;
                if agree {
                    e.1 += 1;
                }
            }
            used_pairs += 1;
        }
    }

    // 每次从真实样本完整重建；不允许无样本时继续沿用旧/演示权重。
    // 迁移的存档触发器会保存被替换的原始权重及其来源。
    conn.execute(
        "UPDATE judge_model SET samples=0, agreements=0, weight=1.0,
        calibration_source='human', calibration_trusted=0, updated_at=?1",
        [crate::now_iso()],
    )?;

    println!("可用于校准的成对样本：{used_pairs}");
    println!(
        "{:<14} {:>6} {:>6} {:>8} {:>6} 应用状态",
        "裁判", "样本", "一致", "一致率", "建议权重"
    );
    for (name, (n, a)) in &stats {
        let rate = if *n > 0 { *a as f64 / *n as f64 } else { 0.0 };
        // 保持原收缩公式：样本越少越往默认 1.0 收缩；是否应用另受可信阈值控制。
        let shrink = (*n as f64 / (*n as f64 + 4.0)).min(1.0);
        let weight = 1.0 + (rate - 0.5) * 2.0 * shrink;
        let trusted = *n >= MIN_CALIBRATION_SAMPLES;
        conn.execute(
            "INSERT INTO judge_model(name,samples,agreements,weight,updated_at,calibration_source,calibration_trusted)
             VALUES(?1,?2,?3,?4,?5,'human',?6)
             ON CONFLICT(name) DO UPDATE SET samples=?2, agreements=?3, weight=?4, updated_at=?5,
                 calibration_source='human', calibration_trusted=?6",
            params![name, n, a, weight, crate::now_iso(), trusted],
        )?;
        println!(
            "{name:<14} {n:>6} {a:>6} {:>7.1}% {weight:>6.2} {}",
            rate * 100.0,
            if trusted {
                "可信 human 校准"
            } else {
                "少样本，不应用（至少20）"
            }
        );
    }
    if stats.is_empty() {
        println!(
            "没有真实样本：已恢复默认权重，不应用校准；请用 --source human --reviewer <本人> 回填"
        );
    }
    conn.commit()?;
    Ok(())
}

pub fn models(db: &str) -> Result<()> {
    print!("{}", models_text(db)?);
    Ok(())
}

pub fn models_text(db: &str) -> Result<String> {
    let conn = Connection::open(db)?;
    let mut stmt = conn.prepare(
        "SELECT name, samples, agreements, weight, updated_at, calibration_source, calibration_trusted
         FROM judge_model ORDER BY name",
    )?;
    type ModelRow = (String, i64, i64, f64, String, String, bool);
    let rows: Vec<ModelRow> = stmt
        .query_map([], |r| {
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
        return Ok("还没有校准记录\n".into());
    }
    let mut text = format!(
        "{:<14} {:>6} {:>6} {:>8} 应用状态（阈值20）· 更新\n",
        "裁判", "样本", "一致", "建议权重"
    );
    for (name, n, a, w, at, source, trusted) in rows {
        let status = if source != "human" {
            "legacy，不应用"
        } else if !trusted || n < MIN_CALIBRATION_SAMPLES || !w.is_finite() || w <= 0.0 {
            "human 少样本，不应用"
        } else {
            "可信 human 校准"
        };
        text.push_str(&format!(
            "{name:<14} {n:>6} {a:>6} {w:>8.2} {status} · {at}\n"
        ));
    }
    Ok(text)
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
    let sql = "SELECT i.id, i.run_id, i.label, i.pill_id, i.machine_total, i.precheck,
                      CASE WHEN i.human_score IS NOT NULL THEN printf('%.0f [%s]',i.human_score,i.human_source) END
               FROM judge_item i WHERE i.status='human_needed' AND (?1 IS NULL OR i.run_id=?1) ORDER BY i.run_id, i.display_order";
    let mut stmt = conn.prepare(sql)?;
    type QRow = (
        i64,
        i64,
        String,
        i64,
        Option<f64>,
        Option<f64>,
        Option<String>,
    );
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
        "item", "run", "标签", "丹药", "机器分", "预检", "评分/来源"
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
            human.unwrap_or("-".into())
        );
    }
    println!(
        "\n真实回填：ie judge record --item <item_id> --score 0-100 --source human --reviewer <本人> [--note ...]"
    );
    Ok(())
}

/// 一屏审阅：把一次 run 的所有候选、分数、规格摘要和成对理由放在同一屏。
pub fn screen(db: &str, run_id: i64) -> Result<()> {
    print!("{}", screen_text(db, run_id)?);
    Ok(())
}

/// 与 CLI 完全相同的审阅屏，允许离线调用者展示并测试，不调用模型。
pub fn screen_text(db: &str, run_id: i64) -> Result<String> {
    let mut text = String::new();
    macro_rules! line {
        ($($arg:tt)*) => {{
            text.push_str(&format!($($arg)*));
            text.push('\n');
        }};
    }
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

    line!("══ run #{run_id} · {status} · seed {seed} ══");
    if let Some(pairs) = j["pairs"].as_array() {
        for p in pairs {
            let probability = p["combined"]
                .as_f64()
                .or_else(|| p["win_i"].as_f64())
                .filter(|v| v.is_finite() && (0.0..=1.0).contains(v));
            let winner = match probability {
                Some(v) if v > 0.5 + 1e-9 => p["i"].as_str().unwrap_or("?"),
                Some(v) if v < 0.5 - 1e-9 => p["j"].as_str().unwrap_or("?"),
                Some(_) => "平",
                None => "未知",
            };
            let votes: Vec<(&str, f64)> = p["votes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| Some((v[0].as_str()?, v[1].as_f64()?)))
                .collect();
            let verdict = |v: f64| {
                if v > 0.5 + 1e-9 {
                    1
                } else if v < 0.5 - 1e-9 {
                    -1
                } else {
                    0
                }
            };
            let disputed = p["disputed"].as_bool().unwrap_or(false)
                || votes.first().is_some_and(|(_, first)| {
                    votes.iter().any(|(_, v)| verdict(*v) != verdict(*first))
                });
            let votes = votes
                .iter()
                .map(|(name, v)| format!("{name}={v:.2}"))
                .collect::<Vec<_>>()
                .join(" ");
            line!(
                "  {} vs {} → {}  胜率 {}  {}{} （{}）",
                p["i"].as_str().unwrap_or("?"),
                p["j"].as_str().unwrap_or("?"),
                winner,
                probability.map(|v| format!("{v:.2}")).unwrap_or("-".into()),
                votes,
                if disputed { " ⚠ 裁判分歧" } else { "" },
                p["reason"].as_str().unwrap_or("")
            );
        }
    }

    let mut stmt = conn.prepare(
        "SELECT i.id, i.label, i.pill_id, i.machine_total, i.precheck, i.human_score, i.status,
                p.output_json, p.output_md, i.human_source, i.human_reviewer,
                i.human_recorded_at, i.human_dimension
         FROM judge_item i JOIN pill p ON p.id=i.pill_id
         WHERE i.run_id=?1 ORDER BY i.display_order",
    )?;
    type ReviewMeta = (String, Option<String>, Option<String>, Option<String>);
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
        ReviewMeta,
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
                (r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?),
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    for (
        item_id,
        label,
        pill_id,
        machine,
        precheck,
        human,
        status,
        output_json,
        md,
        (source, reviewer, at, dimension),
    ) in rows
    {
        let out: Value = serde_json::from_str(&output_json).unwrap_or_default();
        let prompt = out["prompt"].as_str().unwrap_or("");
        let excerpt: String = prompt.chars().take(180).collect();
        line!("──────────────────────────────────────────────");
        line!(
            "[{label}] item#{item_id} pill#{pill_id}  机器 {}  预检 {}  评分 {} [{source}]  {}",
            machine.map(|v| format!("{v:.1}")).unwrap_or("-".into()),
            precheck.map(|v| format!("{v:.3}")).unwrap_or("-".into()),
            human.map(|v| format!("{v:.0}")).unwrap_or("-".into()),
            status
        );
        if human.is_some() {
            line!(
                "  reviewer={} at={} dimension={}",
                reviewer.as_deref().unwrap_or("未知"),
                at.as_deref().unwrap_or("未知"),
                dimension.as_deref().unwrap_or("未标注")
            );
        }
        if prompt.is_empty() {
            let plain: String = md.chars().take(180).collect();
            line!("  规格：{plain}");
        } else {
            line!("  规格：{excerpt}");
        }
    }
    line!("──────────────────────────────────────────────");
    line!(
        "真实人评：ie judge fill --run {run_id} --scores \"A=82,B=70,C=75\" --source human --reviewer <本人> [--dimension ...]"
    );
    line!("省略 --source 默认 demo；ai/demo/legacy 不进入真实校准，不完成人工队列。");
    Ok(text)
}

/// 旧接口仅用于 demo；真实人工评分必须使用带 provenance 的接口。
#[allow(
    dead_code,
    reason = "保留兼容公开接口，默认 demo，绝不隐式创建真人评分"
)]
pub fn fill(db: &str, run_id: i64, scores: &str, note: Option<&str>) -> Result<()> {
    fill_with_opts(&FillOpts {
        db,
        run_id,
        scores,
        note,
        provenance: ReviewProvenance::demo(),
    })
}

pub fn fill_with_opts(o: &FillOpts<'_>) -> Result<()> {
    o.provenance.validate()?;
    let FillOpts {
        db,
        run_id,
        scores,
        note,
        ..
    } = *o;
    let mut conn = Connection::open(db)?;
    conn.busy_timeout(Duration::from_secs(10))?;
    let tx = conn.transaction()?;
    let mut stmt = tx.prepare("SELECT id, label FROM judge_item WHERE run_id=?1")?;
    let items: Vec<(i64, String)> = stmt
        .query_map(params![run_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    if items.is_empty() {
        bail!("run #{run_id} 没有条目");
    }

    let mut validated = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for part in scores.split([',', ';', '、']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((label, val)) = part.split_once('=') else {
            bail!("看不懂 \"{part}\"，格式应为 A=82");
        };
        let label = label.trim().to_uppercase();
        if !seen.insert(label.clone()) {
            bail!("重复标签 {label}，整批未写入");
        }
        let score: f64 = val.trim().parse().context("分数不是数字")?;
        if !(0.0..=100.0).contains(&score) {
            bail!("{label} 的分数 {score} 超出 0–100");
        }
        let Some((item_id, _)) = items.iter().find(|(_, l)| *l == label) else {
            bail!("run #{run_id} 里没有标签 {label}");
        };
        ensure_review_can_replace(&tx, *item_id, &o.provenance)?;
        validated.push((*item_id, score));
    }
    let applied = validated.len();
    if applied == 0 {
        bail!("没有解析到任何分数");
    }
    let at = crate::now_iso();
    for (item_id, score) in validated {
        write_review(&tx, item_id, score, note, &o.provenance, &at)?;
    }
    recompute(&tx, run_id)?;
    tx.commit()?;

    let mut stmt = conn.prepare(
        "SELECT label, pill_id, machine_total, human_score, final_total FROM judge_item WHERE run_id=?1 ORDER BY display_order",
    )?;
    type FillRow = (String, i64, Option<f64>, Option<f64>, Option<f64>);
    let rows: Vec<FillRow> = stmt
        .query_map(params![run_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    println!(
        "已回填 {applied} 条（source={}），run #{run_id} 重算：",
        o.provenance.source.as_str()
    );
    println!(
        "{:<6} {:<8} {:>8} {:>6} {:>8}",
        "标签", "丹药", "机器分", "评分", "最终分"
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

#[allow(
    dead_code,
    reason = "保留兼容公开接口，默认 demo，绝不隐式创建真人评分"
)]
pub fn record(db: &str, item_id: i64, score: f64, note: Option<&str>) -> Result<()> {
    record_with_provenance(db, item_id, score, note, &ReviewProvenance::demo())
}

pub fn record_with_provenance(
    db: &str,
    item_id: i64,
    score: f64,
    note: Option<&str>,
    provenance: &ReviewProvenance<'_>,
) -> Result<()> {
    provenance.validate()?;
    if !score.is_finite() || !(0.0..=100.0).contains(&score) {
        bail!("评分必须是 0–100 的有限数字");
    }
    let mut conn = Connection::open(db)?;
    conn.busy_timeout(Duration::from_secs(10))?;
    let tx = conn.transaction()?;
    let run_id: i64 = tx.query_row(
        "SELECT run_id FROM judge_item WHERE id=?1",
        params![item_id],
        |r| r.get(0),
    )?;
    ensure_review_can_replace(&tx, item_id, provenance)?;
    write_review(&tx, item_id, score, note, provenance, &crate::now_iso())?;
    recompute(&tx, run_id)?;
    tx.commit()?;
    println!(
        "已记录 item #{item_id} {} 分 {score:.0}，run #{run_id} 已重算",
        provenance.source.as_str()
    );
    Ok(())
}

fn ensure_review_can_replace(
    conn: &Connection,
    item_id: i64,
    provenance: &ReviewProvenance<'_>,
) -> Result<()> {
    if provenance.source != ReviewSource::Human {
        let accepted_human: bool = conn.query_row(
            "SELECT human_score IS NOT NULL
            AND human_source='human' AND length(trim(coalesce(human_reviewer,'')))>0
            FROM judge_item WHERE id=?1",
            [item_id],
            |r| r.get(0),
        )?;
        if accepted_human {
            bail!("item #{item_id} 已有真实 human 评分，拒绝用 ai/demo/legacy 覆盖；整批未写入");
        }
    }
    Ok(())
}

fn write_review(
    conn: &Connection,
    item_id: i64,
    score: f64,
    note: Option<&str>,
    provenance: &ReviewProvenance<'_>,
    at: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE judge_item SET human_score=?1, human_note=?2, human_source=?3,
             human_reviewer=?4, human_recorded_at=?5,
             human_dimension=coalesce(?6,(SELECT json_extract(input_json,'$.dimension')
                                          FROM judge_run WHERE id=judge_item.run_id)),
             status=CASE WHEN ?3='human' THEN 'done'
                         WHEN status='done' AND human_score IS NOT NULL THEN 'human_needed'
                         ELSE status END WHERE id=?7",
        params![
            score,
            note,
            provenance.source.as_str(),
            provenance.reviewer.map(str::trim),
            at,
            provenance.dimension.map(str::trim),
            item_id
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DB: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        path: std::path::PathBuf,
        conn: Connection,
    }

    impl Fixture {
        fn new() -> Self {
            Self::with_provenance(true)
        }

        fn with_provenance(upgrade: bool) -> Self {
            let path = std::env::temp_dir().join(format!(
                "ie-judgment-test-{}-{}.sqlite",
                std::process::id(),
                NEXT_DB.fetch_add(1, Ordering::Relaxed)
            ));
            assert!(path.is_absolute());
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE schema_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE pill(id INTEGER PRIMARY KEY, output_json TEXT NOT NULL DEFAULT '{}',
                     output_md TEXT NOT NULL DEFAULT '', score_json TEXT NOT NULL DEFAULT '{}');",
            )
            .unwrap();
            conn.execute_batch(include_str!(
                "../../../../docs/design/inspiration/migrations/012-judge-pipeline.sql"
            ))
            .unwrap();
            conn.execute_batch(include_str!(
                "../../../../docs/design/inspiration/migrations/020-judge-model.sql"
            ))
            .unwrap();
            if upgrade {
                conn.execute_batch(include_str!(
                    "../../../../docs/design/inspiration/migrations/022-judgment-provenance.sql"
                ))
                .unwrap();
            }
            conn.execute_batch(
                "INSERT INTO pill(id) VALUES(1),(2);
                 INSERT INTO judge_run(id,status,stage,created_at,updated_at)
                     VALUES(1,'awaiting_human','panel','before','before');
                 INSERT INTO judge_item(id,run_id,pill_id,label,display_order,precheck,machine_total,status)
                     VALUES(1,1,1,'A',0,0.8,80,'human_needed'),(2,1,2,'B',1,0.8,60,'human_needed');",
            ).unwrap();
            Self { path, conn }
        }

        fn db(&self) -> &str {
            self.path.to_str().unwrap()
        }

        fn scored_items(&self) -> i64 {
            self.conn
                .query_row(
                    "SELECT count(*) FROM judge_item WHERE human_score IS NOT NULL",
                    [],
                    |r| r.get(0),
                )
                .unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            // Only remove the exact isolated file created by this fixture, never a user database.
            assert_eq!(self.path.parent(), Some(std::env::temp_dir().as_path()));
            assert!(
                self.path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("ie-judgment-test-")
            );
            std::fs::remove_file(&self.path).unwrap();
        }
    }

    #[test]
    fn models_display_distinguishes_legacy_small_and_trusted_calibrations() {
        let f = Fixture::new();
        f.conn.execute_batch("INSERT INTO judge_model(name,samples,agreements,weight,updated_at,calibration_source,calibration_trusted)
            VALUES('legacy-judge',100,90,1.8,'fixture','legacy',0),
                  ('small-judge',1,1,1.2,'fixture','human',0),
                  ('trusted-judge',20,20,1.8,'fixture','human',1);").unwrap();
        let text = models_text(f.db()).unwrap();
        assert!(text.contains("legacy，不应用"), "{text}");
        assert!(text.contains("human 少样本，不应用"), "{text}");
        assert!(text.contains("可信 human 校准"), "{text}");
    }

    #[test]
    fn reviews_keep_reviewer_timestamp_dimension_and_an_append_only_history() {
        let f = Fixture::new();
        f.conn
            .execute(
                "UPDATE judge_run SET input_json=?1 WHERE id=1",
                [json!({"dimension":"hierarchy"}).to_string()],
            )
            .unwrap();
        record_with_provenance(
            f.db(),
            1,
            90.0,
            Some("first"),
            &ReviewProvenance {
                source: ReviewSource::Human,
                reviewer: Some(" fixture-reviewer "),
                dimension: None,
            },
        )
        .unwrap();
        let first_screen = screen_text(f.db(), 1).unwrap();
        assert!(
            first_screen.contains("评分 90 [human]")
                && first_screen.contains("reviewer=fixture-reviewer")
        );
        assert!(first_screen.contains("dimension=hierarchy") && !first_screen.contains("at=未知"));
        record_with_provenance(
            f.db(),
            1,
            88.0,
            Some("second"),
            &ReviewProvenance {
                source: ReviewSource::Human,
                reviewer: Some("fixture-reviewer"),
                dimension: Some("restraint"),
            },
        )
        .unwrap();
        let reviews: Vec<(f64, String, String, String, String)> = f
            .conn
            .prepare(
                "SELECT score,reviewer,recorded_at,dimension,note
            FROM judge_review ORDER BY id",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(reviews.len(), 2);
        assert_eq!(reviews[0].0, 90.0);
        assert_eq!(reviews[0].1, "fixture-reviewer");
        assert!(!reviews[0].2.is_empty());
        assert_eq!(
            (&reviews[0].3, &reviews[0].4),
            (&"hierarchy".to_string(), &"first".to_string())
        );
        assert_eq!(reviews[1].0, 88.0);
        assert_eq!(
            (&reviews[1].3, &reviews[1].4),
            (&"restraint".to_string(), &"second".to_string())
        );
    }

    #[test]
    fn explicit_human_reviews_require_a_nonblank_reviewer_before_writing() {
        let f = Fixture::new();
        for reviewer in [None, Some(""), Some("   "), Some("\u{2003}")] {
            assert!(
                fill_with_opts(&FillOpts {
                    db: f.db(),
                    run_id: 1,
                    scores: "A=90,B=10",
                    note: None,
                    provenance: ReviewProvenance {
                        source: ReviewSource::Human,
                        reviewer,
                        dimension: None
                    },
                })
                .is_err()
            );
            assert!(
                record_with_provenance(
                    f.db(),
                    1,
                    90.0,
                    None,
                    &ReviewProvenance {
                        source: ReviewSource::Human,
                        reviewer,
                        dimension: None,
                    }
                )
                .is_err()
            );
        }
        assert_eq!(f.scored_items(), 0);
    }

    #[test]
    fn migration_preserves_all_old_scores_and_weights_as_unknown_legacy() {
        let f = Fixture::with_provenance(false);
        f.conn.execute("UPDATE judge_item SET human_score=73,human_note='unknown old review',status='done' WHERE id=1",[]).unwrap();
        f.conn.execute("INSERT INTO judge_model(name,samples,agreements,weight,updated_at) VALUES('old-demo',100,99,1.9,'old')",[]).unwrap();
        f.conn
            .execute_batch(include_str!(
                "../../../../docs/design/inspiration/migrations/022-judgment-provenance.sql"
            ))
            .unwrap();
        let text = screen_text(f.db(), 1).unwrap();
        assert!(
            text.contains("评分 73 [legacy]") && text.contains("reviewer=未知 at=未知"),
            "{text}"
        );
        let review: (f64, String, Option<String>, Option<String>) = f
            .conn
            .query_row(
                "SELECT score,source,reviewer,recorded_at FROM judge_review",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(review, (73.0, "legacy".into(), None, None));
        let archived: (f64, String, i64) = f
            .conn
            .query_row(
                "SELECT weight,calibration_source,calibration_trusted FROM judge_model_history",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(archived, (1.9, "legacy".into(), 0));
        assert_eq!(
            crate::panel::calibrated_weight(&f.conn, "old-demo", 1.0).unwrap(),
            1.0
        );
        let version: String = f
            .conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(version, "23");
    }

    #[test]
    fn trusted_calibration_is_applied_only_at_twenty_real_pairs() {
        let f = Fixture::new();
        let pairs = json!({"pairs":[{"i":"A","j":"B","votes":[["judge-one",1.0]]}]}).to_string();
        for run in 1..=20 {
            if run > 1 {
                f.conn
                    .execute(
                        "INSERT INTO judge_run(id,status,stage,created_at,updated_at)
                    VALUES(?1,'awaiting_human','panel','fixture','fixture')",
                        [run],
                    )
                    .unwrap();
                f.conn.execute("INSERT INTO judge_item(run_id,pill_id,label,display_order,precheck,machine_total,status)
                    SELECT ?1,pill_id,label,display_order,precheck,machine_total,'human_needed'
                    FROM judge_item WHERE run_id=1",[run]).unwrap();
            }
            f.conn
                .execute(
                    "UPDATE judge_run SET judge_json=?1 WHERE id=?2",
                    params![pairs, run],
                )
                .unwrap();
            fill_with_opts(&FillOpts {
                db: f.db(),
                run_id: run,
                scores: "A=90,B=10",
                note: None,
                provenance: ReviewProvenance {
                    source: ReviewSource::Human,
                    reviewer: Some("fixture-reviewer"),
                    dimension: None,
                },
            })
            .unwrap();
            if run == 19 {
                calibrate(f.db()).unwrap();
                assert_eq!(
                    crate::panel::calibrated_weight(&f.conn, "judge-one", 1.0).unwrap(),
                    1.0
                );
            }
        }
        calibrate(f.db()).unwrap();
        let row: (i64, i64, String, i64) = f
            .conn
            .query_row(
                "SELECT samples,agreements,calibration_source,calibration_trusted
            FROM judge_model WHERE name='judge-one'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(row, (20, 20, "human".into(), 1));
        assert!(crate::panel::calibrated_weight(&f.conn, "judge-one", 1.0).unwrap() > 1.0);
    }

    #[test]
    fn panel_never_loads_legacy_weights_even_with_many_samples() {
        let f = Fixture::new();
        f.conn.execute("INSERT INTO judge_model(name,samples,agreements,weight,updated_at) VALUES('old-demo',100,99,1.9,'old')",[]).unwrap();
        assert_eq!(
            crate::panel::calibrated_weight(&f.conn, "old-demo", 0.75).unwrap(),
            0.75
        );
        f.conn
            .execute(
                "UPDATE judge_model SET calibration_trusted=1 WHERE name='old-demo'",
                [],
            )
            .unwrap();
        assert_eq!(
            crate::panel::calibrated_weight(&f.conn, "old-demo", 0.75).unwrap(),
            0.75,
            "the source guard is required independently of the trusted flag"
        );
    }

    #[test]
    fn untrusted_batches_cannot_replace_accepted_human_reviews() {
        for source in [ReviewSource::Demo, ReviewSource::Ai, ReviewSource::Legacy] {
            let f = Fixture::new();
            record_with_provenance(
                f.db(),
                1,
                90.0,
                None,
                &ReviewProvenance {
                    source: ReviewSource::Human,
                    reviewer: Some("fixture-reviewer"),
                    dimension: None,
                },
            )
            .unwrap();
            let before = screen_text(f.db(), 1).unwrap();
            assert!(
                fill_with_opts(&FillOpts {
                    db: f.db(),
                    run_id: 1,
                    scores: "B=60,A=30",
                    note: None,
                    provenance: ReviewProvenance {
                        source,
                        reviewer: None,
                        dimension: None
                    },
                })
                .is_err(),
                "{source:?} must not replace accepted human review"
            );
            assert!(
                record_with_provenance(
                    f.db(),
                    1,
                    30.0,
                    None,
                    &ReviewProvenance {
                        source,
                        reviewer: None,
                        dimension: None,
                    }
                )
                .is_err()
            );
            assert_eq!(screen_text(f.db(), 1).unwrap(), before);
            assert_eq!(
                f.scored_items(),
                1,
                "the earlier valid B must also remain unwritten"
            );
        }
    }

    #[test]
    fn all_invalid_scores_are_rejected_before_any_persistent_change() {
        let f = Fixture::new();
        let before = screen_text(f.db(), 1).unwrap();
        for scores in [
            "A=90,B=NaN",
            "A=90,B=inf",
            "A=90,B=-inf",
            "A=90,B=101",
            "A=90,B=-1",
            "A=90,B=oops",
            "A=90,Z=70",
            "A=90,broken",
            "A=90,a=10",
            " , ; ",
        ] {
            assert!(fill(f.db(), 1, scores, None).is_err(), "accepted {scores}");
            assert_eq!(
                screen_text(f.db(), 1).unwrap(),
                before,
                "changed after {scores}"
            );
        }
        for score in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 101.0] {
            assert!(record(f.db(), 1, score, None).is_err());
        }
        assert_eq!(f.scored_items(), 0);
        let reviews: i64 = f
            .conn
            .query_row("SELECT count(*) FROM judge_review", [], |r| r.get(0))
            .unwrap();
        assert_eq!(reviews, 0);
    }

    #[test]
    fn recompute_failure_rolls_back_scores_queue_finals_and_audit_for_fill_and_record() {
        for batch in [true, false] {
            let f = Fixture::new();
            f.conn.execute_batch("CREATE TRIGGER fixture_recompute_failure BEFORE UPDATE OF final_json ON judge_run
                BEGIN SELECT RAISE(ABORT,'fixture recompute failure'); END;").unwrap();
            let before = screen_text(f.db(), 1).unwrap();
            let provenance = ReviewProvenance {
                source: ReviewSource::Human,
                reviewer: Some("fixture-reviewer"),
                dimension: None,
            };
            let result = if batch {
                fill_with_opts(&FillOpts {
                    db: f.db(),
                    run_id: 1,
                    scores: "A=90,B=10",
                    note: None,
                    provenance,
                })
            } else {
                record_with_provenance(f.db(), 1, 90.0, None, &provenance)
            };
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("fixture recompute failure")
            );
            assert_eq!(screen_text(f.db(), 1).unwrap(), before);
            assert_eq!(f.scored_items(), 0);
            let (finals, reviews): (i64, i64) = f
                .conn
                .query_row(
                    "SELECT
                (SELECT count(*) FROM judge_item WHERE final_total IS NOT NULL),
                (SELECT count(*) FROM judge_review)",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!((finals, reviews), (0, 0));
        }
    }

    #[test]
    fn run_input_snapshots_keep_dimension_and_model_configuration() {
        let input = input_metadata(&RunOpts {
            db: "unused",
            pills: vec![1, 2],
            provider: "fixture",
            model: "fixture-model",
            base_url: "unused",
            thinking: 0,
            seed: 7,
            repeats: 3,
            dimension: "hierarchy".into(),
            rpm: 0,
        });
        assert_eq!(input["dimension"], "hierarchy");
        assert_eq!(input["provider"], "fixture");
        assert_eq!(input["model"], "fixture-model");
        assert_eq!(input["repeats"], 3);
        let panel = crate::panel::input_metadata(
            &crate::panel::PanelOpts {
                db: "unused",
                pills: vec![1, 2],
                judges: vec!["judge-one".into()],
                cfg_path: std::path::Path::new("unused"),
                repeats: 4,
                dimension: "restraint".into(),
                seed: 9,
            },
            vec![
                json!({"name":"judge-one","provider":"fixture","model":"fixture-model","weight":1.0}),
            ],
        );
        assert_eq!(panel["dimension"], "restraint");
        assert_eq!(panel["repeats"], 4);
        assert_eq!(panel["judge_configs"][0]["provider"], "fixture");
        assert_eq!(panel["judge_configs"][0]["model"], "fixture-model");
    }

    #[test]
    fn screen_labels_demo_scores_instead_of_presenting_them_as_human() {
        let f = Fixture::new();
        record(f.db(), 1, 90.0, None).unwrap();
        let text = screen_text(f.db(), 1).unwrap();
        assert!(
            text.contains("评分 90 [demo]"),
            "provenance must be visible: {text}"
        );
        assert!(
            !text.contains("人工 90"),
            "demo must never be labeled as human"
        );
    }

    #[test]
    fn screen_renders_panel_combined_votes_and_disagreement() {
        let f = Fixture::new();
        f.conn.execute("UPDATE judge_run SET judge_json=?1 WHERE id=1", [json!({"pairs":[
            {"i":"A","j":"B","combined":0.75,"votes":[["judge-one",1.0],["judge-two",0.0]],"disputed":true}
        ]}).to_string()]).unwrap();
        let text = screen_text(f.db(), 1).unwrap();
        assert!(
            text.contains("A vs B → A"),
            "combined must not be rendered as a tie: {text}"
        );
        assert!(
            text.contains("judge-one=1.00") && text.contains("judge-two=0.00"),
            "{text}"
        );
        assert!(text.contains("裁判分歧"), "{text}");
    }

    #[test]
    fn untrusted_reviews_do_not_change_final_scores_or_complete_human_queue() {
        for source in [ReviewSource::Demo, ReviewSource::Ai, ReviewSource::Legacy] {
            let f = Fixture::new();
            fill_with_opts(&FillOpts {
                db: f.db(),
                run_id: 1,
                scores: "A=90,B=10",
                note: None,
                provenance: ReviewProvenance {
                    source,
                    reviewer: Some("not-a-human"),
                    dimension: None,
                },
            })
            .unwrap();
            let text = screen_text(f.db(), 1).unwrap();
            assert!(
                text.contains("awaiting_human"),
                "{source:?} cannot complete a real review queue: {text}"
            );
            let final_a: f64 = f
                .conn
                .query_row("SELECT final_total FROM judge_item WHERE id=1", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(
                final_a, 80.0,
                "{source:?} must not be blended into final production scores"
            );
        }
    }

    #[test]
    fn real_reviews_are_counted_but_small_calibrations_are_not_applied() {
        let f = Fixture::new();
        f.conn
            .execute(
                "UPDATE judge_run SET judge_json=?1 WHERE id=1",
                [json!({"pairs":[{"i":"A","j":"B","votes":[["judge-one",1.0]]}]}).to_string()],
            )
            .unwrap();
        fill_with_opts(&FillOpts {
            db: f.db(),
            run_id: 1,
            scores: "A=90,B=10",
            note: None,
            provenance: ReviewProvenance {
                source: ReviewSource::Human,
                reviewer: Some("fixture-reviewer"),
                dimension: None,
            },
        })
        .unwrap();
        calibrate(f.db()).unwrap();
        let (n, agreements, proposed): (i64, i64, f64) = f
            .conn
            .query_row(
                "SELECT samples,agreements,weight FROM judge_model WHERE name='judge-one'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((n, agreements), (1, 1));
        assert!(proposed > 1.0, "small-sample statistics remain visible");
        assert_eq!(
            crate::panel::calibrated_weight(&f.conn, "judge-one", 1.0).unwrap(),
            1.0,
            "fewer than 20 paired comparisons must not change panel weights"
        );
        let source: String = f
            .conn
            .query_row(
                "SELECT calibration_source FROM judge_model WHERE name='judge-one'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(source, "human");
    }

    #[test]
    fn calibration_without_samples_resets_current_weights_but_preserves_history() {
        let f = Fixture::new();
        f.conn.execute("INSERT INTO judge_model(name,samples,agreements,weight,updated_at) VALUES('old-demo',90,89,1.9,'old')", []).unwrap();
        calibrate(f.db()).unwrap();
        let current: (i64, f64, i64) = f
            .conn
            .query_row(
                "SELECT samples,weight,calibration_trusted FROM judge_model WHERE name='old-demo'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            current,
            (0, 1.0, 0),
            "no real samples must not leave stale weights active"
        );
        let archived: f64 = f
            .conn
            .query_row(
                "SELECT weight FROM judge_model_history WHERE name='old-demo' ORDER BY id LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(archived, 1.9);
    }

    #[test]
    fn calibration_requires_the_same_reviewer_for_a_pair() {
        let f = Fixture::new();
        f.conn
            .execute(
                "UPDATE judge_run SET judge_json=?1 WHERE id=1",
                [json!({"pairs":[{"i":"A","j":"B","votes":[["judge-one",1.0]]}]}).to_string()],
            )
            .unwrap();
        for (item, score, reviewer) in [(1, 90.0, "reviewer-one"), (2, 10.0, "reviewer-two")] {
            record_with_provenance(
                f.db(),
                item,
                score,
                None,
                &ReviewProvenance {
                    source: ReviewSource::Human,
                    reviewer: Some(reviewer),
                    dimension: None,
                },
            )
            .unwrap();
        }
        calibrate(f.db()).unwrap();
        let samples: i64 = f
            .conn
            .query_row(
                "SELECT coalesce(sum(samples),0) FROM judge_model",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            samples, 0,
            "different reviewers are not a paired comparison"
        );
    }

    #[test]
    fn calibration_excludes_demo_and_legacy_scores() {
        let f = Fixture::new();
        f.conn
            .execute(
                "UPDATE judge_run SET judge_json=?1 WHERE id=1",
                [
                    json!({"pairs":[{"i":"A","j":"B","votes":[["judge-one",1.0]],"combined":1.0}]})
                        .to_string(),
                ],
            )
            .unwrap();
        f.conn
            .execute("UPDATE judge_item SET human_score=10 WHERE id=2", [])
            .unwrap();
        record(f.db(), 1, 90.0, Some("demo, not a human review")).unwrap();
        calibrate(f.db()).unwrap();
        let samples: i64 = f
            .conn
            .query_row(
                "SELECT coalesce(sum(samples),0) FROM judge_model",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            samples, 0,
            "neither demo nor unknown legacy scores may train real weights"
        );
    }

    #[test]
    fn fill_rejects_duplicate_labels_without_writing() {
        let f = Fixture::new();
        assert!(fill(f.db(), 1, "A=82,a=70", None).is_err());
        assert_eq!(f.scored_items(), 0);
    }

    #[test]
    fn fill_rejects_invalid_later_item_without_partial_write() {
        let f = Fixture::new();
        assert!(fill(f.db(), 1, "A=82,B=101", None).is_err());
        assert_eq!(
            f.scored_items(),
            0,
            "the entire batch must remain unchanged"
        );
    }
}
