//! 从丹药到产物：出简报（brief）、验产物（verify）。
//!
//! 边界很清楚：引擎不生成 HTML，它只交付"可执行规格 + 验收清单"；
//! 产物由编码 agent 写，写完回来验收。这样规格才是真正的接口。

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;

pub(crate) fn frozen_snapshot(out: &Value) -> Result<bool> {
    let Some(version) = out.get("snapshot_version") else {
        return Ok(false);
    };
    if version != 1 || !out["recipe_definition"].is_object() {
        bail!("不支持或不完整的丹药快照，不能声称历史可复现");
    }
    let components = out["components"]
        .as_array()
        .context("丹药快照缺 components")?;
    for c in components {
        use sha2::{Digest, Sha256};
        let content = c.get("content").context("丹药快照缺药液内容")?;
        let hash = format!("{:x}", Sha256::digest(content.to_string().as_bytes()));
        if c["content_hash"].as_str() != Some(hash.as_str())
            || c["hash"].as_str().is_none()
            || c["extract_revision"].as_i64().unwrap_or(0) < 1
            || c.get("technique_version").is_none()
        {
            bail!("丹药快照药液内容或版本/hash不完整，不能声称历史可复现");
        }
    }
    Ok(true)
}

fn provenance(frozen: bool, legacy: bool) -> Value {
    json!({"source": if frozen { "pill_snapshot" } else if legacy { "legacy_live_extract" } else { "missing_snapshot" },
        "historically_reproducible": frozen,
        "note":if frozen { "使用冻结快照" } else { "历史不可复原；legacy fallback 仅代表当前数据，非历史证据" }})
}

fn component_content(
    conn: &Connection,
    c: &Value,
    frozen: bool,
    legacy: bool,
) -> Result<Option<Value>> {
    if frozen {
        return Ok(Some(c["content"].clone()));
    }
    if !legacy {
        return Ok(None);
    }
    let content: Option<String> = conn
        .query_row(
            "SELECT content_json FROM extract WHERE id=?1",
            params![c["extract_id"].as_i64().unwrap_or(0)],
            |r| r.get(0),
        )
        .optional()?;
    content
        .map(|s| serde_json::from_str(&s).context("legacy 药液不是合法 JSON"))
        .transpose()
}

pub fn brief(db: &str, pill_id: i64, out_dir: &Path) -> Result<()> {
    let conn = Connection::open(db)?;
    let (recipe_key, output_json, output_md, score_json): (String, String, String, String) = conn
        .query_row(
            "SELECT coalesce(r.key,'-'), p.output_json, p.output_md, p.score_json
             FROM pill p LEFT JOIN recipe r ON r.id=p.recipe_id WHERE p.id=?1",
            params![pill_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .with_context(|| format!("没有丹药 #{pill_id}"))?;
    let out: Value = serde_json::from_str(&output_json).unwrap_or_default();
    let frozen = frozen_snapshot(&out)?;
    let legacy = !frozen && out["legacy_provenance_fallback"] == true;
    let provenance = provenance(frozen, legacy);
    let recipe_key = out["recipe_definition"]["key"]
        .as_str()
        .or_else(|| out["recipe"].as_str())
        .unwrap_or(&recipe_key);
    let score: Value = serde_json::from_str(&score_json).unwrap_or_default();
    let prompt = out["prompt"].as_str().unwrap_or("");
    let comps = out["components"].as_array().cloned().unwrap_or_default();

    // 从药液内容里抽可验收的 token
    let mut colors: BTreeSet<String> = BTreeSet::new();
    let mut checks: Vec<Value> = Vec::new();
    let mut tokens_extra = serde_json::Map::new();
    let mut contrast_added = false;
    for c in &comps {
        let dim = c["dimension"].as_str().unwrap_or("");
        if let Some(v) = component_content(&conn, c, frozen, legacy)? {
            if dim == "palette" {
                if let Some(roles) = v["roles"].as_object() {
                    for (role, hex) in roles {
                        if let Some(h) = hex.as_str() {
                            colors.insert(h.to_string());
                            tokens_extra.insert(format!("color.{role}"), serde_json::json!(h));
                        }
                    }
                    if let Some(c) = roles.get("text_contrast").and_then(|x| x.as_f64()) {
                        checks.push(json!({"id":"contrast","rule": format!("正文对比 ≥4.5:1（药液实测 {c:.2}:1）")}));
                        contrast_added = true;
                    }
                } else {
                    for col in v["colors"].as_array().cloned().unwrap_or_default() {
                        if let Some(hex) = col["hex"].as_str() {
                            colors.insert(hex.to_string());
                        }
                    }
                }
                if !contrast_added && let Some(min) = v["stats"]["contrast"].as_f64() {
                    checks.push(json!({"id":"contrast","rule":format!("正文对比度 ≥ 4.5:1（药液实测 {min:.2}:1）")}));
                }
            }
            if dim == "type" {
                if let Some(steps) = v["steps"].as_array() {
                    let list: Vec<String> = steps
                        .iter()
                        .filter_map(|s| {
                            Some(format!(
                                "{} {}px/{}",
                                s["name"].as_str().unwrap_or("?"),
                                s["size_px"].as_f64()?,
                                s["weight"].as_i64().unwrap_or(400)
                            ))
                        })
                        .collect();
                    checks.push(
                        json!({"id":"type_scale","rule": format!("字号层级：{}", list.join("、"))}),
                    );
                    tokens_extra.insert("type.steps".into(), v["steps"].clone());
                } else {
                    checks.push(json!({"id":"type_scale","rule": format!("字号层级：{}", v["scale"].as_str().unwrap_or("按药液"))}));
                }
            }
            if dim == "spacing" {
                checks.push(json!({"id":"spacing","rule": format!(
                    "间距模数 {}px，档位 {}，页边 {}px，段距 {}px",
                    v["base_px"].as_f64().unwrap_or(8.0),
                    v["steps"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| format!("{x:.0}")).collect::<Vec<_>>().join("、"))
                        .unwrap_or_default(),
                    v["page_margin_px"].as_f64().unwrap_or(0.0),
                    v["section_gap_px"].as_f64().unwrap_or(0.0)
                )}));
                tokens_extra.insert("spacing".into(), v.clone());
            }
            if dim == "donts" {
                for d in v["donts"].as_array().cloned().unwrap_or_default() {
                    let text = d["text"].as_str().or_else(|| d.as_str()).unwrap_or("");
                    let ctx = d["applies_to"].as_str().unwrap_or("any");
                    if !text.is_empty() {
                        checks.push(
                            json!({"id":"forbidden","rule": format!("禁止（{ctx}）：{text}")}),
                        );
                    }
                }
            }
        }
    }
    checks.push(json!({"id":"palette","rule": format!("必须出现药液色板：{}", colors.iter().cloned().collect::<Vec<_>>().join("、"))}));
    checks.push(json!({"id":"no_shadow","rule":"页面不得使用 box-shadow（药液禁忌）"}));

    std::fs::create_dir_all(out_dir)?;
    let brief = format!(
        "# 生成简报 · 丹药 #{pill_id}\n\n> 配方 {recipe_key}｜预检 {:.2}\n\n> 来源：{}\n\n## 合成规格\n\n{}\n\n## 验收清单\n\n{}\n\n## 原始丹药\n\n{}\n",
        score["total"].as_f64().unwrap_or(0.0),
        provenance["note"].as_str().unwrap_or("历史不可复原"),
        prompt,
        checks
            .iter()
            .map(|c| format!(
                "- [ ] {}：{}",
                c["id"].as_str().unwrap_or(""),
                c["rule"].as_str().unwrap_or("")
            ))
            .collect::<Vec<_>>()
            .join("\n"),
        output_md
    );
    std::fs::write(out_dir.join("brief.md"), &brief)?;
    let mut tokens = json!({"pill": pill_id, "provenance":provenance,
        "colors": colors.iter().cloned().collect::<Vec<_>>(), "checks": checks});
    for (k, v) in &tokens_extra {
        tokens[k] = v.clone();
    }
    std::fs::write(
        out_dir.join("tokens.json"),
        serde_json::to_string_pretty(&tokens)?,
    )?;
    println!("简报已写出：{}", out_dir.join("brief.md").display());
    println!("验收项 {} 条，色板 {} 个", checks.len(), colors.len());
    Ok(())
}

/// Pure, structured static checks bound to a frozen pill; never a visual score.
pub(crate) fn static_report(snapshot: &Value, source: &str) -> Result<Value> {
    anyhow::ensure!(
        frozen_snapshot(snapshot)?,
        "静态报告需要完整冻结规格，旧文本不能作为历史依据"
    );
    let mut palette = Vec::new();
    for component in snapshot["components"]
        .as_array()
        .context("快照缺 components")?
    {
        if component["dimension"] == "palette" {
            for color in component["content"]["colors"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if let Some(hex) = color["hex"].as_str() {
                    palette.push(hex.to_uppercase());
                }
            }
        }
    }
    Ok(analyze_source(source, &palette, true, false))
}

fn analyze_source(src: &str, palette: &[String], frozen: bool, legacy: bool) -> Value {
    let upper = src.to_uppercase();
    let used_palette: Vec<String> = palette
        .iter()
        .filter(|h| upper.contains(h.as_str()))
        .cloned()
        .collect();
    let palette_cov = if palette.is_empty() {
        0.0
    } else {
        used_palette.len() as f64 / palette.len() as f64
    };

    let shadows = upper.matches("BOX-SHADOW").count();
    let sizes: Vec<f64> = regex_sizes(src);
    let (min_s, max_s) = sizes
        .iter()
        .fold((f64::MAX, 0.0f64), |(mn, mx), v| (mn.min(*v), mx.max(*v)));
    let ratio = if sizes.is_empty() || min_s <= 0.0 {
        0.0
    } else {
        max_s / min_s
    };

    // 正文对比度：解析 :root 变量 + body 的 background/color
    let vars = css_vars(src);
    let block = css_block(src, "body").unwrap_or_default();
    let resolve = |decl: &str, key: &str| -> Option<String> {
        let idx = decl.find(key)?;
        let rest = &decl[idx + key.len()..];
        let value: String = rest
            .chars()
            .take_while(|c| *c != ';' && *c != '}')
            .collect();
        let value = value.trim();
        if let Some(name) = value.strip_prefix("var(") {
            let name = name.trim_end_matches(')').trim();
            return vars.get(name).cloned();
        }
        value
            .find('#')
            .and_then(|h| value.get(h..h + 7))
            .map(|s| s.to_string())
    };
    let pick =
        |names: &[&str]| -> Option<String> { names.iter().find_map(|n| vars.get(*n).cloned()) };
    let body_bg = resolve(&block, "background")
        .or_else(|| pick(&["--bg", "--background", "--paper", "--surface"]))
        .or_else(|| vars.values().next().cloned());
    let body_fg = resolve(&block, "color")
        .or_else(|| pick(&["--text", "--ink", "--fg", "--foreground"]))
        .or_else(|| {
            let bg = vars.get("--bg").cloned();
            vars.iter()
                .find(|(k, v)| Some(*v) != bg.as_ref().map(|_| *v) && **k != "--bg")
                .map(|(_, v)| v.clone())
        });
    let contrast = match (body_bg, body_fg) {
        (Some(bg), Some(fg)) => contrast_ratio(&bg, &fg),
        _ => 0.0,
    };

    let mut checks: Vec<Value> = Vec::new();
    checks.push(
        json!({"id":"provenance","ok":frozen,"provenance":provenance(frozen, legacy),
        "detail":if frozen { "冻结的丹药快照" } else { "缺少快照：历史不可复原" }}),
    );
    checks.push(json!({"id":"palette","ok": palette_cov >= 0.6, "detail": format!("用到 {:.0}%（{}/{}）", palette_cov*100.0, used_palette.len(), palette.len())}));
    checks.push(json!({"id":"no_shadow","ok": shadows == 0, "detail": format!("box-shadow 出现 {shadows} 次")}));
    checks.push(json!({"id":"contrast","ok": contrast >= 4.5, "detail": format!("正文对比 {contrast:.2}:1")}));
    checks.push(json!({"id":"type_scale","ok": ratio >= 2.0, "detail": format!("最大/最小字号 {ratio:.2}×")}));

    let passed = checks.iter().all(|c| c["ok"].as_bool().unwrap_or(false));
    json!({"passed":passed,"checks":checks,"evidence_kind":"static_check","visual_quality":false,
        "basis":"html_css_source_heuristic",
        "measurements":{"shadow_count":shadows,"palette_coverage":palette_cov,"body_contrast":contrast,"type_ratio":ratio},
        "limitations":["源码启发式不证明浏览器计算样式、交互或视觉美感"]})
}

pub fn verify(db: &str, pill_id: i64, html: &Path) -> Result<()> {
    let src =
        std::fs::read_to_string(html).with_context(|| format!("读不到 {}", html.display()))?;
    let conn = Connection::open(db)?;
    let (output_json,): (String,) = conn.query_row(
        "SELECT output_json FROM pill WHERE id=?1",
        params![pill_id],
        |r| Ok((r.get(0)?,)),
    )?;
    let out: Value =
        serde_json::from_str(&output_json).context("丹药 output_json 不是合法 JSON")?;
    let frozen = frozen_snapshot(&out)?;
    let legacy = !frozen && out["legacy_provenance_fallback"] == true;
    let comps = out["components"].as_array().cloned().unwrap_or_default();

    // 药液色板
    let mut palette: Vec<String> = Vec::new();
    let mut donts: Vec<String> = Vec::new();
    for c in &comps {
        let dim = c["dimension"].as_str().unwrap_or("");
        if let Some(v) = component_content(&conn, c, frozen, legacy)? {
            if dim == "palette" {
                for col in v["colors"].as_array().cloned().unwrap_or_default() {
                    if let Some(h) = col["hex"].as_str() {
                        palette.push(h.to_uppercase());
                    }
                }
            }
            if dim == "donts" {
                for d in v["donts"].as_array().cloned().unwrap_or_default() {
                    if let Some(s) = d.as_str() {
                        donts.push(s.to_string());
                    }
                }
            }
        }
    }

    let report = analyze_source(&src, &palette, frozen, legacy);
    let checks = report["checks"].as_array().context("静态报告缺 checks")?;
    let passed = report["passed"].as_bool().unwrap_or(false);
    let shadows = report["measurements"]["shadow_count"].as_u64().unwrap_or(0);
    println!("验收丹药 #{pill_id} ← {}", html.display());
    for c in checks {
        println!(
            "  [{}] {:<12} {}",
            if c["ok"].as_bool().unwrap_or(false) {
                "通过"
            } else {
                "未过"
            },
            c["id"].as_str().unwrap_or(""),
            c["detail"].as_str().unwrap_or("")
        );
    }
    println!("结论：{}", if passed { "通过" } else { "未通过" });

    conn.execute(
        "INSERT INTO pill_check(pill_id,artifact,checks_json,passed,checked_at) VALUES(?1,?2,?3,?4,?5)",
        params![pill_id, html.display().to_string(), json!(checks).to_string(), passed as i64, crate::now_iso()],
    )?;
    if !donts.is_empty() && shadows > 0 {
        println!(
            "提示：药液禁忌里有 {} 条，其中「加投影」被违反",
            donts.len()
        );
    }
    Ok(())
}

fn regex_sizes(src: &str) -> Vec<f64> {
    // 粗糙但够用：font-size: 12px / 1.5rem
    let mut out = Vec::new();
    let bytes: Vec<char> = src.chars().collect();
    let needle: Vec<char> = "font-size:".chars().collect();
    let mut i = 0;
    while i + needle.len() < bytes.len() {
        if bytes[i..i + needle.len()] == needle[..] {
            let rest: String = bytes[i + needle.len()..(i + needle.len() + 24).min(bytes.len())]
                .iter()
                .collect();
            let num: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(v) = num.parse::<f64>() {
                if rest.contains("rem") {
                    out.push(v * 16.0);
                } else {
                    out.push(v);
                }
            }
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

fn css_vars(src: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i + 4 < chars.len() {
        if chars[i] == '-' && chars[i + 1] == '-' {
            let name: String = chars[i..]
                .iter()
                .take_while(|c| **c != ':' && **c != ';' && **c != '}')
                .collect();
            if let Some(colon) = chars[i..].iter().position(|c| *c == ':') {
                let start = i + colon + 1;
                let hex: String = chars[start..(start + 40).min(chars.len())]
                    .iter()
                    .collect::<String>();
                if let Some(h) = hex.find('#')
                    && let Some(c) = hex.get(h..h + 7)
                    && c.chars().skip(1).all(|ch| ch.is_ascii_hexdigit())
                {
                    out.insert(name.trim().to_string(), c.to_string());
                }
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

fn css_block(src: &str, selector: &str) -> Option<String> {
    let lower = src.to_lowercase();
    let idx = lower.find(&format!("{selector}{{"))?;
    let rest = &src[idx..];
    let end = rest.find('}')?;
    Some(rest[..end].to_string())
}

#[allow(dead_code)]
fn first_color_after<'a>(src: &'a str, key: &str) -> Option<&'a str> {
    let idx = src.to_lowercase().find(&key.to_lowercase())?;
    let rest = &src[idx..];
    let hash = rest.find('#')?;
    let s = &rest[hash..];
    let end = s[1..]
        .find(|c: char| !c.is_ascii_hexdigit())
        .map(|e| e + 1)?;
    if end >= 7 { Some(&s[..7]) } else { None }
}

fn contrast_ratio(bg: &str, fg: &str) -> f64 {
    fn lum(hex: &str) -> f64 {
        let v = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0) as f64 / 255.0;
        let f = |c: f64| {
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(v(1)) + 0.7152 * f(v(3)) + 0.0722 * f(v(5))
    }
    let (a, b) = (lum(bg), lum(fg));
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alchemy::tests::Fixture;

    #[test]
    fn structured_static_report_is_bound_to_frozen_evidence_not_a_visual_score() {
        let f = Fixture::new();
        f.refine(false);
        let (recipe, _) = f.recipe();
        let (_, snapshot) = f.brew(&recipe);
        let source = "body{background:#FFFFFF;color:#000000}h1{font-size:32px}p{font-size:16px}";
        let report = static_report(&snapshot, source).unwrap();
        assert_eq!(report["passed"], true);
        assert_eq!(report["evidence_kind"], "static_check");
        assert_eq!(report["visual_quality"], false);
        assert_eq!(report["checks"].as_array().unwrap().len(), 5);
        assert!(static_report(&json!({"prompt":"old text spec"}), source).is_err());
    }

    #[test]
    fn legacy_pill_requires_explicit_fallback_and_never_claims_historical_reproduction() {
        let f = Fixture::new();
        f.refine(false);
        let mut legacy = json!({"recipe":"old.recipe", "prompt":"old prompt", "components":[
            {"dimension":"palette","extract_id":f.extracts()[0].0}
        ]});
        f.conn()
            .execute(
                "INSERT INTO pill(id,output_md,output_json,created_at) VALUES(1,'legacy',?1,'now')",
                params![legacy.to_string()],
            )
            .unwrap();
        let out = f.root.join("legacy");
        brief(&f.db, 1, &out).unwrap();
        let tokens: Value =
            serde_json::from_str(&std::fs::read_to_string(out.join("tokens.json")).unwrap())
                .unwrap();
        assert!(
            tokens["colors"].as_array().unwrap().is_empty(),
            "default must not consult live legacy extracts"
        );
        assert_eq!(tokens["provenance"]["historically_reproducible"], false);
        assert!(
            std::fs::read_to_string(out.join("brief.md"))
                .unwrap()
                .contains("历史不可复原")
        );

        // Explicit opt-in in the persisted pill, not a silent inference from an extract ID.
        legacy["legacy_provenance_fallback"] = json!(true);
        f.conn()
            .execute(
                "UPDATE pill SET output_json=?1 WHERE id=1",
                params![legacy.to_string()],
            )
            .unwrap();
        brief(&f.db, 1, &out).unwrap();
        let tokens: Value =
            serde_json::from_str(&std::fs::read_to_string(out.join("tokens.json")).unwrap())
                .unwrap();
        assert!(!tokens["colors"].as_array().unwrap().is_empty());
        assert_eq!(tokens["provenance"]["source"], "legacy_live_extract");
        assert_eq!(tokens["provenance"]["historically_reproducible"], false);
        let artifact = f.root.join("legacy.html");
        std::fs::write(
            &artifact,
            "body{background:#FFFFFF;color:#000000}h1{font-size:32px}p{font-size:16px}",
        )
        .unwrap();
        verify(&f.db, 1, &artifact).unwrap();
        let (checks, passed): (String, bool) = f
            .conn()
            .query_row(
                "SELECT checks_json,passed FROM pill_check ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(
            !passed,
            "a current-data fallback cannot certify historical reproduction"
        );
        let checks: Vec<Value> = serde_json::from_str(&checks).unwrap();
        assert!(
            checks
                .iter()
                .any(|c| c["id"] == "palette" && c["ok"] == true)
        );
        assert!(
            checks
                .iter()
                .any(|c| c["id"] == "provenance" && c["ok"] == false)
        );
    }

    #[test]
    fn verify_uses_frozen_palette_without_reading_live_extracts() {
        let f = Fixture::new();
        f.refine(false);
        let (recipe, _) = f.recipe();
        let (pill, snapshot) = f.brew(&recipe);
        let restored = Fixture::new();
        restored.conn().execute(
            "INSERT INTO pill(id,output_md,output_json,created_at) VALUES(?1,'frozen',?2,'now')",
            params![pill, snapshot.to_string()],
        ).unwrap();
        let artifact = restored.root.join("artifact.html");
        std::fs::write(
            &artifact,
            "body{background:#FFFFFF;color:#000000}h1{font-size:32px}p{font-size:16px}",
        )
        .unwrap();
        verify(&restored.db, pill, &artifact).unwrap();
        let (checks, passed): (String, bool) = restored
            .conn()
            .query_row(
                "SELECT checks_json,passed FROM pill_check ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(
            passed,
            "verification must use the pill palette, not a missing live extract: {checks}"
        );
        let checks: Vec<Value> = serde_json::from_str(&checks).unwrap();
        assert!(
            checks
                .iter()
                .any(|c| c["id"] == "provenance" && c["ok"] == true)
        );
    }

    #[test]
    fn brief_uses_self_contained_pill_snapshot_after_refresh_and_without_extracts() {
        let f = Fixture::new();
        f.refine(false);
        let (recipe, _) = f.recipe();
        let (pill, snapshot) = f.brew(&recipe);
        let before = f.root.join("before");
        brief(&f.db, pill, &before).unwrap();
        f.image([255, 200, 0]);
        f.refine(true);
        let after = f.root.join("after");
        brief(&f.db, pill, &after).unwrap();
        assert_eq!(
            std::fs::read(before.join("tokens.json")).unwrap(),
            std::fs::read(after.join("tokens.json")).unwrap()
        );

        // Import only the pill into an independent DB: snapshots must not need live extracts/recipes.
        let restored = Fixture::new();
        restored.conn().execute(
            "INSERT INTO pill(id,output_md,output_json,created_at) VALUES(?1,'frozen markdown',?2,'now')",
            params![pill, snapshot.to_string()],
        ).unwrap();
        let out = restored.root.join("brief");
        brief(&restored.db, pill, &out).unwrap();
        let tokens: Value =
            serde_json::from_str(&std::fs::read_to_string(out.join("tokens.json")).unwrap())
                .unwrap();
        assert!(
            !tokens["colors"].as_array().unwrap().is_empty(),
            "frozen palette is independent of extract table"
        );
        assert_eq!(tokens["provenance"]["source"], "pill_snapshot");
        assert_eq!(tokens["provenance"]["historically_reproducible"], true);
        let markdown = std::fs::read_to_string(out.join("brief.md")).unwrap();
        assert!(
            markdown.contains("test.recipe"),
            "recipe identity comes from snapshot too"
        );
    }
}
