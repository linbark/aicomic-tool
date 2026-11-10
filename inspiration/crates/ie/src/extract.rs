//! 抽取：把素材图变成 observation + measure + work_primitive。
//!
//! 当前只做像素可测的部分（6 个度量 → 7 个基元代理），
//! 语义手法（这个按钮为什么放这）留给视觉模型，来源字段会标成 pixel-proxy。

use anyhow::Result;
use rayon::prelude::*;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::metrics::{Metrics, compute};

pub const EXTRACTOR: &str = "pixel-proxy";
pub const VERSION: &str = "1";

struct Job {
    asset_id: i64,
    work_id: i64,
    path: PathBuf,
}

pub fn run(db: &str, root: &Path, limit: Option<usize>) -> Result<()> {
    let conn = Connection::open(db)?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;

    let mut stmt = conn.prepare(
        "SELECT a.id, a.work_id, a.local_path FROM asset a
         WHERE a.local_path IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM observation o
                           WHERE o.asset_id = a.id AND o.extractor = ?1)
         ORDER BY a.id",
    )?;
    let mut jobs: Vec<Job> = stmt
        .query_map(params![EXTRACTOR], |r| {
            Ok(Job {
                asset_id: r.get(0)?,
                work_id: r.get(1)?,
                path: root.join(r.get::<_, String>(2)?),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if let Some(n) = limit {
        jobs.truncate(n);
    }
    println!("待抽取素材：{}", jobs.len());
    if jobs.is_empty() {
        return Ok(());
    }

    let results: Vec<(i64, i64, Result<Metrics>)> = jobs
        .par_iter()
        .map(|j| (j.asset_id, j.work_id, compute(&j.path)))
        .collect();
    let ok: Vec<(i64, i64, Metrics)> = results
        .into_iter()
        .filter_map(|(a, w, r)| match r {
            Ok(m) => Some((a, w, m)),
            Err(e) => {
                eprintln!("跳过 asset {a}: {e}");
                None
            }
        })
        .collect();
    println!("解码成功：{}", ok.len());

    let metrics = metric_ids(&conn)?;
    let prims = primitive_ids(&conn)?;
    let now = crate::now_iso();
    let tx = conn.unchecked_transaction()?;
    let mut measures = 0usize;
    let mut vectors = 0usize;
    for (asset_id, work_id, m) in &ok {
        let payload = serde_json::json!({
            "width": m.width, "height": m.height,
            "edge_density": m.edge_density, "symmetry": m.symmetry,
            "whitespace": m.whitespace, "contrast": m.contrast,
            "compactness": m.compactness, "alignment": m.alignment,
        });
        tx.execute(
            "INSERT OR IGNORE INTO observation(asset_id, extractor, extractor_version, payload_json, confidence, created_at)
             VALUES(?1, ?2, ?3, ?4, 0.7, ?5)",
            params![asset_id, EXTRACTOR, VERSION, payload.to_string(), now],
        )?;
        let obs_id = tx.last_insert_rowid();
        let rows: [(&str, f32, &str); 6] = [
            ("measure.edge-density", m.edge_density, "ratio"),
            ("measure.symmetry-index", m.symmetry, "ratio"),
            ("measure.palette-compactness", m.compactness, "ratio"),
            ("measure.whitespace-ratio", m.whitespace, "ratio"),
            ("measure.contrast-ratio", m.contrast, "ratio"),
            ("measure.alignment-residual", 1.0 - m.alignment, "norm"),
        ];
        for (term, value, unit) in rows {
            if let Some(id) = metrics.get(term) {
                tx.execute(
                    "INSERT INTO measure(observation_id, metric_term_id, value, unit, confidence)
                     VALUES(?1, ?2, ?3, ?4, 0.7)",
                    params![obs_id, id, value, unit],
                )?;
                measures += 1;
            }
        }
        // 空白/纯色海报：几何量全部退化，不能当成"极高留白"喂给内核
        let blank = m.edge_density < 0.01 && m.whitespace > 0.98;
        if blank {
            tx.execute(
                "INSERT INTO validation_result(scope_table, scope_id, rule, severity, message, checked_at)
                 VALUES('asset', ?1, 'asset.blank', 'warn', '近似空白海报，已跳过基元向量', ?2)",
                params![asset_id, now],
            )?;
            continue;
        }
        for (term, score) in proxies(m) {
            if let Some(id) = prims.get(term) {
                tx.execute(
                    "INSERT INTO work_primitive(work_id, primitive_term_id, score, confidence, source, extractor_version, updated_at)
                     VALUES(?1, ?2, ?3, 0.45, 'pixel-proxy', ?4, ?5)
                     ON CONFLICT(work_id, primitive_term_id) DO UPDATE SET
                       score=excluded.score, confidence=excluded.confidence,
                       source=excluded.source, extractor_version=excluded.extractor_version,
                       updated_at=excluded.updated_at",
                    params![work_id, id, score, VERSION, now],
                )?;
                vectors += 1;
            }
        }
    }
    tx.commit()?;
    println!("写入 measure {measures} 条、work_primitive {vectors} 条");
    Ok(())
}

/// 像素代理：只覆盖"形状/对比/留白"这类能直接量到的基元。
/// 每个映射都写清楚方向，别让后人猜。
pub fn proxies(m: &Metrics) -> Vec<(&'static str, f32)> {
    let contrast = ((m.contrast - 1.0) / 20.0).clamp(0.0, 1.0);
    vec![
        ("proportion.system", m.alignment),
        ("balance.symmetry", m.symmetry),
        ("negative-space.active", m.whitespace),
        ("contrast.value", contrast),
        ("restraint.single-hue", m.compactness),
        ("detail.density", (m.edge_density * 2.0).clamp(0.0, 1.0)),
    ]
}

fn metric_ids(conn: &Connection) -> Result<HashMap<String, i64>> {
    let mut stmt = conn.prepare("SELECT term, id FROM vocab_term WHERE kind='metric'")?;
    let map = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<HashMap<_, _>>>()?;
    Ok(map)
}

fn primitive_ids(conn: &Connection) -> Result<HashMap<String, i64>> {
    let mut stmt = conn.prepare("SELECT term, id FROM vocab_term WHERE kind='primitive'")?;
    let map = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<HashMap<_, _>>>()?;
    Ok(map)
}
