use crate::*;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

pub struct MemoryStore {
    pub(crate) conn: Connection,
}
fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
fn string(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}
fn required(v: &Value, key: &str) -> Result<String> {
    string(v, key)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid(format!("缺少字段 {key}")))
}
fn pid(v: &Value) -> Result<i64> {
    v["project_id"]
        .as_i64()
        .ok_or_else(|| invalid("缺少 project_id"))
}
fn serialized(v: &Value, key: &str) -> Option<String> {
    v.get(key).filter(|v| !v.is_null()).map(Value::to_string)
}
fn ids(v: &Value, key: &str) -> Vec<String> {
    v[key]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
fn confidence(v: &Value) -> Result<f64> {
    let n = v.get("confidence").and_then(Value::as_f64).unwrap_or(1.0);
    if !n.is_finite() || !(0.0..=1.0).contains(&n) {
        return Err(invalid("confidence 必须为 0 至 1"));
    }
    Ok(n)
}
fn meta(v: &Value) -> Result<(String, f64, String)> {
    let status = string(v, "status").unwrap_or_else(confirmed);
    if !["confirmed", "hypothesis", "retconned"].contains(&status.as_str()) {
        return Err(invalid("未知真值状态"));
    }
    Ok((
        status,
        confidence(v)?,
        string(v, "source_kind").unwrap_or_else(|| "extracted".into()),
    ))
}
fn record_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecord> {
    let decode = |index| -> rusqlite::Result<Option<Value>> {
        let s: Option<String> = row.get(index)?;
        s.map(|s| {
            serde_json::from_str(&s).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    index,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })
        })
        .transpose()
    };
    Ok(MemoryRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        namespace: row.get(2)?,
        memory_type: row.get(3)?,
        entity: row.get(4)?,
        content: row.get(5)?,
        payload_json: decode(6)?,
        source_ref: row.get(7)?,
        time_index: row.get(8)?,
        status: row.get::<_, Option<String>>(9)?.unwrap_or_else(confirmed),
        confidence: row.get::<_, Option<f64>>(10)?.unwrap_or(1.0),
        source_kind: row.get::<_, Option<String>>(11)?.unwrap_or_else(system),
        evidence_ids: decode(12)?
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default(),
        story_order: row.get(13)?,
        story_time: decode(14)?,
        hash: row.get(15)?,
        created_at_ms: row.get(16)?,
    })
}
const RECORD_COLUMNS: &str = "id,project_id,namespace,type,entity,content,payload_json,source_ref,time_index,status,confidence,source_kind,evidence_ids_json,story_order,story_time_json,hash,created_at_ms";
impl MemoryStore {
    pub fn open_root(root: &Path) -> Result<Self> {
        Self::open(&root.join("memory_store.db"))
    }
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(include_str!("schema.sql"))?;
        let mut stmt = conn.prepare("PRAGMA table_info(memory_records)")?;
        let columns = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        for (column, ty) in [
            ("status", "TEXT"),
            ("confidence", "REAL"),
            ("source_kind", "TEXT"),
            ("evidence_ids_json", "TEXT"),
            ("story_order", "TEXT"),
            ("story_time_json", "TEXT"),
        ] {
            if !columns.iter().any(|c| c == column) {
                conn.execute_batch(&format!(
                    "ALTER TABLE memory_records ADD COLUMN {column} {ty}"
                ))?;
            }
        }
        Ok(Self { conn })
    }
    pub fn write(&self, record: &MemoryRecord, skip_duplicate: bool) -> Result<String> {
        write_record(&self.conn, record, skip_duplicate)
    }
    /// Text here is an explicit literal filter. Semantic search requires retrieve_semantic.
    pub fn list_records(&self, q: &MemoryQuery) -> Result<Vec<MemoryRecord>> {
        let mut records = self.filtered_records(q)?;
        records.truncate(q.top_k);
        Ok(records)
    }
    fn filtered_records(&self, q: &MemoryQuery) -> Result<Vec<MemoryRecord>> {
        let mut stmt=self.conn.prepare(&format!("SELECT {RECORD_COLUMNS} FROM memory_records WHERE project_id=? ORDER BY COALESCE(created_at_ms,0) DESC,id"))?;
        let rows = stmt.query_map([q.project_id], record_row)?;
        let mut result = Vec::new();
        for row in rows {
            let r = row?;
            if q.namespace.as_ref().is_some_and(|v| v != &r.namespace)
                || q.memory_type.as_ref().is_some_and(|v| v != &r.memory_type)
                || q.entity
                    .as_ref()
                    .is_some_and(|v| r.entity.as_ref() != Some(v))
                || q.status.as_ref().is_some_and(|v| v != &r.status)
                || q.source_kind.as_ref().is_some_and(|v| v != &r.source_kind)
                || q.min_confidence.is_some_and(|v| r.confidence < v)
            {
                continue;
            }
            if q.time_index_from
                .as_ref()
                .is_some_and(|v| r.time_index.as_ref().is_none_or(|t| t < v))
                || q.time_index_to
                    .as_ref()
                    .is_some_and(|v| r.time_index.as_ref().is_none_or(|t| t > v))
            {
                continue;
            }
            if q.query_text
                .as_ref()
                .is_some_and(|v| !r.content.to_lowercase().contains(&v.to_lowercase()))
            {
                continue;
            }
            result.push(r);
        }
        Ok(result)
    }
    pub fn get_record(&self, project_id: i64, id: &str) -> Result<Option<MemoryRecord>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {RECORD_COLUMNS} FROM memory_records WHERE project_id=? AND id=?"),
                params![project_id, id],
                record_row,
            )
            .optional()?)
    }
    fn upsert_canonical(&self, kind: &str, v: &Value) -> Result<String> {
        let tx = self.conn.unchecked_transaction()?;
        let id = canonical(&tx, kind, v)?;
        tx.commit()?;
        Ok(id)
    }
    pub fn upsert_evidence(&self, v: &Value) -> Result<String> {
        self.upsert_canonical("evidence", v)
    }
    pub fn upsert_entity(&self, v: &Value) -> Result<String> {
        self.upsert_canonical("entity", v)
    }
    pub fn upsert_snapshot(&self, v: &Value) -> Result<String> {
        self.upsert_canonical("snapshot", v)
    }
    pub fn upsert_time_constraint(&self, v: &Value) -> Result<String> {
        self.upsert_canonical("time_constraint", v)
    }
    pub fn upsert_time_block(&self, v: &Value) -> Result<String> {
        self.upsert_canonical("time_block", v)
    }
    pub fn ingest_text(
        &self,
        project_id: i64,
        episode_id: Option<i64>,
        text: &str,
        max_quote_chars: usize,
    ) -> Result<Vec<String>> {
        self.ingest_with_options(
            project_id,
            text,
            &EvidenceIngestOptions {
                episode_id,
                max_quote_chars,
                ..Default::default()
            },
        )
    }
    pub fn ingest_with_options(
        &self,
        project_id: i64,
        text: &str,
        options: &EvidenceIngestOptions,
    ) -> Result<Vec<String>> {
        let max_quote_chars = options.max_quote_chars;
        let episode_id = options.episode_id;
        if max_quote_chars == 0 {
            return Err(invalid("证据切片长度必须大于 0"));
        }
        let normalized = text.replace("\r\n", "\n");
        let mut paragraphs = Vec::new();
        let mut lines = Vec::new();
        for line in normalized.lines() {
            if line.trim().is_empty() {
                if !lines.is_empty() {
                    paragraphs.push(lines.join("\n"));
                    lines.clear();
                }
            } else {
                lines.push(line.trim_end().to_owned());
            }
        }
        if !lines.is_empty() {
            paragraphs.push(lines.join("\n"));
        }
        let tx = self.conn.unchecked_transaction()?;
        let mut out = vec![];
        for (i, paragraph) in paragraphs.iter().enumerate() {
            let chars: Vec<char> = paragraph.trim().chars().collect();
            let chunks: Vec<String> = chars
                .chunks(max_quote_chars)
                .map(|c| c.iter().collect::<String>().trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect();
            for (j, quote) in chunks.iter().enumerate() {
                let id = new_id();
                let mut tags = options.tags.clone();
                if chunks.len() > 1 {
                    tags.push(format!("chunk:{}/{}", j + 1, chunks.len()));
                }
                canonical(
                    &tx,
                    "evidence",
                    &json!({"evidence_id":id,"project_id":project_id,"episode_id":episode_id,"scene_id":options.scene_id,"quote":quote,"span":{"paragraph_index":i},"tags":tags}),
                )?;
                out.push(id);
            }
        }
        tx.commit()?;
        Ok(out)
    }
    pub fn list_evidences(&self, project_id: i64, evidence_ids: &[String]) -> Result<Vec<Value>> {
        let mut out = vec![];
        for id in evidence_ids {
            let v=self.conn.query_row("SELECT evidence_id,episode_id,scene_id,span_json,quote,speaker,tags_json FROM canonical_evidences WHERE project_id=? AND evidence_id=?",params![project_id,id],|r|Ok(json!({"evidence_id":r.get::<_,String>(0)?,"project_id":project_id,"episode_id":r.get::<_,Option<i64>>(1)?,"scene_id":r.get::<_,Option<i64>>(2)?,"span":parse_json(r.get(3)?),"quote":r.get::<_,String>(4)?,"speaker":r.get::<_,Option<String>>(5)?,"tags":parse_json(r.get(6)?)}))).optional()?;
            if let Some(v) = v {
                out.push(v)
            }
        }
        Ok(out)
    }
    pub fn find_entities(&self, project_id: i64, name_or_alias: &str) -> Result<Vec<Value>> {
        let mut stmt=self.conn.prepare("SELECT entity_id,entity_type,canonical_name,aliases_json,status,confidence,source_kind,created_from_evidence_id FROM canonical_entities WHERE project_id=?")?;
        let all=stmt.query_map([project_id],|r|Ok(json!({"entity_id":r.get::<_,String>(0)?,"project_id":project_id,"entity_type":r.get::<_,String>(1)?,"canonical_name":r.get::<_,String>(2)?,"aliases":parse_json(r.get(3)?),"status":r.get::<_,Option<String>>(4)?.unwrap_or_else(confirmed),"confidence":r.get::<_,Option<f64>>(5)?.unwrap_or(1.0),"source_kind":r.get::<_,Option<String>>(6)?.unwrap_or_else(system),"created_from_evidence_id":r.get::<_,Option<String>>(7)?})))?;
        let norm = normalize_name(name_or_alias);
        let mut out = vec![];
        for row in all {
            let row = row?;
            if normalize_name(row["canonical_name"].as_str().unwrap_or("")) == norm
                || ids(&row, "aliases")
                    .iter()
                    .any(|a| normalize_name(a) == norm)
            {
                out.push(row)
            }
        }
        Ok(out)
    }
    pub fn create_changeset(
        &self,
        project_id: i64,
        episode_id: Option<i64>,
        payload: &Value,
    ) -> Result<String> {
        if !payload.is_object() {
            return Err(invalid("变更集必须为 JSON 对象"));
        }
        let id = new_id();
        let now = now_ms();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("INSERT INTO canonical_changesets(changeset_id,project_id,episode_id,payload_json,review_status,review_log_json,created_at_ms,updated_at_ms) VALUES(?,?,?,?,'pending_review','[]',?,?)",params![id,project_id,episode_id,payload.to_string(),now,now])?;
        let mut r = MemoryRecord::new(
            project_id,
            "canonical",
            "changeset",
            "ChangeSet(pending_review)",
        );
        r.id = id.clone();
        r.payload_json = Some(json!({"changeset_id":id,"episode_id":episode_id,"payload":payload}));
        r.source_ref = Some("canonical_changesets".into());
        write_record(&tx, &r, false)?;
        tx.commit()?;
        Ok(id)
    }
    pub fn get_changeset(&self, id: &str) -> Result<Option<Value>> {
        changeset(&self.conn, id)
    }
    pub fn list_changesets(
        &self,
        project_id: i64,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Value>> {
        let mut stmt=self.conn.prepare("SELECT changeset_id FROM canonical_changesets WHERE project_id=? AND (? IS NULL OR review_status=?) ORDER BY updated_at_ms DESC LIMIT ?")?;
        let rows = stmt.query_map(params![project_id, status, status, limit as i64], |r| {
            r.get::<_, String>(0)
        })?;
        let mut out = vec![];
        for id in rows {
            if let Some(v) = self.get_changeset(&id?)? {
                out.push(v)
            }
        }
        Ok(out)
    }
    pub fn apply_changeset(&self, id: &str, reviewer: &str, note: Option<&str>) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let cs = changeset(&tx, id)?.ok_or_else(|| invalid("变更集不存在"))?;
        if cs["review_status"] != "pending_review" {
            return Err(invalid("只能审核待审变更集"));
        }
        let project_id = pid(&cs)?;
        let payload = &cs["payload"];
        let mut latest: HashMap<String, Value> = HashMap::new();
        for (key, kind) in [
            ("evidences", "evidence"),
            ("entities", "entity"),
            ("snapshots", "snapshot"),
            ("time_constraints", "time_constraint"),
            ("time_blocks", "time_block"),
        ] {
            if let Some(values) = payload.get(key) {
                let values = values
                    .as_array()
                    .ok_or_else(|| invalid(format!("{key} 必须为数组")))?;
                for v in values {
                    if pid(v)? != project_id {
                        return Err(invalid("变更集包含其他项目的数据"));
                    }
                    canonical(&tx, kind, v)?;
                    if kind == "snapshot"
                        && v.get("status")
                            .and_then(Value::as_str)
                            .unwrap_or("confirmed")
                            == "confirmed"
                    {
                        let entity_id = required(v, "entity_id")?;
                        let replace = latest.get(&entity_id).is_none_or(|prev| {
                            string(v, "valid_from_story_order").unwrap_or_default()
                                >= string(prev, "valid_from_story_order").unwrap_or_default()
                        });
                        if replace {
                            latest.insert(entity_id, v.clone());
                        }
                    }
                }
            }
        }
        if payload["materialize"]["write_static_bible"]
            .as_bool()
            .unwrap_or(true)
        {
            for (entity_id, snapshot) in latest {
                let profile_id = format!("profile::{entity_id}");
                let previous: Option<Option<String>> = tx
                    .query_row(
                        "SELECT story_order FROM memory_records WHERE project_id=? AND id=?",
                        params![project_id, profile_id],
                        |r| r.get(0),
                    )
                    .optional()?;
                if previous.flatten().is_some_and(|p| {
                    p > string(&snapshot, "valid_from_story_order").unwrap_or_default()
                }) {
                    continue;
                }
                let name:Option<String>=tx.query_row("SELECT canonical_name FROM canonical_entities WHERE project_id=? AND entity_id=?",params![project_id,entity_id],|r|r.get(0)).optional()?;
                let name = name.unwrap_or(entity_id.clone());
                let mut r = MemoryRecord::new(
                    project_id,
                    "static_bible",
                    "snapshot",
                    &format!("【{name}】当前设定: {}", snapshot["fields"]),
                );
                r.id = profile_id;
                r.entity = Some(name.clone());
                r.source_ref = Some(format!("changeset:{id}:materialize_static_bible"));
                r.evidence_ids = ids(&snapshot, "evidence_ids");
                r.story_order = string(&snapshot, "valid_from_story_order");
                r.confidence = confidence(&snapshot)?;
                r.source_kind =
                    string(&snapshot, "source_kind").unwrap_or_else(|| "extracted".into());
                let mut profile = snapshot.clone();
                profile["entity_name"] = json!(name);
                profile["current_snapshot_id"] = snapshot["snapshot_id"].clone();
                r.payload_json = Some(profile);
                write_record(&tx, &r, false)?;
            }
        }
        review(&tx, id, "approved", reviewer, note)?;
        tx.commit()?;
        Ok(())
    }
    pub fn reject_changeset(&self, id: &str, reviewer: &str, note: Option<&str>) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let cs = changeset(&tx, id)?.ok_or_else(|| invalid("变更集不存在"))?;
        if cs["review_status"] != "pending_review" {
            return Err(invalid("只能审核待审变更集"));
        }
        review(&tx, id, "rejected", reviewer, note)?;
        tx.commit()?;
        Ok(())
    }
    pub fn rebuild_vectors(
        &self,
        project_id: i64,
        provider: &dyn EmbeddingProvider,
    ) -> Result<usize> {
        let records = self.filtered_records(&MemoryQuery::new(project_id))?;
        let mut vectors = Vec::new();
        // Even an empty database reports an unconfigured provider explicitly.
        if records.is_empty() {
            provider.embed(&[])?;
        }
        let mut dimension = None;
        for batch in records.chunks(32) {
            let embeddings =
                provider.embed(&batch.iter().map(|r| r.content.clone()).collect::<Vec<_>>())?;
            if embeddings.len() != batch.len() {
                return Err(invalid("Embedding 返回数量与输入不一致"));
            }
            for mut vector in embeddings {
                normalize_vector(&mut vector)?;
                if dimension.is_some_and(|d| d != vector.len()) {
                    return Err(invalid("Embedding 维度不一致"));
                }
                dimension = Some(vector.len());
                vectors.push(vector);
            }
        }
        let tx = self.conn.unchecked_transaction()?;
        let key = provider.identifier();
        tx.execute("DELETE FROM rust_memory_vectors WHERE provider_key=? AND record_id IN (SELECT id FROM memory_records WHERE project_id=?)",params![key,project_id])?;
        for (record, vector) in records.iter().zip(vectors) {
            let bytes: Vec<u8> = vector.iter().flat_map(|f| f.to_le_bytes()).collect();
            tx.execute("INSERT INTO rust_memory_vectors(record_id,provider_key,dimension,content_hash,vector,updated_at_ms) VALUES(?,?,?,?,?,?)",params![record.id,key,vector.len() as i64,hash(&record.content),bytes,now_ms()])?;
        }
        tx.execute("INSERT INTO rust_memory_vector_projects(project_id,provider_key,dimension,updated_at_ms) VALUES(?,?,?,?) ON CONFLICT(project_id,provider_key) DO UPDATE SET dimension=excluded.dimension,updated_at_ms=excluded.updated_at_ms",params![project_id,key,dimension.unwrap_or(0) as i64,now_ms()])?;
        tx.commit()?;
        Ok(records.len())
    }
}
fn parse_json(s: Option<String>) -> Value {
    s.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null)
}
fn normalize_name(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}
pub(crate) fn write_record(
    conn: &Connection,
    r: &MemoryRecord,
    skip_duplicate: bool,
) -> Result<String> {
    if r.content.trim().is_empty() || r.id.is_empty() || !(0.0..=1.0).contains(&r.confidence) {
        return Err(invalid("记忆内容和 ID 不能为空，confidence 必须为 0 至 1"));
    }
    let hash = hash(&r.content);
    if skip_duplicate
        && let Some(id)=conn.query_row("SELECT id FROM memory_records WHERE project_id=? AND namespace=? AND type=? AND COALESCE(entity,'')=COALESCE(?,'') AND hash=? LIMIT 1",params![r.project_id,r.namespace,r.memory_type,r.entity,hash],|row|row.get::<_,String>(0)).optional()? {return Ok(id)}
    // IDs are globally unique in the legacy schema. Prevent cross-project overwrites.
    let existing = conn
        .query_row(
            "SELECT project_id FROM memory_records WHERE id=?",
            [&r.id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if existing.is_some_and(|p| p != r.project_id) {
        return Err(invalid("ID 已被其他项目使用"));
    }
    conn.execute("INSERT INTO memory_records(id,project_id,namespace,type,entity,content,payload_json,source_ref,time_index,status,confidence,source_kind,evidence_ids_json,story_order,story_time_json,hash,created_at_ms,updated_at_ms) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET namespace=excluded.namespace,type=excluded.type,entity=excluded.entity,content=excluded.content,payload_json=excluded.payload_json,source_ref=excluded.source_ref,time_index=excluded.time_index,status=excluded.status,confidence=excluded.confidence,source_kind=excluded.source_kind,evidence_ids_json=excluded.evidence_ids_json,story_order=excluded.story_order,story_time_json=excluded.story_time_json,hash=excluded.hash,updated_at_ms=excluded.updated_at_ms",params![r.id,r.project_id,r.namespace,r.memory_type,r.entity,r.content,r.payload_json.as_ref().map(Value::to_string),r.source_ref,r.time_index,r.status,r.confidence,r.source_kind,serde_json::to_string(&r.evidence_ids)?,r.story_order,r.story_time.as_ref().map(Value::to_string),hash,r.created_at_ms.unwrap_or_else(now_ms),now_ms()])?;
    conn.execute(
        "DELETE FROM rust_memory_vectors WHERE record_id=? AND content_hash<>?",
        params![r.id, hash],
    )?;
    Ok(r.id.clone())
}
fn canonical(conn: &Connection, kind: &str, v: &Value) -> Result<String> {
    let project_id = pid(v)?;
    let mut v = v.clone();
    if ["time_constraint", "time_block"].contains(&kind) {
        if v.get("status").is_none() {
            v["status"] = json!("hypothesis")
        }
        if v.get("confidence").is_none() {
            v["confidence"] = json!(0.5)
        }
    }
    let v = &v;
    let (status, confidence, source) = meta(v)?;
    let now = now_ms();
    let mut r = MemoryRecord::new(project_id, "canonical", kind, "");
    r.status = status.clone();
    r.confidence = confidence;
    r.source_kind = source.clone();
    r.payload_json = Some(v.clone());
    r.evidence_ids = ids(v, "evidence_ids");
    match kind {
        "evidence" => {
            let id = string(v, "evidence_id").unwrap_or_else(new_id);
            let quote = required(v, "quote")?;
            r.id = id.clone();
            r.entity = string(v, "speaker");
            r.content = format!("证据: {quote}");
            r.evidence_ids = vec![id.clone()];
            r.source_ref = Some("canonical_evidences".into());
            conn.execute("INSERT INTO canonical_evidences(evidence_id,project_id,episode_id,scene_id,span_json,quote,speaker,tags_json,created_at_ms) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(evidence_id) DO UPDATE SET episode_id=excluded.episode_id,scene_id=excluded.scene_id,span_json=excluded.span_json,quote=excluded.quote,speaker=excluded.speaker,tags_json=excluded.tags_json WHERE project_id=excluded.project_id",params![id,project_id,v["episode_id"].as_i64(),v["scene_id"].as_i64(),serialized(v,"span"),quote,string(v,"speaker"),serialized(v,"tags"),now])?;
        }
        "entity" => {
            let id = required(v, "entity_id")?;
            let name = required(v, "canonical_name")?;
            let entity_type = required(v, "entity_type")?;
            r.id = id.clone();
            r.entity = Some(name.clone());
            r.content = format!(
                "实体({entity_type}): {name}；别名: {}",
                ids(v, "aliases").join(",")
            );
            r.source_ref = Some("canonical_entities".into());
            conn.execute("INSERT INTO canonical_entities(entity_id,project_id,entity_type,canonical_name,aliases_json,status,confidence,source_kind,created_from_evidence_id,created_at_ms,updated_at_ms) VALUES(?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(entity_id) DO UPDATE SET entity_type=excluded.entity_type,canonical_name=excluded.canonical_name,aliases_json=excluded.aliases_json,status=excluded.status,confidence=excluded.confidence,source_kind=excluded.source_kind,created_from_evidence_id=excluded.created_from_evidence_id,updated_at_ms=excluded.updated_at_ms WHERE project_id=excluded.project_id",params![id,project_id,entity_type,name,serialized(v,"aliases"),status,confidence,source,string(v,"created_from_evidence_id"),now,now])?;
        }
        "snapshot" => {
            let id = required(v, "snapshot_id")?;
            let entity_id = required(v, "entity_id")?;
            if !v["fields"].is_object() {
                return Err(invalid("snapshot.fields 必须为 JSON 对象"));
            }
            r.id = id.clone();
            r.entity = Some(entity_id.clone());
            r.content = format!("切片({entity_id}): {}", v["fields"]);
            r.story_order = string(v, "valid_from_story_order");
            r.source_ref = Some("canonical_snapshots".into());
            conn.execute("INSERT INTO canonical_snapshots(snapshot_id,project_id,entity_id,valid_from_story_time_key,valid_to_story_time_key,valid_from_story_order,valid_to_story_order,fields_json,why,evidence_ids_json,status,confidence,source_kind,created_at_ms,updated_at_ms) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(snapshot_id) DO UPDATE SET entity_id=excluded.entity_id,valid_from_story_time_key=excluded.valid_from_story_time_key,valid_to_story_time_key=excluded.valid_to_story_time_key,valid_from_story_order=excluded.valid_from_story_order,valid_to_story_order=excluded.valid_to_story_order,fields_json=excluded.fields_json,why=excluded.why,evidence_ids_json=excluded.evidence_ids_json,status=excluded.status,confidence=excluded.confidence,source_kind=excluded.source_kind,updated_at_ms=excluded.updated_at_ms WHERE project_id=excluded.project_id",params![id,project_id,entity_id,string(v,"valid_from_story_time_key"),string(v,"valid_to_story_time_key"),string(v,"valid_from_story_order"),string(v,"valid_to_story_order"),v["fields"].to_string(),string(v,"why"),serialized(v,"evidence_ids"),status,confidence,source,now,now])?;
        }
        "time_constraint" => {
            let id = string(v, "id")
                .or_else(|| string(v, "constraint_id"))
                .unwrap_or_else(new_id);
            let relation = required(v, "relation")?;
            if ![
                "before",
                "after",
                "during",
                "overlaps",
                "flashback_of",
                "within_interval",
                "anchored_to",
            ]
            .contains(&relation.as_str())
            {
                return Err(invalid("未知时间约束关系"));
            }
            r.id = id.clone();
            r.content = format!(
                "时间约束({relation}): {} → {}",
                string(v, "from_event_id").unwrap_or_default(),
                string(v, "to_event_id").unwrap_or_default()
            );
            r.source_ref = Some("canonical_time_constraints".into());
            conn.execute("INSERT INTO canonical_time_constraints(constraint_id,project_id,relation,from_event_id,to_event_id,anchor_id,interval_json,evidence_ids_json,status,confidence,source_kind,created_at_ms,updated_at_ms) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(constraint_id) DO UPDATE SET relation=excluded.relation,from_event_id=excluded.from_event_id,to_event_id=excluded.to_event_id,anchor_id=excluded.anchor_id,interval_json=excluded.interval_json,evidence_ids_json=excluded.evidence_ids_json,status=excluded.status,confidence=excluded.confidence,source_kind=excluded.source_kind,updated_at_ms=excluded.updated_at_ms WHERE project_id=excluded.project_id",params![id,project_id,relation,string(v,"from_event_id"),string(v,"to_event_id"),string(v,"anchor_id"),serialized(v,"interval"),serialized(v,"evidence_ids"),status,confidence,source,now,now])?;
        }
        "time_block" => {
            let id = string(v, "id")
                .or_else(|| string(v, "block_id"))
                .unwrap_or_else(new_id);
            r.id = id.clone();
            r.content = format!(
                "时间块: {}",
                string(v, "name").unwrap_or_else(|| id.clone())
            );
            r.source_ref = Some("canonical_time_blocks".into());
            conn.execute("INSERT INTO canonical_time_blocks(block_id,project_id,name,parent_block_id,anchor_id,constraints_json,event_ids_json,status,confidence,source_kind,created_at_ms,updated_at_ms) VALUES(?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(block_id) DO UPDATE SET name=excluded.name,parent_block_id=excluded.parent_block_id,anchor_id=excluded.anchor_id,constraints_json=excluded.constraints_json,event_ids_json=excluded.event_ids_json,status=excluded.status,confidence=excluded.confidence,source_kind=excluded.source_kind,updated_at_ms=excluded.updated_at_ms WHERE project_id=excluded.project_id",params![id,project_id,string(v,"name"),string(v,"parent_block_id"),string(v,"anchor_id"),serialized(v,"constraint_ids"),serialized(v,"event_ids"),status,confidence,source,now,now])?;
        }
        _ => return Err(invalid("不支持的 canonical 类型")),
    }
    write_record(conn, &r, false)
}
fn changeset(conn: &Connection, id: &str) -> Result<Option<Value>> {
    Ok(conn.query_row("SELECT changeset_id,project_id,episode_id,payload_json,review_status,review_log_json,created_at_ms,updated_at_ms FROM canonical_changesets WHERE changeset_id=?",[id],|r|Ok(json!({"changeset_id":r.get::<_,String>(0)?,"project_id":r.get::<_,i64>(1)?,"episode_id":r.get::<_,Option<i64>>(2)?,"payload":parse_json(r.get(3)?),"review_status":r.get::<_,String>(4)?,"review_log":parse_json(r.get(5)?),"created_at_ms":r.get::<_,Option<i64>>(6)?,"updated_at_ms":r.get::<_,Option<i64>>(7)?}))).optional()?)
}
fn review(
    conn: &Connection,
    id: &str,
    status: &str,
    reviewer: &str,
    note: Option<&str>,
) -> Result<()> {
    let mut cs = changeset(conn, id)?.ok_or_else(|| invalid("变更集不存在"))?;
    let mut log = cs["review_log"].as_array().cloned().unwrap_or_default();
    log.push(json!({"at_ms":now_ms(),"action":status,"reviewer":reviewer,"note":note}));
    conn.execute("UPDATE canonical_changesets SET review_status=?,review_log_json=?,updated_at_ms=? WHERE changeset_id=?",params![status,serde_json::to_string(&log)?,now_ms(),id])?;
    cs["review_status"] = json!(status);
    conn.execute(
        "UPDATE memory_records SET content=?,payload_json=?,updated_at_ms=? WHERE id=?",
        params![format!("ChangeSet({status})"), cs.to_string(), now_ms(), id],
    )?;
    Ok(())
}
impl MemoryStore {
    pub fn create_conflict(&self, project_id: i64, v: &Value) -> Result<String> {
        let tx = self.conn.unchecked_transaction()?;
        let id = conflict_record(&tx, project_id, v)?;
        tx.commit()?;
        Ok(id)
    }
    pub fn list_conflicts(
        &self,
        project_id: i64,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Value>> {
        let mut stmt=self.conn.prepare("SELECT conflict_id,changeset_id,conflict_type,entity_id,old_claim_json,new_claim_json,suggested_actions_json,status,resolved_by,resolution_note,created_at_ms,updated_at_ms FROM canonical_conflicts WHERE project_id=? AND (? IS NULL OR status=?) ORDER BY updated_at_ms DESC LIMIT ?")?;
        let rows=stmt.query_map(params![project_id,status,status,limit as i64],|r|Ok(json!({"conflict_id":r.get::<_,String>(0)?,"project_id":project_id,"changeset_id":r.get::<_,Option<String>>(1)?,"conflict_type":r.get::<_,String>(2)?,"entity_id":r.get::<_,Option<String>>(3)?,"old_claim":parse_json(r.get(4)?),"new_claim":parse_json(r.get(5)?),"suggested_actions":parse_json(r.get(6)?),"status":r.get::<_,String>(7)?,"resolved_by":r.get::<_,Option<String>>(8)?,"resolution_note":r.get::<_,Option<String>>(9)?,"created_at_ms":r.get::<_,Option<i64>>(10)?,"updated_at_ms":r.get::<_,Option<i64>>(11)?})))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
    pub fn resolve_conflict(&self, id: &str, reviewer: &str, note: Option<&str>) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let project_id = tx
            .query_row(
                "SELECT project_id FROM canonical_conflicts WHERE conflict_id=?",
                [id],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .ok_or_else(|| invalid("冲突不存在"))?;
        tx.execute("UPDATE canonical_conflicts SET status='resolved',resolved_by=?,resolution_note=?,updated_at_ms=? WHERE conflict_id=?",params![reviewer,note,now_ms(),id])?;
        let payload: Option<String> = tx
            .query_row(
                "SELECT payload_json FROM memory_records WHERE id=? AND project_id=?",
                params![id, project_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let mut payload = parse_json(payload);
        if !payload.is_object() {
            payload = json!({"conflict_id":id})
        }
        payload["status"] = json!("resolved");
        payload["resolved_by"] = json!(reviewer);
        payload["resolution_note"] = json!(note);
        tx.execute(
            "UPDATE memory_records SET payload_json=?,updated_at_ms=? WHERE id=? AND project_id=?",
            params![payload.to_string(), now_ms(), id, project_id],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn list_time_constraints(
        &self,
        project_id: i64,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Value>> {
        let mut stmt=self.conn.prepare("SELECT constraint_id,relation,from_event_id,to_event_id,anchor_id,interval_json,evidence_ids_json,status,confidence,source_kind FROM canonical_time_constraints WHERE project_id=? AND (? IS NULL OR status=?) ORDER BY updated_at_ms DESC LIMIT ?")?;
        let rows=stmt.query_map(params![project_id,status,status,limit as i64],|r|Ok(json!({"id":r.get::<_,String>(0)?,"project_id":project_id,"relation":r.get::<_,String>(1)?,"from_event_id":r.get::<_,Option<String>>(2)?,"to_event_id":r.get::<_,Option<String>>(3)?,"anchor_id":r.get::<_,Option<String>>(4)?,"interval":parse_json(r.get(5)?),"evidence_ids":parse_json(r.get(6)?),"status":r.get::<_,Option<String>>(7)?.unwrap_or_else(||"hypothesis".into()),"confidence":r.get::<_,Option<f64>>(8)?.unwrap_or(0.5),"source_kind":r.get::<_,Option<String>>(9)?.unwrap_or_else(||"extracted".into())})))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
    pub fn list_time_blocks(
        &self,
        project_id: i64,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Value>> {
        let mut stmt=self.conn.prepare("SELECT block_id,name,parent_block_id,anchor_id,constraints_json,event_ids_json,status,confidence,source_kind FROM canonical_time_blocks WHERE project_id=? AND (? IS NULL OR status=?) ORDER BY updated_at_ms DESC LIMIT ?")?;
        let rows=stmt.query_map(params![project_id,status,status,limit as i64],|r|Ok(json!({"id":r.get::<_,String>(0)?,"project_id":project_id,"name":r.get::<_,Option<String>>(1)?,"parent_block_id":r.get::<_,Option<String>>(2)?,"anchor_id":r.get::<_,Option<String>>(3)?,"constraint_ids":parse_json(r.get(4)?),"event_ids":parse_json(r.get(5)?),"status":r.get::<_,Option<String>>(6)?.unwrap_or_else(||"hypothesis".into()),"confidence":r.get::<_,Option<f64>>(7)?.unwrap_or(0.5),"source_kind":r.get::<_,Option<String>>(8)?.unwrap_or_else(||"extracted".into())})))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
    /// Safe exact name/alias resolution. Ambiguity is returned for human review.
    pub fn resolve_changeset_entities(&self, project_id: i64, payload: &Value) -> Result<Value> {
        let mut out = payload.clone();
        if !out.is_object() {
            return Err(invalid("变更集必须为对象"));
        }
        let mut entities = out["entities"].as_array().cloned().unwrap_or_default();
        let mut conflicts = out["conflicts"].as_array().cloned().unwrap_or_default();
        let mut remap = HashMap::<String, String>::new();
        let mut unique = Vec::new();
        let mut seen = HashMap::<(String, String), String>::new();
        let mut name_types = HashMap::<String, Vec<String>>::new();
        for entity in entities {
            let name = required(&entity, "canonical_name")?;
            let kind = required(&entity, "entity_type")?;
            let id = required(&entity, "entity_id")?;
            let normalized = normalize_name(&name);
            let key = (kind.clone(), normalized.clone());
            if let Some(first) = seen.get(&key) {
                if first != &id {
                    remap.insert(id.clone(), first.clone());
                    conflicts.push(json!({"conflict_type":"duplicate_in_payload","entity_id":first,"old_claim":{"entity_id":first},"new_claim":entity,"suggested_actions":[{"action":"dedupe_entities_in_payload"}]}));
                }
                continue;
            }
            seen.insert(key, id);
            name_types.entry(normalized).or_default().push(kind);
            unique.push(entity);
        }
        entities = unique;
        for (name, mut types) in name_types {
            types.sort();
            types.dedup();
            if types.len() > 1 {
                conflicts.push(json!({"conflict_type":"name_type_collision","old_claim":{"canonical_name_norm":name,"types":types},"suggested_actions":[{"action":"split_entities_by_type"},{"action":"review_entity_type"}]}));
            }
        }
        for entity in &mut entities {
            let name = required(entity, "canonical_name")?;
            let entity_type = required(entity, "entity_type")?;
            if let Some(hint) = type_hint(&name).filter(|hint| *hint != entity_type) {
                conflicts.push(json!({"conflict_type":"type_hint_mismatch","entity_id":entity["entity_id"],"old_claim":{"entity_type":entity_type,"canonical_name":name},"new_claim":{"hinted_type":hint},"suggested_actions":[{"action":"review_entity_type"}]}));
            }
            let mut candidates = self.find_entities(project_id, &name)?;
            for alias in ids(entity, "aliases").iter().take(20) {
                candidates.extend(self.find_entities(project_id, alias)?)
            }
            candidates.sort_by_key(|v| string(v, "entity_id").unwrap_or_default());
            candidates.dedup_by_key(|v| string(v, "entity_id").unwrap_or_default());
            let same: Vec<_> = candidates
                .iter()
                .filter(|c| c["entity_type"] == entity_type)
                .collect();
            if same.len() == 1 {
                let target = required(same[0], "entity_id")?;
                if let Some(old) = string(entity, "entity_id") {
                    remap.insert(old, target.clone());
                }
                entity["entity_id"] = json!(target);
                let aliases = ids(entity, "aliases");
                let existing = ids(same[0], "aliases");
                for alias in aliases {
                    if !existing.contains(&alias) && alias != name {
                        conflicts.push(json!({"conflict_type":"alias_new","entity_id":target,"old_claim":{"aliases":existing},"new_claim":{"alias":alias},"suggested_actions":[{"action":"merge_alias","target_entity_id":target,"alias":alias}]}));
                    }
                }
            } else if !candidates.is_empty() {
                conflicts.push(json!({"conflict_type":if same.len()>1{"entity_ambiguous"}else{"canonical_type_mismatch"},"entity_id":entity["entity_id"],"old_claim":entity,"new_claim":{"candidates":candidates},"suggested_actions":[{"action":"choose_existing_entity"},{"action":"create_new_entity"}]}));
            }
        }
        let direct = remap.clone();
        for target in remap.values_mut() {
            for _ in 0..direct.len() {
                match direct.get(target).filter(|next| *next != target) {
                    Some(next) => *target = next.clone(),
                    None => break,
                }
            }
        }
        fn replace_refs(v: &mut Value, map: &HashMap<String, String>) {
            match v {
                Value::Object(o) => {
                    for (key, v) in o {
                        if ["entity_id", "target_entity_id"].contains(&key.as_str()) {
                            if let Some(new) = v.as_str().and_then(|s| map.get(s)) {
                                *v = json!(new)
                            }
                        } else if key == "participants" {
                            if let Some(a) = v.as_array_mut() {
                                for v in a {
                                    if let Some(new) = v.as_str().and_then(|s| map.get(s)) {
                                        *v = json!(new)
                                    }
                                }
                            }
                        } else {
                            replace_refs(v, map)
                        }
                    }
                }
                Value::Array(a) => {
                    for v in a {
                        replace_refs(v, map)
                    }
                }
                _ => {}
            }
        }
        out["entities"] = json!(entities);
        for key in ["snapshots", "events", "state_changes"] {
            if let Some(v) = out.get_mut(key) {
                replace_refs(v, &remap)
            }
        }
        out["conflicts"] = json!(conflicts);
        dedupe_conflicts(&mut out);
        Ok(out)
    }
    pub fn retrieve_semantic(
        &self,
        q: &MemoryQuery,
        provider: &dyn EmbeddingProvider,
    ) -> Result<MemoryRetrievalResult> {
        let text = q
            .query_text
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| invalid("语义检索需要查询文本"))?;
        let mut embeddings = provider.embed(std::slice::from_ref(text))?;
        if embeddings.len() != 1 {
            return Err(invalid("Embedding 查询返回数量错误"));
        }
        let mut query = embeddings.remove(0);
        normalize_vector(&mut query)?;
        let mut filter = q.clone();
        filter.query_text = None;
        let records = self.filtered_records(&filter)?;
        let key = provider.identifier();
        let mut ranked = Vec::new();
        for record in records {
            let cached:Option<(i64,String,Vec<u8>)>=self.conn.query_row("SELECT dimension,content_hash,vector FROM rust_memory_vectors WHERE record_id=? AND provider_key=?",params![record.id,key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            let (dim, content_hash, bytes) =
                cached.ok_or_else(|| invalid("向量索引缺失或已失效，请先重建项目向量索引"))?;
            if dim as usize != query.len()
                || bytes.len() != query.len() * 4
                || content_hash != hash(&record.content)
            {
                return Err(invalid("向量索引模型或内容发生变化，请重建项目向量索引"));
            }
            let vector: Vec<f32> = bytes
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect();
            let score: f32 = query.iter().zip(vector).map(|(a, b)| a * b).sum();
            if !score.is_finite() {
                return Err(invalid("向量索引包含无效数值，请重建"));
            }
            if q.min_score.is_none_or(|min| score >= min) {
                ranked.push((score.clamp(-1., 1.), record))
            }
        }
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
        let total = ranked.len();
        ranked.truncate(q.top_k);
        let (scores, records) = ranked.into_iter().unzip();
        Ok(MemoryRetrievalResult {
            records,
            scores,
            total,
        })
    }
}
fn normalize_vector(vector: &mut [f32]) -> Result<()> {
    let norm = vector
        .iter()
        .map(|v| f64::from(*v).powi(2))
        .sum::<f64>()
        .sqrt();
    if vector.is_empty() || !norm.is_finite() || norm <= 0.0 {
        return Err(invalid("Embedding 必须返回有限且非零的向量"));
    }
    for v in vector {
        *v = (f64::from(*v) / norm) as f32;
    }
    Ok(())
}
impl MemoryStore {
    pub(crate) fn existing_run_changeset(
        &self,
        project_id: i64,
        run_id: &str,
        payload: &Value,
    ) -> Result<Option<String>> {
        let existing:Option<(String,String,String,String)>=self.conn.query_row("SELECT r.changeset_id,r.raw_payload_hash,c.review_status,c.payload_json FROM rust_memory_run_changesets r JOIN canonical_changesets c ON c.changeset_id=r.changeset_id WHERE r.project_id=? AND r.run_id=?",params![project_id,run_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        if let Some((id, raw_hash, status, stored)) = existing {
            if raw_hash == hash(&payload.to_string())
                || serde_json::from_str::<Value>(&stored)? == *payload
            {
                return Ok(Some(id));
            }
            if status != "pending_review" {
                return Err(invalid(
                    "该运行的变更集已审核，不能替换已审核 payload；请创建新的运行",
                ));
            }
        }
        Ok(None)
    }
    pub fn ensure_changeset_for_run(
        &self,
        project_id: i64,
        episode_id: Option<i64>,
        run_id: &str,
        payload: &Value,
    ) -> Result<String> {
        self.ensure_run_changeset(project_id, episode_id, run_id, payload, None)
    }
    pub fn ensure_changeset_for_run_with_provider(
        &self,
        project_id: i64,
        episode_id: Option<i64>,
        run_id: &str,
        payload: &Value,
        provider: &dyn EmbeddingProvider,
    ) -> Result<String> {
        self.ensure_run_changeset(project_id, episode_id, run_id, payload, Some(provider))
    }
    fn ensure_run_changeset(
        &self,
        project_id: i64,
        episode_id: Option<i64>,
        run_id: &str,
        payload: &Value,
        provider: Option<&dyn EmbeddingProvider>,
    ) -> Result<String> {
        if run_id.trim().is_empty() || !payload.is_object() {
            return Err(invalid("run_id 不能为空，变更集必须为对象"));
        }
        if payload
            .get("schema_version")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty() && s != "changeset.v0")
        {
            return Err(invalid("不支持的 changeset schema_version"));
        }
        let raw_hash = hash(&payload.to_string());
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let existing:Option<(String,String,String,String)>=tx.query_row("SELECT r.changeset_id,r.raw_payload_hash,c.review_status,c.payload_json FROM rust_memory_run_changesets r JOIN canonical_changesets c ON c.changeset_id=r.changeset_id WHERE r.project_id=? AND r.run_id=?",params![project_id,run_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        if let Some((id, old_hash, status, stored)) = &existing {
            if old_hash == &raw_hash || serde_json::from_str::<Value>(stored)? == *payload {
                return Ok(id.clone());
            }
            if status != "pending_review" {
                return Err(invalid(
                    "该运行的变更集已审核，不能替换已审核 payload；请创建新的运行",
                ));
            }
        }
        let mut evidence_stmt =
            tx.prepare("SELECT evidence_id FROM canonical_evidences WHERE project_id=?")?;
        let known = evidence_stmt
            .query_map([project_id], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(evidence_stmt);
        let requested = payload.get("evidence_ids").and_then(Value::as_array);
        let evidence = if let Some(requested) = requested {
            requested
                .iter()
                .filter_map(Value::as_str)
                .filter(|s| known.iter().any(|id| id == s))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        } else {
            known
        };
        let order = string(payload, "story_order_base")
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                episode_id
                    .map(|episode| format!("CH{episode:02}"))
                    .unwrap_or_else(|| format!("RUN.{run_id}"))
            });
        let normalized =
            normalize_extracted_changeset(project_id, episode_id, &order, &evidence, payload)?;
        let normalized = if let Some(provider) = provider {
            self.resolve_changeset_entities_with_provider(project_id, &normalized, provider)?
        } else {
            self.resolve_changeset_entities(project_id, &normalized)?
        };
        let id = existing.as_ref().map(|v| v.0.clone()).unwrap_or_else(|| {
            hash(&format!("aicomic-memory-run:{project_id}:{run_id}"))[..32].to_owned()
        });
        let now = now_ms();
        if existing.is_some() {
            tx.execute("UPDATE canonical_changesets SET payload_json=?,episode_id=?,updated_at_ms=? WHERE changeset_id=? AND project_id=?",params![normalized.to_string(),episode_id,now,id,project_id])?;
            tx.execute("UPDATE rust_memory_run_changesets SET raw_payload_hash=?,updated_at_ms=? WHERE project_id=? AND run_id=?",params![raw_hash,now,project_id,run_id])?;
        } else {
            tx.execute("INSERT INTO canonical_changesets(changeset_id,project_id,episode_id,payload_json,review_status,review_log_json,created_at_ms,updated_at_ms) VALUES(?,?,?,?,'pending_review','[]',?,?)",params![id,project_id,episode_id,normalized.to_string(),now,now])?;
            tx.execute("INSERT INTO rust_memory_run_changesets(project_id,run_id,changeset_id,raw_payload_hash,created_at_ms,updated_at_ms) VALUES(?,?,?,?,?,?)",params![project_id,run_id,id,raw_hash,now,now])?;
        }
        let mut record = MemoryRecord::new(
            project_id,
            "canonical",
            "changeset",
            "ChangeSet(pending_review)",
        );
        record.id = id.clone();
        record.source_ref = Some(format!("run:{run_id}"));
        record.payload_json = Some(
            json!({"changeset_id":id,"episode_id":episode_id,"run_id":run_id,"payload":normalized}),
        );
        write_record(&tx, &record, false)?;
        for conflict in normalized["conflicts"].as_array().into_iter().flatten() {
            let mut conflict = conflict.clone();
            conflict["changeset_id"] = json!(id);
            let conflict_id =
                hash(&format!("changeset-conflict:{id}:{}", conflict))[..32].to_owned();
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM canonical_conflicts WHERE conflict_id=?)",
                [&conflict_id],
                |r| r.get(0),
            )?;
            if !exists {
                conflict["conflict_id"] = json!(conflict_id);
                conflict_record(&tx, project_id, &conflict)?;
            }
        }
        tx.commit()?;
        Ok(id)
    }
    pub fn resolve_changeset_entities_with_provider(
        &self,
        project_id: i64,
        payload: &Value,
        provider: &dyn EmbeddingProvider,
    ) -> Result<Value> {
        let mut out = self.resolve_changeset_entities(project_id, payload)?;
        let mut conflicts = out["conflicts"].as_array().cloned().unwrap_or_default();
        for entity in out["entities"].as_array().into_iter().flatten() {
            let name = required(entity, "canonical_name")?;
            let current = required(entity, "entity_id")?;
            let kind = required(entity, "entity_type")?;
            let query = std::iter::once(name)
                .chain(ids(entity, "aliases").into_iter().take(8))
                .collect::<Vec<_>>()
                .join(" ");
            let mut filter = MemoryQuery::new(project_id);
            filter.namespace = Some("canonical".into());
            filter.memory_type = Some("entity".into());
            filter.status = Some("confirmed".into());
            filter.query_text = Some(query.clone());
            filter.top_k = 12;
            let result = self.retrieve_semantic(&filter, provider)?;
            let mut candidates=result.records.into_iter().zip(result.scores).filter(|(record,_)|record.id!=current).map(|(record,score)|{let payload=record.payload_json.unwrap_or_else(||json!({}));json!({"entity_id":record.id,"entity_type":payload["entity_type"],"canonical_name":payload["canonical_name"],"aliases":payload["aliases"],"score":score})}).collect::<Vec<_>>();
            candidates.sort_by_key(|c| c["entity_type"] != kind);
            candidates.truncate(10);
            if !candidates.is_empty() {
                let mut actions=candidates.iter().take(5).map(|c|json!({"action":"choose_existing_entity","params":{"target_entity_id":c["entity_id"]}})).collect::<Vec<_>>();
                actions.push(json!({"action":"create_new_entity"}));
                conflicts.push(json!({"conflict_type":"entity_similar","entity_id":current,"old_claim":entity,"new_claim":{"candidates":candidates,"query":query},"suggested_actions":actions}));
            }
        }
        out["conflicts"] = json!(conflicts);
        dedupe_conflicts(&mut out);
        Ok(out)
    }
}
fn conflict_record(conn: &Connection, project_id: i64, v: &Value) -> Result<String> {
    let id = string(v, "conflict_id").unwrap_or_else(new_id);
    let kind = required(v, "conflict_type")?;
    let now = now_ms();
    conn.execute("INSERT INTO canonical_conflicts(conflict_id,project_id,changeset_id,conflict_type,entity_id,old_claim_json,new_claim_json,suggested_actions_json,status,created_at_ms,updated_at_ms) VALUES(?,?,?,?,?,?,?,?,'open',?,?)",params![id,project_id,string(v,"changeset_id"),kind,string(v,"entity_id"),serialized(v,"old_claim"),serialized(v,"new_claim"),serialized(v,"suggested_actions"),now,now])?;
    let mut record = MemoryRecord::new(
        project_id,
        "canonical",
        "conflict",
        &format!(
            "Conflict({kind}): {}",
            string(v, "entity_id").unwrap_or_default()
        ),
    );
    record.id = id.clone();
    record.entity = string(v, "entity_id");
    let mut payload = v.clone();
    payload["status"] = json!("open");
    payload["conflict_id"] = json!(id);
    record.payload_json = Some(payload);
    record.source_ref = Some("canonical_conflicts".into());
    write_record(conn, &record, false)?;
    Ok(id)
}
fn type_hint(name: &str) -> Option<&'static str> {
    let hints = [
        (
            "location",
            "城|镇|村|乡|街|巷|路|桥|河|湖|海|山|谷|岭|林|洞|寺|庙|宫|殿|府|楼|阁|院|馆|营|寨|港|站|区|岛|界|境",
        ),
        (
            "organization",
            "宗|门|派|帮|会|盟|社|团|教|司|局|署|院|堂|阁|馆|公司|集团|军|队|营|旅|团",
        ),
        (
            "prop",
            "剑|刀|枪|弓|戟|印|令|符|卷|书|图|镜|盔|甲|丹|丸|药|芯片|钥匙|戒|链|石|玉|匕首|手枪",
        ),
    ];
    let mut best = None;
    for (kind, keys) in hints {
        let score = keys.split('|').filter(|key| name.contains(key)).count();
        if score >= 2 && best.is_none_or(|(_, s)| score > s) {
            best = Some((kind, score))
        }
    }
    best.map(|(kind, _)| kind)
}
fn dedupe_conflicts(payload: &mut Value) {
    if let Some(conflicts) = payload["conflicts"].as_array_mut() {
        let mut seen = std::collections::HashSet::new();
        conflicts.retain(|c| seen.insert(c.to_string()));
    }
}
impl MemoryStore {
    /// Incremental maintenance after an explicit initial rebuild. Embeds only
    /// changed/new facts; never reloads or downloads a different model silently.
    pub fn refresh_vectors(
        &self,
        project_id: i64,
        provider: &dyn EmbeddingProvider,
    ) -> Result<usize> {
        let key = provider.identifier();
        let dimension:Option<i64>=self.conn.query_row("SELECT dimension FROM rust_memory_vector_projects WHERE project_id=? AND provider_key=?",params![project_id,key],|r|r.get(0)).optional()?;
        let mut dimension = dimension
            .ok_or_else(|| invalid("该项目和模型尚未建立向量索引，请先显式重建一次"))?
            as usize;
        let all = self.filtered_records(&MemoryQuery::new(project_id))?;
        let mut dirty = vec![];
        for record in all {
            let current:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM rust_memory_vectors WHERE record_id=? AND provider_key=? AND content_hash=?)",params![record.id,key,hash(&record.content)],|r|r.get(0))?;
            if !current {
                dirty.push(record)
            }
        }
        let mut vectors = vec![];
        for batch in dirty.chunks(32) {
            let embeddings =
                provider.embed(&batch.iter().map(|r| r.content.clone()).collect::<Vec<_>>())?;
            if embeddings.len() != batch.len() {
                return Err(invalid("Embedding 返回数量与输入不一致"));
            }
            for mut vector in embeddings {
                normalize_vector(&mut vector)?;
                if dimension != 0 && dimension != vector.len() {
                    return Err(invalid("Embedding 维度与既有索引不一致，请重建"));
                }
                dimension = vector.len();
                vectors.push(vector)
            }
        }
        let tx = self.conn.unchecked_transaction()?;
        for (record, vector) in dirty.iter().zip(vectors) {
            let bytes: Vec<u8> = vector.iter().flat_map(|n| n.to_le_bytes()).collect();
            tx.execute("INSERT INTO rust_memory_vectors(record_id,provider_key,dimension,content_hash,vector,updated_at_ms) VALUES(?,?,?,?,?,?) ON CONFLICT(record_id,provider_key) DO UPDATE SET dimension=excluded.dimension,content_hash=excluded.content_hash,vector=excluded.vector,updated_at_ms=excluded.updated_at_ms",params![record.id,key,dimension as i64,hash(&record.content),bytes,now_ms()])?;
        }
        tx.execute("UPDATE rust_memory_vector_projects SET dimension=?,updated_at_ms=? WHERE project_id=? AND provider_key=?",params![dimension as i64,now_ms(),project_id,key])?;
        tx.commit()?;
        Ok(dirty.len())
    }
}
