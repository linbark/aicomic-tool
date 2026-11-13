//! Frozen-input, two-arm static HTML experiments; visual evidence never reuses text scores.
//!
//! Content coverage is conservative source projection, not DOM visibility or a causal
//! proof. Generator identity is declared, not independently attested. Only the controlled
//! capture path and latest named human visual reviews qualify for descriptive statistics.
//! Original/archive/receipt/screenshots and every public package must remain in place.
//! SQL history is append-only, but hashes are not signatures against a database owner.
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;

pub struct CreateOpts<'a> {
    pub db: &'a str,
    pub pill_a: i64,
    pub pill_b: i64,
    pub content: &'a Value,
    pub conditions: &'a Value,
}

pub struct AttachOpts<'a> {
    pub db: &'a str,
    pub experiment_id: i64,
    pub arm: &'a str,
    pub html: &'a std::path::Path,
    pub out_dir: &'a std::path::Path,
    pub chrome: Option<&'a std::path::Path>,
}
pub struct BlindOpts<'a> {
    pub db: &'a str,
    pub experiment_id: i64,
    pub out_dir: &'a std::path::Path,
}
pub struct RecordOpts<'a> {
    pub db: &'a str,
    pub experiment_id: i64,
    pub manifest_hash: &'a str,
    pub source: &'a str,
    pub reviewer: Option<&'a str>,
    pub dimension: &'a str,
    pub winner: &'a str,
    pub note: Option<&'a str>,
}
pub struct StatsOpts<'a> {
    pub db: &'a str,
}

/// Read-only private evidence view for the personal bridge; not a public blind payload.
/// Revalidates the original/archive/receipt/PNG/anonymous exports, never trusts IDs alone.
#[allow(dead_code)] // Standalone read-only API; the bridge uses the transaction-sharing variant.
pub fn eligible_evidence(db: &str, experiment_id: i64, manifest_hash: &str) -> Result<Value> {
    let mut conn = connect(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let tx = conn.transaction()?;
    eligible_evidence_in(&tx, experiment_id, manifest_hash)
}
/// Shares the caller's SQLite snapshot/transaction, and performs no database writes.
pub(crate) fn eligible_evidence_in(
    conn: &Connection,
    experiment_id: i64,
    manifest_hash: &str,
) -> Result<Value> {
    let experiment = load_experiment(conn, experiment_id)?;
    let artifacts = verified_pair(conn, experiment_id, &experiment)?;
    let (_, expected_hash) = verify_manifest(&experiment, &artifacts)?;
    if expected_hash != manifest_hash {
        bail!("manifest hash 不匹配冻结匿名证据");
    }
    verify_public_exports(conn, experiment_id, &experiment, &artifacts)?;
    if !generator_known(&experiment.conditions)
        || !artifacts.iter().all(|a| {
            a.receipt["verified"] == true
                && a.receipt["renderer"]["kind"] == "chrome"
                && a.report["static_checks"]["passed"] == true
        })
    {
        bail!("个人偏好只接受正式 eligible 的真实 capture；mock/未知生成器/静态失败不得桥接");
    }
    let mut result = Vec::new();
    for (index, artifact) in artifacts.iter().enumerate() {
        let id: i64 = conn.query_row(
            "SELECT id FROM artifact WHERE experiment_id=?1 AND arm=?2",
            params![experiment_id, artifact.arm],
            |r| r.get(0),
        )?;
        let label = if experiment.mapping["A"] == artifact.arm {
            "A"
        } else {
            "B"
        };
        result.push(json!({"artifact_id":id,"arm":artifact.arm,"label":label,
            "original_path":artifact.original,"archived_html_path":artifact.archive,"screenshot_path":artifact.screenshot,
            "page_sha256":artifact.page_hash,"shot_sha256":artifact.screenshot_hash,
            "receipt_sha256":artifact.receipt_hash,"capture_receipt":artifact.receipt,
            "content_report":artifact.report,"pill_snapshot":experiment.snapshots[index],"recipe_hash":experiment.recipe_hashes[index]}));
    }
    Ok(
        json!({"experiment_id":experiment_id,"manifest_hash":expected_hash,"formal_eligible":true,
        "content_hash":hash_json(&experiment.content),"conditions_hash":experiment.conditions_hash,
        "conditions":experiment.conditions,"artifacts":result,
        "boundaries":"capture 不是 OS 沙箱；hash 不是签名；生成器身份为声明；静态内容不证明可见性或偏好"}),
    )
}

pub fn attach(opts: &AttachOpts<'_>) -> Result<Value> {
    attach_impl(opts, None)
}
/// Test/custom renderers are always unverified, irrespective of claimed identity.
#[cfg_attr(not(test), allow(dead_code))] // Injection seam is intentionally not exposed by the CLI.
pub fn attach_with_renderer(
    opts: &AttachOpts<'_>,
    renderer: &dyn crate::capture::Renderer,
) -> Result<Value> {
    attach_impl(opts, Some(renderer))
}
fn attach_impl(
    opts: &AttachOpts<'_>,
    renderer: Option<&dyn crate::capture::Renderer>,
) -> Result<Value> {
    let arm = match opts.arm {
        "a" | "A" => "a",
        "b" | "B" => "b",
        _ => bail!("arm 只能为 a/b"),
    };
    let mut conn = open_db(opts.db)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let experiment = load_experiment(&tx, opts.experiment_id)?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM artifact WHERE experiment_id=?1 AND arm=?2)",
        params![opts.experiment_id, arm],
        |r| r.get(0),
    )?;
    if exists {
        bail!("每臂仅可 attach 一次；修改页面必须新建实验");
    }
    if experiment.manifest_hash.is_some() {
        bail!("盲评已封存，不能追加产物");
    }
    let original = std::fs::canonicalize(opts.html).context("读取原始 HTML 路径")?;
    let bytes = std::fs::read(&original)?;
    let source = std::str::from_utf8(&bytes).context("HTML 必须为 UTF-8")?;
    crate::capture::validate_self_contained(source)?;
    let report = artifact_report(source, &experiment, arm)?;
    std::fs::create_dir_all(opts.out_dir)?;
    let root = std::fs::canonicalize(opts.out_dir)?;
    let arm_dir = root.join(format!("experiment-{}-{arm}", opts.experiment_id));
    std::fs::create_dir(&arm_dir).context("产物归档目录必须是新目录；失败重试请另选 out-dir")?;
    let archive = arm_dir.join("page.html");
    write_new(&archive, &bytes)?;
    let screenshot = arm_dir.join("capture.png");
    let spec = capture_spec(&experiment.conditions)?;
    let captured = match renderer {
        Some(r) => crate::capture::capture_with(&archive, &screenshot, &spec, r)?,
        None => crate::capture::capture(&archive, &screenshot, &spec, opts.chrome)?,
    };
    if captured.page_sha256 != hash_bytes(&bytes)
        || crate::capture::sha256_file(&original)? != captured.page_sha256
    {
        bail!("捕获期间页面已改变，拒绝绑定证据");
    }
    validate_receipt(
        &captured.receipt,
        &experiment.conditions,
        &captured.page_sha256,
        &captured.screenshot_sha256,
    )?;
    if renderer.is_some()
        && (captured.receipt["verified"] == true
            || captured.receipt["renderer"]["kind"] == "chrome")
    {
        bail!("注入 renderer 不得标记为可信 Chrome 捕获");
    }
    let receipt_path = arm_dir.join("capture-receipt.json");
    let receipt_json = captured.receipt.to_string();
    write_new(&receipt_path, receipt_json.as_bytes())?;
    let receipt_hash = hash_bytes(receipt_json.as_bytes());
    tx.execute("INSERT INTO artifact(experiment_id,arm,original_path,archived_html_path,screenshot_path,
        page_sha256,screenshot_sha256,capture_receipt_json,capture_receipt_path,capture_receipt_sha256,
        capture_conditions_hash,content_status,content_report_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        params![opts.experiment_id,arm,original.to_str().context("原路径不是 UTF-8")?,archive.to_str().context("归档路径不是 UTF-8")?,
            screenshot.to_str().context("截图路径不是 UTF-8")?,captured.page_sha256,captured.screenshot_sha256,
            receipt_json,receipt_path.to_str().context("回执路径不是 UTF-8")?,receipt_hash,experiment.conditions_hash,
            report["status"].as_str().unwrap(),report.to_string()])?;
    let artifact_id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(
        json!({"artifact_id":artifact_id,"experiment_id":opts.experiment_id,"arm":arm,
        "original_path":original,"archived_html_path":archive,"screenshot_path":screenshot,
        "page_sha256":captured.page_sha256,"screenshot_sha256":captured.screenshot_sha256,
        "capture_receipt":captured.receipt,"capture_receipt_path":receipt_path,"content_report":report,
        "note":"静态内容核验不是视觉美观评价，也不证明 CSS 布局下所有文案真实可见"}),
    )
}
pub fn blind(opts: &BlindOpts<'_>) -> Result<Value> {
    let mut conn = open_db(opts.db)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let experiment = load_experiment(&tx, opts.experiment_id)?;
    let artifacts = verified_pair(&tx, opts.experiment_id, &experiment)?;
    let manifest = public_manifest(&experiment, &artifacts)?;
    let manifest_json = manifest.to_string();
    let manifest_hash = hash_bytes(manifest_json.as_bytes());
    if experiment.manifest_hash.is_some() {
        verify_manifest(&experiment, &artifacts)?;
        verify_public_exports(&tx, opts.experiment_id, &experiment, &artifacts)?;
    } else {
        tx.execute("UPDATE experiment SET manifest_json=?1,manifest_hash=?2 WHERE id=?3 AND manifest_hash IS NULL",
            params![manifest_json,manifest_hash,opts.experiment_id])?;
    }
    prepare_public_dir(opts.out_dir)?;
    let mut files = Vec::new();
    for label in ["A", "B"] {
        let artifact = mapped_artifact(&experiment, &artifacts, label)?;
        let bytes = read_regular(&artifact.screenshot)?;
        let path = opts.out_dir.join(format!("{label}.png"));
        export_unchanged(&path, &bytes)?;
        files.push(path);
    }
    let manifest_path = opts.out_dir.join("manifest.json");
    let hash_path = opts.out_dir.join("manifest.sha256");
    export_unchanged(&manifest_path, manifest_json.as_bytes())?;
    export_unchanged(&hash_path, format!("{manifest_hash}\n").as_bytes())?;
    files.push(manifest_path);
    files.push(hash_path);
    let exports_raw: String = tx.query_row(
        "SELECT public_exports_json FROM experiment WHERE id=?1",
        [opts.experiment_id],
        |r| r.get(0),
    )?;
    let mut exports: Vec<std::path::PathBuf> = serde_json::from_str(&exports_raw)?;
    let canonical_dir = std::fs::canonicalize(opts.out_dir)?;
    if !exports.contains(&canonical_dir) {
        exports.push(canonical_dir);
        tx.execute(
            "UPDATE experiment SET public_exports_json=?1 WHERE id=?2",
            params![serde_json::to_string(&exports)?, opts.experiment_id],
        )?;
    }
    // Recheck bytes after export as well: the manifest must bind exactly what was copied.
    verified_pair(&tx, opts.experiment_id, &experiment)?;
    tx.commit()?;
    Ok(
        json!({"manifest_hash":manifest_hash,"manifest":manifest,"files":files,
        "note":"仅截图匿名包；A/B 内部映射不导出。无截图不允许回退文字判断"}),
    )
}
pub fn record(opts: &RecordOpts<'_>) -> Result<Value> {
    let source = if opts.source.is_empty() {
        "demo"
    } else {
        opts.source
    };
    if !["human", "demo", "ai"].contains(&source) {
        bail!("source 只能为 human/demo/ai，默认 demo");
    }
    let reviewer = opts.reviewer.map(str::trim).filter(|s| !s.is_empty());
    if source == "human" && reviewer.is_none() {
        bail!("human 评价必须有非空 reviewer");
    }
    if opts.dimension != "visual" {
        bail!("仅支持独立 visual 维度；static_check/text_spec/judge 分数不能混入");
    }
    if !["A", "B", "tie", "abstain"].contains(&opts.winner) {
        bail!("winner 只能为 A/B/tie/abstain");
    }
    let mut conn = open_db(opts.db)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let experiment = load_experiment(&tx, opts.experiment_id)?;
    let artifacts = verified_pair(&tx, opts.experiment_id, &experiment)?;
    let (_, manifest_hash) = verify_manifest(&experiment, &artifacts)?;
    if manifest_hash != opts.manifest_hash {
        bail!("提交的 manifest hash 不匹配冻结匿名证据");
    }
    verify_public_exports(&tx, opts.experiment_id, &experiment, &artifacts)?;
    tx.execute("INSERT INTO artifact_evaluation(experiment_id,manifest_hash,source,reviewer,dimension,winner,note)
        VALUES(?1,?2,?3,?4,'visual',?5,?6)",params![opts.experiment_id,manifest_hash,source,reviewer,opts.winner,opts.note])?;
    let evaluation_id = tx.last_insert_rowid();
    let evidence_eligible = generator_known(&experiment.conditions)
        && artifacts.iter().all(|a| {
            a.receipt["verified"] == true
                && a.receipt["renderer"]["kind"] == "chrome"
                && a.report["static_checks"]["passed"] == true
        });
    tx.commit()?;
    Ok(
        json!({"evaluation_id":evaluation_id,"source":source,"reviewer":reviewer,"dimension":"visual","winner":opts.winner,
        "manifest_hash":manifest_hash,"formal_eligible":source=="human" && evidence_eligible,
        "note":"评价追加保存；只有命名真人和已验证捕获进入正式视觉统计，demo/AI 不覆盖人评"}),
    )
}
struct VisualReview {
    manifest_hash: String,
    reviewer: String,
    winner: String,
}
#[derive(Default)]
struct RecipeStats {
    definition: Value,
    experiments: std::collections::BTreeSet<i64>,
    reviewers: std::collections::BTreeSet<String>,
    evaluations: usize,
    wins: usize,
    losses: usize,
    ties: usize,
    abstentions: usize,
    within_recipe_comparisons: usize,
}

pub fn stats(opts: &StatsOpts<'_>) -> Result<Value> {
    // A read-only connection and a consistent read transaction: reporting cannot
    // mutate legacy scores, calibration, experimental history or the user DB.
    let mut conn = connect(opts.db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let tx = conn.transaction()?;
    let mut history = json!({"human":0,"demo":0,"ai":0});
    {
        let mut stmt =
            tx.prepare("SELECT source,count(*) FROM artifact_evaluation GROUP BY source")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (source, count) = row?;
            history[&source] = json!(count);
        }
    }
    let mut latest = std::collections::BTreeMap::<i64, Vec<VisualReview>>::new();
    {
        // Filter source BEFORE choosing the latest review. A later demo/AI row
        // must never hide or overwrite the named human's independent judgment.
        let mut stmt = tx.prepare("SELECT e.experiment_id,e.manifest_hash,trim(e.reviewer),e.winner
            FROM artifact_evaluation e WHERE e.source='human' AND e.dimension='visual' AND length(trim(e.reviewer))>0
            AND e.id=(SELECT max(h.id) FROM artifact_evaluation h WHERE h.experiment_id=e.experiment_id
                AND h.source='human' AND h.dimension=e.dimension AND trim(h.reviewer)=trim(e.reviewer)) ORDER BY e.id")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            latest.entry(row.get(0)?).or_default().push(VisualReview {
                manifest_hash: row.get(1)?,
                reviewer: row.get(2)?,
                winner: row.get(3)?,
            });
        }
    }
    let latest_count: usize = latest.values().map(Vec::len).sum();
    let mut recipes = std::collections::BTreeMap::<(String, String), RecipeStats>::new();
    let mut experiments = std::collections::BTreeSet::<i64>::new();
    let mut non_abstaining_experiments = std::collections::BTreeSet::<i64>::new();
    let mut reviewers = std::collections::BTreeSet::<String>::new();
    let mut evaluations = 0usize;
    let mut preferences = 0usize;
    let mut ties = 0usize;
    let mut abstentions = 0usize;
    let mut excluded = Vec::new();
    for (id, reviews) in latest {
        let evidence = (|| -> Result<(Experiment, [Artifact; 2], String)> {
            let experiment = load_experiment(&tx, id)?;
            let artifacts = verified_pair(&tx, id, &experiment)?;
            let (_, hash) = verify_manifest(&experiment, &artifacts)?;
            verify_public_exports(&tx, id, &experiment, &artifacts)?;
            Ok((experiment, artifacts, hash))
        })();
        let (experiment, artifacts, manifest_hash) = match evidence {
            Ok(evidence) => evidence,
            Err(error) => {
                excluded.push(json!({"experiment_id":id,"evaluations":reviews.len(),"reason":"invalid_evidence","detail":error.to_string()}));
                continue;
            }
        };
        let reason = if !generator_known(&experiment.conditions) {
            Some("unknown_generator")
        } else if artifacts
            .iter()
            .any(|a| a.receipt["verified"] != true || a.receipt["renderer"]["kind"] != "chrome")
        {
            Some("unverified_capture")
        } else if artifacts
            .iter()
            .any(|a| a.report["static_checks"]["passed"] != true)
        {
            Some("static_check_failed")
        } else {
            None
        };
        if let Some(reason) = reason {
            excluded.push(json!({"experiment_id":id,"evaluations":reviews.len(),"reason":reason}));
            continue;
        }
        for review in reviews {
            if review.manifest_hash != manifest_hash {
                excluded
                    .push(json!({"experiment_id":id,"evaluations":1,"reason":"manifest_mismatch"}));
                continue;
            }
            experiments.insert(id);
            reviewers.insert(review.reviewer.clone());
            evaluations += 1;
            match review.winner.as_str() {
                "A" | "B" => {
                    preferences += 1;
                    non_abstaining_experiments.insert(id);
                }
                "tie" => {
                    ties += 1;
                    non_abstaining_experiments.insert(id);
                }
                "abstain" => abstentions += 1,
                _ => unreachable!("schema restricts winner"),
            }
            let winning_index = match review.winner.as_str() {
                "A" | "B" => Some(if experiment.mapping[&review.winner] == "a" {
                    0
                } else {
                    1
                }),
                _ => None,
            };
            let same_recipe = experiment.recipe_hashes[0] == experiment.recipe_hashes[1];
            for index in 0..if same_recipe { 1 } else { 2 } {
                let hash = &experiment.recipe_hashes[index];
                let aggregate = recipes
                    .entry((hash.clone(), experiment.conditions_hash.clone()))
                    .or_default();
                aggregate.definition = experiment.snapshots[index]["recipe_definition"].clone();
                aggregate.experiments.insert(id);
                aggregate.reviewers.insert(review.reviewer.clone());
                aggregate.evaluations += 1;
                if same_recipe {
                    aggregate.within_recipe_comparisons += 1;
                }
                match review.winner.as_str() {
                    "tie" => aggregate.ties += 1,
                    "abstain" => aggregate.abstentions += 1,
                    _ if !same_recipe => {
                        if winning_index == Some(index) {
                            aggregate.wins += 1;
                        } else {
                            aggregate.losses += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    let recipe_rows: Vec<Value> = recipes.into_iter().map(|((hash,conditions_hash),r)|json!({
        "recipe_definition_hash":hash,"conditions_hash":conditions_hash,"rubric_id":"artifact-visual-v1",
        "recipe_key":r.definition["key"],"recipe_definition":r.definition,
        "experiments":r.experiments.len(),"reviewers":r.reviewers.len(),"evaluations":r.evaluations,
        "wins":r.wins,"losses":r.losses,"ties":r.ties,"abstentions":r.abstentions,
        "within_recipe_comparisons":r.within_recipe_comparisons})).collect();
    tx.commit()?;
    Ok(
        json!({"formal_visual":{"experiments":experiments.len(),"reviewers":reviewers.len(),"evaluations":evaluations,
            "non_abstaining_experiments":non_abstaining_experiments.len(),"preferences":preferences,"ties":ties,"abstentions":abstentions,
            "generalizable_beauty_claim":false,"basis":"latest_named_human_screenshot_reviews"},
        "recipes":recipe_rows,"history":history,
        "excluded":{"non_human_history":history["demo"].as_i64().unwrap_or(0)+history["ai"].as_i64().unwrap_or(0),
            "superseded_human_evaluations":history["human"].as_u64().unwrap_or(0).saturating_sub(latest_count as u64),
            "latest_human_evaluations":latest_count-evaluations,"evidence":excluded},
        "limitations":["生成条件为操作者声明，不代表已独立验证生成器执行","内容证据为源码文本/素材覆盖，不证明完整 DOM 可见性或严格因果 AB",
            "规格或静态检查通过不能声称成品美观；静态检查不是 visual 分","n=1 不能声称普遍更美；统计仅描述已评审实验",
            "experiments 按实验去重；evaluations 为每实验/人/visual 的最新评价；abstain 不计偏好",
            "配方结果按 recipe_definition_hash × conditions_hash 分组，不混合不同生成器、viewport 或任务的胜负；overall 仅为总计",
             "同配方 hash 的双臂只计组内比较，不归因成该配方胜负；未改写文本评分或校准"]}),
    )
}

pub fn create(opts: &CreateOpts<'_>) -> Result<Value> {
    let mut conn = open_db(opts.db)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if opts.pill_a == opts.pill_b {
        bail!("创建实验需要两颗不同冻结丹药");
    }
    validate_content(opts.content)?;
    validate_conditions(opts.conditions)?;
    let mut snapshots = Vec::new();
    for id in [opts.pill_a, opts.pill_b] {
        let output: String = tx
            .query_row(
                "SELECT output_json FROM pill WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .with_context(|| format!("没有丹药 #{id}"))?;
        let snapshot: Value = serde_json::from_str(&output).context("pill output_json 无效")?;
        validate_snapshot(&snapshot)?;
        snapshots.push(snapshot);
    }
    let a_hash = hash_json(&snapshots[0]);
    let b_hash = hash_json(&snapshots[1]);
    if a_hash == b_hash {
        bail!("两颗丹药的完整冻结快照必须不同");
    }
    let recipe_a_hash = hash_json(&snapshots[0]["recipe_definition"]);
    let recipe_b_hash = hash_json(&snapshots[1]["recipe_definition"]);
    let content_hash = hash_json(opts.content);
    let conditions_hash = hash_json(opts.conditions);
    let swap: bool = tx.query_row("SELECT random() < 0", [], |r| r.get(0))?;
    let mapping = if swap {
        json!({"A":"b","B":"a"})
    } else {
        json!({"A":"a","B":"b"})
    };
    tx.execute("INSERT INTO experiment(content_json,content_hash,conditions_json,conditions_hash,
        pill_a_id,pill_b_id,pill_a_snapshot_json,pill_b_snapshot_json,pill_a_snapshot_hash,pill_b_snapshot_hash,
        recipe_a_hash,recipe_b_hash,blind_mapping_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        params![opts.content.to_string(),content_hash,opts.conditions.to_string(),conditions_hash,
            opts.pill_a,opts.pill_b,snapshots[0].to_string(),snapshots[1].to_string(),a_hash,b_hash,
            recipe_a_hash,recipe_b_hash,mapping.to_string()])?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(
        json!({"id":id,"content_hash":content_hash,"conditions_hash":conditions_hash,
        "arms":[{"arm":"a","pill_id":opts.pill_a,"snapshot_hash":a_hash,"recipe_hash":recipe_a_hash},
                 {"arm":"b","pill_id":opts.pill_b,"snapshot_hash":b_hash,"recipe_hash":recipe_b_hash}],
        "generator_known":generator_known(opts.conditions),
        "note":"冻结的是规格，不是成品美观证据；需要两臂截图和独立真人 visual 评价"}),
    )
}

struct Artifact {
    arm: String,
    original: std::path::PathBuf,
    archive: std::path::PathBuf,
    screenshot: std::path::PathBuf,
    page_hash: String,
    screenshot_hash: String,
    receipt: Value,
    receipt_path: std::path::PathBuf,
    receipt_hash: String,
    conditions_hash: String,
    content_status: String,
    report: Value,
}
fn read_regular(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("缺少证据文件 {}", path.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        bail!("证据必须为非 symlink 普通文件：{}", path.display());
    }
    std::fs::read(path).with_context(|| format!("读取证据 {}", path.display()))
}
fn verified_pair(conn: &Connection, id: i64, experiment: &Experiment) -> Result<[Artifact; 2]> {
    let mut stmt = conn.prepare("SELECT * FROM artifact WHERE experiment_id=?1 ORDER BY arm")?;
    let mut rows = stmt.query([id])?;
    let mut artifacts = Vec::new();
    while let Some(row) = rows.next()? {
        let json_column = |name: &str| -> Result<Value> {
            let raw: String = row.get(name)?;
            Ok(serde_json::from_str(&raw)?)
        };
        let path_column = |name: &str| -> Result<std::path::PathBuf> {
            let raw: String = row.get(name)?;
            Ok(raw.into())
        };
        artifacts.push(Artifact {
            arm: row.get("arm")?,
            original: path_column("original_path")?,
            archive: path_column("archived_html_path")?,
            screenshot: path_column("screenshot_path")?,
            page_hash: row.get("page_sha256")?,
            screenshot_hash: row.get("screenshot_sha256")?,
            receipt: json_column("capture_receipt_json")?,
            receipt_path: path_column("capture_receipt_path")?,
            receipt_hash: row.get("capture_receipt_sha256")?,
            conditions_hash: row.get("capture_conditions_hash")?,
            content_status: row.get("content_status")?,
            report: json_column("content_report_json")?,
        });
    }
    if artifacts.len() != 2 || artifacts[0].arm != "a" || artifacts[1].arm != "b" {
        bail!("两臂均有页面和截图后才开放盲评，不能回退文字");
    }
    for a in &artifacts {
        let source = read_regular(&a.archive)?;
        if hash_bytes(&source) != a.page_hash
            || hash_bytes(&read_regular(&a.original)?) != a.page_hash
            || hash_bytes(&read_regular(&a.screenshot)?) != a.screenshot_hash
            || hash_bytes(&read_regular(&a.receipt_path)?) != a.receipt_hash
            || hash_json(&a.receipt) != a.receipt_hash
        {
            bail!("页面/截图/捕获回执 hash 已变；必须新建实验，不能覆盖旧证据");
        }
        validate_receipt(
            &a.receipt,
            &experiment.conditions,
            &a.page_hash,
            &a.screenshot_hash,
        )?;
        if a.conditions_hash != experiment.conditions_hash {
            bail!("捕获条件与冻结实验条件不一致");
        }
        let report = artifact_report(std::str::from_utf8(&source)?, experiment, &a.arm)?;
        if a.content_status != "compliant" || a.report != report || report["status"] != "compliant"
        {
            bail!("共同内容缺文/素材不匹配或静态核验已变，不属于受控截图对比");
        }
    }
    if artifacts[0].receipt["profile_sha256"] != artifacts[1].receipt["profile_sha256"] {
        bail!("两臂捕获 renderer/profile 条件不一致");
    }
    artifacts
        .try_into()
        .map_err(|_| anyhow::anyhow!("两臂证据不完整"))
}
fn mapped_artifact<'a>(
    experiment: &Experiment,
    artifacts: &'a [Artifact; 2],
    label: &str,
) -> Result<&'a Artifact> {
    let arm = experiment.mapping[label].as_str().context("匿名分配缺失")?;
    artifacts
        .iter()
        .find(|a| a.arm == arm)
        .context("匿名分配缺产物")
}
fn public_manifest(experiment: &Experiment, artifacts: &[Artifact; 2]) -> Result<Value> {
    let a = mapped_artifact(experiment, artifacts, "A")?;
    let b = mapped_artifact(experiment, artifacts, "B")?;
    Ok(json!({"manifest_version":1,"dimension":"visual",
        "rubric":{"id":"artifact-visual-v1","instruction":"仅依据两张截图独立评价整体视觉质量；不推测来源；证据不足可弃权",
            "criteria":["信息层级与可读性","布局节奏与对齐","色彩和字形协调","细节完成度"],
            "choices":["A","B","tie","abstain"]},
        "capture":{"viewport":experiment.conditions["viewport"],"theme":experiment.conditions["theme"]},
        "screenshots":[{"label":"A","file":"A.png","sha256":a.screenshot_hash},
                       {"label":"B","file":"B.png","sha256":b.screenshot_hash}]}))
}
fn verify_manifest(experiment: &Experiment, artifacts: &[Artifact; 2]) -> Result<(Value, String)> {
    let raw = experiment
        .manifest_json
        .as_deref()
        .context("请先导出盲评 manifest")?;
    let hash = experiment
        .manifest_hash
        .as_deref()
        .context("manifest hash 缺失")?;
    let expected = public_manifest(experiment, artifacts)?;
    // Compare serialized bytes, not Value's string-scalar equality.
    let expected_json = expected.to_string();
    if hash_bytes(raw.as_bytes()) != hash || expected_json != raw {
        bail!("manifest hash/匿名证据不匹配或已被篡改");
    }
    Ok((expected, hash.to_owned()))
}
fn prepare_public_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    validate_public_dir(path)
}
fn validate_public_dir(path: &Path) -> Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name();
        if !["A.png", "B.png", "manifest.json", "manifest.sha256"]
            .iter()
            .any(|expected| name == std::ffi::OsStr::new(expected))
        {
            bail!("匿名包目录含其他文件；为防材料/映射泄露，请指定新的空目录");
        }
        let meta = std::fs::symlink_metadata(entry.path())?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            bail!("匿名包不能含目录或 symlink");
        }
    }
    Ok(())
}
fn verify_public_exports(
    conn: &Connection,
    id: i64,
    experiment: &Experiment,
    artifacts: &[Artifact; 2],
) -> Result<()> {
    let raw: String = conn.query_row(
        "SELECT public_exports_json FROM experiment WHERE id=?1",
        [id],
        |r| r.get(0),
    )?;
    let exports: Vec<std::path::PathBuf> = serde_json::from_str(&raw)?;
    if exports.is_empty() {
        bail!("没有持久化的匿名包证据，先导出 blind");
    }
    let (manifest, hash) = verify_manifest(experiment, artifacts)?;
    for dir in exports {
        validate_public_dir(&dir)?;
        if read_regular(&dir.join("manifest.json"))? != manifest.to_string().as_bytes()
            || read_regular(&dir.join("manifest.sha256"))? != format!("{hash}\n").as_bytes()
        {
            bail!("公开 manifest/hash 文件已被篡改");
        }
        for label in ["A", "B"] {
            let artifact = mapped_artifact(experiment, artifacts, label)?;
            if hash_bytes(&read_regular(&dir.join(format!("{label}.png")))?)
                != artifact.screenshot_hash
            {
                bail!("公开 A/B 截图已被篡改");
            }
        }
    }
    Ok(())
}
fn export_unchanged(path: &Path, bytes: &[u8]) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => {
            if read_regular(path)? != bytes {
                bail!("已有匿名文件内容不同，拒绝覆盖：{}", path.display());
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => write_new(path, bytes),
        Err(error) => Err(error.into()),
    }
}

struct Experiment {
    content: Value,
    conditions: Value,
    conditions_hash: String,
    snapshots: [Value; 2],
    recipe_hashes: [String; 2],
    mapping: Value,
    manifest_json: Option<String>,
    manifest_hash: Option<String>,
}
fn load_experiment(conn: &Connection, id: i64) -> Result<Experiment> {
    let mut stmt = conn.prepare("SELECT * FROM experiment WHERE id=?1")?;
    let mut rows = stmt.query([id])?;
    let row = rows.next()?.with_context(|| format!("没有实验 #{id}"))?;
    let json_column = |name: &str| -> Result<Value> {
        let raw: String = row.get(name)?;
        serde_json::from_str(&raw).with_context(|| format!("experiment.{name} JSON 无效"))
    };
    let content = json_column("content_json")?;
    let conditions = json_column("conditions_json")?;
    let conditions_hash: String = row.get("conditions_hash")?;
    let content_hash: String = row.get("content_hash")?;
    let snapshots = [
        json_column("pill_a_snapshot_json")?,
        json_column("pill_b_snapshot_json")?,
    ];
    let snapshot_hashes: [String; 2] = [
        row.get("pill_a_snapshot_hash")?,
        row.get("pill_b_snapshot_hash")?,
    ];
    let recipe_hashes: [String; 2] = [row.get("recipe_a_hash")?, row.get("recipe_b_hash")?];
    if hash_json(&content) != content_hash || hash_json(&conditions) != conditions_hash {
        bail!("实验共同输入快照 hash 已变");
    }
    for i in 0..2 {
        validate_snapshot(&snapshots[i])?;
        if hash_json(&snapshots[i]) != snapshot_hashes[i]
            || hash_json(&snapshots[i]["recipe_definition"]) != recipe_hashes[i]
        {
            bail!("丹药或完整配方快照 hash 已变");
        }
    }
    validate_content(&content)?;
    validate_conditions(&conditions)?;
    let mapping = json_column("blind_mapping_json")?;
    if mapping != json!({"A":"a","B":"b"}) && mapping != json!({"A":"b","B":"a"}) {
        bail!("内部匿名映射无效");
    }
    Ok(Experiment {
        content,
        conditions,
        conditions_hash,
        snapshots,
        recipe_hashes,
        mapping,
        manifest_json: row.get("manifest_json")?,
        manifest_hash: row.get("manifest_hash")?,
    })
}
fn capture_spec(conditions: &Value) -> Result<crate::capture::CaptureSpec> {
    validate_conditions(conditions)?;
    Ok(crate::capture::CaptureSpec {
        width: conditions["viewport"]["width"].as_u64().unwrap() as u32,
        height: conditions["viewport"]["height"].as_u64().unwrap() as u32,
    })
}
fn validate_receipt(
    receipt: &Value,
    conditions: &Value,
    page_hash: &str,
    screenshot_hash: &str,
) -> Result<()> {
    if receipt["page_sha256"] != page_hash
        || receipt["screenshot_sha256"] != screenshot_hash
        || receipt["viewport"] != conditions["viewport"]
        || receipt["theme"] != conditions["theme"]
        || !nonempty(&receipt["profile_sha256"])
        || receipt["policy_version"] != crate::capture::POLICY_VERSION
    {
        bail!("捕获回执页面/截图 hash 或 viewport/theme/profile/policy 不匹配");
    }
    Ok(())
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("只允许写新文件：{}", path.display()))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn artifact_report(html: &str, experiment: &Experiment, arm: &str) -> Result<Value> {
    let mut report = content_report(html, &experiment.content)?;
    let index = match arm {
        "a" => 0,
        "b" => 1,
        _ => bail!("无效内部 arm"),
    };
    report["static_checks"] = crate::forge::static_report(&experiment.snapshots[index], html)?;
    Ok(report)
}
fn content_report(html: &str, content: &Value) -> Result<Value> {
    let projection = crate::capture::text_projection(html)?;
    let normalize = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let projection = normalize(&projection);
    let missing: Vec<&Value> = content["strings"]
        .as_array()
        .context("缺 strings")?
        .iter()
        .filter(|s| !projection.contains(&normalize(s.as_str().unwrap_or(""))))
        .collect();
    // Keep the first slice narrow: images must be explicit <img src=data:...>.
    // No CSS/background assets, even if inline, can silently change the shared task.
    let compact = html
        .split_whitespace()
        .collect::<String>()
        .to_ascii_lowercase();
    let unsupported_media = compact.contains("url(") || compact.contains("image-set(");
    let actual_media = image_hashes(html)?;
    let mut expected_media: Vec<String> = content
        .get("media")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|m| m["sha256"].as_str().unwrap_or("").to_owned())
        .collect();
    expected_media.sort();
    let compliant = missing.is_empty() && actual_media == expected_media && !unsupported_media;
    Ok(
        json!({"status":if compliant { "compliant" } else { "noncompliant" },"basis":"html_text_projection",
        "missing_strings":missing,"expected_media_sha256":expected_media,"actual_media_sha256":actual_media,
        "unsupported_css_media":unsupported_media,"visibility_verified":false,
        "note":"核验静态 body 文本和显式 img 素材覆盖；不证明 CSS 下真实可见性或视觉美观"}),
    )
}
fn image_hashes(html: &str) -> Result<Vec<String>> {
    let mut rest = html;
    let mut hashes = Vec::new();
    while let Some(start) = rest.find('<') {
        rest = &rest[start..];
        if rest.starts_with("<!--") {
            let end = rest.find("-->").context("HTML comment 未闭合")?;
            rest = &rest[end + 3..];
            continue;
        }
        // capture validation rejects angle brackets in attributes, so this boundary is unambiguous.
        let end = rest.find('>').context("HTML tag 未闭合")?;
        let tag = &rest[1..end];
        rest = &rest[end + 1..];
        let name_end = tag
            .find(|c: char| c.is_ascii_whitespace() || c == '/')
            .unwrap_or(tag.len());
        let name = tag[..name_end].to_ascii_lowercase();
        if name == "style" {
            if let Some(end) = rest.to_ascii_lowercase().find("</style>") {
                rest = &rest[end + 8..];
            }
            continue;
        }
        if name != "img" {
            continue;
        }
        let mut attrs = &tag[name_end..];
        let mut source = None;
        while !attrs.trim().is_empty() && attrs.trim() != "/" {
            attrs = attrs.trim_start();
            let end = attrs
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
                .unwrap_or(attrs.len());
            if end == 0 {
                bail!("img 属性解析失败");
            }
            let key = &attrs[..end];
            attrs = attrs[end..].trim_start();
            let mut value = "";
            if let Some(after) = attrs.strip_prefix('=') {
                attrs = after.trim_start();
                if let Some(quote) = attrs.chars().next().filter(|c| *c == '\'' || *c == '"') {
                    let end = attrs[1..].find(quote).context("img 属性未闭合")? + 1;
                    value = &attrs[1..end];
                    attrs = &attrs[end + 1..];
                } else {
                    let end = attrs.find(char::is_whitespace).unwrap_or(attrs.len());
                    value = &attrs[..end];
                    attrs = &attrs[end..];
                }
            }
            if key.eq_ignore_ascii_case("src") {
                source = Some(value);
            }
        }
        hashes.push(hash_bytes(&media_bytes(
            source.context("img 缺 src，无法核验共同素材")?,
        )?));
    }
    hashes.sort();
    Ok(hashes)
}

fn open_db(path: &str) -> Result<Connection> {
    connect(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
}
fn connect(path: &str, flags: OpenFlags) -> Result<Connection> {
    let conn = Connection::open_with_flags(path, flags)?;
    conn.busy_timeout(Duration::from_secs(10))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    let version: String = conn.query_row(
        "SELECT value FROM schema_meta WHERE key='schema_version'",
        [],
        |r| r.get(0),
    )?;
    if !version
        .parse::<u32>()
        .is_ok_and(|v| (24..=crate::db::LATEST_SCHEMA).contains(&v))
    {
        bail!("experiment 需要 schema 24 或兼容的新版本；请先显式迁移，不会自动修改旧库");
    }
    Ok(conn)
}
fn hash_json(value: &Value) -> String {
    hash_bytes(value.to_string().as_bytes())
}
fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn nonempty(value: &Value) -> bool {
    value.as_str().is_some_and(|s| !s.trim().is_empty())
}
fn validate_snapshot(snapshot: &Value) -> Result<()> {
    if !crate::forge::frozen_snapshot(snapshot)? {
        bail!("创建实验必须使用完整冻结丹药，不接受旧 pill 或 legacy fallback");
    }
    if !nonempty(&snapshot["recipe_definition"]["key"])
        || !nonempty(&snapshot["prompt"])
        || snapshot["components"]
            .as_array()
            .is_none_or(|a| a.is_empty())
        || snapshot["recipe_definition"]["components"]
            .as_array()
            .is_none_or(|a| a.is_empty())
    {
        bail!("冻结丹药缺配方定义、组件或 prompt");
    }
    for component in snapshot["components"].as_array().unwrap() {
        if !nonempty(&component["dimension"])
            || !nonempty(&component["slot"])
            || !nonempty(&component["hash"])
            || !nonempty(&component["technique_version"])
            || component["content"].is_null()
        {
            bail!("冻结丹药组件版本、来源或内容不完整");
        }
    }
    Ok(())
}
fn validate_content(content: &Value) -> Result<()> {
    let strings = content["strings"]
        .as_array()
        .context("content.strings 必须是非空文本数组")?;
    if strings.is_empty() || strings.iter().any(|s| !nonempty(s)) {
        bail!("content.strings 必须是非空文本数组");
    }
    if let Some(media) = content.get("media") {
        for item in media.as_array().context("content.media 必须是数组")? {
            let uri = item["data_uri"]
                .as_str()
                .context("media 仅支持自包含 data_uri")?;
            let bytes = media_bytes(uri)?;
            if item["sha256"].as_str() != Some(hash_bytes(&bytes).as_str()) {
                bail!("media.sha256 不匹配");
            }
        }
    }
    Ok(())
}
fn media_bytes(uri: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    let (prefix, data) = uri.split_once(',').context("media data URI 无效")?;
    if ![
        "data:image/png;base64",
        "data:image/jpeg;base64",
        "data:image/webp;base64",
    ]
    .contains(&prefix)
    {
        bail!("media 仅支持 base64 PNG/JPEG/WebP");
    }
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .context("media base64 无效")
}
fn validate_conditions(conditions: &Value) -> Result<()> {
    for dim in ["width", "height"] {
        let size = conditions["viewport"][dim]
            .as_u64()
            .context("viewport 需要正整数 width/height")?;
        if !(1..=10_000).contains(&size) {
            bail!("viewport 超出支持范围 1..10000");
        }
    }
    if conditions["theme"] != "light"
        || !nonempty(&conditions["task"])
        || !conditions["generator"].is_object()
        || !nonempty(&conditions["generator"]["kind"])
    {
        bail!("conditions 需要 theme=light、非配方共同 task 和 generator.kind");
    }
    Ok(())
}
fn generator_known(conditions: &Value) -> bool {
    let g = &conditions["generator"];
    let known = |v: &Value| {
        nonempty(v)
            && !["unknown", "unverified", "mock", "unspecified"]
                .contains(&v.as_str().unwrap().trim().to_ascii_lowercase().as_str())
    };
    match g["kind"].as_str().unwrap_or("") {
        "human" => known(&g["version"]),
        "coding-agent" | "ai" | "model" => known(&g["model"]) && known(&g["version"]),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{Connection, params};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: PathBuf,
        db: String,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "ie-experiment-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).unwrap();
            let db = root.join("test.sqlite").to_str().unwrap().to_owned();
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch("CREATE TABLE schema_meta(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE pill(id INTEGER PRIMARY KEY,output_json TEXT NOT NULL);").unwrap();
            conn.execute_batch(include_str!(
                "../../../../docs/design/inspiration/migrations/023-artifact-experiments.sql"
            ))
            .unwrap();
            Self { root, db }
        }
        fn pill(&self, id: i64, snapshot: Value) {
            Connection::open(&self.db)
                .unwrap()
                .execute(
                    "INSERT INTO pill(id,output_json) VALUES(?1,?2)",
                    params![id, snapshot.to_string()],
                )
                .unwrap();
        }
        fn create(&self) -> Result<Value> {
            create(&CreateOpts {
                db: &self.db,
                pill_a: 1,
                pill_b: 2,
                content: &json!({"strings":["共同标题","共同正文"]}),
                conditions: &json!({"viewport":{"width":800,"height":600},"theme":"light","generator":{"kind":"human","version":"fixture-v1"},"task":"同一介绍页"}),
            })
        }
    }

    fn frozen(key: &str) -> Value {
        let content = json!({"roles":{"background":"#FFFFFF","text":"#000000"},"colors":[{"hex":"#FFFFFF"},{"hex":"#000000"}]});
        use sha2::{Digest, Sha256};
        json!({"snapshot_version":1,"recipe_definition":{"key":key,"components":[{"dimension":"palette","slot":"base","ratio":1.0}],"combination_prompt":"layout"},"prompt":"Frozen design specification","components":[{"dimension":"palette","slot":"base","extract_revision":1,"technique_version":"1","hash":"frozen-extract-hash","content_hash":format!("{:x}",Sha256::digest(content.to_string().as_bytes())),"content":content}]})
    }

    #[test]
    fn create_freezes_distinct_v1_snapshots_and_hashes_the_whole_recipe_definition() {
        let f = Fixture::new();
        let a = frozen("recipe.a");
        let b = frozen("recipe.b");
        f.pill(1, a.clone());
        f.pill(2, b.clone());
        let created = f.create().unwrap();
        assert!(created["id"].as_i64().unwrap_or(0) > 0);
        assert_eq!(created["arms"][0]["snapshot_hash"], sha(&a));
        assert_eq!(
            created["arms"][0]["recipe_hash"],
            sha(&a["recipe_definition"])
        );
        assert_eq!(created["arms"][1]["snapshot_hash"], sha(&b));
    }

    fn sha(v: &Value) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(v.to_string().as_bytes()))
    }

    struct MockRenderer;
    impl crate::capture::Renderer for MockRenderer {
        fn render(
            &self,
            _html: &Path,
            screenshot: &Path,
            spec: &crate::capture::CaptureSpec,
        ) -> Result<Value> {
            image::RgbImage::from_pixel(spec.width, spec.height, image::Rgb([250, 250, 250]))
                .save(screenshot)?;
            Ok(json!({"kind":"chrome","version":"spoofed test renderer"}))
        }
    }
    impl Fixture {
        fn ready(&self) -> i64 {
            self.pill(1, frozen("recipe.a"));
            self.pill(2, frozen("recipe.b"));
            self.create().unwrap()["id"].as_i64().unwrap()
        }
        fn html(&self, name: &str, body: &str) -> PathBuf {
            let path = self.root.join(name);
            std::fs::write(&path,format!("<!doctype html><html><head><style>body{{background:#FFFFFF;color:#000000}}h1{{font-size:32px}}p{{font-size:16px}}</style></head><body>{body}</body></html>")).unwrap();
            path
        }
        fn attach(&self, id: i64, arm: &str, html: &Path) -> Result<Value> {
            attach_with_renderer(
                &AttachOpts {
                    db: &self.db,
                    experiment_id: id,
                    arm,
                    html,
                    out_dir: &self.root.join("archive"),
                    chrome: None,
                },
                &MockRenderer,
            )
        }
    }

    #[test]
    fn attach_archives_and_engine_captures_once_without_trusting_injected_renderer_identity() {
        let f = Fixture::new();
        let id = f.ready();
        let html = f.html("first.html", "<h1>共同标题</h1><p>共同正文</p>");
        let a = f.attach(id, "a", &html).unwrap();
        assert_eq!(
            a["page_sha256"],
            crate::capture::sha256_file(&html).unwrap()
        );
        assert_eq!(a["content_report"]["status"], "compliant");
        assert_eq!(a["content_report"]["basis"], "html_text_projection");
        assert_ne!(a["capture_receipt"]["renderer"]["kind"], "chrome");
        assert_eq!(
            std::fs::read(a["archived_html_path"].as_str().unwrap()).unwrap(),
            std::fs::read(&html).unwrap()
        );
        assert!(
            f.attach(id, "a", &html).is_err(),
            "existing arm cannot be overwritten"
        );
    }

    #[test]
    fn blind_accepts_unchanged_fractional_static_measurements_after_persistence() {
        let f = Fixture::new();
        f.pill(1, frozen("recipe.fractional-a"));
        f.pill(2, frozen("recipe.fractional-b"));
        let created = create(&CreateOpts {
            db: &f.db,
            pill_a: 1,
            pill_b: 2,
            content: &json!({"strings":["共同标题","共同正文"]}),
            conditions: &json!({"viewport":{"width":160,"height":120},"theme":"light",
                "task":"unchanged fractional static measurements",
                "generator":{"kind":"fixture","version":"mock-only"}}),
        })
        .unwrap();
        let id = created["id"].as_i64().unwrap();
        let body = "<style>small{font-size:13px}h1{font-size:48px}</style><small>Mock fixture only</small><h1>共同标题</h1><p>共同正文</p>";
        for arm in ["a", "b"] {
            let html = f.html(&format!("fractional-{arm}.html"), body);
            let attached = f.attach(id, arm, &html).unwrap();
            assert_eq!(attached["content_report"]["status"], "compliant");
            assert_eq!(attached["content_report"]["static_checks"]["passed"], true);
            assert_eq!(
                attached["content_report"]["static_checks"]["measurements"]["type_ratio"]
                    .as_f64()
                    .unwrap()
                    .to_bits(),
                (48.0_f64 / 13.0).to_bits()
            );
            assert_ne!(attached["capture_receipt"]["renderer"]["kind"], "chrome");
        }
        // This real public round trip reloads the stored report and recomputes
        // source measurements. No tolerance or skipped evidence checks is allowed.
        let exported = blind(&BlindOpts {
            db: &f.db,
            experiment_id: id,
            out_dir: &f.root.join("fractional-public"),
        })
        .expect("unchanged compliant artifacts must survive exact JSON persistence");
        assert_eq!(exported["manifest_hash"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn blind_exports_only_stable_anonymous_screenshots_and_a_hashed_shared_rubric() {
        let f = Fixture::new();
        let id = f.ready();
        let a = f.html("recipe-a-secret.html", "<h1>共同标题</h1><p>共同正文</p>");
        let b = f.html(
            "recipe-b-secret.html",
            "<main><h2>共同标题</h2><p>共同正文</p></main>",
        );
        f.attach(id, "a", &a).unwrap();
        f.attach(id, "b", &b).unwrap();
        let out = f.root.join("public");
        let first = blind(&BlindOpts {
            db: &f.db,
            experiment_id: id,
            out_dir: &out,
        })
        .unwrap();
        let second = blind(&BlindOpts {
            db: &f.db,
            experiment_id: id,
            out_dir: &f.root.join("public-again"),
        })
        .unwrap();
        assert_eq!(first["manifest"], second["manifest"]);
        assert_eq!(first["manifest_hash"], second["manifest_hash"]);
        let bytes = std::fs::read(out.join("manifest.json")).unwrap();
        assert_eq!(first["manifest_hash"], hash_bytes(&bytes));
        let public = String::from_utf8(bytes).unwrap();
        for secret in [
            "pill",
            "recipe",
            "material",
            "score",
            "secret",
            "original_path",
        ] {
            assert!(!public.contains(secret), "leaked {secret}");
        }
        for name in ["A.png", "B.png", "manifest.json", "manifest.sha256"] {
            assert_eq!(
                std::fs::read(out.join(name)).unwrap(),
                std::fs::read(f.root.join("public-again").join(name)).unwrap()
            );
        }
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 4);
    }

    impl Fixture {
        fn pair(&self) -> (i64, String) {
            let id = self.ready();
            let a = self.html("left.html", "<h1>共同标题</h1><p>共同正文</p>");
            let b = self.html("right.html", "<h2>共同标题</h2><p>共同正文</p>");
            self.attach(id, "a", &a).unwrap();
            self.attach(id, "b", &b).unwrap();
            let result = blind(&BlindOpts {
                db: &self.db,
                experiment_id: id,
                out_dir: &self.root.join("public"),
            })
            .unwrap();
            (id, result["manifest_hash"].as_str().unwrap().to_owned())
        }
        fn record(
            &self,
            id: i64,
            hash: &str,
            source: &str,
            reviewer: Option<&str>,
            winner: &str,
        ) -> Result<Value> {
            record(&RecordOpts {
                db: &self.db,
                experiment_id: id,
                manifest_hash: hash,
                source,
                reviewer,
                dimension: "visual",
                winner,
                note: Some("synthetic test fixture only"),
            })
        }
    }

    #[test]
    fn record_is_manifest_bound_append_only_and_requires_named_human_for_visual() {
        let f = Fixture::new();
        let (id, hash) = f.pair();
        assert!(f.record(id, "wrong-hash", "demo", None, "A").is_err());
        assert!(f.record(id, &hash, "human", None, "A").is_err());
        assert!(f.record(id, &hash, "human", Some("  "), "A").is_err());
        let demo = f.record(id, &hash, "demo", None, "A").unwrap();
        assert_eq!(demo["source"], "demo");
        assert_eq!(demo["formal_eligible"], false);
        let human = f
            .record(id, &hash, "human", Some("fixture-reviewer"), "B")
            .unwrap();
        assert!(human["evaluation_id"].as_i64().unwrap() > demo["evaluation_id"].as_i64().unwrap());
        assert_eq!(
            human["formal_eligible"], false,
            "mock evidence never enters formal statistics"
        );
        let invalid_dimension = RecordOpts {
            db: &f.db,
            experiment_id: id,
            manifest_hash: &hash,
            source: "human",
            reviewer: Some("fixture-reviewer"),
            dimension: "static_check",
            winner: "A",
            note: None,
        };
        assert!(
            record(&invalid_dimension).is_err(),
            "static/text checks cannot be recorded as visual"
        );
    }

    #[test]
    fn record_rejects_tampered_public_package_not_just_internal_archive() {
        let f = Fixture::new();
        let (id, hash) = f.pair();
        std::fs::write(f.root.join("public/A.png"), b"tampered exported evidence").unwrap();
        assert!(
            f.record(id, &hash, "demo", None, "A").is_err(),
            "the screenshots actually exported to reviewers must also remain intact"
        );
    }

    #[test]
    fn stats_excludes_mock_capture_even_for_named_human_and_keeps_demo_ai_separate() {
        let f = Fixture::new();
        let (id, hash) = f.pair();
        f.record(id, &hash, "demo", None, "A").unwrap();
        f.record(id, &hash, "ai", Some("model-fixture"), "B")
            .unwrap();
        f.record(id, &hash, "human", Some("fixture-reviewer"), "A")
            .unwrap();
        let report = stats(&StatsOpts { db: &f.db }).unwrap();
        assert_eq!(report["formal_visual"]["experiments"], 0);
        assert_eq!(report["formal_visual"]["reviewers"], 0);
        assert_eq!(report["formal_visual"]["evaluations"], 0);
        assert_eq!(report["history"]["demo"], 1);
        assert_eq!(report["history"]["ai"], 1);
        assert_eq!(report["history"]["human"], 1);
        assert!(report["recipes"].as_array().unwrap().is_empty());
    }

    impl Fixture {
        /// Database import fixture ONLY: mock-generated pixels and synthetic verified
        /// receipt exercise aggregation, not actual Chrome authenticity. Never exported
        /// outside this test's temporary directory and never exposed as a trust override.
        fn synthetic_verified_pair(&self, id: i64) -> (String, String) {
            self.synthetic_verified_pair_options(id, true)
        }
        fn synthetic_verified_pair_options(&self, id: i64, static_valid: bool) -> (String, String) {
            let conn = Connection::open(&self.db).unwrap();
            let exp = load_experiment(&conn, id).unwrap();
            let mut a_screenshot = String::new();
            for arm in ["a", "b"] {
                let dir = self.root.join(format!("synthetic-{id}-{arm}"));
                std::fs::create_dir(&dir).unwrap();
                let body = if static_valid {
                    "<h1>共同标题</h1><p>共同正文</p>"
                } else {
                    "<h1>共同标题</h1><p>共同正文</p><style>p{box-shadow:0 1px 3px #000000}</style>"
                };
                let original = self.html(&format!("source-{id}-{arm}.html"), body);
                let archive = dir.join("page.html");
                std::fs::copy(&original, &archive).unwrap();
                let screenshot = dir.join("capture.png");
                crate::capture::Renderer::render(
                    &MockRenderer,
                    &archive,
                    &screenshot,
                    &capture_spec(&exp.conditions).unwrap(),
                )
                .unwrap();
                if arm == "b" {
                    image::RgbImage::from_pixel(800, 600, image::Rgb([220, 220, 220]))
                        .save(&screenshot)
                        .unwrap();
                }
                let page_hash = crate::capture::sha256_file(&archive).unwrap();
                let screenshot_hash = crate::capture::sha256_file(&screenshot).unwrap();
                if arm == "a" {
                    a_screenshot = screenshot_hash.clone();
                }
                let receipt = json!({"policy_version":crate::capture::POLICY_VERSION,"page_sha256":page_hash,
                    "screenshot_sha256":screenshot_hash,"effective_page_sha256":page_hash,
                    "profile_sha256":hash_bytes(b"synthetic aggregation fixture"),"viewport":exp.conditions["viewport"],"theme":"light",
                    "renderer":{"kind":"chrome","version":"SYNTHETIC UNIT FIXTURE - NOT REAL CHROME"},"verified":true});
                let receipt_path = dir.join("receipt.json");
                write_new(&receipt_path, receipt.to_string().as_bytes()).unwrap();
                let report =
                    artifact_report(&std::fs::read_to_string(&archive).unwrap(), &exp, arm)
                        .unwrap();
                assert_eq!(
                    report["static_checks"]["passed"], static_valid,
                    "synthetic fixture static state must be explicit"
                );
                conn.execute("INSERT INTO artifact(experiment_id,arm,original_path,archived_html_path,screenshot_path,
                    page_sha256,screenshot_sha256,capture_receipt_json,capture_receipt_path,capture_receipt_sha256,
                    capture_conditions_hash,content_status,content_report_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'compliant',?12)",
                    params![id,arm,original.to_str().unwrap(),archive.to_str().unwrap(),screenshot.to_str().unwrap(),page_hash,screenshot_hash,
                        receipt.to_string(),receipt_path.to_str().unwrap(),hash_json(&receipt),exp.conditions_hash,report.to_string()]).unwrap();
            }
            let exported = blind(&BlindOpts {
                db: &self.db,
                experiment_id: id,
                out_dir: &self.root.join(format!("synthetic-public-{id}")),
            })
            .unwrap();
            let winner_index = if exported["manifest"]["screenshots"][1]["sha256"] == a_screenshot {
                0
            } else {
                1
            };
            (
                exported["manifest_hash"].as_str().unwrap().to_owned(),
                exp.recipe_hashes[winner_index].clone(),
            )
        }
    }

    #[test]
    fn personal_evidence_view_revalidates_eligible_final_screenshots_without_writes() {
        let f = Fixture::new();
        let id = f.ready();
        let (hash, _) = f.synthetic_verified_pair(id);
        let before = std::fs::read(&f.db).unwrap();
        let evidence = eligible_evidence(&f.db, id, &hash).unwrap();
        assert_eq!(evidence["formal_eligible"], true);
        assert_eq!(evidence["manifest_hash"], hash);
        assert_eq!(evidence["artifacts"].as_array().unwrap().len(), 2);
        for artifact in evidence["artifacts"].as_array().unwrap() {
            assert!(artifact["artifact_id"].as_i64().unwrap() > 0);
            assert_eq!(
                crate::capture::sha256_file(Path::new(
                    artifact["screenshot_path"].as_str().unwrap()
                ))
                .unwrap(),
                artifact["shot_sha256"]
            );
        }
        assert_eq!(std::fs::read(&f.db).unwrap(), before);
    }

    #[test]
    fn formal_stats_use_latest_human_only_deduplicate_experiments_and_attribute_frozen_recipe_hashes()
     {
        let f = Fixture::new();
        let id = f.ready();
        let (hash, b_recipe) = f.synthetic_verified_pair(id);
        f.record(id, &hash, "human", Some("fixture-alice"), "A")
            .unwrap();
        f.record(id, &hash, "human", Some("fixture-alice"), "B")
            .unwrap();
        f.record(id, &hash, "demo", Some("fixture-alice"), "A")
            .unwrap();
        f.record(id, &hash, "ai", Some("fixture-alice"), "A")
            .unwrap();
        f.record(id, &hash, "human", Some("fixture-bob"), "tie")
            .unwrap();
        f.record(id, &hash, "human", Some("fixture-carol"), "abstain")
            .unwrap();
        // Mutating live pills cannot alter already-frozen attribution.
        Connection::open(&f.db)
            .unwrap()
            .execute("UPDATE pill SET output_json='{}'", [])
            .unwrap();
        let report = stats(&StatsOpts { db: &f.db }).unwrap();
        assert_eq!(report["formal_visual"]["experiments"], 1);
        assert_eq!(report["formal_visual"]["reviewers"], 3);
        assert_eq!(report["formal_visual"]["evaluations"], 3);
        assert_eq!(report["formal_visual"]["generalizable_beauty_claim"], false);
        let rows = report["recipes"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        for r in rows {
            assert_eq!(r["experiments"], 1);
            assert_eq!(r["evaluations"], 3);
            assert_eq!(r["ties"], 1);
            assert_eq!(r["abstentions"], 1);
            assert_eq!(
                r["wins"],
                if r["recipe_definition_hash"] == b_recipe {
                    1
                } else {
                    0
                },
                "latest B human wins; later demo/AI cannot mask it"
            );
        }
    }

    #[test]
    fn recipe_statistics_keep_distinct_conditions_in_independent_groups() {
        let f = Fixture::new();
        f.pill(1, frozen("recipe.a"));
        f.pill(2, frozen("recipe.b"));
        let content = json!({"strings":["共同标题","共同正文"]});
        let mut expected = Vec::new();
        let mut target_recipe = String::new();
        for (model, target_wins) in [("fixture-model-one", true), ("fixture-model-two", false)] {
            let conditions = json!({"viewport":{"width":800,"height":600},"theme":"light",
                "generator":{"kind":"coding-agent","model":model,"version":"1"},"task":"共同任务"});
            let created = create(&CreateOpts {
                db: &f.db,
                pill_a: 1,
                pill_b: 2,
                content: &content,
                conditions: &conditions,
            })
            .unwrap();
            let id = created["id"].as_i64().unwrap();
            target_recipe = created["arms"][0]["recipe_hash"]
                .as_str()
                .unwrap()
                .to_owned();
            let (hash, public_b_recipe) = f.synthetic_verified_pair(id);
            let target_label = if public_b_recipe == target_recipe {
                "B"
            } else {
                "A"
            };
            let winner = if target_wins {
                target_label
            } else if target_label == "A" {
                "B"
            } else {
                "A"
            };
            f.record(id, &hash, "human", Some("fixture-reviewer"), winner)
                .unwrap();
            expected.push((
                created["conditions_hash"].as_str().unwrap().to_owned(),
                target_wins,
            ));
        }
        let report = stats(&StatsOpts { db: &f.db }).unwrap();
        assert_eq!(report["formal_visual"]["experiments"], 2);
        assert_eq!(report["formal_visual"]["evaluations"], 2);
        let groups: Vec<&Value> = report["recipes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["recipe_definition_hash"] == target_recipe)
            .collect();
        assert_eq!(
            groups.len(),
            2,
            "one frozen recipe must not pool different generation conditions"
        );
        for (conditions_hash, target_wins) in expected {
            let group = groups
                .iter()
                .find(|row| row["conditions_hash"] == conditions_hash)
                .unwrap();
            assert_eq!(group["experiments"], 1);
            assert_eq!(group["evaluations"], 1);
            assert_eq!(group["wins"], if target_wins { 1 } else { 0 });
            assert_eq!(group["losses"], if target_wins { 0 } else { 1 });
        }
    }

    #[test]
    fn creation_refuses_duplicate_incomplete_or_tampered_frozen_pills_and_bad_common_inputs() {
        for variant in [
            "duplicate",
            "empty-components",
            "tampered-content",
            "null-technique",
        ] {
            let f = Fixture::new();
            let a = frozen("recipe.a");
            let mut b = frozen("recipe.b");
            match variant {
                "duplicate" => b = a.clone(),
                "empty-components" => b["components"] = json!([]),
                "tampered-content" => b["components"][0]["content"] = json!({"forged":true}),
                "null-technique" => b["components"][0]["technique_version"] = Value::Null,
                _ => unreachable!(),
            }
            f.pill(1, a);
            f.pill(2, b);
            assert!(f.create().is_err(), "accepted {variant}");
        }
        let f = Fixture::new();
        f.pill(1, frozen("a"));
        f.pill(2, frozen("b"));
        let content = json!({"strings":["title"]});
        let conditions = json!({"viewport":{"width":800,"height":600},"theme":"light","generator":{"kind":"human","version":"1"},"task":"共同任务"});
        assert!(
            create(&CreateOpts {
                db: &f.db,
                pill_a: 1,
                pill_b: 1,
                content: &content,
                conditions: &conditions
            })
            .is_err()
        );
        assert!(
            create(&CreateOpts {
                db: &f.db,
                pill_a: 1,
                pill_b: 2,
                content: &json!({"strings":[]}),
                conditions: &conditions
            })
            .is_err()
        );
        let mut wrong = conditions.clone();
        wrong["theme"] = json!("dark");
        assert!(
            create(&CreateOpts {
                db: &f.db,
                pill_a: 1,
                pill_b: 2,
                content: &content,
                conditions: &wrong
            })
            .is_err()
        );
    }

    #[test]
    fn matching_input_hash_does_not_excuse_missing_body_text_or_replaced_media() {
        let f = Fixture::new();
        let id = f.ready();
        let missing = f.html("missing.html", "<h1>共同标题</h1><!-- 共同正文 -->");
        let valid = f.html("valid.html", "<h1>共同标题</h1><p>共同正文</p>");
        assert_eq!(
            f.attach(id, "a", &missing).unwrap()["content_report"]["status"],
            "noncompliant"
        );
        f.attach(id, "b", &valid).unwrap();
        assert!(
            blind(&BlindOpts {
                db: &f.db,
                experiment_id: id,
                out_dir: &f.root.join("rejected-public")
            })
            .is_err()
        );

        use base64::Engine;
        let f = Fixture::new();
        f.pill(1, frozen("a"));
        f.pill(2, frozen("b"));
        let image_path = f.root.join("expected.png");
        image::RgbImage::from_pixel(1, 1, image::Rgb([12, 34, 56]))
            .save(&image_path)
            .unwrap();
        let bytes = std::fs::read(&image_path).unwrap();
        let uri = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        );
        let content = json!({"strings":["共同标题","共同正文"],"media":[{"data_uri":uri,"sha256":hash_bytes(&bytes)}]});
        let conditions = json!({"viewport":{"width":800,"height":600},"theme":"light","generator":{"kind":"human","version":"1"},"task":"共同任务"});
        let id = create(&CreateOpts {
            db: &f.db,
            pill_a: 1,
            pill_b: 2,
            content: &content,
            conditions: &conditions,
        })
        .unwrap()["id"]
            .as_i64()
            .unwrap();
        let actual = f.html(
            "actual.html",
            &format!("<h1>共同标题</h1><p>共同正文</p><img src='{uri}' alt='共同素材'>"),
        );
        assert_eq!(
            f.attach(id, "a", &actual).unwrap()["content_report"]["status"],
            "compliant"
        );
        image::RgbImage::from_pixel(1, 1, image::Rgb([200, 1, 2]))
            .save(&image_path)
            .unwrap();
        let swapped = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(std::fs::read(image_path).unwrap())
        );
        let changed = f.html(
            "changed.html",
            &format!("<h1>共同标题</h1><p>共同正文</p><img src='{swapped}' alt='共同素材'>"),
        );
        assert_eq!(
            f.attach(id, "b", &changed).unwrap()["content_report"]["status"],
            "noncompliant"
        );
        assert!(
            blind(&BlindOpts {
                db: &f.db,
                experiment_id: id,
                out_dir: &f.root.join("rejected-media")
            })
            .is_err()
        );
    }

    #[test]
    fn record_rechecks_original_archive_screenshot_receipt_and_public_manifest_bytes() {
        for evidence in ["original", "archive", "screenshot", "receipt", "manifest"] {
            let f = Fixture::new();
            let (id, hash) = f.pair();
            let path = match evidence {
                "original" => f.root.join("left.html"),
                "archive" => f.root.join(format!("archive/experiment-{id}-a/page.html")),
                "screenshot" => f
                    .root
                    .join(format!("archive/experiment-{id}-a/capture.png")),
                "receipt" => f
                    .root
                    .join(format!("archive/experiment-{id}-a/capture-receipt.json")),
                "manifest" => f.root.join("public/manifest.json"),
                _ => unreachable!(),
            };
            std::fs::write(&path, b"changed evidence").unwrap();
            assert!(
                f.record(id, &hash, "demo", None, "A").is_err(),
                "did not detect changed {evidence}"
            );
            assert!(
                blind(&BlindOpts {
                    db: &f.db,
                    experiment_id: id,
                    out_dir: &f.root.join("retry")
                })
                .is_err()
            );
        }
    }

    #[test]
    fn formal_stats_exclude_unknown_generation_and_failed_static_spec_without_using_them_as_visual_scores()
     {
        for reason in ["unknown_generator", "static_check_failed"] {
            let f = Fixture::new();
            f.pill(1, frozen("a"));
            f.pill(2, frozen("b"));
            let generator = if reason == "unknown_generator" {
                json!({"kind":"coding-agent","model":"unknown","version":"unknown"})
            } else {
                json!({"kind":"human","version":"fixture-v1"})
            };
            let content = json!({"strings":["共同标题","共同正文"]});
            let conditions = json!({"viewport":{"width":800,"height":600},"theme":"light","generator":generator,"task":"共同任务"});
            let created = create(&CreateOpts {
                db: &f.db,
                pill_a: 1,
                pill_b: 2,
                content: &content,
                conditions: &conditions,
            })
            .unwrap();
            let id = created["id"].as_i64().unwrap();
            let (hash, _) = f.synthetic_verified_pair_options(id, reason != "static_check_failed");
            assert_eq!(
                f.record(id, &hash, "human", Some("fixture-reviewer"), "A")
                    .unwrap()["formal_eligible"],
                false
            );
            let report = stats(&StatsOpts { db: &f.db }).unwrap();
            assert_eq!(report["formal_visual"]["evaluations"], 0);
            assert_eq!(report["excluded"]["evidence"][0]["reason"], reason);
        }
    }

    #[test]
    fn latest_abstention_replaces_old_preference_and_experiments_are_deduplicated_across_reviewers()
    {
        let f = Fixture::new();
        let first = f.ready();
        let (one, _) = f.synthetic_verified_pair(first);
        let second = f.create().unwrap()["id"].as_i64().unwrap();
        let (two, _) = f.synthetic_verified_pair(second);
        f.record(first, &one, "human", Some("fixture-alice"), "A")
            .unwrap();
        f.record(first, &one, "human", Some(" fixture-alice "), "abstain")
            .unwrap();
        f.record(first, &one, "human", Some("fixture-bob"), "tie")
            .unwrap();
        f.record(second, &two, "human", Some("fixture-alice"), "B")
            .unwrap();
        let report = stats(&StatsOpts { db: &f.db }).unwrap();
        assert_eq!(report["formal_visual"]["experiments"], 2);
        assert_eq!(report["formal_visual"]["reviewers"], 2);
        assert_eq!(report["formal_visual"]["evaluations"], 3);
        assert_eq!(report["formal_visual"]["preferences"], 1);
        assert_eq!(report["formal_visual"]["abstentions"], 1);
        for r in report["recipes"].as_array().unwrap() {
            assert_eq!(r["experiments"], 2);
        }
    }

    #[test]
    fn same_recipe_snapshot_is_not_awarded_a_win_against_itself() {
        let f = Fixture::new();
        let a = frozen("same-recipe");
        let mut b = a.clone();
        b["prompt"] = json!("different frozen pill");
        f.pill(1, a);
        f.pill(2, b);
        let id = f.create().unwrap()["id"].as_i64().unwrap();
        let (hash, _) = f.synthetic_verified_pair(id);
        f.record(id, &hash, "human", Some("fixture-reviewer"), "A")
            .unwrap();
        let report = stats(&StatsOpts { db: &f.db }).unwrap();
        let rows = report["recipes"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["experiments"], 1);
        assert_eq!(rows[0]["evaluations"], 1);
        assert_eq!(rows[0]["wins"], 0);
        assert_eq!(rows[0]["losses"], 0);
        assert_eq!(rows[0]["within_recipe_comparisons"], 1);
    }

    #[test]
    fn database_history_rejects_snapshot_artifact_and_evaluation_overwrite_or_delete() {
        let f = Fixture::new();
        let (id, hash) = f.pair();
        f.record(id, &hash, "demo", None, "tie").unwrap();
        let conn = Connection::open(&f.db).unwrap();
        for sql in [
            "UPDATE experiment SET content_hash='changed'",
            "UPDATE experiment SET pill_a_snapshot_hash='changed'",
            "UPDATE experiment SET blind_mapping_json='{}'",
            "UPDATE experiment SET manifest_hash='changed'",
            "UPDATE experiment SET public_exports_json='[]'",
            "DELETE FROM experiment",
            "UPDATE artifact SET page_sha256='changed'",
            "DELETE FROM artifact",
            "UPDATE artifact_evaluation SET winner='B'",
            "DELETE FROM artifact_evaluation",
        ] {
            assert!(
                conn.execute(sql, []).is_err(),
                "history was mutable via {sql}"
            );
        }
    }

    #[test]
    fn stats_fail_closed_if_evidence_changes_after_a_previously_valid_human_review() {
        let f = Fixture::new();
        let id = f.ready();
        let (hash, _) = f.synthetic_verified_pair(id);
        f.record(id, &hash, "human", Some("fixture-reviewer"), "A")
            .unwrap();
        assert_eq!(
            stats(&StatsOpts { db: &f.db }).unwrap()["formal_visual"]["experiments"],
            1
        );
        std::fs::write(
            f.root.join(format!("synthetic-public-{id}/B.png")),
            b"tampered",
        )
        .unwrap();
        let report = stats(&StatsOpts { db: &f.db }).unwrap();
        assert_eq!(report["formal_visual"]["experiments"], 0);
        assert_eq!(
            report["excluded"]["evidence"][0]["reason"],
            "invalid_evidence"
        );
    }

    #[test]
    fn record_refuses_public_packages_that_acquired_deanonymizing_extra_files() {
        let f = Fixture::new();
        let (id, hash) = f.pair();
        std::fs::write(
            f.root.join("public/pill-and-material-notes.txt"),
            b"private mapping would leak",
        )
        .unwrap();
        assert!(f.record(id, &hash, "demo", None, "A").is_err());
    }

    #[test]
    fn create_rejects_legacy_pills_even_with_explicit_legacy_fallback() {
        let f = Fixture::new();
        f.pill(
            1,
            json!({"recipe":"old","components":[],"legacy_provenance_fallback":true}),
        );
        f.pill(2, json!({"recipe":"other","components":[]}));
        assert!(
            f.create().is_err(),
            "legacy pills cannot be frozen into a controlled experiment"
        );
    }
}
