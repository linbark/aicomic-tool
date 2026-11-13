//! Closed, validated DAG and JSON contract subset; no remote schema resolution.
use anyhow::{Context, Result, bail, ensure};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn components(definition: &Value) -> Result<Value> {
    let items = definition["components"]
        .as_array()
        .context("配方 components 必须为数组")?;
    ensure!(
        !items.is_empty() && items.len() <= 128,
        "配方需要 1..128 个结构化组件"
    );
    ensure!(
        definition["constraints"].is_object() || definition["constraints"].is_array(),
        "配方 constraints 必须为结构化对象或数组"
    );
    let mut seen = BTreeSet::new();
    let mut total = 0.0;
    for item in items {
        let input = item["input"].as_str().context("组件缺少 input 节点 ID")?;
        ensure!(
            !input.is_empty() && seen.insert(input),
            "配方组件 input 为空或重复"
        );
        let ratio = item["ratio"].as_f64().context("配比必须为有限数字")?;
        ensure!(ratio.is_finite() && ratio >= 0.0, "配比必须为非负有限数字");
        total += ratio;
    }
    ensure!(
        total.is_finite() && total > 0.0,
        "配比总和必须为有限正数；相对权重将显式归一化"
    );
    Ok(json!(
        items
            .iter()
            .map(|item| {
                let mut item = item.clone();
                item["effective_ratio"] = json!(item["ratio"].as_f64().unwrap() / total);
                item
            })
            .collect::<Vec<_>>()
    ))
}

pub(super) fn resolve(conn: &Connection, raw: &Value) -> Result<Vec<Value>> {
    let input = raw.as_array().context("丹方 nodes 必须为数组")?;
    ensure!(
        !input.is_empty() && input.len() <= 64,
        "DAG 需要 1..64 个节点"
    );
    let mut nodes = BTreeMap::new();
    for node in input {
        let id = node["id"].as_str().context("节点缺少 id")?;
        ensure!(
            !id.is_empty()
                && id.len() <= 128
                && id.chars().all(|c| c.is_alphanumeric() || "-_.".contains(c)),
            "节点 id 仅接受文字、数字和 -_."
        );
        let op = node["op"].as_str().context("节点缺少 op")?;
        ensure!(
            ["source", "technique", "recipe", "export"].contains(&op),
            "未知节点 op"
        );
        let refs = node["inputs"]
            .as_array()
            .context("节点 inputs 必须为数组")?;
        let mut seen = BTreeSet::new();
        for r in refs {
            ensure!(
                r.as_str().is_some_and(|s| seen.insert(s)),
                "节点 inputs 必须是唯一的节点 ID 字符串"
            );
        }
        ensure!(
            (op == "source" && refs.is_empty()) || (op != "source" && !refs.is_empty()),
            "source 不接受上游；其他节点必须有输入"
        );
        let mut value = json!({"id":id,"op":op,"inputs":refs});
        if ["technique", "recipe"].contains(&op) {
            let asset = &node["asset"];
            ensure!(asset["kind"] == op, "节点资产 kind 必须与 op 相同");
            let key = asset["key"].as_str().context("节点资产缺少 key")?;
            let version = asset["version"]
                .as_u64()
                .filter(|v| *v > 0 && *v <= u32::MAX as u64)
                .context("节点必须绑定明确的正整数版本")? as u32;
            let revision = super::revision(conn, op, key, version)?;
            schema(&revision["output_schema"], 0)?;
            value["asset"] = json!({"kind":op,"key":key,"version":version});
            if op == "recipe" {
                let normalized = components(&revision["definition"])?;
                let components: BTreeSet<_> = normalized
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v["input"].as_str().unwrap())
                    .collect();
                ensure!(
                    components == seen,
                    "配方组件 input 必须与该节点的全部 DAG 上游一一匹配"
                );
                value["components"] = normalized;
            }
            value["revision"] = revision;
        } else {
            ensure!(
                node.get("asset").is_none_or(Value::is_null),
                "source/export 不允许绑定模型资产"
            );
        }
        ensure!(
            nodes.insert(id.to_owned(), value).is_none(),
            "DAG 存在重复节点"
        );
    }
    ensure!(
        nodes.values().any(|n| n["op"] == "source") && nodes.values().any(|n| n["op"] == "export"),
        "DAG 必须包含真实 source 和 export"
    );
    for node in nodes.values() {
        for r in node["inputs"].as_array().unwrap() {
            ensure!(nodes.contains_key(r.as_str().unwrap()), "DAG 存在悬空输入");
        }
    }
    let mut ordered = Vec::new();
    let mut complete = BTreeSet::new();
    while ordered.len() < nodes.len() {
        let next = input.iter().find_map(|original| {
            let id = original["id"].as_str().unwrap();
            let n = &nodes[id];
            if !complete.contains(id)
                && n["inputs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|i| complete.contains(i.as_str().unwrap()))
            {
                Some(id)
            } else {
                None
            }
        });
        let id = next.context("DAG 存在循环；任何 HTTP 前拒绝")?;
        complete.insert(id.to_owned());
        ordered.push(nodes[id].clone());
    }
    Ok(ordered)
}

pub(super) fn schema(value: &Value, depth: usize) -> Result<()> {
    ensure!(depth <= 32, "输出契约嵌套过深");
    let map = value
        .as_object()
        .context("输出契约必须为 JSON Schema 对象")?;
    for key in map.keys() {
        ensure!(
            [
                "type",
                "properties",
                "required",
                "items",
                "enum",
                "const",
                "additionalProperties",
                "minItems",
                "maxItems",
                "minLength",
                "maxLength",
                "minimum",
                "maximum",
                "description",
                "title"
            ]
            .contains(&key.as_str()),
            "输出契约含不支持的关键字；仅支持本地 type/properties/required/items/enum/const/additionalProperties 和长度/数值边界，禁止远程 $ref"
        );
    }
    if let Some(t) = map.get("type") {
        ensure!(
            t.as_str().is_some_and(|s| [
                "object", "array", "string", "number", "integer", "boolean", "null"
            ]
            .contains(&s)),
            "契约 type 不受支持"
        );
    }
    if let Some(props) = map.get("properties") {
        for child in props.as_object().context("properties 必须为对象")?.values() {
            schema(child, depth + 1)?;
        }
    }
    if let Some(required) = map.get("required") {
        let mut seen = BTreeSet::new();
        for key in required.as_array().context("required 必须为数组")? {
            ensure!(
                key.as_str().is_some_and(|k| seen.insert(k)),
                "required 必须含唯一字符串"
            );
        }
    }
    if let Some(items) = map.get("items") {
        schema(items, depth + 1)?;
    }
    if let Some(extra) = map.get("additionalProperties")
        && !extra.is_boolean()
    {
        schema(extra, depth + 1)?;
    }
    if let Some(en) = map.get("enum") {
        ensure!(
            en.as_array().is_some_and(|v| !v.is_empty()),
            "enum 必须为非空数组"
        );
    }
    for key in ["minItems", "maxItems", "minLength", "maxLength"] {
        if let Some(v) = map.get(key) {
            ensure!(v.as_u64().is_some(), "契约长度边界必须为非负整数");
        }
    }
    for key in ["minimum", "maximum"] {
        if let Some(v) = map.get(key) {
            ensure!(
                v.as_f64().is_some_and(f64::is_finite),
                "契约数值边界必须为有限数"
            );
        }
    }
    Ok(())
}

pub(super) fn validate(value: &Value, contract: &Value, depth: usize) -> Result<()> {
    ensure!(depth <= 32, "模型输出嵌套过深");
    let valid = match contract["type"].as_str() {
        None => true,
        Some("object") => value.is_object(),
        Some("array") => value.is_array(),
        Some("string") => value.is_string(),
        Some("number") => value.is_number(),
        Some("integer") => value.as_f64().is_some_and(|n| n.fract() == 0.0),
        Some("boolean") => value.is_boolean(),
        Some("null") => value.is_null(),
        _ => false,
    };
    ensure!(valid, "模型输出违反 type 契约");
    if let Some(items) = contract["enum"].as_array() {
        ensure!(items.contains(value), "模型输出不在 enum 契约内");
    }
    if let Some(expected) = contract.get("const") {
        ensure!(value == expected, "模型输出违反 const 契约");
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = contract["required"].as_array() {
            for key in required {
                ensure!(
                    object.contains_key(key.as_str().unwrap()),
                    "模型输出缺少 required 字段"
                );
            }
        }
        for (key, item) in object {
            if let Some(child) = contract["properties"].get(key) {
                validate(item, child, depth + 1)?;
            } else if contract["additionalProperties"] == false {
                bail!("模型输出含契约禁止的额外字段");
            } else if contract["additionalProperties"].is_object() {
                validate(item, &contract["additionalProperties"], depth + 1)?;
            }
        }
    }
    if let Some(array) = value.as_array() {
        bounds(array.len() as u64, contract, "minItems", "maxItems")?;
        if contract["items"].is_object() {
            for item in array {
                validate(item, &contract["items"], depth + 1)?;
            }
        }
    }
    if let Some(text) = value.as_str() {
        bounds(
            text.chars().count() as u64,
            contract,
            "minLength",
            "maxLength",
        )?;
    }
    if let Some(number) = value.as_f64() {
        ensure!(
            contract["minimum"].as_f64().is_none_or(|n| number >= n)
                && contract["maximum"].as_f64().is_none_or(|n| number <= n),
            "模型输出违反数值边界"
        );
    }
    Ok(())
}
fn bounds(n: u64, c: &Value, min: &str, max: &str) -> Result<()> {
    ensure!(
        c[min].as_u64().is_none_or(|m| n >= m) && c[max].as_u64().is_none_or(|m| n <= m),
        "模型输出违反长度契约"
    );
    Ok(())
}
