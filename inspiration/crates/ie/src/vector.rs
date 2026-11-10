//! 基元向量的写入规则：按来源存证据，只物化置信度最高的那一路，分歧进复核队列。

use anyhow::Result;
use rusqlite::{Connection, params};

pub struct Score<'a> {
    pub work_id: i64,
    pub term_id: i64,
    pub score: f32,
    pub confidence: f32,
    pub source: &'a str,
    pub version: &'a str,
    pub now: &'a str,
}

pub fn record(conn: &Connection, s: &Score<'_>) -> Result<()> {
    let Score {
        work_id,
        term_id,
        score,
        confidence,
        source,
        version,
        now,
    } = *s;
    conn.execute(
        "INSERT INTO work_primitive_evidence(work_id, primitive_term_id, source, score, confidence, extractor_version, updated_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(work_id, primitive_term_id, source) DO UPDATE SET
           score=excluded.score, confidence=excluded.confidence,
           extractor_version=excluded.extractor_version, updated_at=excluded.updated_at",
        params![work_id, term_id, source, score, confidence, version, now],
    )?;

    // 物化：置信度优先，同置信度取更高分（保守：宁可高估特征强度也不制造假阴性）
    let (s, c, src, ver, ts) = conn.query_row(
        "SELECT score, confidence, source, coalesce(extractor_version,''), updated_at
         FROM work_primitive_evidence WHERE work_id=?1 AND primitive_term_id=?2
         ORDER BY confidence DESC, score DESC LIMIT 1",
        params![work_id, term_id],
        |r| {
            Ok((
                r.get::<_, f32>(0)?,
                r.get::<_, f32>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        },
    )?;
    conn.execute(
        "INSERT INTO work_primitive(work_id, primitive_term_id, score, confidence, source, extractor_version, updated_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(work_id, primitive_term_id) DO UPDATE SET
           score=excluded.score, confidence=excluded.confidence, source=excluded.source,
           extractor_version=excluded.extractor_version, updated_at=excluded.updated_at",
        params![work_id, term_id, s, c, src, ver, ts],
    )?;

    // 分歧复核：两路都算可信却相差明显
    let (hi, lo): (Option<f32>, Option<f32>) = conn.query_row(
        "SELECT max(score), min(score) FROM work_primitive_evidence
         WHERE work_id=?1 AND primitive_term_id=?2 AND confidence >= 0.5",
        params![work_id, term_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if let (Some(hi), Some(lo)) = (hi, lo)
        && hi - lo > 0.3
    {
        let reason = format!("基元 {term_id} 两路证据分歧：{lo:.2}..{hi:.2}");
        let exists: i64 = conn.query_row(
            "SELECT count(*) FROM review_queue WHERE kind='work_primitive' AND ref_id=?1
             AND reason LIKE ?2 AND status='open'",
            params![work_id, format!("基元 {term_id} %")],
            |r| r.get(0),
        )?;
        if exists == 0 {
            conn.execute(
                "INSERT INTO review_queue(kind, ref_id, reason, status, created_at)
                 VALUES('work_primitive', ?1, ?2, 'open', ?3)",
                params![work_id, reason, now],
            )?;
        }
    }
    Ok(())
}
