use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub schema: Option<i64>,
    pub demo: bool,
    pub asset_root_hint: Option<PathBuf>,
    pub needs_migration: bool,
    pub works: Vec<Value>,
    pub extracts: Vec<Value>,
    pub recipes: Vec<Value>,
    pub pills: Vec<Value>,
    pub experiments: Vec<Value>,
    pub people: Vec<Value>,
    pub warnings: Vec<String>,
}
impl Snapshot {
    pub fn from_value(value: Value) -> Self {
        let array = |key: &str| value[key].as_array().cloned().unwrap_or_default();
        Self {
            demo: value["demo"].as_bool().unwrap_or(false),
            asset_root_hint: path(&value, "asset_root_hint").filter(|p| p.is_absolute()),
            schema: value["schema_version"].as_i64(),
            needs_migration: value["needs_migration"].as_bool().unwrap_or(false),
            works: array("works"),
            extracts: array("extracts"),
            recipes: array("recipes"),
            pills: array("pills"),
            experiments: array("experiments"),
            people: array("people"),
            warnings: array("warnings")
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        }
    }
    pub fn work(&self, selected: Option<i64>) -> Option<&Value> {
        self.works.iter().find(|v| Some(id(v)) == selected)
    }
    pub fn pill(&self, selected: Option<i64>) -> Option<&Value> {
        self.pills.iter().find(|v| Some(id(v)) == selected)
    }
    pub fn experiment(&self, selected: Option<i64>) -> Option<&Value> {
        self.experiments.iter().find(|v| Some(id(v)) == selected)
    }
    pub fn filtered_experiments(&self, query: &str) -> Vec<&Value> {
        self.experiments
            .iter()
            .filter(|e| matches(e, query, &["content_summary", "manifest_hash"]))
            .collect()
    }
    /// One canonical visible object for the comparison, inspector and record action.
    pub fn visible_experiment(&self, selected: Option<i64>, query: &str) -> Option<&Value> {
        let visible = self.filtered_experiments(query);
        visible
            .iter()
            .copied()
            .find(|e| Some(id(e)) == selected)
            .or_else(|| visible.first().copied())
    }
    pub fn extracts_for(&self, work: i64) -> Vec<&Value> {
        self.extracts
            .iter()
            .filter(|v| v["work_id"].as_i64() == Some(work))
            .collect()
    }
    pub fn filtered_works(&self, query: &str) -> Vec<&Value> {
        self.works
            .iter()
            .filter(|v| matches(v, query, &["title", "domain", "collection"]))
            .collect()
    }
    pub fn filtered_pills(&self, query: &str) -> Vec<&Value> {
        self.pills
            .iter()
            .filter(|v| matches(v, query, &["name", "recipe", "created_at"]))
            .collect()
    }
}
pub fn id(value: &Value) -> i64 {
    value["id"].as_i64().unwrap_or_default()
}
pub fn label<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}
pub fn title(value: &Value) -> String {
    let title = label(value, "title");
    if title.is_empty() {
        format!("未命名素材 · {}", id(value))
    } else {
        title.to_string()
    }
}
pub fn path(value: &Value, key: &str) -> Option<PathBuf> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}
pub fn unavailable_preview(work: &Value) -> &'static str {
    match label(work, "preview_status") {
        "no_local_asset" => "本条尚无本地图像",
        "file_missing" => "图片文件不在当前素材目录，可选择素材目录",
        "unreadable_or_unsupported" => "图片无法读取或格式不支持",
        _ => "无本地预览图",
    }
}
pub fn flag(value: &Value, key: &str) -> bool {
    value[key].as_bool().unwrap_or(false)
}
pub fn pretty(value: &Value) -> String {
    if value.is_null() {
        "暂无内容".into()
    } else if let Some(s) = value.as_str() {
        s.to_owned()
    } else {
        serde_json::to_string_pretty(value).unwrap_or_else(|_| "无法展示内容".into())
    }
}
pub fn short(value: &Value, max: usize) -> String {
    let s = pretty(value).replace('\n', " ");
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max).collect::<String>())
    } else {
        s
    }
}
pub fn matches(value: &Value, query: &str, fields: &[&str]) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || fields
            .iter()
            .any(|field| label(value, field).to_lowercase().contains(&query))
        || id(value).to_string() == query
}
pub fn dimension(value: &Value) -> &str {
    match label(value, "dimension") {
        "palette" => "配色",
        "spacing" => "留白与间距",
        other => other,
    }
}
/// Extract only actual hex colors from the persisted extract; never invent swatches.
pub fn palette(value: &Value) -> Vec<(iced::Color, String)> {
    fn visit(value: &Value, out: &mut Vec<(iced::Color, String)>) {
        match value {
            Value::String(s) if s.starts_with('#') && s.len() == 7 => {
                if let Ok(rgb) = u32::from_str_radix(&s[1..], 16) {
                    let color =
                        iced::Color::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
                    if !out.iter().any(|(_, old)| old.eq_ignore_ascii_case(s)) {
                        out.push((color, s.clone()));
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    visit(item, out);
                }
            }
            Value::Object(fields) => {
                for item in fields.values() {
                    visit(item, out);
                }
            }
            _ => {}
        }
    }
    let mut colors = Vec::new();
    visit(value, &mut colors);
    colors.truncate(8);
    colors
}

pub fn pill_title(pill: &Value) -> String {
    let name = label(pill, "name");
    format!(
        "{} · #{}",
        if name.is_empty() { "丹药" } else { name },
        id(pill)
    )
}
fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}
pub fn percent(value: &Value) -> Option<String> {
    value
        .as_f64()
        .filter(|v| v.is_finite())
        .map(|v| format!("{:.0}%", v * 100.))
}
pub fn component_origin(component: &Value) -> String {
    let extract = component["extract_id"]
        .as_i64()
        .map(|n| format!("药液 #{n}"))
        .unwrap_or_else(|| "药液 ID 未记录".into());
    let revision = component["extract_revision"]
        .as_i64()
        .map(|n| format!("修订 r{n}"))
        .unwrap_or_else(|| "修订未记录".into());
    let version = scalar(&component["technique_version"])
        .map(|v| format!("技术版本 {v}"))
        .unwrap_or_else(|| "技术版本未记录".into());
    format!("{extract} · {revision} · {version}")
}
/// Values come exclusively from the saved component, never a current extract lookup.
pub fn spacing_rows(content: &Value) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    for (key, title) in [
        ("base_px", "间距模数"),
        ("page_margin_px", "页边距"),
        ("section_gap_px", "分区间距"),
    ] {
        if let Some(value) = scalar(&content[key]) {
            rows.push((title.into(), format!("{value} px")));
        }
    }
    if let Some(steps) = content["steps"].as_array() {
        let steps: Vec<_> = steps.iter().filter_map(scalar).collect();
        if !steps.is_empty() {
            rows.push(("间距阶梯".into(), format!("{} px", steps.join(" / "))));
        }
    }
    rows
}
pub fn recipe_rows(pill: &Value) -> Vec<(String, String)> {
    let definition = &pill["output"]["recipe_definition"];
    let mut rows = Vec::new();
    for (key, title) in [
        ("name", "配方"),
        ("key", "配方标识"),
        ("version", "配方版本"),
        ("goal", "目标"),
    ] {
        if let Some(value) = scalar(&definition[key]) {
            rows.push((title.into(), value));
        }
    }
    if rows.is_empty() {
        rows.push((
            "配方".into(),
            if label(pill, "recipe").is_empty() {
                "未记录".into()
            } else {
                label(pill, "recipe").into()
            },
        ));
    }
    rows
}
/// This is the backend's structural preflight, not a visual or aesthetic score.
pub fn preflight_rows(pill: &Value) -> Vec<(String, String)> {
    let score = &pill["score"];
    let mut rows = Vec::new();
    if let Some(total) = score["total"].as_f64().filter(|v| v.is_finite()) {
        rows.push(("结构预检".into(), format!("{:.1} / 100", total * 100.)));
    }
    for (key, title) in [("errors", "错误"), ("warns", "警告")] {
        if let Some(n) = score[key].as_i64() {
            rows.push((title.into(), n.to_string()));
        }
    }
    if let Some(completeness) = percent(&score["completeness"]) {
        rows.push(("必需维度完整度".into(), completeness));
    }
    if let Some(value) = score["ratio_ok"].as_f64().filter(|v| v.is_finite()) {
        rows.push((
            "配比校验".into(),
            if value >= 0.999 {
                "通过".into()
            } else {
                format!("需核对 · {:.0}%", value * 100.)
            },
        ));
    }
    if let Some(components) = pill["output"]["components"].as_array() {
        let ratios: Option<Vec<f64>> = components
            .iter()
            .map(|c| c["ratio"].as_f64().filter(|v| v.is_finite()))
            .collect();
        if let Some(ratios) = ratios.filter(|v| !v.is_empty()) {
            rows.push((
                "实际配比合计".into(),
                format!("{:.0}%", ratios.iter().sum::<f64>() * 100.),
            ));
        }
    }
    if let Some(value) = score["prompt_ok"].as_f64().filter(|v| v.is_finite()) {
        rows.push((
            "规格文字检查".into(),
            if value >= 0.999 {
                "通过".into()
            } else {
                "需核对".into()
            },
        ));
    }
    rows
}
pub fn preflight_issues(pill: &Value) -> Vec<String> {
    let mut issues = Vec::new();
    for key in ["conflicts", "spec_issues"] {
        if let Some(items) = pill["output"][key].as_array() {
            for issue in items {
                if let Some(message) = issue.as_str() {
                    issues.push(message.into());
                    continue;
                }
                let severity = match label(issue, "severity") {
                    "error" => "错误",
                    "warn" => "警告",
                    _ => "提示",
                };
                let detail = ["detail", "message", "rule"]
                    .iter()
                    .find_map(|key| issue[*key].as_str())
                    .unwrap_or("查看原始冻结数据了解详情");
                issues.push(format!("{severity} · {detail}"));
            }
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tolerant_snapshot_and_search() {
        let s = Snapshot::from_value(
            serde_json::json!({"works":[{"id":8,"title":"静谧书店","domain":"editorial"}]}),
        );
        assert_eq!(s.filtered_works("书店").len(), 1);
        assert_eq!(s.filtered_works("EDITORIAL").len(), 1);
        assert_eq!(s.filtered_works("8").len(), 1);
        assert!(!s.needs_migration);
    }
    #[test]
    fn unavailable_preview_explains_backend_status_without_guessing_legacy_records() {
        for (status, expected) in [
            ("no_local_asset", "本条尚无本地图像"),
            ("file_missing", "图片文件不在当前素材目录，可选择素材目录"),
            ("unreadable_or_unsupported", "图片无法读取或格式不支持"),
            ("unknown", "无本地预览图"),
            ("available", "无本地预览图"),
        ] {
            assert_eq!(
                unavailable_preview(&serde_json::json!({"preview_status": status})),
                expected
            );
        }
        assert_eq!(unavailable_preview(&serde_json::json!({})), "无本地预览图");
    }
    #[test]
    fn visible_experiment_falls_back_only_within_search_results() {
        let s = Snapshot::from_value(
            serde_json::json!({"experiments":[{"id":1,"content_summary":"dense"},{"id":2,"content_summary":"quiet","manifest_hash":"HASH2"}]}),
        );
        assert_eq!(id(s.visible_experiment(Some(1), "quiet").unwrap()), 2);
        assert_eq!(id(s.visible_experiment(Some(2), "").unwrap()), 2);
        assert_eq!(id(s.visible_experiment(None, "hash2").unwrap()), 2);
        assert!(s.visible_experiment(Some(1), "missing").is_none());
        assert_eq!(id(s.visible_experiment(None, "").unwrap()), 1);
    }
    #[test]
    fn pill_ids_disambiguate_same_name_and_timestamp() {
        let a = serde_json::json!({"id":1,"name":"同一配方","created_at":"same"});
        let b = serde_json::json!({"id":2,"name":"同一配方","created_at":"same"});
        assert_ne!(pill_title(&a), pill_title(&b));
        assert!(pill_title(&a).contains("#1"));
        assert!(pill_title(&b).contains("#2"));
    }
    #[test]
    fn summary_preserves_saved_values_without_legacy_invention() {
        let component = serde_json::json!({"extract_id":41,"extract_revision":2,"technique_version":"old-v3","ratio":0.6});
        assert_eq!(percent(&component["ratio"]).as_deref(), Some("60%"));
        assert!(component_origin(&component).contains("药液 #41 · 修订 r2 · 技术版本 old-v3"));
        assert!(spacing_rows(&Value::Null).is_empty());
        assert!(component_origin(&serde_json::json!({})).contains("版本未记录"));
        let legacy = serde_json::json!({"recipe":"legacy","output":{"components":[{}]},"score":{}});
        assert!(preflight_rows(&legacy).is_empty());
        assert!(
            !recipe_rows(&legacy)
                .iter()
                .any(|(key, _)| key.contains("版本"))
        );
        assert!(percent(&Value::Null).is_none());
    }
    #[test]
    fn summary_uses_actual_spacing_and_structural_checks() {
        let rows = spacing_rows(
            &serde_json::json!({"base_px":8,"page_margin_px":24,"section_gap_px":32,"steps":[8,16,24]}),
        );
        assert!(rows.contains(&("页边距".into(), "24 px".into())));
        let pill = serde_json::json!({"score":{"total":0.867,"errors":1,"warns":2,"completeness":0.5,"ratio_ok":1.0,"prompt_ok":0.0},"output":{"components":[{"ratio":0.6},{"ratio":0.4}],"conflicts":[{"severity":"error","detail":"缺少必需维度"}]}});
        let checks = preflight_rows(&pill);
        assert!(checks.contains(&("结构预检".into(), "86.7 / 100".into())));
        assert!(checks.contains(&("必需维度完整度".into(), "50%".into())));
        assert!(checks.contains(&("实际配比合计".into(), "100%".into())));
        assert_eq!(preflight_issues(&pill), vec!["错误 · 缺少必需维度"]);
    }
    #[test]
    fn root_hint_only_accepts_absolute_paths() {
        assert!(
            Snapshot::from_value(serde_json::json!({"asset_root_hint":"relative"}))
                .asset_root_hint
                .is_none()
        );
        assert_eq!(
            Snapshot::from_value(serde_json::json!({"asset_root_hint":"/assets"})).asset_root_hint,
            Some(PathBuf::from("/assets"))
        );
    }
    #[test]
    fn palette_uses_only_stored_colors() {
        assert!(palette(&serde_json::json!({"description":"amber"})).is_empty());
        assert_eq!(
            palette(&serde_json::json!(["#AABBCC", "#AABBCC", "#wrong"])).len(),
            1
        );
    }
}
