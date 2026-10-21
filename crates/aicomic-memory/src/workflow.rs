use crate::store::write_record;
use crate::*;
use rusqlite::{Connection, params};
use serde_json::json;
use std::collections::BTreeMap;
impl MemoryStore {
    pub fn write_episodic(
        &self,
        project_id: i64,
        change: &Value,
        source_ref: Option<&str>,
    ) -> Result<String> {
        let tx = self.conn.unchecked_transaction()?;
        let id = episodic(&tx, project_id, change, source_ref)?;
        tx.commit()?;
        Ok(id)
    }
    pub fn extract_structured_output(
        &self,
        project_id: i64,
        data: &Value,
        episode_id: Option<i64>,
        scene_id: Option<i64>,
        source_ref: Option<&str>,
    ) -> Result<Vec<String>> {
        let tx = self.conn.unchecked_transaction()?;
        let mut out = vec![];
        if let Some(beats) = data["beat_sheet"].as_array() {
            for (index, beat) in beats.iter().enumerate() {
                let event = beat["description"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or_else(|| beat["title"].as_str())
                    .unwrap_or("");
                if event.is_empty() {
                    continue;
                }
                out.push(episodic(&tx,project_id,&json!({"event":event,"state_changes":{"beat_type":beat["beat_type"],"emotional_charge":beat["emotional_charge"],"visual_focus":beat["visual_focus"]},"entities":[],"episode_id":episode_id,"scene_id":scene_id,"beat_index":index}),source_ref)?);
            }
            if !beats.is_empty() {
                let titles = beats
                    .iter()
                    .take(5)
                    .map(|b| {
                        b["title"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .or_else(|| b["description"].as_str())
                            .unwrap_or("")
                            .chars()
                            .take(30)
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut r = MemoryRecord::new(
                    project_id,
                    "dynamic_plot",
                    "event",
                    &format!("章节摘要：包含 {} 个节拍。主要节拍：{titles}", beats.len()),
                );
                r.payload_json = Some(
                    json!({"beat_count":beats.len(),"beat_sheet":beats.iter().take(10).collect::<Vec<_>>()}),
                );
                r.source_ref = source_ref.map(str::to_owned);
                out.push(write_record(&tx, &r, true)?);
            }
        }
        if let Some(shots) = data["shots"].as_array() {
            for (index, shot) in shots.iter().enumerate() {
                let text = shot["action_text"].as_str().unwrap_or("");
                let mut changes = json!({});
                for (key, keywords) in [
                    ("prop_acquired", vec!["获得", "拿到", "捡起", "拾取"]),
                    ("prop_lost", vec!["失去", "丢失", "丢弃", "扔掉"]),
                    ("injured", vec!["受伤", "中弹", "被击", "流血"]),
                    ("character_entered", vec!["进入", "来到", "出现", "登场"]),
                    ("character_exited", vec!["离开", "退出", "消失", "退场"]),
                ] {
                    if keywords.iter().any(|keyword| text.contains(keyword)) {
                        changes[key] = json!(true)
                    }
                }
                if changes.as_object().is_some_and(|o| !o.is_empty()) {
                    out.push(episodic(&tx,project_id,&json!({"event":format!("镜头 {}: {text}",index+1),"state_changes":changes,"entities":[],"episode_id":episode_id,"scene_id":scene_id,"beat_index":index}),source_ref)?);
                }
            }
        }
        tx.commit()?;
        Ok(out)
    }
    pub fn extract_qc_report(
        &self,
        project_id: i64,
        data: &Value,
        episode_id: Option<i64>,
        scene_id: Option<i64>,
        source_ref: Option<&str>,
    ) -> Result<Vec<String>> {
        let tx = self.conn.unchecked_transaction()?;
        let mut out = vec![];
        if let Some(issues) = data["issues"].as_array() {
            for issue in issues {
                if !issue.is_object() {
                    continue;
                }
                out.push(episodic(&tx,project_id,&json!({"event":format!("QC 问题: {} - {}",issue["type"].as_str().unwrap_or("unknown"),issue["message"].as_str().unwrap_or("")),"state_changes":{"issue_type":issue["type"],"location":issue["location"]},"entities":[],"episode_id":episode_id,"scene_id":scene_id}),source_ref)?);
            }
        }
        if data["revised_script_fountain"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
        {
            out.push(episodic(&tx,project_id,&json!({"event":"QC 修订完成","state_changes":{"has_revision":true},"entities":[],"episode_id":episode_id,"scene_id":scene_id}),source_ref)?);
        }
        tx.commit()?;
        Ok(out)
    }
    pub fn retrieve_for_task(
        &self,
        project_id: i64,
        task: &str,
        entity: Option<&str>,
        provider: Option<&dyn EmbeddingProvider>,
    ) -> Result<TaskMemoryContext> {
        let has = |keywords: &[&str]| keywords.iter().any(|keyword| task.contains(keyword));
        let plot = has(&["之前", "历史", "伏笔"]);
        let rules = has(&["规则", "禁忌", "不允许", "不能"]);
        let visual = has(&["外观", "视觉", "画面", "角色设计"]);
        let general = !plot && !rules && !visual;
        let mut layers = BTreeMap::<String, MemoryRetrievalResult>::new();
        let mut retrieve = |layer: &str,
                            namespace: &str,
                            query: Option<&str>,
                            entity: Option<&str>,
                            status: Option<&str>,
                            limit|
         -> Result<()> {
            let mut q = MemoryQuery::new(project_id);
            q.namespace = Some(namespace.into());
            q.entity = entity.map(str::to_owned);
            q.status = status.map(str::to_owned);
            q.top_k = limit;
            let result = if let (Some(text), Some(provider)) = (query, provider) {
                q.query_text = Some(text.into());
                let result = self.retrieve_semantic(&q, provider)?;
                self.mmr(result, provider)?
            } else {
                let records = self.list_records(&q)?;
                let total = records.len();
                MemoryRetrievalResult {
                    scores: vec![1.; total],
                    records,
                    total,
                }
            };
            layers.insert(layer.into(), result);
            Ok(())
        };
        if plot || entity.is_some() {
            retrieve(
                "L1",
                "episodic",
                if plot || general { Some(task) } else { None },
                entity,
                None,
                10,
            )?
        }
        if rules || visual {
            retrieve(
                "L2_static",
                "static_bible",
                Some(task),
                entity,
                Some("confirmed"),
                5,
            )?
        }
        if plot || general {
            retrieve("L2_dynamic", "dynamic_plot", Some(task), entity, None, 10)?
        }
        retrieve(
            "negative_constraints",
            "world_rules_negative",
            None,
            None,
            Some("confirmed"),
            20,
        )?;
        let all = layers
            .values()
            .flat_map(|r| r.records.iter().cloned())
            .collect::<Vec<_>>();
        let conflicts = Self::detect_conflicts(&all);
        let prompt_fragments = layers
            .iter()
            .filter(|(_, r)| !r.records.is_empty())
            .map(|(layer, r)| {
                (
                    layer.clone(),
                    r.records
                        .iter()
                        .map(|r| format!("- {}", r.content))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            })
            .collect();
        Ok(TaskMemoryContext {
            retrieval_mode: if provider.is_some() {
                "semantic".into()
            } else {
                "structured".into()
            },
            layers,
            conflicts,
            prompt_fragments,
        })
    }
    /// Matches the legacy potential-conflict heuristic; these are review hints.
    pub fn detect_conflicts(records: &[MemoryRecord]) -> Vec<Value> {
        let mut groups = BTreeMap::<(String, String), Vec<&MemoryRecord>>::new();
        for record in records {
            if let Some(entity) = record.entity.as_ref().filter(|e| !e.is_empty()) {
                groups
                    .entry((entity.clone(), record.memory_type.clone()))
                    .or_default()
                    .push(record);
            }
        }
        let priority = |r: &MemoryRecord| match r.namespace.as_str() {
            "static_bible" => 3,
            "episodic" => 2,
            "dynamic_plot" => 1,
            _ => 0,
        };
        let mut out = vec![];
        for ((entity, kind), mut group) in groups {
            group.sort_by_key(|r| std::cmp::Reverse(priority(r)));
            let mut contents = group.iter().map(|r| r.content.as_str()).collect::<Vec<_>>();
            contents.sort_unstable();
            contents.dedup();
            if contents.len() > 1 {
                out.push(json!({"entity":entity,"type":kind,"records":group.iter().map(|r|&r.id).collect::<Vec<_>>(),"contents":group.iter().map(|r|&r.content).collect::<Vec<_>>(),"priority":group.iter().map(|r|priority(r)).collect::<Vec<_>>()}));
            }
        }
        out
    }
    fn mmr(
        &self,
        result: MemoryRetrievalResult,
        provider: &dyn EmbeddingProvider,
    ) -> Result<MemoryRetrievalResult> {
        if result.records.len() < 2 {
            return Ok(result);
        }
        let mut vectors = vec![];
        for record in &result.records {
            let bytes: Vec<u8> = self.conn.query_row(
                "SELECT vector FROM rust_memory_vectors WHERE record_id=? AND provider_key=?",
                params![record.id, provider.identifier()],
                |r| r.get(0),
            )?;
            vectors.push(
                bytes
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect::<Vec<_>>(),
            );
        }
        let mut selected = vec![0];
        let mut remaining = (1..result.records.len()).collect::<Vec<_>>();
        while !remaining.is_empty() {
            let best = remaining
                .iter()
                .enumerate()
                .map(|(position, &i)| {
                    let max = selected
                        .iter()
                        .map(|&j| {
                            vectors[i]
                                .iter()
                                .zip(&vectors[j])
                                .map(|(a, b)| a * b)
                                .sum::<f32>()
                        })
                        .fold(f32::NEG_INFINITY, f32::max);
                    (position, 0.5 * result.scores[i] + 0.5 * (1. - max))
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|v| v.0)
                .unwrap_or(0);
            selected.push(remaining.remove(best));
        }
        Ok(MemoryRetrievalResult {
            records: selected
                .iter()
                .map(|&i| result.records[i].clone())
                .collect(),
            scores: selected.iter().map(|&i| result.scores[i]).collect(),
            total: result.total,
        })
    }
}
fn episodic(
    conn: &Connection,
    project_id: i64,
    change: &Value,
    source_ref: Option<&str>,
) -> Result<String> {
    let event = change["event"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid("Episodic event 不能为空"))?;
    let changes = change
        .get("state_changes")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !changes.is_object() {
        return Err(invalid("state_changes 必须为对象"));
    }
    let entities: Vec<String> =
        serde_json::from_value(change.get("entities").cloned().unwrap_or_else(|| json!([])))?;
    let mut r = MemoryRecord::new(
        project_id,
        "episodic",
        "event",
        &format!("{event}. 状态变更: {changes}"),
    );
    r.entity = if entities.is_empty() {
        None
    } else {
        Some(entities.join(","))
    };
    let mut time = vec![];
    for (key, prefix) in [
        ("episode_id", "ep"),
        ("scene_id", "sc"),
        ("beat_index", "bt"),
    ] {
        if let Some(n) = change[key].as_i64() {
            time.push(format!("{prefix}{n}"))
        }
    }
    r.time_index = Some(time.join("_"));
    r.payload_json = Some(change.clone());
    r.source_ref = source_ref.map(str::to_owned);
    r.created_at_ms = change["created_at_ms"].as_i64();
    let id = write_record(conn, &r, true)?;
    conn.execute("INSERT INTO episodic_memories(id,project_id,event,state_changes_json,entities_json,episode_id,scene_id,beat_index,created_at_ms) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET event=excluded.event,state_changes_json=excluded.state_changes_json,entities_json=excluded.entities_json,episode_id=excluded.episode_id,scene_id=excluded.scene_id,beat_index=excluded.beat_index",params![id,project_id,event,changes.to_string(),serde_json::to_string(&entities)?,change["episode_id"].as_i64(),change["scene_id"].as_i64(),change["beat_index"].as_i64(),r.created_at_ms.unwrap_or_else(now_ms)])?;
    Ok(id)
}
