//! Frozen, local-only personal preference trials and a separate synthetic demo path.
//!
//! Human fits consume only personal::dataset's final-pixel evidence. Content-group
//! holdout precedes train-only normalization; both the dataset and normalization
//! are persisted, never refreshed from the library when ranking. Qualification is
//! an engineering trial gate, not stable aesthetics or cross-domain evidence.
//! Activation and ranking each require their own explicit opt-in. Demo judgments
//! and demo kernels never satisfy human thresholds or replace human active kernels.

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};

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

pub const SCOPE: &str = "ui-static";
pub const FEATURE_CONTRACT: &str = "pixel-personal-v1";

pub struct FitOpts<'a> {
    pub db: &'a str,
    pub person: &'a str,
    pub scope: &'a str,
    pub seed: u64,
    pub activate: bool,
}
pub struct RankOpts<'a> {
    pub db: &'a str,
    pub person: &'a str,
    pub scope: &'a str,
    pub candidates: &'a [i64],
    pub apply: bool,
}

pub fn fit_person(opts: &FitOpts<'_>) -> Result<serde_json::Value> {
    fit_dataset(
        opts,
        crate::personal::dataset(opts.db, opts.person, opts.scope)?,
    )
}

/// The only production caller supplies personal::dataset, never mutable work_primitive.
fn fit_dataset(opts: &FitOpts<'_>, mut dataset: serde_json::Value) -> Result<serde_json::Value> {
    anyhow::ensure!(
        opts.scope == SCOPE
            && dataset["scope"] == SCOPE
            && dataset["feature_contract"] == FEATURE_CONTRACT
            && dataset["person_id"] == opts.person,
        "dataset person/scope/feature contract mismatch"
    );
    sanitize_dataset(&mut dataset)?;
    let n = dataset["pairs"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("dataset pairs missing"))?
        .len();
    if n < 10 {
        return Ok(
            serde_json::json!({"person_id":opts.person,"scope":opts.scope,"feature_contract":FEATURE_CONTRACT,"training_source":"human","status":"cold_start","fitted":false,"activated":false,"kernel_id":null,"counts":dataset["counts"],"reasons":["fewer_than_10_usable_pairs"],"dataset":dataset}),
        );
    }
    let frozen = frozen_pairs(&dataset)?;
    let (train, holdout, grouping) = split_content_groups(&frozen, opts.seed)?;
    let normalizer = train_normalizer(&train)?;
    let train_pairs = normalized_pairs(&train, &normalizer)?;
    let holdout_pairs = normalized_pairs(&holdout, &normalizer)?;
    let weights = fit_delta(&train_pairs, 2, 0.05, 2500, 0.2);
    let weights_json = json!({"negative-space.active":weights[0],"detail.density":weights[1]});
    let validation = validate_trial(n, &grouping, &train_pairs, &holdout_pairs, &weights);
    let activated = opts.activate && validation["eligible_for_trial"] == true;
    let status = if activated { "active" } else { "draft" };
    let fit = json!({"format_version":1,"training_source":"human","scope":opts.scope,"feature_contract":FEATURE_CONTRACT,
        "dataset_hash":hash_json(&dataset),"dataset":dataset,"normalizer":normalizer,"lambda":0.05,"seed":opts.seed,
        "grouping":grouping,"validation":validation,
        "activation":{"requested":opts.activate,"explicit":activated,"reason":if activated {"explicit_trial_activation"} else if !opts.activate {"explicit_activation_not_requested"} else {"validation_failed"}},"note":"工程试用门槛，不代表稳定审美内核或跨域结论"});
    let mut conn = open_kernel_db(opts.db, true)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version: i64 = tx.query_row(
        "SELECT coalesce(max(version),0)+1 FROM kernel WHERE person_id=?1",
        [opts.person],
        |r| r.get(0),
    )?;
    if activated {
        tx.execute("UPDATE kernel SET status='superseded' WHERE person_id=?1 AND scope=?2 AND training_source='human' AND status='active'",params![opts.person,opts.scope])?;
    }
    tx.execute("INSERT INTO kernel(person_id,version,model_kind,weights_json,prior_json,fit_json,maturity,status,created_at,training_source,scope,feature_contract)
        VALUES(?1,?2,'pixel-bt-trial',?3,'{}',?4,'L0',?8,?5,'human',?6,?7)",
        params![opts.person,version,weights_json.to_string(),fit.to_string(),crate::now_iso(),opts.scope,FEATURE_CONTRACT,status])?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(
        json!({"person_id":opts.person,"scope":opts.scope,"feature_contract":FEATURE_CONTRACT,"training_source":"human","status":status,"maturity":"L0","fitted":true,"activated":activated,"kernel_id":id,"version":version,"counts":dataset["counts"],"fit":fit}),
    )
}

const TERMS: [&str; 2] = ["negative-space.active", "detail.density"];
#[derive(Clone)]
struct FrozenPair {
    snapshot: Value,
    a: [Option<f64>; 2],
    b: [Option<f64>; 2],
    y: f32,
}
fn feature_vector(value: &Value) -> [Option<f64>; 2] {
    TERMS.map(|t| {
        value[t]
            .as_f64()
            .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
    })
}
fn sanitize_dataset(dataset: &mut Value) -> Result<()> {
    let mut raw = dataset["pairs"]
        .as_array()
        .context("dataset pairs missing")?
        .clone();
    raw.sort_by_key(|p| std::cmp::Reverse(p["judgment_id"].as_i64().unwrap_or(0)));
    let mut seen = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut kept = Vec::new();
    let mut duplicates = 0u64;
    let mut zero = 0u64;
    let mut missing = 0u64;
    for p in raw {
        let a = p["a"]["artifact_id"]
            .as_i64()
            .context("artifact a missing")?;
        let b = p["b"]["artifact_id"]
            .as_i64()
            .context("artifact b missing")?;
        let key = p["pair_key"]
            .as_str()
            .context("pair key missing")?
            .to_owned();
        let canonical = (a.min(b), a.max(b));
        if seen.contains(&canonical) || keys.contains(&key) {
            duplicates += 1;
            continue;
        }
        seen.insert(canonical);
        keys.insert(key);
        let va = feature_vector(&p["a"]["features"]);
        let vb = feature_vector(&p["b"]["features"]);
        let common: Vec<f64> = (0..2).filter_map(|i| Some(va[i]? - vb[i]?)).collect();
        if common.is_empty() {
            missing += 1;
            continue;
        }
        if common.iter().all(|d| d.abs() <= 1e-12) {
            zero += 1;
            continue;
        }
        kept.push(p);
    }
    kept.sort_by_key(|p| p["judgment_id"].as_i64().unwrap_or(0));
    let counts = &mut dataset["counts"];
    let unique = counts["unique"]
        .as_u64()
        .unwrap_or(seen.len() as u64 + duplicates)
        .saturating_sub(duplicates);
    counts["unique"] = json!(unique);
    counts["usable"] = json!(kept.len());
    for (key, n) in [
        ("duplicates", duplicates),
        ("zero_difference", zero),
        ("missing_common_features", missing),
    ] {
        counts[key] = json!(counts[key].as_u64().unwrap_or(0) + n);
    }
    dataset["pairs"] = json!(kept);
    Ok(())
}
fn frozen_pairs(dataset: &Value) -> Result<Vec<FrozenPair>> {
    dataset["pairs"]
        .as_array()
        .context("dataset pairs missing")?
        .iter()
        .map(|p| {
            let y = match p["outcome"].as_str() {
                Some("a_wins") => 1.0,
                Some("b_wins") => 0.0,
                _ => anyhow::bail!("dataset contains non-decisive outcome"),
            };
            Ok(FrozenPair {
                snapshot: p.clone(),
                a: feature_vector(&p["a"]["features"]),
                b: feature_vector(&p["b"]["features"]),
                y,
            })
        })
        .collect()
}
fn split_content_groups(
    pairs: &[FrozenPair],
    seed: u64,
) -> Result<(Vec<FrozenPair>, Vec<FrozenPair>, Value)> {
    let mut groups = BTreeMap::<String, usize>::new();
    let mut artifact_groups = BTreeMap::new();
    for p in pairs {
        let group = p.snapshot["group_key"]
            .as_str()
            .filter(|g| !g.is_empty())
            .context("content group missing")?;
        *groups.entry(group.to_owned()).or_default() += 1;
        for arm in ["a", "b"] {
            let id = p.snapshot[arm]["artifact_id"]
                .as_i64()
                .context("artifact id missing")?;
            if let Some(previous) = artifact_groups.insert(id, group) {
                ensure!(previous == group, "artifact appears across content groups");
            }
        }
    }
    let mut order: Vec<String> = groups.keys().cloned().collect();
    order.sort_by_cached_key(|group| hash_json(&json!({"seed":seed,"group":group})));
    let mut held = BTreeSet::new();
    let mut hold_n = 0;
    if pairs.len() >= 20 {
        let target = (pairs.len() / 5).max(8);
        for group in order {
            let size = groups[&group];
            if hold_n < target && groups.len() - held.len() > 3 && pairs.len() - hold_n - size >= 10
            {
                held.insert(group);
                hold_n += size;
            }
        }
    }
    let (hold, train): (Vec<_>, Vec<_>) = pairs
        .iter()
        .cloned()
        .partition(|p| held.contains(p.snapshot["group_key"].as_str().unwrap()));
    let train_groups: Vec<_> = groups
        .keys()
        .filter(|g| !held.contains(*g))
        .cloned()
        .collect();
    let train_snapshot: Vec<_> = train.iter().map(|p| &p.snapshot).collect();
    let hold_snapshot: Vec<_> = hold.iter().map(|p| &p.snapshot).collect();
    let grouping = json!({"method":"content_hash_group_holdout","independent_groups":groups.len(),"train_groups":train_groups,"holdout_groups":held,
        "train_snapshot_hash":hash_json(&json!(train_snapshot)),"holdout_snapshot_hash":hash_json(&json!(hold_snapshot)),
        "train_judgment_ids":train.iter().map(|p|p.snapshot["judgment_id"].clone()).collect::<Vec<_>>(),
        "holdout_judgment_ids":hold.iter().map(|p|p.snapshot["judgment_id"].clone()).collect::<Vec<_>>()});
    Ok((train, hold, grouping))
}
fn validate_trial(
    n: usize,
    grouping: &Value,
    train: &[Pair],
    hold: &[Pair],
    weights: &[f32],
) -> Value {
    let acc = accuracy(weights, hold) as f64;
    let majority = train.iter().filter(|p| p.y > 0.5).count() * 2 > train.len();
    let baseline = if hold.is_empty() {
        0.0
    } else {
        hold.iter().filter(|p| (p.y > 0.5) == majority).count() as f64 / hold.len() as f64
    };
    let wilson = wilson_lower((acc * hold.len() as f64).round() as usize, hold.len());
    let mut reasons = Vec::new();
    if n < 20 {
        reasons.push("fewer_than_20_usable_pairs");
    }
    if grouping["independent_groups"].as_u64().unwrap_or(0) < 5 {
        reasons.push("fewer_than_5_content_groups");
    }
    if train.len() < 10 {
        reasons.push("fewer_than_10_training_pairs");
    }
    if grouping["train_groups"].as_array().map_or(0, Vec::len) < 3 {
        reasons.push("fewer_than_3_training_groups");
    }
    if hold.len() < 8 {
        reasons.push("fewer_than_8_holdout_pairs");
    }
    if acc <= baseline {
        reasons.push("not_better_than_constant_baseline");
    }
    if wilson <= 0.5 {
        reasons.push("wilson_lower_not_above_half");
    }
    json!({"eligible_for_trial":reasons.is_empty(),"reasons":reasons,"train_n":train.len(),"holdout_n":hold.len(),"train_accuracy":accuracy(weights,train),"accuracy":acc,
        "constant_baseline_accuracy":baseline,"constant_prediction":if majority {"a_wins"} else {"b_wins"},"wilson_lower":wilson,"wilson_confidence":0.95,
        "thresholds":{"usable":20,"independent_groups":5,"train_n":10,"train_groups":3,"holdout_n":8,"wilson_lower_exclusive":0.5},
        "interpretation":"engineering_trial_only_not_stable_aesthetic_evidence"})
}
fn wilson_lower(correct: usize, n: usize) -> f64 {
    if n == 0 {
        return 0.0;
    }
    let n = n as f64;
    let p = correct as f64 / n;
    let z = 1.959963984540054;
    (p + z * z / (2.0 * n) - z * ((p * (1.0 - p) + z * z / (4.0 * n)) / n).sqrt())
        / (1.0 + z * z / n)
}
fn train_normalizer(pairs: &[FrozenPair]) -> Result<Value> {
    let mut artifacts = BTreeMap::new();
    for p in pairs {
        for (arm, v) in [("a", p.a), ("b", p.b)] {
            let id = p.snapshot[arm]["artifact_id"]
                .as_i64()
                .context("artifact id missing")?;
            if let Some(old) = artifacts.insert(id, v) {
                ensure!(old == v, "conflicting frozen phi for one artifact");
            }
        }
    }
    let mut mean = [0.0; 2];
    let mut std = [1.0; 2];
    let mut mask = [false; 2];
    for i in 0..2 {
        let values: Vec<f64> = artifacts.values().filter_map(|v| v[i]).collect();
        if !values.is_empty() {
            mean[i] = values.iter().sum::<f64>() / values.len() as f64;
        }
        if values.len() > 1 {
            let variance =
                values.iter().map(|v| (v - mean[i]).powi(2)).sum::<f64>() / values.len() as f64;
            if variance > 1e-12 {
                std[i] = variance.sqrt();
                mask[i] = true;
            }
        }
    }
    Ok(
        json!({"terms":TERMS,"mask":mask,"mean":mean,"std":std,"fit_on":"train_only","artifact_ids":artifacts.keys().collect::<Vec<_>>()}),
    )
}
fn normalized_pairs(pairs: &[FrozenPair], norm: &Value) -> Result<Vec<Pair>> {
    let std = norm["std"].as_array().context("normalizer std missing")?;
    let mask = norm["mask"].as_array().context("normalizer mask missing")?;
    Ok(pairs
        .iter()
        .map(|p| Pair {
            diff: (0..2)
                .map(|i| match (p.a[i], p.b[i]) {
                    (Some(a), Some(b)) if mask[i] == true => {
                        ((a - b) / std[i].as_f64().unwrap()) as f32
                    }
                    _ => 0.0,
                })
                .collect(),
            y: p.y,
        })
        .collect())
}
fn hash_json(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}
fn open_kernel_db(db: &str, write: bool) -> Result<Connection> {
    if write {
        crate::db::require_current(std::path::Path::new(db))?;
    }
    let conn = Connection::open_with_flags(
        db,
        if write {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        },
    )?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    if !write {
        let version: String = conn.query_row(
            "SELECT value FROM schema_meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            version
                .parse::<u32>()
                .is_ok_and(|v| (25..=crate::db::LATEST_SCHEMA).contains(&v)),
            "只读个人内核需要 schema25 或兼容新版本；不会自动迁移"
        );
    }
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(conn)
}
pub fn show(db: &str, person: &str, scope: &str) -> Result<Value> {
    let conn = open_kernel_db(db, false)?;
    let mut stmt = conn.prepare("SELECT id,version,training_source,status,maturity,feature_contract,weights_json,fit_json FROM kernel WHERE person_id=?1 AND scope=?2 ORDER BY version DESC")?;
    let rows = stmt
        .query_map(params![person, scope], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let kernels: Vec<Value> = rows.into_iter().map(|(id,version,source,status,maturity,contract,weights,fit)| Ok(json!({"id":id,"version":version,"person_id":person,"scope":scope,"training_source":source,"status":status,"maturity":maturity,"feature_contract":contract,"weights":serde_json::from_str::<Value>(&weights)?,"fit":serde_json::from_str::<Value>(&fit)?}))).collect::<Result<_>>()?;
    let current = crate::personal::dataset(db, person, scope).and_then(|mut data| {
        sanitize_dataset(&mut data)?;
        Ok(data)
    });
    let (counts, dataset_reason) = match current {
        Ok(data) => (data["counts"].clone(), None),
        Err(error) => (Value::Null, Some(error.to_string())),
    };
    let active = if dataset_reason.is_none() {
        kernels.iter().find(|k| {
            k["status"] == "active"
                && k["training_source"] == "human"
                && k["feature_contract"] == FEATURE_CONTRACT
                && validate_stored_kernel(k, person, scope).is_ok()
        })
    } else {
        None
    };
    let status = if active.is_some() {
        "active"
    } else if dataset_reason.is_some() {
        "dataset_unavailable"
    } else if counts["usable"].as_u64().unwrap_or(0) < 10 {
        "cold_start"
    } else if kernels.is_empty() {
        "not_fitted"
    } else {
        "draft"
    };
    let reason = if active.is_some() {
        "explicit_trial_activation"
    } else if dataset_reason.is_some() {
        "current_human_dataset_unavailable"
    } else if status == "cold_start" {
        "fewer_than_10_usable_pairs"
    } else {
        "no_trusted_human_active_kernel"
    };
    Ok(
        json!({"person_id":person,"scope":scope,"feature_contract":FEATURE_CONTRACT,"status":status,"counts":counts,"activated":active.is_some(),"active_kernel_id":active.map(|k|k["id"].clone()),"activation_reason":reason,"dataset_reason":dataset_reason,"kernels":kernels}),
    )
}
pub fn rank(opts: &RankOpts<'_>) -> Result<Value> {
    let conn = open_kernel_db(opts.db, false)?;
    let row: Option<(i64,i64,String,String)> = conn.query_row("SELECT id,version,weights_json,fit_json FROM kernel WHERE person_id=?1 AND scope=?2 AND feature_contract=?3 AND status='active' AND training_source='human' ORDER BY version DESC LIMIT 1",params![opts.person,opts.scope,FEATURE_CONTRACT],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let kernel = row.map(|(id,version,weights,fit)|json!({"id":id,"version":version,"training_source":"human","weights":serde_json::from_str::<Value>(&weights).unwrap_or(Value::Null),"fit":serde_json::from_str::<Value>(&fit).unwrap_or(Value::Null)})).unwrap_or(Value::Null);
    let mut candidates: Vec<Value> = opts.candidates.iter().map(|id| match crate::personal::candidate(opts.db,*id,opts.scope) {
        Ok(value) => json!({"artifact_id":id,"evidence_valid":true,"candidate_snapshot":value,"recipe_hash":value["recipe_hash"],"features_hash":value["features_hash"],"conditions_hash":value["conditions_hash"],"page_sha256":value["page_sha256"],"shot_sha256":value["shot_sha256"],"score":null}),
        Err(error) => json!({"artifact_id":id,"evidence_valid":false,"reason":error.to_string(),"candidate_snapshot":null,"score":null})
    }).collect();
    let mut reason: Option<String> = if opts.candidates.is_empty() {
        Some("no_candidates".into())
    } else if !opts.apply {
        Some("apply_not_requested".into())
    } else if opts.scope != SCOPE {
        Some("scope_or_feature_contract_mismatch".into())
    } else if kernel.is_null() {
        Some("no_trusted_human_active_kernel".into())
    } else {
        None
    };
    if reason.is_none()
        && let Err(error) = validate_stored_kernel(&kernel, opts.person, opts.scope)
    {
        reason = Some(format!("kernel_gate_rejected: {error}"));
    }
    if reason.is_none() {
        let consent: Option<String> = conn
            .query_row(
                "SELECT consent_json FROM person WHERE id=?1 AND privacy_level='local'",
                [opts.person],
                |r| r.get(0),
            )
            .optional()?;
        let consent = consent
            .and_then(|v| serde_json::from_str::<Value>(&v).ok())
            .unwrap_or(Value::Null);
        if consent["source"] != "human"
            || consent["personal_preference"] != true
            || consent["reviewer"] != opts.person
            || consent["usage"] != "local-only"
            || consent["demo"] == true
        {
            reason = Some("person_consent_not_current_human".into());
        }
    }
    if reason.is_none() {
        let conditions = candidates
            .first()
            .map(|c| c["conditions_hash"].clone())
            .unwrap_or(Value::Null);
        for c in &candidates {
            let snapshot = &c["candidate_snapshot"];
            if c["evidence_valid"] != true {
                reason = Some("candidate_evidence_invalid".into());
                break;
            }
            if snapshot["scope"] != opts.scope || snapshot["feature_contract"] != FEATURE_CONTRACT {
                reason = Some("candidate_scope_or_feature_contract_mismatch".into());
                break;
            }
            if !conditions.is_string() || snapshot["conditions_hash"] != conditions {
                reason = Some("candidate_conditions_mismatch".into());
                break;
            }
            if snapshot["pill_snapshot"]["snapshot_version"] != 1
                || snapshot["recipe_hash"]
                    != hash_json(&snapshot["pill_snapshot"]["recipe_definition"])
                || snapshot["features_hash"] != hash_json(&snapshot["features"])
            {
                reason = Some("candidate_snapshot_hash_mismatch".into());
                break;
            }
        }
    }
    if reason.is_none() {
        for c in &mut candidates {
            match score_candidate(&kernel, &c["candidate_snapshot"]) {
                Ok(score) => c["score"] = json!(score),
                Err(error) => {
                    reason = Some(format!("candidate_features_unusable: {error}"));
                    break;
                }
            }
        }
    }
    let applied = reason.is_none();
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    if applied {
        order.sort_by(|a, b| {
            candidates[*b]["score"]
                .as_f64()
                .unwrap()
                .total_cmp(&candidates[*a]["score"].as_f64().unwrap())
        });
    }
    let ids: Vec<_> = order.iter().map(|i| opts.candidates[*i]).collect();
    let chosen = ids.first().copied();
    let snapshot = order
        .first()
        .map(|i| candidates[*i]["candidate_snapshot"]["pill_snapshot"].clone())
        .unwrap_or(Value::Null);
    Ok(
        json!({"chosen_artifact_id":chosen,"chosen_snapshot":snapshot,"ordered_artifact_ids":ids,"applied":applied,
        "trace":{"person_id":opts.person,"kernel_id":kernel["id"],"kernel_version":kernel["version"],"training_source":kernel["training_source"],"dataset_hash":kernel["fit"]["dataset_hash"],"scope":opts.scope,"feature_contract":FEATURE_CONTRACT,
        "normalizer_hash":if kernel.is_null() {Value::Null} else {json!(hash_json(&kernel["fit"]["normalizer"]))},
        "apply_requested":opts.apply,"applied":applied,"original_order":opts.candidates,"ordered_artifact_ids":ids,"candidates":candidates,"chosen_artifact_id":chosen,"fallback_reason":reason,
        "interpretation":"仅对当前完整实渲证据的配方候选试用；不是药液质量、稳定审美或跨域结论"}}),
    )
}
fn validate_stored_kernel(kernel: &Value, person: &str, scope: &str) -> Result<()> {
    let fit = &kernel["fit"];
    let validation = &fit["validation"];
    let norm = &fit["normalizer"];
    ensure!(
        fit["format_version"] == 1
            && fit["training_source"] == "human"
            && fit["scope"] == scope
            && fit["feature_contract"] == FEATURE_CONTRACT,
        "fit provenance mismatch"
    );
    ensure!(
        fit["activation"]["requested"] == true && fit["activation"]["explicit"] == true,
        "kernel not explicitly activated"
    );
    ensure!(
        fit["dataset"]["person_id"] == person
            && fit["dataset"]["scope"] == scope
            && fit["dataset"]["feature_contract"] == FEATURE_CONTRACT
            && fit["dataset_hash"] == hash_json(&fit["dataset"]),
        "frozen dataset hash/provenance mismatch"
    );
    ensure!(
        validation["eligible_for_trial"] == true && validation["reasons"] == json!([]),
        "kernel not eligible for trial"
    );
    ensure!(
        fit["dataset"]["pairs"]
            .as_array()
            .is_some_and(|p| p.len() >= 20)
            && validation["train_n"].as_u64().unwrap_or(0) >= 10
            && validation["holdout_n"].as_u64().unwrap_or(0) >= 8,
        "insufficient validated samples"
    );
    ensure!(
        validation["accuracy"].as_f64().unwrap_or(0.0)
            > validation["constant_baseline_accuracy"]
                .as_f64()
                .unwrap_or(1.0)
            && validation["wilson_lower"].as_f64().unwrap_or(0.0) > 0.5,
        "validation metrics failed"
    );
    let train = fit["grouping"]["train_groups"]
        .as_array()
        .context("training groups missing")?;
    let hold = fit["grouping"]["holdout_groups"]
        .as_array()
        .context("holdout groups missing")?;
    ensure!(
        train.len() >= 3
            && train.len() + hold.len() >= 5
            && !hold.is_empty()
            && train.iter().all(|g| !hold.contains(g)),
        "group leakage/coverage"
    );
    ensure!(
        norm["terms"] == json!(TERMS) && norm["fit_on"] == "train_only",
        "normalizer feature contract mismatch"
    );
    for field in ["mean", "std", "mask"] {
        ensure!(
            norm[field].as_array().is_some_and(|v| v.len() == 2),
            "normalizer {field} malformed"
        );
    }
    let mut active = false;
    for (i, term) in TERMS.iter().enumerate() {
        ensure!(
            norm["mask"][i].is_boolean()
                && norm["mean"][i].as_f64().is_some_and(f64::is_finite)
                && norm["std"][i]
                    .as_f64()
                    .is_some_and(|s| s.is_finite() && s > 0.0)
                && kernel["weights"][*term]
                    .as_f64()
                    .is_some_and(f64::is_finite),
            "normalizer/weights invalid"
        );
        active |= norm["mask"][i] == true;
    }
    ensure!(active, "normalizer has no usable dimensions");
    Ok(())
}
fn score_candidate(kernel: &Value, candidate: &Value) -> Result<f64> {
    let norm = &kernel["fit"]["normalizer"];
    let features = feature_vector(&candidate["features"]);
    let mut score = 0.0;
    for (i, term) in TERMS.iter().enumerate() {
        if norm["mask"][i] == true {
            score += kernel["weights"][*term]
                .as_f64()
                .context("weight missing")?
                * (features[i].context("trained dimension missing")?
                    - norm["mean"][i].as_f64().context("mean missing")?)
                / norm["std"][i].as_f64().context("std missing")?;
        }
    }
    ensure!(score.is_finite(), "score is not finite");
    Ok(score)
}
#[allow(dead_code)] // Compatibility API; current CLI prints fit_person's structured result.
pub fn run(db: &str, person: &str, seed: u64) -> Result<()> {
    let result = fit_person(&FitOpts {
        db,
        person,
        scope: SCOPE,
        seed,
        activate: false,
    })?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

pub fn run_demo(db: &str, person: &str, seed: u64) -> Result<()> {
    let conn = Connection::open(db)?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    let consent: String = conn.query_row(
        "SELECT consent_json FROM person WHERE id=?1",
        [person],
        |r| r.get(0),
    )?;
    anyhow::ensure!(
        serde_json::from_str::<serde_json::Value>(&consent)?["demo"] == true,
        "run_demo 仅允许 consent.demo=true 的演练 person"
    );
    let dims = dims(&conn)?;
    let raw = raw_vectors(&conn, &dims)?;
    let stats = stats_of(&raw, dims.len());
    let vectors = standardize(&raw, &stats);

    let mut stmt = conn.prepare(
        "SELECT work_id, opponent_work_id, outcome FROM judgment
         WHERE actor_id=?1 AND source='demo' AND kind='pairwise' AND opponent_work_id IS NOT NULL ORDER BY id",
    )?;
    let judgments: Vec<(i64, i64, String)> = stmt
        .query_map(params![person], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let pairs = build_pairs(&judgments, &vectors, dims.len());
    if pairs.len() < 10 {
        println!(
            "演练判定不足（可用 {} 对）；仅可在演练 person 生成 source=demo 数据，不能补真人门槛",
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
        "UPDATE kernel SET status='superseded' WHERE person_id=?1 AND status='active' AND training_source='demo' AND scope='ui-static'",
        params![person],
    )?;
    let mut weights = serde_json::Map::new();
    for (d, w) in dims.iter().zip(delta.iter()) {
        weights.insert(d.term.clone(), serde_json::json!(w));
    }
    let fit_json = serde_json::json!({
        "n": n, "train_acc": train_acc, "hold_acc": hold_acc, "rho": rho,
        "standardized": true,"training_source":"demo","scope":SCOPE,"feature_contract":"demo-library-v1",
        "normalizer":{"terms":dims.iter().map(|d|&d.term).collect::<Vec<_>>(),"mean":stats.mean,"std":stats.std},
        "seed":seed,"eligible_for_trial":false,"note":"仅合成演练，不是真人个人偏好或稳定审美结论",
    });
    conn.execute(
        "INSERT INTO kernel(person_id, version, model_kind, weights_json, prior_json, fit_json, maturity, status, created_at,training_source,scope,feature_contract)
         VALUES(?1,?2,'demo-bt',?3,'{}',?4,?5,'draft',?6,'demo','ui-static','demo-library-v1')",
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

    println!(
        "{}",
        serde_json::json!({"kernel_id":kernel_id,"version":version,"training_source":"demo","status":"draft","fit":fit_json})
    );
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
    let consent: Option<String> = conn
        .query_row(
            "SELECT consent_json FROM person WHERE id=?1",
            [person],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(consent) = consent {
        anyhow::ensure!(
            serde_json::from_str::<serde_json::Value>(&consent)?["demo"] == true,
            "simulate 仅允许 consent.demo=true 的演练 person，拒绝借用真人"
        );
    }
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
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM judgment WHERE actor_id=?1 AND source='demo' AND source_evaluation_id IS NULL AND note='simulated'",
        params![person],
    )?;
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
            "INSERT INTO judgment(work_id, kind, opponent_work_id, context_json, population, outcome, grade, note, created_at, actor_id, visibility,source,intent,scope)
             VALUES(?1,'pairwise',?2,'{\"domain\":\"ui\",\"synthetic\":true}','demo',?3,'C','simulated',?4,?5,'private','demo','simulation','ui-static')",
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn temp_db() -> String {
        let root = std::env::temp_dir().join(format!(
            "ie-fit-{}-{}-{}",
            std::process::id(),
            crate::now_iso().replace(':', "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        root.join("fixture.sqlite").to_str().unwrap().to_owned()
    }
    fn snapshot(n: usize) -> serde_json::Value {
        serde_json::json!({"person_id":"synthetic-human","scope":SCOPE,"feature_contract":FEATURE_CONTRACT,"terms":["negative-space.active","detail.density"],"counts":{"raw":n,"unique":n,"usable":n},"pairs":(0..n).map(|i| serde_json::json!({
            "judgment_id":i+1,"evaluation_id":i+1,"pair_key":format!("pair-{i}"),"group_key":format!("content-{}",i/4),"outcome":if i%2==0 {"a_wins"} else {"b_wins"},
            "a":{"artifact_id":i*2+1,"work_id":i*2+1,"features":{"negative-space.active":if i%2==0 {0.2} else {0.8},"detail.density":0.3},"features_hash":format!("phi-a-{i}")},
            "b":{"artifact_id":i*2+2,"work_id":i*2+2,"features":{"negative-space.active":if i%2==0 {0.8} else {0.2},"detail.density":0.3},"features_hash":format!("phi-b-{i}")}
        })).collect::<Vec<_>>()})
    }
    fn training_db() -> String {
        let db = temp_db();
        let mut conn = Connection::open(&db).unwrap();
        crate::db::migrate(&mut conn).unwrap();
        conn.execute("INSERT INTO person(id,consent_json,created_at) VALUES('synthetic-human','{\"personal_preference\":true,\"source\":\"human\",\"reviewer\":\"synthetic-human\",\"usage\":\"local-only\"}','fixture')", []).unwrap();
        db
    }
    fn demo_fixture(demo_n: usize) -> crate::personal::tests::Fixture {
        let f = crate::personal::tests::Fixture::new();
        crate::personal::init(&crate::personal::InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (id, manifest, _, _) = f.pair("demo-isolation", false, false, true);
        f.record(id, &manifest, "fixture-alice", "A").unwrap();
        let conn = Connection::open(&f.db).unwrap();
        conn.execute("INSERT INTO person(id,consent_json,created_at) VALUES('demo','{\"demo\":true}','fixture')",[]).unwrap();
        conn.execute("INSERT INTO kernel(person_id,version,weights_json,prior_json,fit_json,status,created_at,training_source,scope,feature_contract) VALUES('demo',1,'{}','{}','{}','active','fixture','human',?1,?2)",params![SCOPE,FEATURE_CONTRACT]).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO pack(id,title,version) VALUES('core','core','1')",
            [],
        )
        .unwrap();
        for term in TERMS {
            conn.execute("INSERT OR IGNORE INTO vocab_term(pack_id,kind,term,label,definition) VALUES('core','primitive',?1,?1,'synthetic fixture')",[term]).unwrap();
        }
        conn.execute("INSERT INTO work_primitive(work_id,primitive_term_id,score,confidence,source,updated_at) SELECT w.id,v.id,CASE WHEN w.id%2=0 THEN 0.8 ELSE 0.2 END,0.75,'pixel-proxy','fixture' FROM work w CROSS JOIN vocab_term v WHERE v.kind='primitive'",[]).unwrap();
        for source in std::iter::repeat_n("demo", demo_n).chain(std::iter::repeat_n("human", 20)) {
            conn.execute("INSERT INTO judgment(work_id,opponent_work_id,kind,outcome,actor_id,created_at,source) VALUES(1,2,'pairwise','a_wins','demo','fixture',?1)",[source]).unwrap();
        }
        f
    }
    #[test]
    fn demo_legacy_draft_mismatched_or_rejected_kernels_never_implicitly_reorder() {
        let f = crate::personal::tests::Fixture::new();
        crate::personal::init(&crate::personal::InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (_, _, a, b) = f.pair("rank-untrusted", false, false, true);
        let mut data = snapshot(24);
        data["person_id"] = json!("fixture-alice");
        let fitted = fit_dataset(
            &FitOpts {
                db: &f.db,
                person: "fixture-alice",
                scope: SCOPE,
                seed: 7,
                activate: true,
            },
            data,
        )
        .unwrap();
        let conn = Connection::open(&f.db).unwrap();
        let original_fit = fitted["fit"].clone();
        for case in [
            "demo",
            "legacy",
            "draft",
            "scope",
            "contract",
            "not_eligible",
            "not_explicit",
            "dataset_hash",
            "normalizer",
            "wilson",
        ] {
            conn.execute("UPDATE kernel SET training_source='human',status='active',scope=?1,feature_contract=?2,fit_json=?3",params![SCOPE,FEATURE_CONTRACT,original_fit.to_string()]).unwrap();
            let mut fit = original_fit.clone();
            match case {
                "demo" | "legacy" => {
                    conn.execute("UPDATE kernel SET training_source=?1", [case])
                        .unwrap();
                }
                "draft" => {
                    conn.execute("UPDATE kernel SET status='draft'", [])
                        .unwrap();
                }
                "scope" => {
                    conn.execute("UPDATE kernel SET scope='other'", []).unwrap();
                }
                "contract" => {
                    conn.execute("UPDATE kernel SET feature_contract='other'", [])
                        .unwrap();
                }
                "not_eligible" => fit["validation"]["eligible_for_trial"] = json!(false),
                "not_explicit" => fit["activation"]["explicit"] = json!(false),
                "dataset_hash" => fit["dataset_hash"] = json!("corrupt"),
                "normalizer" => fit["normalizer"]["std"][0] = json!(0.0),
                "wilson" => fit["validation"]["wilson_lower"] = json!(0.5),
                _ => unreachable!(),
            }
            if fit != original_fit {
                conn.execute("UPDATE kernel SET fit_json=?1", [fit.to_string()])
                    .unwrap();
            }
            let value = rank(&RankOpts {
                db: &f.db,
                person: "fixture-alice",
                scope: SCOPE,
                candidates: &[a, b],
                apply: true,
            })
            .unwrap();
            assert_eq!(value["ordered_artifact_ids"], json!([a, b]), "{case}");
            assert_eq!(value["applied"], false, "{case}");
            assert!(value["trace"]["fallback_reason"].is_string(), "{case}");
        }
        conn.execute("UPDATE kernel SET training_source='human',status='active',scope=?1,feature_contract=?2,fit_json=?3",params![SCOPE,FEATURE_CONTRACT,original_fit.to_string()]).unwrap();
        let shot = crate::personal::candidate(&f.db, a, SCOPE).unwrap()["screenshot_path"]
            .as_str()
            .unwrap()
            .to_owned();
        std::fs::write(&shot, b"corrupted SYNTHETIC UNIT FIXTURE png").unwrap();
        let value = rank(&RankOpts {
            db: &f.db,
            person: "fixture-alice",
            scope: SCOPE,
            candidates: &[a, b],
            apply: true,
        })
        .unwrap();
        assert_eq!(value["ordered_artifact_ids"], json!([a, b]));
        assert_eq!(value["applied"], false);
        assert!(value["chosen_snapshot"].is_null());
        assert_eq!(
            value["trace"]["fallback_reason"],
            "candidate_evidence_invalid"
        );
    }
    #[test]
    fn changing_only_holdout_pixels_cannot_change_training_normalization_or_weights() {
        let db = training_db();
        let opts = FitOpts {
            db: &db,
            person: "synthetic-human",
            scope: SCOPE,
            seed: 7,
            activate: false,
        };
        let first = fit_dataset(&opts, snapshot(24)).unwrap();
        let hold = first["fit"]["grouping"]["holdout_groups"]
            .as_array()
            .unwrap();
        let mut changed = snapshot(24);
        for p in changed["pairs"].as_array_mut().unwrap() {
            if hold.contains(&p["group_key"]) {
                p["a"]["features"][TERMS[0]] = json!(0.01);
                p["b"]["features"][TERMS[0]] = json!(0.99);
            }
        }
        let second = fit_dataset(&opts, changed).unwrap();
        assert_eq!(first["fit"]["normalizer"], second["fit"]["normalizer"]);
        let shown = show(&db, "synthetic-human", SCOPE).unwrap();
        assert_eq!(
            shown["kernels"][0]["weights"],
            shown["kernels"][1]["weights"]
        );
    }
    #[test]
    fn failed_validation_and_a_small_draft_never_replace_an_existing_human_active() {
        let db = training_db();
        let opts = FitOpts {
            db: &db,
            person: "synthetic-human",
            scope: SCOPE,
            seed: 7,
            activate: true,
        };
        let good = fit_dataset(&opts, snapshot(24)).unwrap();
        let mut misleading = snapshot(24);
        for p in misleading["pairs"].as_array_mut().unwrap() {
            p["outcome"] = json!("a_wins");
        }
        let failed = fit_dataset(&opts, misleading).unwrap();
        assert_eq!(failed["fit"]["validation"]["eligible_for_trial"], false);
        assert_eq!(failed["activated"], false);
        assert_eq!(failed["status"], "draft");
        assert!(
            failed["fit"]["validation"]["reasons"]
                .as_array()
                .unwrap()
                .contains(&json!("not_better_than_constant_baseline"))
        );
        assert_eq!(fit_dataset(&opts, snapshot(10)).unwrap()["status"], "draft");
        let shown = show(&db, "synthetic-human", SCOPE).unwrap();
        assert_eq!(shown["active_kernel_id"], good["kernel_id"]);
        assert_eq!(shown["kernels"][2]["status"], "active");
    }
    #[test]
    fn failed_insert_rolls_back_superseding_the_previous_human_active() {
        let db = training_db();
        let opts = FitOpts {
            db: &db,
            person: "synthetic-human",
            scope: SCOPE,
            seed: 7,
            activate: true,
        };
        fit_dataset(&opts, snapshot(24)).unwrap();
        let before = show(&db, "synthetic-human", SCOPE).unwrap()["kernels"].clone();
        Connection::open(&db).unwrap().execute_batch("CREATE TRIGGER fail_fixture_fit BEFORE INSERT ON kernel WHEN NEW.training_source='human' BEGIN SELECT RAISE(ABORT,'synthetic insert failure'); END;").unwrap();
        let error = fit_dataset(&opts, snapshot(24)).unwrap_err();
        assert!(error.to_string().contains("synthetic insert failure"));
        assert_eq!(
            show(&db, "synthetic-human", SCOPE).unwrap()["kernels"],
            before
        );
    }
    #[test]
    fn unrelated_library_demo_and_live_phi_updates_cannot_change_a_frozen_kernel_or_scores() {
        let f = crate::personal::tests::Fixture::new();
        crate::personal::init(&crate::personal::InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (id, manifest, a, b) = f.pair("frozen-rank", false, false, true);
        f.record(id, &manifest, "fixture-alice", "B").unwrap();
        let mut data = snapshot(24);
        data["person_id"] = json!("fixture-alice");
        fit_dataset(
            &FitOpts {
                db: &f.db,
                person: "fixture-alice",
                scope: SCOPE,
                seed: 7,
                activate: true,
            },
            data,
        )
        .unwrap();
        let opts = RankOpts {
            db: &f.db,
            person: "fixture-alice",
            scope: SCOPE,
            candidates: &[a, b],
            apply: true,
        };
        let before = rank(&opts).unwrap();
        let frozen = show(&f.db, "fixture-alice", SCOPE).unwrap()["kernels"].clone();
        let conn = Connection::open(&f.db).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO pack(id,title,version) VALUES('core','core','1')",
            [],
        )
        .unwrap();
        for term in TERMS {
            conn.execute("INSERT OR IGNORE INTO vocab_term(pack_id,kind,term,label,definition) VALUES('core','primitive',?1,?1,'synthetic fixture')",[term]).unwrap();
        }
        conn.execute("INSERT OR REPLACE INTO work_primitive(work_id,primitive_term_id,score,confidence,source,updated_at) SELECT w.id,v.id,0.999,0.75,'pixel-proxy','reextracted fixture' FROM work w CROSS JOIN vocab_term v WHERE v.kind='primitive'",[]).unwrap();
        for i in 0..30 {
            conn.execute("INSERT INTO work(source_id,domain_id,collection,slug,canonical_url,title,first_seen_at,last_seen_at) SELECT source_id,domain_id,'unrelated-library',?1,?2,'fixture','fixture','fixture' FROM work WHERE id=1",params![format!("unrelated-{i}"),format!("synthetic-fixture://unrelated-{i}")]).unwrap();
            conn.execute("INSERT INTO artifact_evaluation(experiment_id,manifest_hash,source,reviewer,dimension,winner) VALUES(?1,?2,'demo','fixture-alice','visual','A')",params![id,manifest]).unwrap();
        }
        conn.execute("INSERT OR REPLACE INTO work_primitive(work_id,primitive_term_id,score,confidence,source,updated_at) SELECT w.id,v.id,0.001,0.75,'pixel-proxy','unrelated fixture' FROM work w CROSS JOIN vocab_term v WHERE v.kind='primitive'",[]).unwrap();
        assert_eq!(rank(&opts).unwrap(), before);
        assert_eq!(
            show(&f.db, "fixture-alice", SCOPE).unwrap()["kernels"],
            frozen
        );
    }
    #[test]
    fn real_public_fit_and_rank_use_only_explicit_synthetic_human_png_evidence() {
        let f = crate::personal::tests::Fixture::new();
        crate::personal::init(&crate::personal::InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        for i in 0..24 {
            let (id, manifest, _, _) = f.pair(
                &format!("independent-training-content-{i}"),
                false,
                false,
                true,
            );
            assert_eq!(
                f.record(id, &manifest, "fixture-alice", "B").unwrap()["bridge_created"],
                true
            );
        }
        let fitted = fit_person(&FitOpts {
            db: &f.db,
            person: "fixture-alice",
            scope: SCOPE,
            seed: 7,
            activate: true,
        })
        .unwrap();
        assert_eq!(fitted["counts"]["usable"], 24);
        assert_eq!(
            fitted["activated"], true,
            "{:?}",
            fitted["fit"]["validation"]
        );
        let (_, _, a, b) = f.pair("new-rendered-candidates", false, false, true);
        let value = rank(&RankOpts {
            db: &f.db,
            person: "fixture-alice",
            scope: SCOPE,
            candidates: &[a, b],
            apply: true,
        })
        .unwrap();
        assert_eq!(value["ordered_artifact_ids"], json!([b, a]));
        assert_eq!(value["trace"]["training_source"], "human");
        assert_eq!(value["applied"], true);
    }
    #[test]
    fn simulate_labels_new_rows_demo_and_never_deletes_human_rows_even_with_a_simulated_note() {
        let f = demo_fixture(0);
        let conn = Connection::open(&f.db).unwrap();
        for term in ["synthetic-demo-3", "synthetic-demo-4"] {
            conn.execute("INSERT INTO vocab_term(pack_id,kind,term,label,definition) VALUES('core','primitive',?1,?1,'synthetic fixture')",[term]).unwrap();
        }
        for i in 0..24 {
            conn.execute("INSERT INTO work(source_id,domain_id,collection,slug,canonical_url,title,first_seen_at,last_seen_at) SELECT source_id,domain_id,'synthetic-demo',?1,?2,'fixture','fixture','fixture' FROM work WHERE id=1",params![format!("demo-{i}"),format!("synthetic-fixture://demo-{i}")]).unwrap();
        }
        conn.execute("INSERT OR REPLACE INTO work_primitive(work_id,primitive_term_id,score,confidence,source,updated_at) SELECT w.id,v.id,((w.id+v.id)%10)/10.0,0.75,'pixel-proxy','fixture' FROM work w CROSS JOIN vocab_term v WHERE v.kind='primitive'",[]).unwrap();
        conn.execute("INSERT INTO judgment(work_id,opponent_work_id,kind,outcome,actor_id,created_at,source,note) VALUES(1,2,'pairwise','a_wins','demo','fixture','human','simulated')",[]).unwrap();
        simulate(&f.db, "demo", 5, 7, false).unwrap();
        let human: i64 = conn
            .query_row(
                "SELECT count(*) FROM judgment WHERE actor_id='demo' AND source='human'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let demo: i64 = conn
            .query_row(
                "SELECT count(*) FROM judgment WHERE actor_id='demo' AND source='demo'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(human, 21);
        assert_eq!(demo, 5);
        assert_eq!(
            show(&f.db, "demo", SCOPE).unwrap()["kernels"][0]["status"],
            "active"
        );
    }
    #[test]
    fn two_hundred_demo_plus_one_real_preference_still_show_one_cold_human_pair() {
        let f = crate::personal::tests::Fixture::new();
        crate::personal::init(&crate::personal::InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (id, manifest, _, _) = f.pair("mixed-fit-sources", false, false, true);
        let conn = Connection::open(&f.db).unwrap();
        for _ in 0..200 {
            conn.execute("INSERT INTO artifact_evaluation(experiment_id,manifest_hash,source,reviewer,dimension,winner) VALUES(?1,?2,'demo','fixture-alice','visual','A')",params![id,manifest]).unwrap();
        }
        f.record(id, &manifest, "fixture-alice", "A").unwrap();
        let value = fit_person(&FitOpts {
            db: &f.db,
            person: "fixture-alice",
            scope: SCOPE,
            seed: 7,
            activate: true,
        })
        .unwrap();
        assert_eq!(value["counts"]["raw"], 1);
        assert_eq!(value["counts"]["usable"], 1);
        assert_eq!(value["counts"]["excluded_demo"], 200);
        assert_eq!(value["status"], "cold_start");
        assert_eq!(value["fitted"], false);
        assert_eq!(value["activated"], false);
        let shown = show(&f.db, "fixture-alice", SCOPE).unwrap();
        assert_eq!(shown["counts"]["raw"], 1);
        assert_eq!(shown["counts"]["usable"], 1);
        assert_eq!(shown["counts"]["excluded_demo"], 200);
        assert_eq!(shown["status"], "cold_start");
        assert_eq!(shown["kernels"], json!([]));
    }
    #[test]
    fn demo_kernel_is_labeled_draft_and_cannot_supersede_a_human_active() {
        let f = demo_fixture(10);
        run_demo(&f.db, "demo", 7).unwrap();
        let shown = show(&f.db, "demo", SCOPE).unwrap();
        assert_eq!(shown["kernels"].as_array().unwrap().len(), 2, "{shown:#}");
        assert_eq!(shown["kernels"][0]["training_source"], "demo");
        assert_eq!(shown["kernels"][0]["status"], "draft");
        assert_eq!(shown["kernels"][1]["training_source"], "human");
        assert_eq!(shown["kernels"][1]["status"], "active");
        assert_eq!(shown["kernels"][0]["fit"]["training_source"], "demo");
    }
    #[test]
    fn demo_fit_cannot_borrow_human_or_legacy_judgments_to_meet_its_threshold() {
        let f = demo_fixture(1);
        run_demo(&f.db, "demo", 7).unwrap();
        let shown = show(&f.db, "demo", SCOPE).unwrap();
        assert_eq!(shown["kernels"].as_array().unwrap().len(), 1);
        assert_eq!(shown["kernels"][0]["status"], "active");
        assert_eq!(shown["kernels"][0]["training_source"], "human");
    }
    #[test]
    fn explicit_apply_uses_the_active_human_kernel_and_frozen_recipe_snapshot() {
        let f = crate::personal::tests::Fixture::new();
        crate::personal::init(&crate::personal::InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (_, _, a, b) = f.pair("rank-trial", false, false, true);
        let mut data = snapshot(24);
        data["person_id"] = json!("fixture-alice");
        let fitted = fit_dataset(
            &FitOpts {
                db: &f.db,
                person: "fixture-alice",
                scope: SCOPE,
                seed: 7,
                activate: true,
            },
            data,
        )
        .unwrap();
        assert_eq!(fitted["activated"], true);
        let not_applied = rank(&RankOpts {
            db: &f.db,
            person: "fixture-alice",
            scope: SCOPE,
            candidates: &[a, b],
            apply: false,
        })
        .unwrap();
        assert_eq!(not_applied["ordered_artifact_ids"], json!([a, b]));
        let result = rank(&RankOpts {
            db: &f.db,
            person: "fixture-alice",
            scope: SCOPE,
            candidates: &[a, b],
            apply: true,
        })
        .unwrap();
        assert_eq!(result["ordered_artifact_ids"], json!([b, a]), "{result:#}");
        assert_eq!(result["applied"], true);
        assert_eq!(result["chosen_artifact_id"], b);
        assert_eq!(
            result["chosen_snapshot"],
            crate::personal::candidate(&f.db, b, SCOPE).unwrap()["pill_snapshot"]
        );
        assert_eq!(result["trace"]["kernel_id"], fitted["kernel_id"]);
        assert_eq!(result["trace"]["training_source"], "human");
        assert_eq!(
            result["trace"]["dataset_hash"],
            fitted["fit"]["dataset_hash"]
        );
        assert!(result["trace"]["fallback_reason"].is_null());
    }
    #[test]
    fn ranking_without_apply_or_a_human_active_kernel_keeps_the_original_order() {
        let f = crate::personal::tests::Fixture::new();
        crate::personal::init(&crate::personal::InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (_, _, a, b) = f.pair("rank-cold", false, false, true);
        for apply in [false, true] {
            let result = rank(&RankOpts {
                db: &f.db,
                person: "fixture-alice",
                scope: SCOPE,
                candidates: &[b, a],
                apply,
            })
            .unwrap();
            assert_eq!(result["ordered_artifact_ids"], json!([b, a]));
            assert_eq!(result["chosen_artifact_id"], b);
            assert_eq!(
                result["chosen_snapshot"],
                crate::personal::candidate(&f.db, b, SCOPE).unwrap()["pill_snapshot"]
            );
            assert_eq!(result["applied"], false);
            assert_eq!(
                result["trace"]["fallback_reason"],
                if apply {
                    "no_trusted_human_active_kernel"
                } else {
                    "apply_not_requested"
                }
            );
        }
    }
    #[test]
    fn explicit_activation_supersedes_only_human_active_in_the_same_person_scope() {
        let db = training_db();
        let conn = Connection::open(&db).unwrap();
        for (version, source, scope) in [
            (1, "human", SCOPE),
            (2, "demo", SCOPE),
            (3, "legacy", SCOPE),
            (4, "human", "other-scope"),
        ] {
            conn.execute("INSERT INTO kernel(person_id,version,weights_json,prior_json,fit_json,status,created_at,training_source,scope,feature_contract) VALUES('synthetic-human',?1,'{}','{}','{}','active','fixture',?2,?3,?4)",params![version,source,scope,FEATURE_CONTRACT]).unwrap();
        }
        let value = fit_dataset(
            &FitOpts {
                db: &db,
                person: "synthetic-human",
                scope: SCOPE,
                seed: 7,
                activate: true,
            },
            snapshot(24),
        )
        .unwrap();
        assert_eq!(value["activated"], true);
        assert_eq!(value["status"], "active");
        let shown = show(&db, "synthetic-human", SCOPE).unwrap();
        for k in shown["kernels"].as_array().unwrap() {
            assert_eq!(
                k["status"],
                if k["version"] == 1 {
                    "superseded"
                } else {
                    "active"
                }
            );
        }
        assert_eq!(
            show(&db, "synthetic-human", "other-scope").unwrap()["kernels"][0]["status"],
            "active"
        );
        assert_eq!(value["fit"]["activation"]["explicit"], true);
    }
    #[test]
    fn content_group_holdout_is_disjoint_and_qualification_is_not_automatic_activation() {
        let db = training_db();
        let value = fit_dataset(
            &FitOpts {
                db: &db,
                person: "synthetic-human",
                scope: SCOPE,
                seed: 7,
                activate: false,
            },
            snapshot(24),
        )
        .unwrap();
        assert_eq!(value["status"], "draft");
        assert_eq!(
            value["fit"]["validation"]["eligible_for_trial"], true,
            "{value:#}"
        );
        assert_eq!(value["activated"], false);
        let groups = &value["fit"]["grouping"];
        let train = groups["train_groups"].as_array().unwrap();
        let hold = groups["holdout_groups"].as_array().unwrap();
        assert!(train.len() >= 3 && !hold.is_empty());
        assert!(train.iter().all(|g| !hold.contains(g)));
        assert!(value["fit"]["validation"]["holdout_n"].as_u64().unwrap() >= 8);
        assert_eq!(value["fit"]["validation"]["accuracy"], 1.0);
        assert_eq!(
            value["fit"]["validation"]["constant_baseline_accuracy"],
            0.5
        );
        assert!(value["fit"]["validation"]["wilson_lower"].as_f64().unwrap() > 0.5);
        let ids = value["fit"]["normalizer"]["artifact_ids"]
            .as_array()
            .unwrap();
        for pair in value["fit"]["dataset"]["pairs"].as_array().unwrap() {
            if hold.contains(&pair["group_key"]) {
                assert!(!ids.contains(&pair["a"]["artifact_id"]));
                assert!(!ids.contains(&pair["b"]["artifact_id"]));
            }
        }
    }
    #[test]
    fn duplicates_reversed_pairs_and_zero_difference_cannot_reach_the_threshold() {
        let db = training_db();
        let mut data = snapshot(10);
        let pairs = data["pairs"].as_array_mut().unwrap();
        pairs[9]["b"]["features"] = pairs[9]["a"]["features"].clone();
        let duplicate = pairs[0].clone();
        pairs.push(duplicate);
        let mut reversed = pairs[1].clone();
        let a = reversed["a"].clone();
        reversed["a"] = reversed["b"].clone();
        reversed["b"] = a;
        reversed["outcome"] = json!("a_wins");
        pairs.push(reversed);
        data["counts"] = json!({"raw":12,"unique":12,"usable":12});
        let value = fit_dataset(
            &FitOpts {
                db: &db,
                person: "synthetic-human",
                scope: SCOPE,
                seed: 7,
                activate: true,
            },
            data,
        )
        .unwrap();
        assert_eq!(value["status"], "cold_start");
        assert_eq!(value["counts"]["raw"], 12);
        assert_eq!(value["counts"]["unique"], 10);
        assert_eq!(value["counts"]["usable"], 9);
        assert_eq!(value["counts"]["zero_difference"], 1);
        assert_eq!(value["counts"]["duplicates"], 2);
        assert_eq!(
            show(&db, "synthetic-human", SCOPE).unwrap()["kernels"],
            json!([])
        );
    }
    #[test]
    fn ten_usable_pairs_persist_only_a_draft_l0_with_a_frozen_normalizer() {
        let db = training_db();
        let value = fit_dataset(
            &FitOpts {
                db: &db,
                person: "synthetic-human",
                scope: SCOPE,
                seed: 7,
                activate: true,
            },
            snapshot(10),
        )
        .unwrap();
        assert_eq!(value["status"], "draft");
        assert_eq!(value["maturity"], "L0");
        assert_eq!(value["fitted"], true);
        assert_eq!(value["activated"], false);
        let shown = show(&db, "synthetic-human", SCOPE).unwrap();
        let kernel = &shown["kernels"][0];
        assert_eq!(kernel["training_source"], "human");
        assert_eq!(
            kernel["fit"]["normalizer"]["terms"],
            serde_json::json!(["negative-space.active", "detail.density"])
        );
        assert_eq!(
            kernel["fit"]["normalizer"]["mean"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(kernel["fit"]["seed"], 7);
        assert_eq!(
            kernel["fit"]["dataset"]["pairs"].as_array().unwrap().len(),
            10
        );
    }
    #[test]
    fn nine_usable_human_pairs_are_honest_cold_start_without_writing_a_kernel() {
        let db = temp_db();
        let value = fit_dataset(
            &FitOpts {
                db: &db,
                person: "synthetic-human",
                scope: SCOPE,
                seed: 7,
                activate: true,
            },
            snapshot(9),
        )
        .unwrap();
        assert_eq!(value["status"], "cold_start");
        assert_eq!(value["counts"]["raw"], 9);
        assert_eq!(value["counts"]["usable"], 9);
        assert_eq!(value["fitted"], false);
        assert_eq!(value["activated"], false);
        assert!(value["kernel_id"].is_null());
        assert!(
            !std::path::Path::new(&db).exists(),
            "cold start must not create or mutate a DB"
        );
    }
    #[test]
    fn demo_fit_refuses_a_real_person_before_reading_any_training_data() {
        let db = temp_db();
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE person(id TEXT PRIMARY KEY,consent_json TEXT); INSERT INTO person VALUES('real','{}');").unwrap();
        let err = run_demo(&db, "real", 7).unwrap_err();
        assert!(err.to_string().contains("consent.demo=true"), "{err:#}");
    }
    #[test]
    fn simulate_refuses_a_real_person_before_reading_any_training_data() {
        let db = temp_db();
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE person(id TEXT PRIMARY KEY,consent_json TEXT); INSERT INTO person VALUES('real','{}');").unwrap();
        let err = simulate(&db, "real", 200, 7, false).unwrap_err();
        assert!(err.to_string().contains("consent.demo=true"), "{err:#}");
    }
}
