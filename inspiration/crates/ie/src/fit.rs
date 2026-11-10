//! 内核拟合与演练数据。
//!
//! 特征先按维度标准化（z-score），权重因此是"每 1σ 特征变化带来的偏好变化"，
//! 与内核卡里"+1.2σ"的说法一致，也避免原始 0–1 尺度把权重压扁。
//!
//! `simulate` 只用于把管线跑通：用已知的个人内核生成合成判定，再让 `fit` 把它估回来。
//! 它验证的是代码路径与可恢复性，不代表真实用户结论。

use anyhow::Result;
use rusqlite::{Connection, params};
use std::collections::HashMap;

pub struct Dim {
    pub id: i64,
    pub term: String,
    pub label: String,
}

struct Stats {
    mean: Vec<f32>,
    std: Vec<f32>,
    count: Vec<f32>,
}

fn sigmoid(z: f32) -> f32 {
    1.0 / (1.0 + (-z).exp())
}

fn dims(conn: &Connection) -> Result<Vec<Dim>> {
    let mut stmt = conn.prepare(
        "SELECT id, term, label FROM vocab_term WHERE kind='primitive' AND status='active' ORDER BY id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Dim {
                id: r.get(0)?,
                term: r.get(1)?,
                label: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn raw_vectors(conn: &Connection, dims: &[Dim]) -> Result<HashMap<i64, Vec<Option<f32>>>> {
    let mut index: HashMap<i64, usize> = HashMap::new();
    for (i, d) in dims.iter().enumerate() {
        index.insert(d.id, i);
    }
    let mut stmt = conn.prepare("SELECT work_id, primitive_term_id, score FROM work_primitive")?;
    let mut map: HashMap<i64, Vec<Option<f32>>> = HashMap::new();
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, f32>(2)?,
        ))
    })?;
    for row in rows {
        let (w, term, score) = row?;
        if let Some(&i) = index.get(&term) {
            let v = map.entry(w).or_insert_with(|| vec![None; dims.len()]);
            v[i] = Some(score);
        }
    }
    Ok(map)
}

fn stats_of(vectors: &HashMap<i64, Vec<Option<f32>>>, n: usize) -> Stats {
    let mut mean = vec![0.0f32; n];
    let mut sq = vec![0.0f32; n];
    let mut cnt = vec![0.0f32; n];
    for v in vectors.values() {
        for i in 0..n {
            if let Some(x) = v[i] {
                mean[i] += x;
                sq[i] += x * x;
                cnt[i] += 1.0;
            }
        }
    }
    let mut std = vec![1.0f32; n];
    let count = cnt.clone();
    for i in 0..n {
        if cnt[i] > 1.0 {
            mean[i] /= cnt[i];
            let var = (sq[i] / cnt[i] - mean[i] * mean[i]).max(1e-6);
            std[i] = var.sqrt();
        }
    }
    Stats { mean, std, count }
}

fn standardize(
    vectors: &HashMap<i64, Vec<Option<f32>>>,
    s: &Stats,
) -> HashMap<i64, Vec<Option<f32>>> {
    vectors
        .iter()
        .map(|(w, v)| {
            let z = v
                .iter()
                .enumerate()
                .map(|(i, x)| x.map(|x| (x - s.mean[i]) / s.std[i]))
                .collect();
            (*w, z)
        })
        .collect()
}

struct Pair {
    diff: Vec<f32>,
    y: f32,
}

fn build_pairs(
    judgments: &[(i64, i64, String)],
    vectors: &HashMap<i64, Vec<Option<f32>>>,
    len: usize,
) -> Vec<Pair> {
    let mut out = Vec::new();
    for (a, b, outcome) in judgments {
        let (Some(va), Some(vb)) = (vectors.get(a), vectors.get(b)) else {
            continue;
        };
        let y = match outcome.as_str() {
            "a_wins" => 1.0,
            "b_wins" => 0.0,
            _ => continue,
        };
        let mut diff = vec![0.0f32; len];
        let mut any = false;
        for i in 0..len {
            if let (Some(x), Some(z)) = (va[i], vb[i]) {
                diff[i] = x - z;
                any = true;
            }
        }
        if any {
            out.push(Pair { diff, y });
        }
    }
    out
}

fn fit_delta(pairs: &[Pair], len: usize, lambda: f32, iters: usize, lr: f32) -> Vec<f32> {
    let mut d = vec![0.0f32; len];
    if pairs.is_empty() {
        return d;
    }
    for _ in 0..iters {
        let mut grad = vec![0.0f32; len];
        for p in pairs {
            let z: f32 = p.diff.iter().zip(d.iter()).map(|(x, w)| x * w).sum();
            let err = p.y - sigmoid(z);
            for (g, d) in grad.iter_mut().zip(p.diff.iter()) {
                *g += err * d;
            }
        }
        for i in 0..len {
            d[i] += lr * (grad[i] / pairs.len() as f32 - lambda * d[i]);
        }
    }
    d
}

fn accuracy(delta: &[f32], pairs: &[Pair]) -> f32 {
    if pairs.is_empty() {
        return 0.0;
    }
    let ok = pairs
        .iter()
        .filter(|p| {
            let z: f32 = p.diff.iter().zip(delta.iter()).map(|(x, w)| x * w).sum();
            (sigmoid(z) > 0.5) == (p.y > 0.5)
        })
        .count();
    ok as f32 / pairs.len() as f32
}

fn corr(a: &[f32], b: &[f32]) -> Option<f32> {
    let n = a.len() as f32;
    if n < 3.0 {
        return None;
    }
    let ma = a.iter().sum::<f32>() / n;
    let mb = b.iter().sum::<f32>() / n;
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    let mut syy = 0.0;
    for (x, y) in a.iter().zip(b.iter()) {
        sxy += (x - ma) * (y - mb);
        sxx += (x - ma).powi(2);
        syy += (y - mb).powi(2);
    }
    if sxx <= 1e-9 || syy <= 1e-9 {
        return None;
    }
    Some(sxy / (sxx.sqrt() * syy.sqrt()))
}

fn source_delta(conn: &Connection, person: &str) -> Option<HashMap<String, f32>> {
    let consent: String = conn
        .query_row(
            "SELECT consent_json FROM person WHERE id=?1",
            params![person],
            |r| r.get(0),
        )
        .ok()?;
    let v: serde_json::Value = serde_json::from_str(&consent).ok()?;
    let d = v.get("source_delta")?.clone();
    serde_json::from_value(d).ok()
}

pub fn run(db: &str, person: &str, seed: u64) -> Result<()> {
    let conn = Connection::open(db)?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    let dims = dims(&conn)?;
    let raw = raw_vectors(&conn, &dims)?;
    let stats = stats_of(&raw, dims.len());
    let vectors = standardize(&raw, &stats);

    let mut stmt = conn.prepare(
        "SELECT work_id, opponent_work_id, outcome FROM judgment
         WHERE actor_id=?1 AND kind='pairwise' AND opponent_work_id IS NOT NULL ORDER BY id",
    )?;
    let judgments: Vec<(i64, i64, String)> = stmt
        .query_map(params![person], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let pairs = build_pairs(&judgments, &vectors, dims.len());
    if pairs.len() < 10 {
        println!(
            "判定不足（可用 {} 对）；先跑 simulate 或接入真实判定",
            pairs.len()
        );
        return Ok(());
    }

    let mut idx: Vec<usize> = (0..pairs.len()).collect();
    idx.sort_by_key(|i| (i.wrapping_mul(2_654_435_761) as u64) ^ seed);
    let hold_n = (pairs.len() / 5).max(3);
    let ho: Vec<Pair> = idx[..hold_n]
        .iter()
        .map(|i| Pair {
            diff: pairs[*i].diff.clone(),
            y: pairs[*i].y,
        })
        .collect();
    let tr: Vec<Pair> = idx[hold_n..]
        .iter()
        .map(|i| Pair {
            diff: pairs[*i].diff.clone(),
            y: pairs[*i].y,
        })
        .collect();

    let delta = fit_delta(&tr, dims.len(), 0.01, 2500, 0.35);
    let train_acc = accuracy(&delta, &tr);
    let hold_acc = accuracy(&delta, &ho);
    let n = pairs.len();
    let maturity = if n < 20 {
        "L0"
    } else if n < 60 {
        "L1"
    } else if n < 150 {
        "L2"
    } else {
        "L3"
    };

    // 只在真正有数据的维度上比，否则大量 0 会把相关系数虚高
    let covered: Vec<usize> = (0..dims.len()).filter(|i| stats.count[*i] > 50.0).collect();
    let rho = source_delta(&conn, person).and_then(|s| {
        let t: Vec<f32> = covered
            .iter()
            .map(|i| s.get(&dims[*i].term).copied().unwrap_or(0.0) * stats.std[*i])
            .collect();
        let d: Vec<f32> = covered.iter().map(|i| delta[*i]).collect();
        corr(&d, &t)
    });

    let version: i64 = conn
        .query_row(
            "SELECT coalesce(max(version),0)+1 FROM kernel WHERE person_id=?1",
            params![person],
            |r| r.get(0),
        )
        .unwrap_or(1);
    conn.execute(
        "UPDATE kernel SET status='superseded' WHERE person_id=?1 AND status='active'",
        params![person],
    )?;
    let mut weights = serde_json::Map::new();
    for (d, w) in dims.iter().zip(delta.iter()) {
        weights.insert(d.term.clone(), serde_json::json!(w));
    }
    let fit_json = serde_json::json!({
        "n": n, "train_acc": train_acc, "hold_acc": hold_acc, "rho": rho,
        "standardized": true,
    });
    conn.execute(
        "INSERT INTO kernel(person_id, version, model_kind, weights_json, prior_json, fit_json, maturity, status, created_at)
         VALUES(?1,?2,'hierarchical-bt',?3,'{}',?4,?5,'active',?6)",
        params![
            person,
            version,
            serde_json::Value::Object(weights).to_string(),
            fit_json.to_string(),
            maturity,
            crate::now_iso()
        ],
    )?;
    let kernel_id = conn.last_insert_rowid();

    let mut order: Vec<usize> = (0..dims.len()).collect();
    order.sort_by(|a, b| delta[*b].abs().partial_cmp(&delta[*a].abs()).unwrap());
    let mut claims = 0;
    for i in order.into_iter().take(5) {
        let w = delta[i];
        if w.abs() < 0.5 {
            continue;
        }
        let kind = if w > 0.0 { "prefer" } else { "avoid" };
        let dir = if w > 0.0 { "更看重" } else { "更不在意" };
        let statement = format!(
            "你在「{}」上{}（{:+.2}σ，基于 {} 次判定）",
            dims[i].label, dir, w, n
        );
        conn.execute(
            "INSERT INTO kernel_claim(kernel_id, kind, statement, primitive_term_id, effect, confidence, evidence_count, counterexample)
             VALUES(?1,?2,?3,?4,?5,?6,?7,NULL)",
            params![
                kernel_id,
                kind,
                statement,
                dims[i].id,
                w,
                (0.35 + 0.4 * (w.abs() / 1.5).min(1.0)).min(0.9),
                n as i64
            ],
        )?;
        claims += 1;
    }
    conn.execute(
        "INSERT INTO kernel_claim(kernel_id, kind, statement, effect, confidence, evidence_count, counterexample)
         VALUES(?1,'signature',?2,NULL,0.3,?3,'样本量不足以确认跨领域一致性')",
        params![
            kernel_id,
            format!(
                "{maturity}：当前只覆盖像素可测的 {} 个基元，语义手法待视觉抽取",
                covered.len()
            ),
            n as i64
        ],
    )?;

    println!(
        "内核 v{version}：n={n} 训练准确率={train_acc:.3} 留出准确率={hold_acc:.3} 成熟度={maturity}"
    );
    if let Some(r) = rho {
        println!("与演练真值的相关系数 rho={r:.3}");
    }
    println!("主张 {claims} 条：");
    let mut stmt = conn.prepare(
        "SELECT kind, statement, effect FROM kernel_claim WHERE kernel_id=?1 ORDER BY id",
    )?;
    let rows = stmt.query_map(params![kernel_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<f32>>(2)?,
        ))
    })?;
    for row in rows {
        let (kind, s, eff) = row?;
        match eff {
            Some(e) => println!("  [{kind}] {s}  ({e:+.2})"),
            None => println!("  [{kind}] {s}"),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- 演练

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    fn f(&mut self) -> f32 {
        (self.next() % 10_000) as f32 / 10_000.0
    }
    fn pick(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

pub fn simulate(db: &str, person: &str, n: usize, seed: u64, active: bool) -> Result<()> {
    let conn = Connection::open(db)?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    let dims = dims(&conn)?;
    let raw = raw_vectors(&conn, &dims)?;
    let stats = stats_of(&raw, dims.len());
    let vectors = standardize(&raw, &stats);
    let works: Vec<i64> = vectors
        .iter()
        .filter(|(_, v)| v.iter().filter(|x| x.is_some()).count() >= 4)
        .map(|(w, _)| *w)
        .collect();
    println!("可用于演练的作品：{}", works.len());
    if works.len() < 20 {
        return Ok(());
    }

    let mut rng = Lcg(seed
        .wrapping_mul(2862933555777941757)
        .wrapping_add(3037000493));
    let covered: Vec<usize> = (0..dims.len())
        .filter(|i| vectors.values().filter(|v| v[*i].is_some()).count() > 50)
        .collect();
    let mut truth = vec![0.0f32; dims.len()];
    let mut chosen: Vec<usize> = Vec::new();
    while chosen.len() < 4 && chosen.len() < covered.len() {
        let i = covered[rng.pick(covered.len())];
        if !chosen.contains(&i) {
            chosen.push(i);
            truth[i] = if rng.f() > 0.5 { 1.0 } else { -1.0 } * (1.0 + rng.f() * 0.8);
        }
    }

    conn.execute(
        "INSERT OR IGNORE INTO person(id, display_name, locale, privacy_level, consent_json, reliability, created_at)
         VALUES(?1,'演练人格','zh-CN','local',?2,0.8,?3)",
        params![
            person,
            serde_json::json!({"demo": true, "source_delta": chosen.iter().map(|i| (dims[*i].term.clone(), truth[*i])).collect::<HashMap<_,_>>()}).to_string(),
            crate::now_iso()
        ],
    )?;
    conn.execute(
        "DELETE FROM judgment WHERE actor_id=?1 AND note='simulated'",
        params![person],
    )?;

    let tx = conn.unchecked_transaction()?;
    let now = crate::now_iso();
    let mut made = 0usize;
    let mut pairs: Vec<Pair> = Vec::new();
    let mut est = vec![0.0f32; dims.len()];
    let mut guard = 0usize;
    while made < n && guard < n * 40 {
        guard += 1;
        let (a, b) = if active && made.is_multiple_of(10) && !pairs.is_empty() {
            // 不确定度采样：在候选里挑 p 最接近 0.5 的一对
            let mut best: Option<(f32, i64, i64)> = None;
            for _ in 0..200 {
                let x = works[rng.pick(works.len())];
                let y = works[rng.pick(works.len())];
                if x == y {
                    continue;
                }
                let z: f32 = (0..dims.len())
                    .map(|i| match (vectors[&x][i], vectors[&y][i]) {
                        (Some(p), Some(q)) => est[i] * (p - q),
                        _ => 0.0,
                    })
                    .sum();
                let score = (1.0 / (1.0 + (-z).exp())).clamp(1e-4, 1.0 - 1e-4);
                let unc = (score * (1.0 - score)).min(0.25);
                if best.map(|(u, _, _)| unc < u).unwrap_or(true) {
                    best = Some((unc, x, y));
                }
            }
            match best {
                Some((_, x, y)) => (x, y),
                None => continue,
            }
        } else {
            (works[rng.pick(works.len())], works[rng.pick(works.len())])
        };
        if a == b {
            continue;
        }
        let mut z = 0.0f32;
        let mut diff = vec![0.0f32; dims.len()];
        let mut any = false;
        for i in 0..dims.len() {
            if let (Some(x), Some(y)) = (vectors[&a][i], vectors[&b][i]) {
                diff[i] = x - y;
                z += truth[i] * diff[i];
                any = true;
            }
        }
        if !any {
            continue;
        }
        let p = 1.0 / (1.0 + (-z).exp());
        let y = if rng.f() < p { "a_wins" } else { "b_wins" };
        tx.execute(
            "INSERT INTO judgment(work_id, kind, opponent_work_id, context_json, population, outcome, grade, note, created_at, actor_id, visibility)
             VALUES(?1,'pairwise',?2,'{\"domain\":\"ui\"}','demo',?3,'C','simulated',?4,?5,'private')",
            params![a, b, y, now, person],
        )?;
        pairs.push(Pair {
            diff,
            y: if y == "a_wins" { 1.0 } else { 0.0 },
        });
        made += 1;
        if active && made.is_multiple_of(10) {
            est = fit_delta(&pairs, dims.len(), 0.05, 300, 0.35);
        }
    }
    tx.commit()?;
    println!(
        "已生成 {made} 次合成判定（{}；真值覆盖：{}）",
        if active {
            "主动选择"
        } else {
            "随机选择"
        },
        chosen
            .iter()
            .map(|i| dims[*i].label.clone())
            .collect::<Vec<_>>()
            .join("、")
    );
    Ok(())
}
