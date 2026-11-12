use crate::store::write_record;
use crate::*;
use rusqlite::{Connection, TransactionBehavior, params};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const PROJECTION: &str = "context_projection";
const BIBLE_SOURCE: &str = "series_bible.v1";

impl MemoryStore {
    /// Atomically replace this JSON source's current projection, retaining history.
    pub fn index_series_bible(&self, project_id: i64, bible: &Value) -> Result<Vec<String>> {
        let records = Self::bible_projection(project_id, bible)?;
        self.sync_one_projection(project_id, BIBLE_SOURCE, records)
    }

    fn bible_projection(project_id: i64, bible: &Value) -> Result<Vec<MemoryRecord>> {
        require_object(bible)?;
        for key in ["world_rules", "glossary", "characters"] {
            require_optional_object(bible, key)?;
        }
        if !bible["constraints"].is_null()
            && !bible["constraints"].is_array()
            && !bible["constraints"].is_object()
        {
            return Err(invalid("constraints 必须为数组或对象"));
        }
        if let Some(characters) = bible["characters"].as_object() {
            for data in characters.values() {
                if !data.is_null() {
                    require_object(data)?;
                }
            }
        }
        let mut records = vec![];
        for key in ["world_rules", "glossary"] {
            if let Some(values) = bible[key].as_object() {
                for (name, value) in values {
                    let (content, payload) = if key == "world_rules" {
                        (
                            format!("世界规则: {name}. {}", value_text(value)),
                            json!({"key":name,"value":value}),
                        )
                    } else {
                        (
                            format!("术语 {name}: {}", value_text(value)),
                            json!({"term":name,"definition":value}),
                        )
                    };
                    let mut r =
                        MemoryRecord::new(project_id, "static_bible", "world_rule", &content);
                    r.payload_json = Some(payload);
                    records.push(r);
                }
            }
        }
        if let Some(characters) = bible["characters"].as_object() {
            for (name, data) in characters {
                if let Some(data) = data.as_object() {
                    if let Some(reference) = data.get("visual_dna_ref").filter(|v| !v.is_null()) {
                        let mut r = MemoryRecord::new(
                            project_id,
                            "static_bible",
                            "character_design",
                            &format!("角色 {name} 的视觉设计引用: {}", value_text(reference)),
                        );
                        r.entity = Some(name.clone());
                        r.payload_json = Some(json!({"character":name,"visual_dna_ref":reference}));
                        records.push(r);
                    }
                    let mut facts = data.clone();
                    facts.remove("visual_dna_ref");
                    if !facts.is_empty() {
                        let mut r = MemoryRecord::new(
                            project_id,
                            "static_bible",
                            "character_fact",
                            &format!("角色 {name} 的设定: {}", Value::Object(facts.clone())),
                        );
                        r.entity = Some(name.clone());
                        r.payload_json = Some(json!({"character":name,"facts":facts}));
                        records.push(r);
                    }
                }
            }
        }
        let constraints = &bible["constraints"];
        if let Some(values) = constraints.as_array() {
            for value in values {
                let mut r = MemoryRecord::new(
                    project_id,
                    "world_rules_negative",
                    "negative_constraint",
                    &format!("约束: {}", value_text(value)),
                );
                r.payload_json = Some(json!({"constraint":value}));
                records.push(r)
            }
        } else if let Some(values) = constraints.as_object() {
            for (name, value) in values {
                let mut r = MemoryRecord::new(
                    project_id,
                    "world_rules_negative",
                    "negative_constraint",
                    &format!("约束 {name}: {}", value_text(value)),
                );
                r.payload_json = Some(json!({"key":name,"value":value}));
                records.push(r)
            }
        }
        Ok(records)
    }

    /// Atomically replace one asset's current VisualDNA projection.
    pub fn index_visual_dna(
        &self,
        project_id: i64,
        item_id: i64,
        dna: &Value,
    ) -> Result<Vec<String>> {
        let source = dna_source(item_id)?;
        let records = Self::dna_projection(project_id, item_id, dna)?;
        self.sync_one_projection(project_id, &source, records)
    }

    fn dna_projection(project_id: i64, item_id: i64, dna: &Value) -> Result<Vec<MemoryRecord>> {
        require_object(dna)?;
        require_optional_object(dna, "character_core")?;
        let mut records = vec![];
        for (key, value, label, memory_type) in [
            (
                "visual_dna",
                &dna["character_core"]["visual_dna"],
                "角色视觉 DNA",
                "character_design",
            ),
            (
                "attire",
                &dna["character_core"]["attire"],
                "角色服装",
                "character_design",
            ),
            (
                "technical_specs",
                &dna["technical_specs"],
                "技术参数",
                "style_guide",
            ),
            (
                "stable_diffusion_tags",
                &dna["stable_diffusion_tags"],
                "Stable Diffusion Tags",
                "style_guide",
            ),
        ] {
            if value.is_null()
                || value.as_object().is_some_and(|v| v.is_empty())
                || value.as_str().is_some_and(|v| v.trim().is_empty())
            {
                continue;
            }
            let mut r = MemoryRecord::new(
                project_id,
                "static_bible",
                memory_type,
                &format!("{label}: {}", value_text(value)),
            );
            r.entity = Some(format!("item_{item_id}"));
            let mut payload = json!({"item_id":item_id});
            payload[key] = value.clone();
            r.payload_json = Some(payload);
            records.push(r);
        }
        Ok(records)
    }

    /// Synchronize a complete, authoritative project snapshot without a model.
    /// Missing DNA sources and an empty Bible retire their old projections.
    /// Returns the number of current source-owned slices, excluding manual facts.
    pub fn sync_current_settings(
        &self,
        project_id: i64,
        bible: &Value,
        visual_dna: &[(i64, Value)],
    ) -> Result<usize> {
        let mut sources = BTreeMap::from([(
            BIBLE_SOURCE.to_owned(),
            Self::bible_projection(project_id, bible)?,
        )]);
        // Validate the entire snapshot before starting any database changes.
        for (item_id, dna) in visual_dna {
            let source = dna_source(*item_id)?;
            let records = Self::dna_projection(project_id, *item_id, dna)?;
            if let Some(previous) = sources.get(&source) {
                if previous.iter().map(projection_key).collect::<Vec<_>>()
                    != records.iter().map(projection_key).collect::<Vec<_>>()
                {
                    return Err(invalid("同一素材包含互相矛盾的 VisualDNA 来源"));
                }
            } else {
                sources.insert(source, records);
            }
        }
        let tx = rusqlite::Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let mut statement = tx.prepare(
            "SELECT DISTINCT source_ref FROM memory_records WHERE project_id=? AND source_ref IS NOT NULL",
        )?;
        let existing = statement
            .query_map([project_id], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        for source in existing {
            if source == BIBLE_SOURCE || dna_source_item(&source).is_some() {
                sources.entry(source).or_default();
            }
        }
        let mut count = 0;
        for (source, records) in sources {
            count += self
                .sync_projection_source(&tx, project_id, &source, records)?
                .len();
        }
        tx.commit()?;
        Ok(count)
    }

    fn sync_one_projection(
        &self,
        project_id: i64,
        source: &str,
        records: Vec<MemoryRecord>,
    ) -> Result<Vec<String>> {
        let tx = rusqlite::Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let ids = self.sync_projection_source(&tx, project_id, source, records)?;
        tx.commit()?;
        Ok(ids)
    }

    fn sync_projection_source(
        &self,
        conn: &Connection,
        project_id: i64,
        source: &str,
        records: Vec<MemoryRecord>,
    ) -> Result<Vec<String>> {
        let mut statement = conn.prepare(
            "SELECT id FROM memory_records WHERE project_id=? AND source_ref=? ORDER BY id",
        )?;
        let existing = statement
            .query_map(params![project_id, source], |row| row.get::<_, String>(0))?
            .map(|id| {
                self.get_record(project_id, &id?)?
                    .ok_or_else(|| invalid("设定投影记录不存在"))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|record| is_projection(record, source))
            .collect::<Vec<_>>();
        drop(statement);
        let mut by_content = BTreeMap::new();
        for record in &existing {
            by_content.entry(projection_key(record)).or_insert(record);
        }
        let mut current = BTreeSet::new();
        let mut ids = vec![];
        for mut record in records {
            let key = projection_key(&record);
            let previous = by_content.get(&key).copied();
            let id = previous.map(|r| r.id.clone()).unwrap_or_else(|| {
                let identity = json!(["setting-projection-v1", project_id, source, key]);
                format!(
                    "projection::{:x}",
                    Sha256::digest(identity.to_string().as_bytes())
                )
            });
            if !current.insert(id.clone()) {
                continue;
            }
            if self
                .get_record(project_id, &id)?
                .is_some_and(|previous| !is_projection(&previous, source))
            {
                return Err(invalid("设定投影 ID 已被非来源记录使用"));
            }
            record.id = id.clone();
            record.source_ref = Some(source.into());
            record.source_kind = PROJECTION.into();
            // Source identity is part of this projection's key. The generic
            // content deduplicator intentionally has different legacy semantics.
            if previous.is_none_or(|old| {
                old.status != record.status
                    || old.source_kind != record.source_kind
                    || old.confidence != record.confidence
                    || old.evidence_ids != record.evidence_ids
                    || old.time_index != record.time_index
                    || old.story_order != record.story_order
                    || old.story_time != record.story_time
            }) {
                write_record(conn, &record, false)?;
            }
            ids.push(id);
        }
        for record in existing {
            if !current.contains(&record.id) {
                if record.status != "retconned" {
                    conn.execute(
                        "UPDATE memory_records SET status='retconned',updated_at_ms=? WHERE project_id=? AND id=?",
                        params![now_ms(), project_id, record.id],
                    )?;
                }
                conn.execute(
                    "DELETE FROM rust_memory_vectors WHERE record_id=?",
                    [&record.id],
                )?;
            }
        }
        Ok(ids)
    }
}

fn require_object(value: &Value) -> Result<()> {
    if value.is_object() {
        Ok(())
    } else {
        Err(invalid("当前设定必须为 JSON 对象，未撤销现有投影"))
    }
}

fn require_optional_object(value: &Value, key: &str) -> Result<()> {
    if value[key].is_null() || value[key].is_object() {
        Ok(())
    } else {
        Err(invalid(format!("{key} 必须为 JSON 对象")))
    }
}

fn projection_key(record: &MemoryRecord) -> String {
    json!([
        record.namespace,
        record.memory_type,
        record.entity,
        record.content,
        record.payload_json
    ])
    .to_string()
}

fn is_projection(record: &MemoryRecord, source: &str) -> bool {
    if record.source_ref.as_deref() != Some(source)
        || !["system", PROJECTION].contains(&record.source_kind.as_str())
    {
        return false;
    }
    let payload = record.payload_json.as_ref().unwrap_or(&Value::Null);
    if source == BIBLE_SOURCE {
        match (record.namespace.as_str(), record.memory_type.as_str()) {
            ("static_bible", "world_rule") => {
                (payload.get("key").is_some() && payload.get("value").is_some())
                    || (payload.get("term").is_some() && payload.get("definition").is_some())
            }
            ("static_bible", "character_design") => {
                payload.get("character").is_some() && payload.get("visual_dna_ref").is_some()
            }
            ("static_bible", "character_fact") => {
                payload.get("character").is_some() && payload.get("facts").is_some()
            }
            ("world_rules_negative", "negative_constraint") => {
                payload.get("constraint").is_some()
                    || (payload.get("key").is_some() && payload.get("value").is_some())
            }
            _ => false,
        }
    } else if let Some(item_id) = dna_source_item(source) {
        record.namespace == "static_bible"
            && ["character_design", "style_guide"].contains(&record.memory_type.as_str())
            && record.entity.as_deref() == Some(format!("item_{item_id}").as_str())
            && payload["item_id"].as_i64() == Some(item_id)
            && [
                "visual_dna",
                "attire",
                "technical_specs",
                "stable_diffusion_tags",
            ]
            .iter()
            .any(|key| payload.get(key).is_some())
    } else {
        false
    }
}

fn dna_source(item_id: i64) -> Result<String> {
    if item_id <= 0 {
        return Err(invalid("VisualDNA 素材 ID 必须为正数"));
    }
    Ok(format!("visual_dna.asset_item_{item_id}.v1"))
}

fn dna_source_item(source: &str) -> Option<i64> {
    let item = source
        .strip_prefix("visual_dna.asset_item_")?
        .strip_suffix(".v1")?;
    let item_id: i64 = item.parse().ok()?;
    (item_id > 0 && item_id.to_string() == item).then_some(item_id)
}

fn value_text(v: &Value) -> String {
    v.as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| v.to_string())
}
