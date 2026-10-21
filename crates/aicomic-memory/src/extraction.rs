use crate::*;
use serde_json::json;
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};
fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").trim().to_owned()
}
fn array(v: &Value, key: &str) -> Vec<Value> {
    v[key].as_array().cloned().unwrap_or_default()
}
fn references(v: &Value, valid: &HashSet<&str>) -> Vec<String> {
    array(v, "evidence_ids")
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|s| valid.contains(*s))
        .map(str::to_owned)
        .collect()
}
fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut underscore = false;
    for c in s.chars() {
        if c.is_alphanumeric() || c == '_' {
            out.push(c);
            underscore = false;
        } else if !underscore {
            out.push('_');
            underscore = true;
        }
    }
    let value = out.trim_matches('_').chars().take(60).collect::<String>();
    if value.is_empty() {
        "unnamed".into()
    } else {
        value
    }
}
fn metadata(v: &Value, project_id: i64) -> Value {
    json!({"project_id":project_id,"status":v["status"].as_str().unwrap_or("confirmed"),"confidence":v["confidence"].as_f64().unwrap_or(0.8),"source_kind":v["source_kind"].as_str().unwrap_or("extracted")})
}
fn with_fields(mut base: Value, extra: Value) -> Value {
    if let (Some(base), Some(extra)) = (base.as_object_mut(), extra.as_object()) {
        base.extend(extra.clone());
    }
    base
}
/// Normalize an AI extraction before human review, using only the supplied evidence IDs.
/// It never fabricates semantic facts or rewrites the original evidence quotes.
pub fn normalize_extracted_changeset(
    project_id: i64,
    episode_id: Option<i64>,
    story_order_base: &str,
    evidence_ids: &[String],
    payload: &Value,
) -> Result<Value> {
    if !payload.is_object() {
        return Err(invalid("抽取结果必须为 changeset.v0 JSON 对象"));
    }
    let valid = evidence_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut out = payload.clone();
    out["schema_version"] = json!("changeset.v0");
    out["project_id"] = json!(project_id);
    out["episode_id"] = json!(episode_id);
    out["story_order_base"] = json!(story_order_base);
    if !out["materialize"].is_object() {
        out["materialize"] = json!({})
    }
    if out["materialize"].get("write_static_bible").is_none() {
        out["materialize"]["write_static_bible"] = json!(true)
    }
    // Input evidence already exists in SQLite; a model cannot replace the source text.
    out["evidences"] = json!([]);
    let mut conflicts = array(payload, "conflicts");
    let mut entities = vec![];
    let mut by_name = HashMap::new();
    for e in array(payload, "entities") {
        let kind = text(&e, "entity_type");
        let name = text(&e, "canonical_name");
        if name.is_empty()
            || !["character", "location", "organization", "prop"].contains(&kind.as_str())
        {
            continue;
        }
        let id = match text(&e, "entity_id") {
            id if !id.is_empty() => id,
            _ => format!(
                "{kind}:{}:{}",
                slug(&name),
                &format!(
                    "{:x}",
                    Sha1::digest(format!("{project_id}:{kind}:{name}").as_bytes())
                )[..10]
            ),
        };
        let aliases = array(&e, "aliases")
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .take(50)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let mut from = text(&e, "created_from_evidence_id");
        if !from.is_empty() && !valid.contains(from.as_str()) {
            conflicts.push(json!({"conflict_type":"invalid_evidence_ref","entity_id":id,"old_claim":{"created_from_evidence_id":from},"new_claim":{"created_from_evidence_id":null},"suggested_actions":[{"action":"remove_invalid_evidence_ref"}]}));
            from.clear();
        }
        by_name.insert(name.clone(), id.clone());
        entities.push(with_fields(metadata(&e,project_id),json!({"entity_id":id,"entity_type":kind,"canonical_name":name,"aliases":aliases,"created_from_evidence_id":if from.is_empty(){None}else{Some(from)}})));
    }
    out["entities"] = json!(entities);
    let mut snapshots = vec![];
    for s in array(payload, "snapshots") {
        let mut entity_id = text(&s, "entity_id");
        if entity_id.is_empty() {
            entity_id = by_name
                .get(&text(&s, "canonical_name"))
                .cloned()
                .unwrap_or_default()
        }
        let evidence = references(&s, &valid);
        if entity_id.is_empty() || evidence.is_empty() {
            continue;
        }
        let id = match text(&s, "snapshot_id") {
            id if !id.is_empty() => id,
            _ => format!("snap:{}:{}", slug(&entity_id), new_id()),
        };
        let mut fields = json!({});
        for key in [
            "visual",
            "personality",
            "goals",
            "relations",
            "items",
            "injuries",
        ] {
            fields[key] = s["fields"].get(key).cloned().unwrap_or_else(|| {
                if ["visual", "personality"].contains(&key) {
                    json!({})
                } else {
                    json!([])
                }
            })
        }
        let order = match text(&s, "valid_from_story_order") {
            order if !order.is_empty() => order,
            _ => format!("{story_order_base}.E0001"),
        };
        snapshots.push(with_fields(metadata(&s,project_id),json!({"snapshot_id":id,"entity_id":entity_id,"valid_from_story_order":order,"valid_to_story_order":s["valid_to_story_order"],"valid_from_story_time_key":s["valid_from_story_time_key"],"valid_to_story_time_key":s["valid_to_story_time_key"],"fields":fields,"why":s["why"],"evidence_ids":evidence})));
    }
    out["snapshots"] = json!(snapshots);
    let mut events = vec![];
    let mut event_ids = HashSet::new();
    for (index, event) in array(payload, "events").iter().enumerate() {
        let summary = text(event, "summary");
        let evidence = references(event, &valid);
        if summary.is_empty() || evidence.is_empty() {
            continue;
        }
        let id = match text(event, "event_id") {
            id if !id.is_empty() => id,
            _ => format!("ev:{}:{}", slug(story_order_base), new_id()),
        };
        let order = match text(event, "story_order") {
            id if !id.is_empty() => id,
            _ => format!("{story_order_base}.EV{:04}", index + 1),
        };
        let participants = array(event, "participants")
            .iter()
            .filter_map(Value::as_str)
            .map(|s| by_name.get(s).cloned().unwrap_or_else(|| s.to_owned()))
            .take(50)
            .collect::<Vec<_>>();
        event_ids.insert(id.clone());
        events.push(with_fields(metadata(event,project_id),json!({"event_id":id,"story_order":order,"episode_id":episode_id,"scene_id":event["scene_id"],"event_type":event["event_type"],"summary":summary,"participants":participants,"evidence_ids":evidence,"story_time":event["story_time"],"story_time_key":event["story_time_key"]})));
    }
    out["events"] = json!(events);
    let mut changes = vec![];
    for change in array(payload, "state_changes") {
        let event_id = text(&change, "event_id");
        let evidence = references(&change, &valid);
        let patch = change
            .get("patch_json")
            .or_else(|| change.get("patch"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !event_ids.contains(&event_id)
            || evidence.is_empty()
            || !patch.is_object()
            || patch.as_object().is_none_or(|v| v.is_empty())
        {
            continue;
        }
        let id = match text(&change, "state_change_id") {
            id if !id.is_empty() => id,
            _ => format!("sc:{}:{}", slug(story_order_base), new_id()),
        };
        let mut target = text(&change, "target_entity_id");
        if target.is_empty() {
            target = by_name
                .get(&text(&change, "target_canonical_name"))
                .cloned()
                .unwrap_or_default()
        }
        changes.push(with_fields(metadata(&change,project_id),json!({"state_change_id":id,"event_id":event_id,"target_entity_id":if target.is_empty(){None}else{Some(target)},"patch_json":patch,"before_snapshot_id":change["before_snapshot_id"],"after_snapshot_id":change["after_snapshot_id"],"evidence_ids":evidence})));
    }
    out["state_changes"] = json!(changes);
    let mut constraints = vec![];
    for mut constraint in array(payload, "time_constraints") {
        let evidence = references(&constraint, &valid);
        if text(&constraint, "relation").is_empty() || evidence.is_empty() {
            continue;
        }
        constraint["project_id"] = json!(project_id);
        constraint["evidence_ids"] = json!(evidence);
        if constraint.get("status").is_none() {
            constraint["status"] = json!("hypothesis")
        }
        if constraint.get("confidence").is_none() {
            constraint["confidence"] = json!(0.6)
        }
        if constraint.get("source_kind").is_none() {
            constraint["source_kind"] = json!("extracted")
        }
        constraints.push(constraint);
    }
    out["time_constraints"] = json!(constraints);
    let blocks = array(payload, "time_blocks")
        .into_iter()
        .filter(Value::is_object)
        .map(|mut block| {
            block["project_id"] = json!(project_id);
            block
        })
        .collect::<Vec<_>>();
    out["time_blocks"] = json!(blocks);
    out["conflicts"] = json!(conflicts);
    Ok(out)
}
