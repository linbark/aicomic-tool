use crate::*;
use serde_json::json;
impl MemoryStore {
    /// The JSON settings remain the source; repeated indexing deduplicates records.
    pub fn index_series_bible(&self, project_id: i64, bible: &Value) -> Result<Vec<String>> {
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
        let mut ids = vec![];
        for mut r in records {
            r.source_ref = Some("series_bible.v1".into());
            ids.push(self.write(&r, true)?)
        }
        Ok(ids)
    }
    pub fn index_visual_dna(
        &self,
        project_id: i64,
        item_id: i64,
        dna: &Value,
    ) -> Result<Vec<String>> {
        let mut out = vec![];
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
            r.source_ref = Some(format!("visual_dna.asset_item_{item_id}.v1"));
            out.push(self.write(&r, true)?)
        }
        Ok(out)
    }
}
fn value_text(v: &Value) -> String {
    v.as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| v.to_string())
}
