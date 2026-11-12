//! 灵感引擎研究原型 CLI。
//!
//! 只做 M1（抽取）与 P0（个人层闭环）需要的最小闭环：
//!   ie extract  --db <db> --root <data dir> [--limit N]
//!   ie simulate --db <db> --person demo --n 200 [--seed 7]
//!   ie fit      --db <db> --person <local-id> [--source demo]
//!   ie stats    --db <db>

use ie::{
    alchemy, bundle, db, distill, experiment, extract, fit, forge, judge, panel, personal, vision,
};

use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = if args.is_empty() {
        String::new()
    } else {
        args.remove(0)
    };
    let mut opts: HashMap<String, String> = HashMap::new();
    let mut implicit_flags = std::collections::HashSet::new();
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(k) = a.strip_prefix("--") {
            // 布尔开关不吞下一个参数：后面紧跟另一个 --flag 或没有值时记作 "1"
            let has_value = i + 1 < args.len() && !args[i + 1].starts_with("--");
            if has_value {
                implicit_flags.remove(k);
                opts.insert(k.to_string(), args[i + 1].clone());
                i += 2;
            } else {
                implicit_flags.insert(k.to_string());
                opts.insert(k.to_string(), "1".to_string());
                i += 1;
            }
        } else {
            positional.push(a.clone());
            i += 1;
        }
    }
    for key in ["activate", "apply-personal"] {
        anyhow::ensure!(
            !opts.contains_key(key) || implicit_flags.contains(key),
            "--{key} 是无值开关；不要写 false，关闭时应移除此开关"
        );
    }
    // Keep an omitted value distinct from a literal "1", especially for human provenance.
    for key in ["source", "reviewer", "dimension", "manifest-hash"] {
        anyhow::ensure!(!implicit_flags.contains(key), "需要 --{key} <value>");
    }
    if matches!(cmd.as_str(), "judge" | "experiment" | "person" | "kernel")
        && !positional.is_empty()
    {
        opts.insert("sub".into(), positional[0].clone());
    }
    let required = |key: &str| -> Result<&str> {
        opts.get(key)
            .map(String::as_str)
            .filter(|value| !implicit_flags.contains(key) && !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("需要 --{key} <value>"))
    };
    let db = opts
        .get("db")
        .cloned()
        .unwrap_or_else(|| "data/inspiration/inspiration.db".to_string());
    if matches!(
        cmd.as_str(),
        "refine"
            | "brew"
            | "formula"
            | "mutate"
            | "judge"
            | "brief"
            | "verify"
            | "motion"
            | "bag"
            | "experiment"
            | "person"
            | "kernel"
            | "fit"
            | "simulate"
    ) {
        db::require_current(std::path::Path::new(&db))?;
    }
    match cmd.as_str() {
        "migrate" => {
            let path = opts
                .get("db")
                .filter(|p| p.as_str() != "1")
                .map(PathBuf::from)
                .ok_or_else(|| {
                    anyhow::anyhow!("迁移必须显式指定 --db <path>；不会自动修改默认用户库")
                })?;
            if let Some(backup) = db::migrate_path(&path)? {
                println!("迁移前备份：{}", backup.display());
            }
            println!("数据库 schema {} 已就绪", db::LATEST_SCHEMA);
            Ok(())
        }
        "extract" => {
            let root = PathBuf::from(
                opts.get("root")
                    .cloned()
                    .unwrap_or_else(|| "data/inspiration".to_string()),
            );
            let limit = opts.get("limit").and_then(|v| v.parse().ok());
            extract::run(&db, &root, limit)
        }
        "simulate" => {
            let person = opts.get("person").cloned().unwrap_or_else(|| "demo".into());
            let n: usize = opts.get("n").and_then(|v| v.parse().ok()).unwrap_or(200);
            let seed: u64 = opts.get("seed").and_then(|v| v.parse().ok()).unwrap_or(7);
            let active = !matches!(opts.get("mode").map(|s| s.as_str()), Some("random"));
            fit::simulate(&db, &person, n, seed, active)
        }
        "fit" => {
            let seed: u64 = opts
                .get("seed")
                .map(|v| v.parse())
                .transpose()?
                .unwrap_or(7);
            match opts.get("source").map(String::as_str).unwrap_or("human") {
                "demo" => fit::run_demo(
                    &db,
                    opts.get("person").map(String::as_str).unwrap_or("demo"),
                    seed,
                ),
                "human" => {
                    let value = fit::fit_person(&fit::FitOpts {
                        db: &db,
                        person: required("person")?,
                        scope: opts
                            .get("scope")
                            .map(String::as_str)
                            .unwrap_or(personal::SCOPE),
                        seed,
                        activate: opts.contains_key("activate"),
                    })?;
                    println!("{}", serde_json::to_string_pretty(&value)?);
                    Ok(())
                }
                _ => anyhow::bail!("fit 只支持 human（默认）或显式 --source demo；不能混用数据"),
            }
        }
        "person" => {
            let person = required("person")?;
            let value = match opts.get("sub").map(String::as_str) {
                Some("init") => personal::init(&personal::InitOpts { db: &db, person })?,
                Some("bridge") => personal::bridge(&personal::BridgeOpts {
                    db: &db,
                    person,
                    evaluation_id: required("evaluation")?.parse()?,
                    intent: required("intent")?,
                })?,
                _ => anyhow::bail!(
                    "用法：ie person <init|bridge> --person <local-id> --db <path> ..."
                ),
            };
            println!("{}", serde_json::to_string_pretty(&value)?);
            Ok(())
        }
        "kernel" => {
            anyhow::ensure!(
                opts.get("sub").map(String::as_str) == Some("show"),
                "用法：ie kernel show --person <local-id> --db <path>"
            );
            let value = fit::show(
                &db,
                required("person")?,
                opts.get("scope")
                    .map(String::as_str)
                    .unwrap_or(personal::SCOPE),
            )?;
            println!("{}", serde_json::to_string_pretty(&value)?);
            Ok(())
        }
        "vision" => {
            let root = PathBuf::from(
                opts.get("root")
                    .cloned()
                    .unwrap_or_else(|| "data/inspiration".to_string()),
            );
            let model = opts
                .get("model")
                .cloned()
                .unwrap_or_else(|| "gemini-3.1-flash-lite".to_string());
            let base_url = opts
                .get("base-url")
                .cloned()
                .unwrap_or_else(|| "https://generativelanguage.googleapis.com".to_string());
            let limit = opts.get("limit").and_then(|v| v.parse().ok());
            let thinking: u32 = opts
                .get("thinking")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let rpm: u32 = opts.get("rpm").and_then(|v| v.parse().ok()).unwrap_or(5);
            let provider = opts
                .get("provider")
                .cloned()
                .unwrap_or_else(|| "gemini".to_string());
            let shard: (usize, usize) = opts
                .get("shard")
                .and_then(|s| s.split_once('/'))
                .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
                .unwrap_or((0, 1));
            let finds_first = opts.get("priority").map(|v| v == "finds").unwrap_or(false);
            let asset = opts.get("asset").and_then(|v| v.parse().ok());
            vision::run(&vision::Opts {
                db: &db,
                root: &root,
                model: &model,
                limit,
                base_url: &base_url,
                thinking,
                rpm,
                shard,
                finds_first,
                provider: &provider,
                asset,
            })
        }
        "distill" => {
            let model = opts
                .get("model")
                .cloned()
                .unwrap_or_else(|| "gemini-3.6-flash".to_string());
            let base_url = opts
                .get("base-url")
                .cloned()
                .unwrap_or_else(|| "https://generativelanguage.googleapis.com".to_string());
            let out_dir = PathBuf::from(
                opts.get("out")
                    .cloned()
                    .unwrap_or_else(|| "data/inspiration/distills".to_string()),
            );
            let work = opts.get("work").and_then(|v| v.parse().ok());
            let thinking: u32 = opts
                .get("thinking")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let provider = opts
                .get("provider")
                .cloned()
                .unwrap_or_else(|| "gemini".to_string());
            distill::run(&distill::Opts {
                db: &db,
                image: opts.get("image").map(|s| s.as_str()),
                work,
                model: &model,
                base_url: &base_url,
                out_dir: &out_dir,
                thinking,
                provider: &provider,
            })
        }
        "export" => {
            let out = PathBuf::from(
                opts.get("out")
                    .cloned()
                    .unwrap_or_else(|| "ie-bundle.json".into()),
            );
            let root = PathBuf::from(
                opts.get("root")
                    .cloned()
                    .unwrap_or_else(|| "data/inspiration".to_string()),
            );
            bundle::export(&bundle::ExportOpts {
                db: &db,
                out: &out,
                root: &root,
                format: opts.get("format").map(|s| s.as_str()).unwrap_or("auto"),
                with_assets: opts.contains_key("with-assets"),
                with_personal: opts.contains_key("with-personal"),
                collection: opts.get("collection").map(|s| s.as_str()),
                domain: opts.get("domain").map(|s| s.as_str()),
                since: opts.get("since").map(|s| s.as_str()),
            })
        }
        "import" => {
            let input = opts
                .get("in")
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("需要 --in <bundle.json>"))?;
            let root = PathBuf::from(
                opts.get("root")
                    .cloned()
                    .unwrap_or_else(|| "data/inspiration".to_string()),
            );
            bundle::import(&bundle::ImportOpts {
                db: &db,
                input: &input,
                root: &root,
                dry_run: opts.contains_key("dry-run"),
            })
        }
        "refine" => {
            let work: i64 = opts
                .get("work")
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| anyhow::anyhow!("需要 --work <id>"))?;
            let techniques: Vec<String> = opts
                .get("technique")
                .map(|s| s.split(',').map(|x| x.trim().to_string()).collect())
                .unwrap_or_else(|| vec!["t.palette".into()]);
            let provider = opts
                .get("provider")
                .cloned()
                .unwrap_or_else(|| "gemini".to_string());
            let model = opts
                .get("model")
                .cloned()
                .unwrap_or_else(|| "gemini-3.1-flash-lite".to_string());
            let base_url = opts
                .get("base-url")
                .cloned()
                .unwrap_or_else(|| "https://generativelanguage.googleapis.com".to_string());
            let root = PathBuf::from(
                opts.get("root")
                    .cloned()
                    .unwrap_or_else(|| "data/inspiration".to_string()),
            );
            let thinking: u32 = opts
                .get("thinking")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            alchemy::refine(&alchemy::RefineOpts {
                db: &db,
                root: &root,
                work,
                techniques,
                provider: &provider,
                model: &model,
                base_url: &base_url,
                thinking,
                to_bag: !opts.contains_key("no-bag"),
                refresh: opts.contains_key("refresh"),
            })
        }
        "brew" => {
            if opts.contains_key("personal-candidates") {
                anyhow::ensure!(
                    !opts.contains_key("recipe")
                        && !opts.contains_key("force")
                        && !opts.contains_key("allow-mixed")
                        && !opts.contains_key("out"),
                    "个人候选炼丹不混用 --recipe/--force/--allow-mixed/--out；可随后用 brief 导出规格"
                );
                let manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
                    required("personal-candidates")?,
                )?)?;
                let candidates: Vec<i64> = manifest["artifact_ids"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("候选清单需要 artifact_ids 数组"))?
                    .iter()
                    .map(|v| {
                        v.as_i64()
                            .ok_or_else(|| anyhow::anyhow!("artifact ID 必须是整数"))
                    })
                    .collect::<Result<_>>()?;
                let ranked = fit::rank(&fit::RankOpts {
                    db: &db,
                    person: required("person")?,
                    scope: manifest["scope"].as_str().unwrap_or(personal::SCOPE),
                    candidates: &candidates,
                    apply: opts.contains_key("apply-personal"),
                })?;
                anyhow::ensure!(
                    ranked["chosen_snapshot"].is_object() && ranked["trace"].is_object(),
                    "候选证据不可核验，未炼制丹药"
                );
                let result =
                    alchemy::brew_snapshot(&db, &ranked["chosen_snapshot"], &ranked["trace"])?;
                println!("{}", serde_json::to_string_pretty(&result)?);
                return Ok(());
            }
            anyhow::ensure!(
                !opts.contains_key("person") && !opts.contains_key("apply-personal"),
                "个人炼丹模式需要 --personal-candidates <manifest.json>；未应用个性化"
            );
            let recipe = opts
                .get("recipe")
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("需要 --recipe <path>"))?;
            let out_dir = opts.get("out").map(PathBuf::from);
            alchemy::brew(&alchemy::BrewOpts {
                db: &db,
                recipe: &recipe,
                pinned: None,
                formula_id: None,
                allow_mixed: opts.contains_key("allow-mixed"),
                force: opts.contains_key("force"),
                out_dir: out_dir.as_deref(),
                quiet: false,
            })
        }
        "formula" => {
            let formula = opts
                .get("in")
                .or_else(|| opts.get("formula"))
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("需要 --in <formula.json>"))?;
            let recipes_dir = PathBuf::from(
                opts.get("recipes-dir")
                    .cloned()
                    .unwrap_or_else(|| "docs/design/inspiration/recipes".to_string()),
            );
            let root = PathBuf::from(
                opts.get("root")
                    .cloned()
                    .unwrap_or_else(|| "data/inspiration".to_string()),
            );
            let provider = opts
                .get("provider")
                .cloned()
                .unwrap_or_else(|| "gemini".to_string());
            let model = opts
                .get("model")
                .cloned()
                .unwrap_or_else(|| "gemini-3.1-flash-lite".to_string());
            let base_url = opts
                .get("base-url")
                .cloned()
                .unwrap_or_else(|| "https://generativelanguage.googleapis.com".to_string());
            let thinking: u32 = opts
                .get("thinking")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            alchemy::formula_run(&alchemy::FormulaOpts {
                db: &db,
                formula: &formula,
                recipes_dir: &recipes_dir,
                root: &root,
                provider: &provider,
                model: &model,
                base_url: &base_url,
                thinking,
            })
        }
        "mutate" => {
            let recipe = opts
                .get("recipe")
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("需要 --recipe <path>"))?;
            let runs: usize = opts.get("runs").and_then(|v| v.parse().ok()).unwrap_or(3);
            let delta: f64 = opts
                .get("delta")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.1);
            let seed: u64 = opts.get("seed").and_then(|v| v.parse().ok()).unwrap_or(7);
            alchemy::mutate(&alchemy::MutateOpts {
                db: &db,
                recipe: &recipe,
                runs,
                delta,
                seed,
            })
        }
        "pills" => {
            let limit: usize = opts.get("limit").and_then(|v| v.parse().ok()).unwrap_or(20);
            alchemy::pills(&db, limit)
        }
        "bag" => {
            let limit: usize = opts.get("limit").and_then(|v| v.parse().ok()).unwrap_or(30);
            alchemy::bag(&alchemy::BagOpts {
                db: &db,
                dimension: opts.get("dimension").map(|s| s.as_str()),
                limit,
            })
        }
        "experiment" => {
            let required = |key: &str| -> Result<&str> {
                opts.get(key)
                    .map(String::as_str)
                    .filter(|value| !implicit_flags.contains(key) && !value.trim().is_empty())
                    .ok_or_else(|| anyhow::anyhow!("需要 --{key} <value>"))
            };
            let experiment_id = || -> Result<i64> {
                let id: i64 = required("experiment")?.parse()?;
                anyhow::ensure!(id > 0, "experiment ID 必须为正数");
                Ok(id)
            };
            let value = match opts.get("sub").map(String::as_str) {
                Some("create") => {
                    let pills: Vec<i64> = required("pills")?
                        .split(',')
                        .map(|p| p.trim().parse())
                        .collect::<std::result::Result<_, _>>()?;
                    anyhow::ensure!(
                        pills.len() == 2 && pills.iter().all(|p| *p > 0),
                        "需要 --pills <id>,<id>，恰好两颗正整数丹药"
                    );
                    let content: serde_json::Value =
                        serde_json::from_str(&std::fs::read_to_string(required("content")?)?)?;
                    let conditions: serde_json::Value =
                        serde_json::from_str(&std::fs::read_to_string(required("conditions")?)?)?;
                    experiment::create(&experiment::CreateOpts {
                        db: &db,
                        pill_a: pills[0],
                        pill_b: pills[1],
                        content: &content,
                        conditions: &conditions,
                    })?
                }
                Some("attach") => {
                    let html = PathBuf::from(required("html")?);
                    let out_dir = PathBuf::from(required("out")?);
                    let chrome = opts.get("chrome").map(PathBuf::from);
                    experiment::attach(&experiment::AttachOpts {
                        db: &db,
                        experiment_id: experiment_id()?,
                        arm: required("arm")?,
                        html: &html,
                        out_dir: &out_dir,
                        chrome: chrome.as_deref(),
                    })?
                }
                Some("blind") => {
                    let out_dir = PathBuf::from(required("out")?);
                    experiment::blind(&experiment::BlindOpts {
                        db: &db,
                        experiment_id: experiment_id()?,
                        out_dir: &out_dir,
                    })?
                }
                Some("record") => {
                    let visual = experiment::RecordOpts {
                        db: &db,
                        experiment_id: experiment_id()?,
                        manifest_hash: required("manifest-hash")?,
                        source: opts.get("source").map(String::as_str).unwrap_or("demo"),
                        reviewer: opts.get("reviewer").map(String::as_str),
                        dimension: opts
                            .get("dimension")
                            .map(String::as_str)
                            .unwrap_or("visual"),
                        winner: required("winner")?,
                        note: opts.get("note").map(String::as_str),
                    };
                    if opts.contains_key("person") {
                        personal::record(&personal::RecordOpts {
                            visual,
                            person: required("person")?,
                            intent: required("intent")?,
                        })?
                    } else {
                        anyhow::ensure!(
                            !opts.contains_key("intent"),
                            "个人用途需要同时指定 --person 和 --intent"
                        );
                        experiment::record(&visual)?
                    }
                }
                Some("stats") => experiment::stats(&experiment::StatsOpts { db: &db })?,
                _ => anyhow::bail!(
                    "用法：ie experiment <create|attach|blind|record|stats> --db <path> ..."
                ),
            };
            println!("{}", serde_json::to_string_pretty(&value)?);
            Ok(())
        }
        "judge" => {
            let sub = opts.get("sub").cloned().unwrap_or_default();
            match sub.as_str() {
                "run" => {
                    let pills: Vec<i64> = opts
                        .get("pills")
                        .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
                        .unwrap_or_default();
                    if pills.len() < 2 {
                        anyhow::bail!("需要 --pills 12,13（至少两颗）");
                    }
                    let provider = opts
                        .get("provider")
                        .cloned()
                        .unwrap_or_else(|| "gemini".into());
                    let model = opts
                        .get("model")
                        .cloned()
                        .unwrap_or_else(|| "gemini-3.1-flash-lite".into());
                    let base_url = opts
                        .get("base-url")
                        .cloned()
                        .unwrap_or_else(|| "https://generativelanguage.googleapis.com".into());
                    let thinking: u32 = opts
                        .get("thinking")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    let seed: u64 = opts.get("seed").and_then(|v| v.parse().ok()).unwrap_or(7);
                    let repeats: usize = opts
                        .get("repeats")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(3);
                    let dimension = opts
                        .get("dimension")
                        .cloned()
                        .unwrap_or_else(|| "overall".to_string());
                    judge::run(&judge::RunOpts {
                        db: &db,
                        pills,
                        provider: &provider,
                        model: &model,
                        base_url: &base_url,
                        thinking,
                        seed,
                        repeats,
                        dimension,
                        rpm: opts.get("rpm").and_then(|v| v.parse().ok()).unwrap_or(5),
                    })
                }
                "panel" => {
                    let pills: Vec<i64> = opts
                        .get("pills")
                        .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
                        .unwrap_or_default();
                    let judges: Vec<String> = opts
                        .get("judges")
                        .map(|s| s.split(',').map(|x| x.trim().to_string()).collect())
                        .unwrap_or_else(|| vec!["local".into(), "gemini-lite".into()]);
                    let cfg = PathBuf::from(
                        opts.get("judges-config")
                            .cloned()
                            .unwrap_or_else(|| "docs/design/inspiration/judges.json".to_string()),
                    );
                    let repeats: usize = opts
                        .get("repeats")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(2);
                    let dimension = opts
                        .get("dimension")
                        .cloned()
                        .unwrap_or_else(|| "overall".into());
                    let seed: u64 = opts.get("seed").and_then(|v| v.parse().ok()).unwrap_or(7);
                    panel::run(&panel::PanelOpts {
                        db: &db,
                        pills,
                        judges,
                        cfg_path: &cfg,
                        repeats,
                        dimension,
                        seed,
                    })
                }
                "calibrate" => judge::calibrate(&db),
                "models" => judge::models(&db),
                "list" => judge::list(&db),
                "review" => {
                    let run_id: Option<i64> = opts.get("run").and_then(|v| v.parse().ok());
                    judge::review(&db, run_id)
                }
                "screen" => {
                    let run_id: i64 = opts
                        .get("run")
                        .and_then(|v| v.parse().ok())
                        .ok_or_else(|| anyhow::anyhow!("需要 --run <id>"))?;
                    judge::screen(&db, run_id)
                }
                "fill" => {
                    let run_id: i64 = opts
                        .get("run")
                        .and_then(|v| v.parse().ok())
                        .ok_or_else(|| anyhow::anyhow!("需要 --run <id>"))?;
                    let scores = opts
                        .get("scores")
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("需要 --scores \"A=82,B=70\""))?;
                    let source = opts
                        .get("source")
                        .map(String::as_str)
                        .unwrap_or("demo")
                        .parse()?;
                    judge::fill_with_opts(&judge::FillOpts {
                        db: &db,
                        run_id,
                        scores: &scores,
                        note: opts.get("note").map(String::as_str),
                        provenance: judge::ReviewProvenance {
                            source,
                            reviewer: opts.get("reviewer").map(String::as_str),
                            dimension: opts.get("dimension").map(String::as_str),
                        },
                    })
                }
                "record" => {
                    let item: i64 = opts
                        .get("item")
                        .and_then(|v| v.parse().ok())
                        .ok_or_else(|| anyhow::anyhow!("需要 --item"))?;
                    let score: f64 = opts
                        .get("score")
                        .and_then(|v| v.parse().ok())
                        .ok_or_else(|| anyhow::anyhow!("需要 --score 0-100"))?;
                    let source = opts
                        .get("source")
                        .map(String::as_str)
                        .unwrap_or("demo")
                        .parse()?;
                    judge::record_with_provenance(
                        &db,
                        item,
                        score,
                        opts.get("note").map(String::as_str),
                        &judge::ReviewProvenance {
                            source,
                            reviewer: opts.get("reviewer").map(String::as_str),
                            dimension: opts.get("dimension").map(String::as_str),
                        },
                    )
                }
                _ => anyhow::bail!(
                    "用法：ie judge <run|panel|list|review|screen|fill|record|calibrate|models> ...；真人回填需 --source human --reviewer <id>"
                ),
            }
        }
        "motion" => {
            let video = opts
                .get("video")
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("需要 --video <path>"))?;
            let provider = opts
                .get("provider")
                .cloned()
                .unwrap_or_else(|| "gemini".into());
            let model = opts
                .get("model")
                .cloned()
                .unwrap_or_else(|| "gemini-3.1-flash-lite".into());
            let base_url = opts
                .get("base-url")
                .cloned()
                .unwrap_or_else(|| "https://generativelanguage.googleapis.com".into());
            let thinking: u32 = opts
                .get("thinking")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            alchemy::refine_video(&alchemy::MotionOpts {
                db: &db,
                video: &video,
                provider: &provider,
                model: &model,
                base_url: &base_url,
                thinking,
            })
        }
        "brief" => {
            let pill: i64 = opts
                .get("pill")
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| anyhow::anyhow!("需要 --pill <id>"))?;
            let out = PathBuf::from(
                opts.get("out")
                    .cloned()
                    .unwrap_or_else(|| format!("data/inspiration/briefs/pill-{pill}")),
            );
            forge::brief(&db, pill, &out)
        }
        "verify" => {
            let pill: i64 = opts
                .get("pill")
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| anyhow::anyhow!("需要 --pill <id>"))?;
            let html = opts
                .get("html")
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("需要 --html <file>"))?;
            forge::verify(&db, pill, &html)
        }
        "stats" => stats(&db),
        _ => {
            eprintln!(
                "用法：ie <migrate|extract|vision|distill|refine|brew|formula|mutate|bag|pills|judge|experiment|person|kernel|motion|brief|verify|export|import|simulate|fit|stats> --db <path> [options]"
            );
            Ok(())
        }
    }
}

fn stats(db: &str) -> Result<()> {
    let conn = rusqlite::Connection::open(db)?;
    let one = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };
    println!("works        {}", one("SELECT count(*) FROM work"));
    println!("assets       {}", one("SELECT count(*) FROM asset"));
    println!("observations {}", one("SELECT count(*) FROM observation"));
    println!("measures     {}", one("SELECT count(*) FROM measure"));
    println!(
        "vectors      {}",
        one("SELECT count(DISTINCT work_id) FROM work_primitive")
    );
    println!(
        "primitives   {}",
        one("SELECT count(DISTINCT primitive_term_id) FROM work_primitive")
    );
    let mut stmt = conn.prepare("SELECT primitive, works, domain_works FROM v_primitive_coverage WHERE domain_id='ui' AND works>0 ORDER BY works DESC")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (p, w, d) = row?;
        println!("  {p:<28} {w:>4}/{d}");
    }
    Ok(())
}
