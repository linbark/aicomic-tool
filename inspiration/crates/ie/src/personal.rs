//! Local-only personal preferences, bridged exclusively from verified final screenshots.
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use std::time::Duration;

pub const SCOPE: &str = "ui-static";
pub const FEATURE_CONTRACT: &str = "pixel-personal-v1";
pub const TERMS: [&str; 2] = ["negative-space.active", "detail.density"];

pub struct InitOpts<'a> {
    pub db: &'a str,
    pub person: &'a str,
}
pub struct RecordOpts<'a> {
    pub visual: crate::experiment::RecordOpts<'a>,
    pub person: &'a str,
    pub intent: &'a str,
}
pub struct BridgeOpts<'a> {
    pub db: &'a str,
    pub evaluation_id: i64,
    pub person: &'a str,
    pub intent: &'a str,
}
pub fn init(opts: &InitOpts<'_>) -> Result<Value> {
    if opts.person.is_empty() || opts.person.trim() != opts.person {
        bail!("person 必须为非空且无首尾空格的本机标识");
    }
    let mut conn = connect(opts.db, false)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let old: Option<(String, String)> = tx
        .query_row(
            "SELECT privacy_level,consent_json FROM person WHERE id=?1",
            [opts.person],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let created = old.is_none();
    let consent = if let Some((privacy, raw)) = old {
        let consent: Value = serde_json::from_str(&raw)?;
        if privacy != "local"
            || consent["source"] != "human"
            || consent["personal_preference"] != true
            || consent["usage"] != "local-only"
            || consent["reviewer"] != opts.person
        {
            bail!(
                "已有身份不是明确本机 human personal_preference consent；不得将 demo/legacy 人格提升为真人，请使用新标识"
            );
        }
        consent
    } else {
        let consent = json!({"source":"human", "personal_preference":true,"usage":"local-only", "reviewer":opts.person,"recorded_at":crate::now_iso()});
        tx.execute(
            "INSERT INTO person(id,privacy_level,consent_json,created_at) VALUES(?1,'local',?2,?3)",
            params![opts.person, consent.to_string(), crate::now_iso()],
        )?;
        consent
    };
    tx.commit()?;
    Ok(
        json!({"person_id":opts.person,"created":created,"privacy_level":"local","consent":consent,"note":"仅本机个人偏好用途同意；不是身份认证，不授权公开或联邦上传"}),
    )
}
pub fn record(opts: &RecordOpts<'_>) -> Result<Value> {
    // Validate intent/actor before appending even the independent visual evaluation.
    let conn = connect(opts.visual.db, true)?;
    require_person(&conn, opts.person)?;
    require_choice(
        opts.person,
        opts.intent,
        opts.visual.source,
        opts.visual.reviewer,
    )?;
    drop(conn);
    let mut visual = crate::experiment::record(&opts.visual)?;
    let id = visual["evaluation_id"]
        .as_i64()
        .context("visual evaluation id 缺失")?;
    let result = bridge(&BridgeOpts {
        db: opts.visual.db,
        evaluation_id: id,
        person: opts.person,
        intent: opts.intent,
    });
    match result {
        Ok(result) => visual
            .as_object_mut()
            .unwrap()
            .extend(result.as_object().unwrap().clone()),
        Err(error) => {
            visual["bridge_created"] = json!(false);
            visual["bridge_reason"] = json!(error.to_string());
        }
    }
    visual["trained"] = json!(false); // Recording evidence is never fitting a kernel.
    Ok(visual)
}
pub fn bridge(opts: &BridgeOpts<'_>) -> Result<Value> {
    let mut conn = connect(opts.db, false)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_person(&tx, opts.person)?;
    let (experiment_id,manifest,source,reviewer,winner): (i64,String,String,Option<String>,String) = tx.query_row(
        "SELECT experiment_id,manifest_hash,source,reviewer,winner FROM artifact_evaluation WHERE id=?1 AND dimension='visual'", [opts.evaluation_id],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).context("没有可桥接的 visual evaluation")?;
    require_choice(opts.person, opts.intent, &source, reviewer.as_deref())?;
    let evidence = crate::experiment::eligible_evidence_in(&tx, experiment_id, &manifest)?;
    let existing: Option<i64> = tx
        .query_row(
            "SELECT id FROM judgment WHERE source_evaluation_id=?1 AND actor_id=?2",
            params![opts.evaluation_id, opts.person],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        let row = tx.query_row("SELECT id,source_evaluation_id,work_id,opponent_work_id,outcome,pair_key,context_json FROM judgment WHERE id=?1",[id],|r|Ok(TrainingRow{id:r.get(0)?,evaluation_id:r.get(1)?,a_work:r.get(2)?,b_work:r.get(3)?,outcome:r.get(4)?,pair_key:r.get(5)?,context:r.get(6)?}))?;
        training_pair(&tx, opts.person, &row)?;
        return Ok(
            json!({"evaluation_id":opts.evaluation_id,"judgment_id":id,"bridge_created":false,"already_bridged":true,"trained":false}),
        );
    }
    let mut sides = Vec::new();
    for artifact in evidence["artifacts"].as_array().context("缺 artifacts")? {
        let id = artifact["artifact_id"].as_i64().context("缺 artifact id")?;
        let mut side = candidate_in(&tx, id, &evidence)?;
        if side["frozen"] != true {
            freeze_work(&tx, &mut side)?;
        }
        side["label"] = artifact["label"].clone();
        sides.push(side);
    }
    sides.sort_by_key(fingerprint);
    let pair_key = sha(json!([fingerprint(&sides[0]), fingerprint(&sides[1])])
        .to_string()
        .as_bytes());
    let outcome = match winner.as_str() {
        "tie" => "tie",
        "abstain" => "abstain",
        label if sides[0]["label"] == label => "a_wins",
        label if sides[1]["label"] == label => "b_wins",
        _ => bail!("evaluation winner 映射无效"),
    };
    let context = json!({"feature_contract":FEATURE_CONTRACT,"scope":SCOPE,"group_key":evidence["content_hash"],
        "pair_key":pair_key,"evaluation_id":opts.evaluation_id,"manifest_hash":manifest,
        "reviewer":reviewer,"source":"human","intent":"personal_preference","evidence":evidence,
        "a":sides[0],"b":sides[1],"outcome":outcome});
    tx.execute("INSERT INTO judgment(work_id,opponent_work_id,kind,context_json,outcome,created_at,actor_id,visibility,source,source_evaluation_id,intent,pair_key,scope) VALUES(?1,?2,'pairwise',?3,?4,?5,?6,'private','human',?7,'personal_preference',?8,?9)",
        params![sides[0]["work_id"].as_i64(),sides[1]["work_id"].as_i64(),context.to_string(),outcome,crate::now_iso(),opts.person,opts.evaluation_id,pair_key,SCOPE])?;
    let id = tx.last_insert_rowid();
    // Revalidate file evidence after extraction and before committing all generated rows.
    crate::experiment::eligible_evidence_in(&tx, experiment_id, &manifest)?;
    tx.commit()?;
    Ok(
        json!({"evaluation_id":opts.evaluation_id,"judgment_id":id,"bridge_created":true,"already_bridged":false,"pair_key":pair_key,"outcome":outcome,"trained":false}),
    )
}
fn require_choice(person: &str, intent: &str, source: &str, reviewer: Option<&str>) -> Result<()> {
    if source != "human"
        || reviewer.map(str::trim) != Some(person)
        || person.is_empty()
        || intent != "personal_preference"
    {
        bail!("个人桥接必须显式 source=human、非空 reviewer=person、intent=personal_preference");
    }
    Ok(())
}
fn require_person(conn: &Connection, person: &str) -> Result<()> {
    let (privacy, raw): (String, String) = conn
        .query_row(
            "SELECT privacy_level,consent_json FROM person WHERE id=?1",
            [person],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .context("请先为新本机身份显式 person init")?;
    let consent: Value = serde_json::from_str(&raw)?;
    if privacy != "local"
        || consent["source"] != "human"
        || consent["reviewer"] != person
        || consent["personal_preference"] != true
        || consent["usage"] != "local-only"
    {
        bail!("身份无明确 local-only human personal_preference consent；demo/legacy 不可作真人");
    }
    Ok(())
}
fn fingerprint(side: &Value) -> String {
    format!(
        "{}:{}",
        side["page_sha256"].as_str().unwrap_or(""),
        side["shot_sha256"].as_str().unwrap_or("")
    )
}
fn freeze_work(tx: &Connection, side: &mut Value) -> Result<()> {
    let now = crate::now_iso();
    tx.execute("INSERT OR IGNORE INTO pack(id,title,version) VALUES('personal-ui-static','Local static captures','1')",[])?;
    tx.execute("INSERT OR IGNORE INTO domain(id,title,medium,pack_id) VALUES('personal-ui-static','Static UI','image','personal-ui-static')",[])?;
    tx.execute("INSERT OR IGNORE INTO source(id,kind,title,created_at) VALUES('generated-final-png','capture','Generated final screenshot',?1)",[&now])?;
    let artifact_id = side["artifact_id"].as_i64().context("artifact id 缺失")?;
    tx.execute("INSERT INTO work(source_id,domain_id,collection,slug,canonical_url,title,payload_json,first_seen_at,last_seen_at,visibility) VALUES('generated-final-png','personal-ui-static','generated-static',?1,?2,'Generated static artifact',?3,?4,?4,'private')",
        params![format!("artifact-{artifact_id}"),format!("local-generated://artifact/{artifact_id}"),json!({"origin":"generated-final-png","feature_contract":FEATURE_CONTRACT}).to_string(),now])?;
    let work_id = tx.last_insert_rowid();
    tx.execute("INSERT INTO asset(work_id,role,local_path,sha256,fetched_at) VALUES(?1,'screenshot',?2,?3,?4)",params![work_id,side["screenshot_path"].as_str(),side["shot_sha256"].as_str(),now])?;
    tx.execute("INSERT INTO artifact_work(artifact_id,work_id,feature_contract,features_json,features_sha256,page_sha256,shot_sha256,scope) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![artifact_id,work_id,FEATURE_CONTRACT,side["features"].to_string(),side["features_hash"].as_str(),side["page_sha256"].as_str(),side["shot_sha256"].as_str(),SCOPE])?;
    side["work_id"] = json!(work_id);
    side["frozen"] = json!(true);
    Ok(())
}
/// Read-only, provenance-checked frozen pixel pairs. Missing dimensions stay absent.
pub fn dataset(db: &str, person: &str, scope: &str) -> Result<Value> {
    if scope != SCOPE {
        bail!("personal 当前仅支持 scope={SCOPE}");
    }
    let mut conn = connect(db, true)?;
    let tx = conn.transaction()?;
    require_person(&tx, person)?;
    let mut counts = json!({"raw":0,"unique":0,"usable":0,"zero_difference":0,"ties":0,"abstentions":0,
        "excluded_demo":0,"excluded_ai":0,"excluded_legacy":0,"excluded_invalid":0,"excluded_missing_features":0,"duplicate":0});
    for source in ["demo", "ai", "legacy"] {
        let judgments: i64 = tx.query_row(
            "SELECT count(*) FROM judgment WHERE actor_id=?1 AND source=?2",
            params![person, source],
            |r| r.get(0),
        )?;
        let evaluations: i64 = if source == "legacy" {
            0
        } else {
            tx.query_row("SELECT count(*) FROM artifact_evaluation e WHERE e.reviewer=?1 AND e.source=?2 AND NOT EXISTS(SELECT 1 FROM judgment j WHERE j.source_evaluation_id=e.id AND j.actor_id=?1)",params![person,source],|r|r.get(0))?
        };
        counts[format!("excluded_{source}")] = json!(judgments + evaluations);
    }
    let mut stmt = tx.prepare("SELECT id,source_evaluation_id,work_id,opponent_work_id,outcome,pair_key,context_json FROM judgment WHERE actor_id=?1 AND source='human' AND intent='personal_preference' AND scope=?2 AND kind='pairwise' AND visibility='private' ORDER BY source_evaluation_id,id")?;
    let rows = stmt
        .query_map(params![person, scope], |r| {
            Ok(TrainingRow {
                id: r.get(0)?,
                evaluation_id: r.get(1)?,
                a_work: r.get(2)?,
                b_work: r.get(3)?,
                outcome: r.get(4)?,
                pair_key: r.get(5)?,
                context: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    counts["raw"] = json!(rows.len());
    // Deduplicate *before* outcome/feature filtering. A newer tie/abstention must
    // suppress an older decisive vote, including when old evaluations bridge late.
    let mut latest = std::collections::BTreeMap::new();
    for row in rows {
        let key = row
            .pair_key
            .clone()
            .unwrap_or_else(|| format!("invalid-{}", row.id));
        latest.insert(key, row);
    }
    counts["unique"] = json!(latest.len());
    counts["duplicate"] = json!(counts["raw"].as_u64().unwrap() - latest.len() as u64);
    let mut pairs = Vec::new();
    let mut invalid = Vec::new();
    for row in latest.into_values() {
        match training_pair(&tx, person, &row) {
            Ok(pair) => {
                if row.outcome == "tie" {
                    increment(&mut counts, "ties");
                    continue;
                }
                if row.outcome == "abstain" {
                    increment(&mut counts, "abstentions");
                    continue;
                }
                let common: Vec<f64> = TERMS
                    .iter()
                    .filter_map(|term| {
                        Some(
                            pair["a"]["features"][*term].as_f64()?
                                - pair["b"]["features"][*term].as_f64()?,
                        )
                    })
                    .collect();
                if common.is_empty() {
                    increment(&mut counts, "excluded_missing_features");
                    continue;
                }
                if common.iter().all(|x| *x == 0.0) {
                    increment(&mut counts, "zero_difference");
                    continue;
                }
                pairs.push(pair);
            }
            Err(error) => {
                increment(&mut counts, "excluded_invalid");
                invalid.push(json!({"judgment_id":row.id,"reason":error.to_string()}));
            }
        }
    }
    counts["usable"] = json!(pairs.len());
    Ok(
        json!({"person_id":person,"scope":scope,"feature_contract":FEATURE_CONTRACT,"terms":TERMS,"counts":counts,"pairs":pairs,"invalid":invalid,
        "note":"仅本机显式human个人偏好，冻结最终像素代理；raw不等于可训练独立样本，记录不表示已经拟合"}),
    )
}
struct TrainingRow {
    id: i64,
    evaluation_id: Option<i64>,
    a_work: i64,
    b_work: i64,
    outcome: String,
    pair_key: Option<String>,
    context: String,
}
fn increment(counts: &mut Value, key: &str) {
    counts[key] = json!(counts[key].as_u64().unwrap_or(0) + 1);
}
fn training_pair(conn: &Connection, person: &str, row: &TrainingRow) -> Result<Value> {
    let context: Value = serde_json::from_str(&row.context)?;
    let evaluation_id = row.evaluation_id.context("缺源evaluation")?;
    let (experiment_id,manifest,source,reviewer,winner): (i64,String,String,Option<String>,String) = conn.query_row("SELECT experiment_id,manifest_hash,source,reviewer,winner FROM artifact_evaluation WHERE id=?1 AND dimension='visual'",[evaluation_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    require_choice(person, "personal_preference", &source, reviewer.as_deref())?;
    let evidence = crate::experiment::eligible_evidence_in(conn, experiment_id, &manifest)?;
    if context["feature_contract"] != FEATURE_CONTRACT
        || context["scope"] != SCOPE
        || context["group_key"] != evidence["content_hash"]
        || context["evaluation_id"] != evaluation_id
        || context["manifest_hash"] != manifest
        || context["reviewer"] != person
        || context["source"] != "human"
        || context["intent"] != "personal_preference"
        || context["evidence"] != evidence
        || context["outcome"] != row.outcome
    {
        bail!("judgment context 冻结训练来源不匹配");
    }
    for (key, work) in [("a", row.a_work), ("b", row.b_work)] {
        let saved = &context[key];
        let id = saved["artifact_id"]
            .as_i64()
            .context("context缺artifact id")?;
        let mut current = candidate_in(conn, id, &evidence)?;
        let artifact = evidence["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["artifact_id"] == id)
            .context("缺artifact")?;
        current["label"] = artifact["label"].clone();
        if current["frozen"] != true || current["work_id"] != work || &current != saved {
            bail!("judgment 冻结phi/asset/work来源不匹配");
        }
    }
    let expected_key = sha(
        json!([fingerprint(&context["a"]), fingerprint(&context["b"])])
            .to_string()
            .as_bytes(),
    );
    if fingerprint(&context["a"]) > fingerprint(&context["b"])
        || context["pair_key"] != expected_key
        || row.pair_key.as_deref() != Some(expected_key.as_str())
    {
        bail!("pair key 不是规范页面/截图hash对");
    }
    let outcome = match winner.as_str() {
        "tie" => "tie",
        "abstain" => "abstain",
        label if context["a"]["label"] == label => "a_wins",
        label if context["b"]["label"] == label => "b_wins",
        _ => bail!("winner映射无效"),
    };
    if outcome != row.outcome {
        bail!("winner与规范pair映射不一致");
    }
    Ok(
        json!({"judgment_id":row.id,"evaluation_id":evaluation_id,"pair_key":expected_key,"group_key":context["group_key"],"outcome":outcome,"a":context["a"],"b":context["b"]}),
    )
}
pub fn candidate(db: &str, artifact_id: i64, scope: &str) -> Result<Value> {
    let mut conn = connect(db, true)?;
    let tx = conn.transaction()?;
    let evidence = artifact_evidence(&tx, artifact_id, scope)?;
    candidate_in(&tx, artifact_id, &evidence)
}
fn artifact_evidence(conn: &Connection, artifact_id: i64, scope: &str) -> Result<Value> {
    if scope != SCOPE {
        bail!("personal 当前仅支持 scope={SCOPE}");
    }
    let (id,manifest): (i64,String) = conn.query_row("SELECT a.experiment_id,e.manifest_hash FROM artifact a JOIN experiment e ON e.id=a.experiment_id WHERE a.id=?1",[artifact_id],|r|Ok((r.get(0)?,r.get(1)?))).context("candidate artifact 不存在或尚未 blind")?;
    crate::experiment::eligible_evidence_in(conn, id, &manifest)
}
fn candidate_in(conn: &Connection, artifact_id: i64, evidence: &Value) -> Result<Value> {
    let artifact = evidence["artifacts"]
        .as_array()
        .context("缺 artifacts")?
        .iter()
        .find(|a| a["artifact_id"] == artifact_id)
        .context("证据中无指定 artifact")?;
    let frozen: Option<(i64,String,String,String,String,String,String)> = conn.query_row("SELECT work_id,feature_contract,features_json,features_sha256,page_sha256,shot_sha256,scope FROM artifact_work WHERE artifact_id=?1",[artifact_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional()?;
    let (work_id, features, features_hash) = if let Some((
        work_id,
        contract,
        raw,
        hash,
        page,
        shot,
        scope,
    )) = frozen
    {
        if contract != FEATURE_CONTRACT
            || scope != SCOPE
            || sha(raw.as_bytes()) != hash
            || artifact["page_sha256"] != page
            || artifact["shot_sha256"] != shot
        {
            bail!("冻结 artifact_work contract/feature/page/shot hash 不匹配");
        }
        let sourced: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM work w JOIN asset a ON a.work_id=w.id WHERE w.id=?1 AND w.source_id='generated-final-png' AND w.visibility='private' AND a.role='screenshot' AND a.local_path=?2 AND a.sha256=?3)",params![work_id,artifact["screenshot_path"].as_str(),shot],|r|r.get(0))?;
        if !sourced {
            bail!("冻结 generated work/截图 asset 来源或 private 标记无效");
        }
        let features: Value = serde_json::from_str(&raw)?;
        validate_features(&features)?;
        (Some(work_id), features, hash)
    } else {
        let path =
            std::path::Path::new(artifact["screenshot_path"].as_str().context("缺截图路径")?);
        let metrics = crate::metrics::compute(path)?;
        if crate::capture::sha256_file(path)?
            != artifact["shot_sha256"].as_str().context("缺截图hash")?
        {
            bail!("提取期间最终PNG已改变");
        }
        // Match extract's existing blank-image guard: these proxies are undefined,
        // not measured zeroes (nor a claim of perfect negative space).
        let blank = metrics.edge_density < 0.01 && metrics.whitespace > 0.98;
        let features: serde_json::Map<String, Value> = crate::extract::proxies(&metrics)
            .into_iter()
            .filter(|(term, _, confidence)| !blank && TERMS.contains(term) && *confidence >= 0.7)
            .map(|(term, score, _)| (term.to_owned(), json!(score)))
            .collect();
        let features = Value::Object(features);
        let hash = sha(features.to_string().as_bytes());
        (None, features, hash)
    };
    Ok(
        json!({"artifact_id":artifact_id,"work_id":work_id,"scope":SCOPE,"feature_contract":FEATURE_CONTRACT,
        "features_hash":features_hash,"features":features,"frozen":work_id.is_some(),
        "page_sha256":artifact["page_sha256"],"shot_sha256":artifact["shot_sha256"],
        "screenshot_path":artifact["screenshot_path"],"pill_snapshot":artifact["pill_snapshot"],"recipe_hash":artifact["recipe_hash"],
        "conditions_hash":evidence["conditions_hash"],"group_key":evidence["content_hash"],"manifest_hash":evidence["manifest_hash"]}),
    )
}
fn validate_features(features: &Value) -> Result<()> {
    let object = features.as_object().context("冻结features必须是object")?;
    for (term, value) in object {
        if !TERMS.contains(&term.as_str())
            || !value
                .as_f64()
                .is_some_and(|x| x.is_finite() && (0.0..=1.0).contains(&x))
        {
            bail!("不可信或不支持的冻结特征");
        }
    }
    Ok(())
}
fn sha(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

fn connect(db: &str, readonly: bool) -> Result<Connection> {
    let flags = if readonly {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    };
    let conn =
        Connection::open_with_flags(db, flags).context("personal 需要已显式初始化/迁移的数据库")?;
    conn.busy_timeout(Duration::from_secs(10))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    let version: String = conn.query_row(
        "SELECT value FROM schema_meta WHERE key='schema_version'",
        [],
        |r| r.get(0),
    )?;
    if !version
        .parse::<u32>()
        .is_ok_and(|v| (25..=crate::db::LATEST_SCHEMA).contains(&v))
    {
        bail!("personal 需要 schema 25 或兼容的新版本；请显式迁移，不会自动修改旧库");
    }
    Ok(conn)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rusqlite::Connection;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    pub(crate) struct Fixture {
        pub(crate) root: PathBuf,
        pub(crate) db: String,
    }
    impl Fixture {
        pub(crate) fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "ie-personal-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).unwrap();
            let db = root.join("fixture.sqlite").to_str().unwrap().to_owned();
            let mut conn = Connection::open(&db).unwrap();
            crate::db::migrate(&mut conn).unwrap();
            Self { root, db }
        }
    }
    fn sha(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(bytes))
    }
    impl Fixture {
        /// SYNTHETIC UNIT FIXTURE RECEIPTS ONLY, not real Chrome or identity attestation.
        /// Kept exclusively in this fixture's temp DB/files; no production trust override.
        pub(crate) fn pair(
            &self,
            group: &str,
            reverse: bool,
            same_pixels: bool,
            verified: bool,
        ) -> (i64, String, i64, i64) {
            self.pair_with_blank(group, reverse, same_pixels, verified, false)
        }
        fn pair_with_blank(
            &self,
            group: &str,
            reverse: bool,
            same_pixels: bool,
            verified: bool,
            blank: bool,
        ) -> (i64, String, i64, i64) {
            let conn = Connection::open(&self.db).unwrap();
            let frozen = |key: &str| {
                let content = json!({"roles":{"background":"#FFFFFF","text":"#000000"},"colors":[{"hex":"#FFFFFF"},{"hex":"#000000"}]});
                json!({"snapshot_version":1,"recipe_definition":{"key":key,"components":[{"dimension":"palette","slot":"base","ratio":1.0}],"combination_prompt":"layout"},"prompt":"Frozen design specification","components":[{"dimension":"palette","slot":"base","extract_revision":1,"technique_version":"1","hash":"fixture-extract-hash","content_hash":sha(content.to_string().as_bytes()),"content":content}]})
            };
            let a = frozen("fixture.recipe.a");
            let b = frozen("fixture.recipe.b");
            for (id, snapshot) in [(1, &a), (2, &b)] {
                conn.execute("INSERT OR IGNORE INTO pill(id,output_md,output_json,created_at) VALUES(?1,'fixture',?2,'fixture')", params![id,snapshot.to_string()]).unwrap();
            }
            let content = json!({"strings":[group,"Shared body"]});
            let conditions = json!({"viewport":{"width":96,"height":80},"theme":"light","generator":{"kind":"human","version":"SYNTHETIC UNIT FIXTURE - NOT REAL CHROME"},"task":"same static task"});
            let mapping = if reverse {
                json!({"A":"b","B":"a"})
            } else {
                json!({"A":"a","B":"b"})
            };
            conn.execute("INSERT INTO experiment(content_json,content_hash,conditions_json,conditions_hash,pill_a_id,pill_b_id,pill_a_snapshot_json,pill_b_snapshot_json,pill_a_snapshot_hash,pill_b_snapshot_hash,recipe_a_hash,recipe_b_hash,blind_mapping_json) VALUES(?1,?2,?3,?4,1,2,?5,?6,?7,?8,?9,?10,?11)", params![content.to_string(),sha(content.to_string().as_bytes()),conditions.to_string(),sha(conditions.to_string().as_bytes()),a.to_string(),b.to_string(),sha(a.to_string().as_bytes()),sha(b.to_string().as_bytes()),sha(a["recipe_definition"].to_string().as_bytes()),sha(b["recipe_definition"].to_string().as_bytes()),mapping.to_string()]).unwrap();
            let id = conn.last_insert_rowid();
            let mut ids = Vec::new();
            for arm in ["a", "b"] {
                let dir = self.root.join(format!("artifact-{id}-{arm}"));
                std::fs::create_dir(&dir).unwrap();
                let html = format!(
                    "<!doctype html><html><head><style>body{{background:#FFFFFF;color:#000000}}h1{{font-size:32px}}p{{font-size:16px}}</style></head><body><h1>{group}</h1><p>Shared body</p><div data-arm=\"{arm}\"></div></body></html>"
                );
                let original = dir.join("original.html");
                let archive = dir.join("page.html");
                let shot = dir.join("capture.png");
                let receipt_path = dir.join("receipt.json");
                std::fs::write(&original, &html).unwrap();
                std::fs::write(&archive, &html).unwrap();
                let seed = group.bytes().map(u32::from).sum::<u32>();
                image::RgbImage::from_fn(96, 80, |x, y| {
                    let ink = !blank
                        && if arm == "a" || same_pixels {
                            x > 10 && x < 20 + seed % 20 && y > 12 && y < 68
                        } else {
                            (x / (3 + seed % 5) + y / 7) % 2 == 0
                        };
                    if ink {
                        image::Rgb([0, 0, 0])
                    } else {
                        image::Rgb([255, 255, 255])
                    }
                })
                .save(&shot)
                .unwrap();
                let page_hash = sha(html.as_bytes());
                let shot_hash = crate::capture::sha256_file(&shot).unwrap();
                let receipt = json!({"policy_version":crate::capture::POLICY_VERSION,"page_sha256":page_hash,"effective_page_sha256":page_hash,"screenshot_sha256":shot_hash,"profile_sha256":sha(b"synthetic unit fixture receipt, NOT REAL CHROME"),"viewport":conditions["viewport"],"theme":"light","renderer":{"kind":if verified {"chrome"} else {"mock"},"version":"SYNTHETIC UNIT FIXTURE - NOT REAL CHROME"},"verified":verified});
                std::fs::write(&receipt_path, receipt.to_string()).unwrap();
                let snapshot = if arm == "a" { &a } else { &b };
                let report = json!({"status":"compliant","basis":"html_text_projection","missing_strings":[],"expected_media_sha256":[],"actual_media_sha256":[],"unsupported_css_media":false,"visibility_verified":false,"note":"核验静态 body 文本和显式 img 素材覆盖；不证明 CSS 下真实可见性或视觉美观","static_checks":crate::forge::static_report(snapshot,&html).unwrap()});
                conn.execute("INSERT INTO artifact(experiment_id,arm,original_path,archived_html_path,screenshot_path,page_sha256,screenshot_sha256,capture_receipt_json,capture_receipt_path,capture_receipt_sha256,capture_conditions_hash,content_status,content_report_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'compliant',?12)", params![id,arm,original.to_str().unwrap(),archive.to_str().unwrap(),shot.to_str().unwrap(),page_hash,shot_hash,receipt.to_string(),receipt_path.to_str().unwrap(),sha(receipt.to_string().as_bytes()),sha(conditions.to_string().as_bytes()),report.to_string()]).unwrap();
                ids.push(conn.last_insert_rowid());
            }
            let blind = crate::experiment::blind(&crate::experiment::BlindOpts {
                db: &self.db,
                experiment_id: id,
                out_dir: &self.root.join(format!("blind-{id}")),
            })
            .unwrap();
            (
                id,
                blind["manifest_hash"].as_str().unwrap().to_owned(),
                ids[0],
                ids[1],
            )
        }
        pub(crate) fn record(
            &self,
            id: i64,
            manifest: &str,
            person: &str,
            winner: &str,
        ) -> Result<Value> {
            super::record(&RecordOpts {
                visual: crate::experiment::RecordOpts {
                    db: &self.db,
                    experiment_id: id,
                    manifest_hash: manifest,
                    source: "human",
                    reviewer: Some(person),
                    dimension: "visual",
                    winner,
                    note: None,
                },
                person,
                intent: "personal_preference",
            })
        }
    }
    #[test]
    fn degenerate_blank_png_has_missing_features_not_zero_filled_or_extreme_preferences() {
        let f = Fixture::new();
        init(&InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (id, hash, a, _) = f.pair_with_blank("blank-image", false, true, true, true);
        let ca = candidate(&f.db, a, SCOPE).unwrap();
        assert_eq!(
            ca["features"],
            json!({}),
            "existing extract blank guard must not become synthetic 0/1 training"
        );
        f.record(id, &hash, "fixture-alice", "A").unwrap();
        let data = dataset(&f.db, "fixture-alice", SCOPE).unwrap();
        assert_eq!(data["counts"]["raw"], 1);
        assert_eq!(data["counts"]["usable"], 0);
        assert_eq!(data["counts"]["excluded_missing_features"], 1);
    }
    #[test]
    fn identical_trusted_phi_is_retained_raw_but_never_counted_usable() {
        let f = Fixture::new();
        init(&InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (id, hash, a, b) = f.pair("zero-difference", false, true, true);
        let recorded = f.record(id, &hash, "fixture-alice", "A").unwrap();
        assert_eq!(recorded["bridge_created"], true);
        assert_eq!(
            candidate(&f.db, a, SCOPE).unwrap()["features"],
            candidate(&f.db, b, SCOPE).unwrap()["features"]
        );
        let data = dataset(&f.db, "fixture-alice", SCOPE).unwrap();
        assert_eq!(data["counts"]["raw"], 1);
        assert_eq!(data["counts"]["unique"], 1);
        assert_eq!(data["counts"]["usable"], 0);
        assert_eq!(data["counts"]["zero_difference"], 1);
        assert!(data["pairs"].as_array().unwrap().is_empty());
    }
    #[test]
    fn latest_tie_or_abstention_suppresses_older_vote_even_when_it_bridges_late() {
        let f = Fixture::new();
        init(&InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (id, hash, _, _) = f.pair("latest-undecided", false, false, true);
        let old = crate::experiment::record(&crate::experiment::RecordOpts {
            db: &f.db,
            experiment_id: id,
            manifest_hash: &hash,
            source: "human",
            reviewer: Some("fixture-alice"),
            dimension: "visual",
            winner: "A",
            note: None,
        })
        .unwrap();
        f.record(id, &hash, "fixture-alice", "tie").unwrap();
        bridge(&BridgeOpts {
            db: &f.db,
            evaluation_id: old["evaluation_id"].as_i64().unwrap(),
            person: "fixture-alice",
            intent: "personal_preference",
        })
        .unwrap();
        let tie = dataset(&f.db, "fixture-alice", SCOPE).unwrap();
        assert_eq!(tie["counts"]["raw"], 2);
        assert_eq!(tie["counts"]["unique"], 1);
        assert_eq!(tie["counts"]["usable"], 0);
        assert_eq!(tie["counts"]["ties"], 1);
        f.record(id, &hash, "fixture-alice", "abstain").unwrap();
        let abstain = dataset(&f.db, "fixture-alice", SCOPE).unwrap();
        assert_eq!(abstain["counts"]["raw"], 3);
        assert_eq!(abstain["counts"]["usable"], 0);
        assert_eq!(abstain["counts"]["abstentions"], 1);
        assert_eq!(abstain["counts"]["ties"], 0);
    }
    #[test]
    fn later_live_vector_upsert_cannot_rewrite_frozen_candidate_or_training_dataset() {
        let f = Fixture::new();
        init(&InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (id, hash, a, _) = f.pair("immutable-phi", false, false, true);
        f.record(id, &hash, "fixture-alice", "A").unwrap();
        let before = dataset(&f.db, "fixture-alice", SCOPE).unwrap();
        let ca = candidate(&f.db, a, SCOPE).unwrap();
        let conn = Connection::open(&f.db).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO pack(id,title,version) VALUES('core','Core','1')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO vocab_term(pack_id,kind,term,label,definition) VALUES('core','primitive','negative-space.active','Whitespace','fixture')",[]).unwrap();
        let term = conn.last_insert_rowid();
        let work = ca["work_id"].as_i64().unwrap();
        for score in [0.1, 0.9] {
            crate::vector::record(
                &conn,
                &crate::vector::Score {
                    work_id: work,
                    term_id: term,
                    score,
                    confidence: 1.0,
                    source: "manual",
                    version: "fixture-upsert",
                    now: "fixture",
                },
            )
            .unwrap();
        }
        assert_eq!(candidate(&f.db, a, SCOPE).unwrap(), ca);
        assert_eq!(dataset(&f.db, "fixture-alice", SCOPE).unwrap(), before);
        assert!(
            conn.execute("UPDATE artifact_work SET features_json='{}'", [])
                .is_err()
        );
        assert!(
            conn.execute("UPDATE judgment SET outcome='b_wins'", [])
                .is_err()
        );
    }
    #[test]
    fn mock_capture_stays_visual_only_even_with_explicit_human_personal_intent() {
        let f = Fixture::new();
        init(&InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (id, hash, a, _) = f.pair("mock-capture", false, false, false);
        let result = f.record(id, &hash, "fixture-alice", "A").unwrap();
        assert_eq!(result["formal_eligible"], false);
        assert_eq!(result["bridge_created"], false);
        assert!(
            result["bridge_reason"]
                .as_str()
                .unwrap()
                .contains("eligible")
        );
        assert!(candidate(&f.db, a, SCOPE).is_err());
        let data = dataset(&f.db, "fixture-alice", SCOPE).unwrap();
        assert_eq!(data["counts"]["raw"], 0);
        assert_eq!(data["counts"]["usable"], 0);
    }
    #[test]
    fn record_rejects_wrong_source_reviewer_and_intent_before_any_evaluation_write() {
        let f = Fixture::new();
        init(&InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (id, hash, _, _) = f.pair("guard-personal", false, false, true);
        for (source, reviewer, intent) in [
            ("demo", Some("fixture-alice"), "personal_preference"),
            ("ai", Some("fixture-alice"), "personal_preference"),
            ("human", Some("fixture-bob"), "personal_preference"),
            ("human", Some(""), "personal_preference"),
            ("human", None, "personal_preference"),
            ("human", Some("fixture-alice"), "visual"),
            ("human", Some("fixture-alice"), "personal_preference "),
        ] {
            let visual = crate::experiment::RecordOpts {
                db: &f.db,
                experiment_id: id,
                manifest_hash: &hash,
                source,
                reviewer,
                dimension: "visual",
                winner: "A",
                note: None,
            };
            assert!(
                record(&RecordOpts {
                    visual,
                    person: "fixture-alice",
                    intent
                })
                .is_err()
            );
        }
        let conn = Connection::open(&f.db).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM artifact_evaluation", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
    #[test]
    fn bridge_failure_rolls_back_all_generated_rows_but_explicitly_retains_visual_record() {
        let f = Fixture::new();
        init(&InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (id, hash, _, _) = f.pair("transaction-error", false, false, true);
        let conn = Connection::open(&f.db).unwrap();
        conn.execute_batch("CREATE TRIGGER fixture_fail_second_freeze BEFORE INSERT ON artifact_work WHEN EXISTS(SELECT 1 FROM artifact_work) BEGIN SELECT RAISE(ABORT,'synthetic transaction failure'); END;").unwrap();
        let recorded = f.record(id, &hash, "fixture-alice", "A").unwrap();
        assert_eq!(recorded["bridge_created"], false);
        assert_eq!(recorded["trained"], false);
        assert!(
            recorded["bridge_reason"]
                .as_str()
                .unwrap()
                .contains("synthetic transaction failure")
        );
        for table in ["work", "asset", "artifact_work", "judgment"] {
            let count: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 0, "{table} must roll back with bridge failure");
        }
        let visuals: i64 = conn
            .query_row("SELECT count(*) FROM artifact_evaluation", [], |r| r.get(0))
            .unwrap();
        assert_eq!(visuals, 1);
        conn.execute_batch("DROP TRIGGER fixture_fail_second_freeze")
            .unwrap();
        let retry = bridge(&BridgeOpts {
            db: &f.db,
            evaluation_id: recorded["evaluation_id"].as_i64().unwrap(),
            person: "fixture-alice",
            intent: "personal_preference",
        })
        .unwrap();
        assert_eq!(
            retry["bridge_created"], true,
            "independent repair does not invent a second visual evaluation"
        );
    }
    #[test]
    fn bridge_retries_are_idempotent_and_revalidate_frozen_work_sources() {
        let f = Fixture::new();
        init(&InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (id, hash, _, _) = f.pair("retry-bridge", false, false, true);
        let recorded = f.record(id, &hash, "fixture-alice", "A").unwrap();
        let opts = BridgeOpts {
            db: &f.db,
            evaluation_id: recorded["evaluation_id"].as_i64().unwrap(),
            person: "fixture-alice",
            intent: "personal_preference",
        };
        let retry = bridge(&opts).unwrap();
        assert_eq!(retry["bridge_created"], false);
        assert_eq!(retry["already_bridged"], true);
        assert_eq!(retry["judgment_id"], recorded["judgment_id"]);
        let conn = Connection::open(&f.db).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM judgment", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        conn.execute("UPDATE asset SET sha256='tampered'", [])
            .unwrap();
        assert!(
            bridge(&opts).is_err(),
            "idempotent success must not conceal invalid frozen evidence"
        );
    }
    #[test]
    fn dataset_normalizes_reverse_labels_by_content_hashes_and_counts_only_latest_pair() {
        let f = Fixture::new();
        init(&InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (id, hash, _, _) = f.pair("repeat-pair", false, false, true);
        f.record(id, &hash, "fixture-alice", "A").unwrap();
        let first = dataset(&f.db, "fixture-alice", SCOPE).unwrap();
        let (reverse_id, reverse_hash, _, _) = f.pair("repeat-pair", true, false, true);
        f.record(reverse_id, &reverse_hash, "fixture-alice", "B")
            .unwrap();
        let reversed = dataset(&f.db, "fixture-alice", SCOPE).unwrap();
        assert_eq!(reversed["counts"]["raw"], 2);
        assert_eq!(reversed["counts"]["unique"], 1);
        assert_eq!(reversed["counts"]["usable"], 1);
        assert_eq!(
            reversed["pairs"][0]["pair_key"],
            first["pairs"][0]["pair_key"]
        );
        assert_eq!(
            reversed["pairs"][0]["outcome"], first["pairs"][0]["outcome"],
            "reversed labels cannot reverse the physical winner"
        );
        f.record(reverse_id, &reverse_hash, "fixture-alice", "A")
            .unwrap();
        let changed = dataset(&f.db, "fixture-alice", SCOPE).unwrap();
        assert_eq!(changed["counts"]["raw"], 3);
        assert_eq!(changed["counts"]["unique"], 1);
        assert_ne!(
            changed["pairs"][0]["outcome"], first["pairs"][0]["outcome"],
            "latest real choice wins; repeats never inflate n"
        );
    }
    #[test]
    fn dataset_counts_two_hundred_demo_as_excluded_not_as_human_training() {
        let f = Fixture::new();
        init(&InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (id, hash, _, _) = f.pair("mixed-sources", false, false, true);
        let conn = Connection::open(&f.db).unwrap();
        for _ in 0..200 {
            conn.execute("INSERT INTO artifact_evaluation(experiment_id,manifest_hash,source,reviewer,dimension,winner) VALUES(?1,?2,'demo','fixture-alice','visual','A')",params![id,hash]).unwrap();
        }
        conn.execute("INSERT INTO artifact_evaluation(experiment_id,manifest_hash,source,reviewer,dimension,winner) VALUES(?1,?2,'ai','fixture-alice','visual','B')",params![id,hash]).unwrap();
        f.record(id, &hash, "fixture-alice", "A").unwrap();
        let data = dataset(&f.db, "fixture-alice", SCOPE).unwrap();
        assert_eq!(data["terms"], json!(TERMS));
        assert_eq!(data["counts"]["raw"], 1);
        assert_eq!(data["counts"]["unique"], 1);
        assert_eq!(data["counts"]["usable"], 1);
        assert_eq!(data["counts"]["excluded_demo"], 200);
        assert_eq!(data["counts"]["excluded_ai"], 1);
        assert_eq!(data["pairs"].as_array().unwrap().len(), 1);
        let pair = &data["pairs"][0];
        assert!(pair["group_key"].is_string());
        assert!(pair["a"]["features_hash"].is_string());
        assert!(pair["b"]["features"].is_object());
    }
    #[test]
    fn explicit_personal_record_creates_private_generated_works_and_freezes_final_pixel_phi() {
        let f = Fixture::new();
        init(&InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        let (id, hash, a, b) = f.pair("personal-record", false, false, true);
        let before = candidate(&f.db, a, SCOPE).unwrap();
        let recorded = f.record(id, &hash, "fixture-alice", "A").unwrap();
        assert_eq!(recorded["bridge_created"], true);
        assert!(recorded["judgment_id"].as_i64().unwrap() > 0);
        let frozen = candidate(&f.db, a, SCOPE).unwrap();
        assert_eq!(frozen["features"], before["features"]);
        assert_eq!(frozen["frozen"], true);
        assert_ne!(
            frozen["work_id"],
            candidate(&f.db, b, SCOPE).unwrap()["work_id"]
        );
        let conn = Connection::open(&f.db).unwrap();
        let private: i64 = conn
            .query_row(
                "SELECT count(*) FROM work WHERE visibility='private'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(private, 2);
        let raw: String = conn
            .query_row(
                "SELECT context_json FROM judgment WHERE id=?1",
                [recorded["judgment_id"].as_i64().unwrap()],
                |r| r.get(0),
            )
            .unwrap();
        let context: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(context["feature_contract"], FEATURE_CONTRACT);
        assert!(context["a"]["features_hash"].is_string());
        assert!(context["b"]["shot_sha256"].is_string());
        let exposed: i64 = conn.query_row("SELECT count(*) FROM work WHERE payload_json LIKE '%fixture-alice%' OR payload_json LIKE '%evaluation%'",[],|r|r.get(0)).unwrap();
        assert_eq!(exposed, 0);
    }
    #[test]
    fn candidate_uses_final_png_two_trusted_proxies_not_material_or_recipe_features() {
        let f = Fixture::new();
        let (_id, _hash, a, b) = f.pair("candidate-group", false, false, true);
        let before: i64 = Connection::open(&f.db)
            .unwrap()
            .query_row("SELECT count(*) FROM work", [], |r| r.get(0))
            .unwrap();
        let ca = candidate(&f.db, a, SCOPE).unwrap();
        let cb = candidate(&f.db, b, SCOPE).unwrap();
        assert_ne!(ca["features"], cb["features"]);
        assert_eq!(ca["feature_contract"], FEATURE_CONTRACT);
        assert_eq!(ca["features"].as_object().unwrap().len(), 2);
        assert!(ca["features"]["negative-space.active"].is_number());
        assert!(ca["features"]["detail.density"].is_number());
        assert_eq!(
            ca["pill_snapshot"]["components"], cb["pill_snapshot"]["components"],
            "same materials cannot imply same final-pixel phi"
        );
        let after: i64 = Connection::open(&f.db)
            .unwrap()
            .query_row("SELECT count(*) FROM work", [], |r| r.get(0))
            .unwrap();
        assert_eq!(after, before, "read-only candidate must not create a work");
    }
    #[test]
    fn migration_preserves_existing_active_kernels_and_unknown_judgments_as_legacy() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrate_to(&mut conn, 24).unwrap();
        conn.execute_batch("INSERT INTO pack(id,title,version) VALUES('fixture','Fixture','1'); INSERT INTO domain(id,title,medium,pack_id) VALUES('fixture','Fixture','image','fixture'); INSERT INTO source(id,kind,title,created_at) VALUES('fixture','manual','Fixture','fixture'); INSERT INTO work(id,source_id,domain_id,collection,slug,canonical_url,title,first_seen_at,last_seen_at) VALUES(1,'fixture','fixture','fixture','old-a','fixture://old-a','Old A','fixture','fixture'),(2,'fixture','fixture','fixture','old-b','fixture://old-b','Old B','fixture','fixture'); INSERT INTO person(id,consent_json,created_at) VALUES('demo','{\"purpose\":\"demo\"}','fixture'); INSERT INTO judgment(work_id,opponent_work_id,kind,outcome,actor_id,created_at) VALUES(1,2,'pairwise','a_wins','demo','fixture'); INSERT INTO kernel(person_id,version,weights_json,prior_json,fit_json,status,created_at) VALUES('demo',1,'{}','{}','{}','active','fixture');").unwrap();
        conn.execute_batch(include_str!(
            "../../../../docs/design/inspiration/migrations/024-personal-evidence.sql"
        ))
        .unwrap();
        let source: String = conn
            .query_row("SELECT source FROM judgment", [], |r| r.get(0))
            .unwrap();
        assert_eq!(source, "legacy");
        let kernel: (String, String) = conn
            .query_row("SELECT status,training_source FROM kernel", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(kernel, ("active".into(), "legacy".into()));
        let public: i64 = conn
            .query_row(
                "SELECT count(*) FROM work WHERE visibility='public'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(public, 2);
        let consent: String = conn
            .query_row("SELECT consent_json FROM person WHERE id='demo'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(consent, "{\"purpose\":\"demo\"}");
    }
    #[test]
    fn migration_marks_existing_rows_legacy_and_protects_frozen_feature_rows() {
        let f = Fixture::new();
        let conn = Connection::open(&f.db).unwrap();
        let default_source: String = conn
            .query_row(
                "SELECT dflt_value FROM pragma_table_info('judgment') WHERE name='source'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(default_source, "'legacy'");
        let version: String = conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(version, crate::db::LATEST_SCHEMA.to_string());
        let immutable: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND tbl_name='artifact_work'", [], |r| r.get(0)).unwrap();
        assert_eq!(immutable, 2);
    }
    #[test]
    fn init_refuses_ambiguous_existing_consent_instead_of_claiming_local_readiness() {
        let f = Fixture::new();
        let conn = Connection::open(&f.db).unwrap();
        let ambiguous =
            json!({"source":"human","personal_preference":true,"reviewer":"fixture-alice"});
        conn.execute(
            "INSERT INTO person(id,consent_json,created_at) VALUES('fixture-alice',?1,'fixture')",
            [ambiguous.to_string()],
        )
        .unwrap();
        assert!(
            init(&InitOpts {
                db: &f.db,
                person: "fixture-alice"
            })
            .is_err(),
            "missing local-only purpose cannot be silently inferred"
        );
    }
    #[test]
    fn init_records_explicit_local_consent_and_never_promotes_demo_identity() {
        let f = Fixture::new();
        let result = init(&InitOpts {
            db: &f.db,
            person: "fixture-alice",
        })
        .unwrap();
        assert_eq!(result["person_id"], "fixture-alice");
        assert_eq!(result["privacy_level"], "local");
        assert_eq!(result["consent"]["personal_preference"], true);
        assert_eq!(result["consent"]["source"], "human");
        assert_eq!(
            init(&InitOpts {
                db: &f.db,
                person: "fixture-alice"
            })
            .unwrap()["created"],
            false
        );
        let conn = Connection::open(&f.db).unwrap();
        conn.execute(
            "INSERT INTO person(id,consent_json,created_at) VALUES('demo',?1,'fixture')",
            [json!({"purpose":"demo", "source_delta": {}}).to_string()],
        )
        .unwrap();
        assert!(
            init(&InitOpts {
                db: &f.db,
                person: "demo"
            })
            .is_err()
        );
    }
}
