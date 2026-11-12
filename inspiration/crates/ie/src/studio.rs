//! Native, local-only studio boundary. Opening a snapshot never migrates a database.
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct StudioConfig {
    pub db: PathBuf,
    pub root: PathBuf,
    pub recipes: PathBuf,
}

#[derive(Clone, Debug)]
pub enum StudioAction {
    CreateCopy {
        target: PathBuf,
    },
    CreateEmpty {
        target: PathBuf,
    },
    RefineLocal {
        work: i64,
    },
    BrewLocal {
        work: i64,
        palette_ratio: f64,
    },
    BrewRecipe {
        path: PathBuf,
    },
    ExportBrief {
        pill: i64,
        out: PathBuf,
    },
    RecordPreference {
        experiment: i64,
        manifest_hash: String,
        winner: String,
        person: Option<String>,
        personal: bool,
        demo: bool,
    },
}

pub fn snapshot(config: &StudioConfig) -> Result<Value> {
    let mut view = json!({
        "schema_version":null, "needs_migration":false,
        "counts":{"works":0,"extracts":0,"pills":0,"experiments":0},
        "works":[], "assets":[], "extracts":[], "recipes":[], "pills":[], "experiments":[], "people":[],
        "missing_database": !config.db.try_exists()?, "demo":false, "asset_root_hint":null,
    });
    view["recipes"] = json!(recipe_files(&config.recipes)?);
    if view["missing_database"] == true {
        return Ok(view);
    }
    let mut conn = Connection::open_with_flags(&config.db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    let tx = conn.transaction()?;
    let Some(version) = schema_version(&tx)? else {
        return Ok(view);
    };
    ensure!(
        (21..=crate::db::LATEST_SCHEMA).contains(&version),
        "不支持只读浏览 schema {version}"
    );
    view["schema_version"] = json!(version);
    view["demo"] = json!(demo_database(&tx)?);
    // A hint is metadata only. Asset resolution always honors config.root; the UI
    // may explicitly reload with this hint when no CLI root was supplied.
    let root_hint: Option<String> = tx
        .query_row(
            "SELECT value FROM schema_meta WHERE key='studio_asset_root'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    view["asset_root_hint"] = json!(root_hint.filter(|p| Path::new(p).is_absolute()));
    merge_recipes(&tx, &mut view["recipes"])?;
    if version >= 24 {
        view["counts"]["experiments"] =
            json!(tx.query_row("SELECT count(*) FROM experiment", [], |r| {
                r.get::<_, i64>(0)
            })?);
        view["experiments"] = json!(experiment_metadata(&tx, &config.root)?);
    }
    view["needs_migration"] = json!(version != crate::db::LATEST_SCHEMA);
    for (key, table) in [
        ("works", "work"),
        ("extracts", "extract"),
        ("pills", "pill"),
    ] {
        let count: i64 =
            tx.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
        view["counts"][key] = json!(count);
    }
    let mut stmt = tx.prepare("SELECT w.id,w.title,w.domain_id,w.collection,
        (SELECT a.local_path FROM asset a WHERE a.work_id=w.id AND a.local_path IS NOT NULL ORDER BY a.id LIMIT 1),
        (SELECT count(*) FROM extract e WHERE e.work_id=w.id) FROM work w ORDER BY w.id DESC LIMIT 1000")?;
    view["works"] = json!(stmt.query_map([], |r| {
        let path: Option<String> = r.get(4)?;
        Ok(json!({"id":r.get::<_,i64>(0)?,"title":r.get::<_,String>(1)?,"domain":r.get::<_,String>(2)?,
            "collection":r.get::<_,String>(3)?,"image_path":existing_image(&config.root,path.as_deref()),
            "extract_count":r.get::<_,i64>(5)?}))
    })?.collect::<rusqlite::Result<Vec<_>>>()?);
    let mut stmt = tx.prepare("SELECT id,work_id,role,local_path,width,height,sha256 FROM asset ORDER BY id DESC LIMIT 1000")?;
    view["assets"] = json!(stmt.query_map([], |r| {
        let raw: Option<String> = r.get(3)?;
        Ok(json!({"id":r.get::<_,i64>(0)?,"work_id":r.get::<_,Option<i64>>(1)?,"role":r.get::<_,String>(2)?,
            "image_path":existing_image(&config.root,raw.as_deref()),"stored_path":raw,
            "width":r.get::<_,Option<i64>>(4)?,"height":r.get::<_,Option<i64>>(5)?,"sha256":r.get::<_,Option<String>>(6)?}))
    })?.collect::<rusqlite::Result<Vec<_>>>()?);
    let current = if version >= 22 {
        "EXISTS(SELECT 1 FROM v_extract_head h WHERE h.id=e.id)"
    } else {
        "0"
    };
    let mut stmt = tx.prepare(&format!(
        "SELECT e.id,e.work_id,e.dimension,e.quality,e.technique_version,e.content_json,{current}
        FROM extract e ORDER BY e.id DESC LIMIT 2000"
    ))?;
    view["extracts"] = json!(stmt.query_map([], |r| {
        Ok(json!({"id":r.get::<_,i64>(0)?,"work_id":r.get::<_,Option<i64>>(1)?,"dimension":r.get::<_,String>(2)?,
            "quality":r.get::<_,f64>(3)?,"technique_version":r.get::<_,Option<String>>(4)?,
            "content":json_text(r.get::<_,String>(5)?),"current":r.get::<_,bool>(6)?}))
    })?.collect::<rusqlite::Result<Vec<_>>>()?);
    let mut stmt = tx.prepare("SELECT p.id,coalesce(r.key,'-'),coalesce(r.name,'-'),p.output_json,p.score_json,p.created_at
        FROM pill p LEFT JOIN recipe r ON r.id=p.recipe_id ORDER BY p.id DESC LIMIT 50")?;
    view["pills"] = json!(stmt.query_map([], |r| {
        let output = json_text(r.get::<_,String>(3)?);
        let frozen = crate::forge::frozen_snapshot(&output).unwrap_or(false);
        let recipe = output["recipe_definition"]["key"].as_str().or_else(|| output["recipe"].as_str()).map(str::to_owned)
            .unwrap_or(r.get::<_,String>(1)?);
        let name = output["recipe_definition"]["name"].as_str().map(str::to_owned).unwrap_or(r.get::<_,String>(2)?);
        Ok(json!({"id":r.get::<_,i64>(0)?,"recipe":recipe,"name":name,"snapshot":frozen,
            "output":output,"score":json_text(r.get::<_,String>(4)?),"created_at":r.get::<_,String>(5)?}))
    })?.collect::<rusqlite::Result<Vec<_>>>()?);
    let mut stmt = tx.prepare("SELECT p.id,coalesce(p.display_name,p.id),
        (SELECT count(*) FROM judgment j WHERE j.actor_id=p.id) FROM person p ORDER BY p.id LIMIT 100")?;
    view["people"] = json!(stmt.query_map([], |r| {
        let all_source_records: i64 = r.get(2)?;
        Ok(json!({"id":r.get::<_,String>(0)?,"label":r.get::<_,String>(1)?,"raw":all_source_records,
            "all_source_records":all_source_records,
            "usable":0,"status":"legacy","active":false,"active_kernel":null}))
    })?.collect::<rusqlite::Result<Vec<_>>>()?);
    if version >= 25 {
        for person in view["people"].as_array_mut().unwrap() {
            let id = person["id"].as_str().unwrap();
            match crate::fit::show(
                config.db.to_str().context("数据库路径不是 UTF-8")?,
                id,
                crate::personal::SCOPE,
            ) {
                Ok(metadata) => {
                    if metadata["counts"]["raw"].is_number() {
                        person["raw"] = metadata["counts"]["raw"].clone();
                    }
                    person["usable"] = json!(metadata["counts"]["usable"].as_u64().unwrap_or(0));
                    person["status"] = metadata["status"].clone();
                    person["active"] = json!(metadata["activated"] == true);
                    person["active_kernel"] = metadata["kernels"]
                        .as_array()
                        .and_then(|kernels| {
                            kernels
                                .iter()
                                .find(|k| k["id"] == metadata["active_kernel_id"])
                        })
                        .cloned()
                        .unwrap_or(Value::Null);
                    person["dataset_counts"] = metadata["counts"].clone();
                    person["reason"] = metadata["dataset_reason"].clone();
                }
                Err(error) => {
                    person["status"] = json!("unavailable");
                    person["reason"] = json!(error.to_string());
                }
            }
        }
    }
    Ok(view)
}

fn demo_database(conn: &Connection) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key='studio_demo'",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .as_deref()
        == Some("1"))
}

fn recipe_files(root: &Path) -> Result<Vec<Value>> {
    if !root.try_exists()? {
        return Ok(Vec::new());
    }
    let mut paths = std::fs::read_dir(root)?
        .take(1000)
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort_by_key(|entry| entry.path());
    let mut recipes = Vec::new();
    for entry in paths {
        if !entry.file_type()?.is_file()
            || entry.path().extension().and_then(|e| e.to_str()) != Some("json")
        {
            continue;
        }
        let path = entry.path().canonicalize()?;
        let parsed = std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|raw| serde_json::from_str::<Value>(&raw).map_err(Into::into));
        let recipe = match parsed {
            Ok(recipe) if recipe["components"].is_array() && recipe["key"].is_string() => recipe,
            Ok(_) => continue, // Formula files are not recipes.
            Err(error) => {
                recipes.push(json!({"key":entry.file_name().to_string_lossy(),"name":entry.file_name().to_string_lossy(),
                    "path":path,"goal":"","required_dimensions":[],"valid":false,"error":error.to_string()}));
                continue;
            }
        };
        let dimensions: std::collections::BTreeSet<_> = recipe["components"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["required"].as_bool().unwrap_or(true))
            .filter_map(|c| c["dimension"].as_str())
            .collect();
        recipes.push(json!({"key":recipe["key"],"name":recipe["name"].as_str().or(recipe["key"].as_str()),
            "path":path,"goal":recipe["goal"].as_str().unwrap_or(""),"required_dimensions":dimensions,
            "definition":recipe,"valid":true}));
        if recipes.len() >= 100 {
            break;
        }
    }
    recipes.truncate(100);
    Ok(recipes)
}

fn merge_recipes(conn: &Connection, recipes: &mut Value) -> Result<()> {
    let list = recipes.as_array_mut().unwrap();
    let mut stmt = conn.prepare("SELECT id,key,name,goal FROM recipe ORDER BY key LIMIT 100")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })? {
        let (id, key, name, goal) = row?;
        if list.iter().any(|r| r["key"] == key) {
            continue;
        }
        let mut dimensions = conn.prepare("SELECT DISTINCT dimension FROM recipe_component WHERE recipe_id=?1 AND required=1 ORDER BY dimension")?;
        let required = dimensions
            .query_map([id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        list.push(json!({"key":key,"name":name,"goal":goal,"path":null,"required_dimensions":required,"database_recipe_id":id}));
    }
    list.sort_by(|a, b| a["key"].as_str().cmp(&b["key"].as_str()));
    list.truncate(100);
    Ok(())
}

fn experiment_metadata(conn: &Connection, root: &Path) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare("SELECT id,content_json,blind_mapping_json,manifest_hash,public_exports_json,created_at FROM experiment ORDER BY id DESC LIMIT 100")?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut result = Vec::new();
    for (id, content, mapping, manifest, exports, created_at) in rows {
        let content = json_text(content);
        let mapping = json_text(mapping);
        let exports = json_text(exports);
        let mut paths = [None, None];
        let mut public = false;
        for dir in exports
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let a = existing_image(root, Some(&Path::new(dir).join("A.png").to_string_lossy()));
            let b = existing_image(root, Some(&Path::new(dir).join("B.png").to_string_lossy()));
            if a.is_some() && b.is_some() {
                paths = [a, b];
                public = true;
                break;
            }
        }
        if !public {
            for (i, label) in ["A", "B"].iter().enumerate() {
                if let Some(arm) = mapping[*label].as_str() {
                    let raw: Option<String> = conn.query_row("SELECT screenshot_path FROM artifact WHERE experiment_id=?1 AND arm=?2", rusqlite::params![id,arm], |r| r.get(0)).optional()?;
                    paths[i] = existing_image(root, raw.as_deref());
                }
            }
        }
        let ready = public
            && manifest.as_deref().is_some_and(|s| !s.is_empty())
            && paths.iter().all(Option::is_some);
        let formal = ready
            && crate::experiment::eligible_evidence_in(conn, id, manifest.as_deref().unwrap())
                .is_ok();
        let summary = content["strings"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" · ")
            })
            .unwrap_or_default()
            .chars()
            .take(160)
            .collect::<String>();
        result.push(json!({"id":id,"ready":ready,"manifest_hash":manifest,"A":paths[0],"B":paths[1],
            "content_summary":summary,"formal":formal,"created_at":created_at,
            "readiness_note":"仅现有匿名截图可提交；原评价 API 会重新验证页面、回执、截图与匿名包，预检/文本分不是视觉证据"}));
    }
    Ok(result)
}

fn json_text(raw: String) -> Value {
    serde_json::from_str(&raw).unwrap_or(Value::String(raw))
}

fn schema_version(conn: &Connection) -> Result<Option<u32>> {
    let has_meta: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_meta')",
        [],
        |r| r.get(0),
    )?;
    if !has_meta {
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        ensure!(count == 0, "未知数据库没有 schema_meta，拒绝猜测");
        return Ok(None);
    }
    let raw: String = conn.query_row(
        "SELECT value FROM schema_meta WHERE key='schema_version'",
        [],
        |r| r.get(0),
    )?;
    Ok(Some(raw.parse().context("schema_version 不是合法整数")?))
}

fn existing_image(root: &Path, raw: Option<&str>) -> Option<String> {
    let path = root.join(raw?);
    let path = path.canonicalize().ok()?;
    if !path.is_file() || image::image_dimensions(&path).is_err() {
        return None;
    }
    path.to_str().map(str::to_owned)
}

pub fn execute(config: &StudioConfig, action: &StudioAction) -> Result<Value> {
    if let StudioAction::CreateCopy { target } = action {
        let source = Connection::open_with_flags(&config.db, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .context("源数据库不存在或不可只读打开，不会创建源库")?;
        let version = schema_version(&source)?.context("源数据库为空，不能作为工作副本")?;
        ensure!(
            (21..=crate::db::LATEST_SCHEMA).contains(&version),
            "不支持复制 schema {version}"
        );
        let target = reserve_database(target)?;
        {
            let mut destination =
                Connection::open_with_flags(&target, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
            let backup = rusqlite::backup::Backup::new(&source, &mut destination)?;
            ensure!(
                backup.step(-1)? == rusqlite::backup::StepResult::Done,
                "源库繁忙，一致复制未完成；不会迁移源库"
            );
        }
        let backup = crate::db::migrate_path(&target)?;
        store_asset_root_hint(&target, &config.root)?;
        return Ok(
            json!({"message":"已一致复制并迁移新工作副本；源库未迁移", "switch_db":target,"backup":backup}),
        );
    }
    if let StudioAction::CreateEmpty { target } = action {
        let target = reserve_database(target)?;
        let backup = crate::db::migrate_path(&target)?;
        let root = target.parent().context("新库缺少父目录")?;
        let root = store_asset_root_hint(&target, root)?;
        return Ok(
            json!({"message":"已显式创建空白工作库", "switch_db":target,"switch_root":root,"backup":backup}),
        );
    }
    crate::db::require_current(&config.db)?;
    if let StudioAction::RefineLocal { work } = action {
        let ids = refine_local(config, *work)?;
        return Ok(json!({"message":"已本地像素提炼色彩与间距；未调用模型", "extract_ids":ids}));
    }
    if let StudioAction::BrewLocal {
        work,
        palette_ratio,
    } = action
    {
        ensure!(
            palette_ratio.is_finite() && (0.0..=1.0).contains(palette_ratio),
            "色彩配比必须为 0..1 的有限数值"
        );
        let ids = refine_local(config, *work)?;
        let conn = Connection::open_with_flags(&config.db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let domain: String =
            conn.query_row("SELECT domain_id FROM work WHERE id=?1", [work], |r| {
                r.get(0)
            })?;
        let mut pins = std::collections::HashMap::new();
        for id in &ids {
            let (dimension, actual_work, fire): (String, Option<i64>, String) = conn.query_row(
                "SELECT dimension,work_id,fire_model FROM extract WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            ensure!(
                actual_work == Some(*work)
                    && fire == "pixel-proxy"
                    && ["palette", "spacing"].contains(&dimension.as_str()),
                "本次药液不是所选素材的像素二维药液"
            );
            ensure!(
                pins.insert(dimension, *id).is_none(),
                "本次返回重复药液维度"
            );
        }
        ensure!(
            pins.len() == 2 && pins.contains_key("palette") && pins.contains_key("spacing"),
            "本次提炼没有完整色彩与间距药液，拒绝猜测替代"
        );
        drop(conn);
        let recipe = json!({"key":"studio.local-color-space","name":"本地色彩 × 间距","version":"1",
            "goal":"只使用所选素材的像素色彩与间距代理；不推断版式或字体，不生成页面",
            "domain":domain,"components":[
                {"slot":"base","dimension":"palette","ratio":palette_ratio,"required":true},
                {"slot":"base","dimension":"spacing","ratio":1.0-palette_ratio,"required":true}],
            "constraints":{"contrast_min":4.5},
            "combination_prompt":"色彩比例 {{base.palette.ratio}}：{{base.palette.content}}。间距比例 {{base.spacing.ratio}}：{{base.spacing.content}}。仅为像素测量的二维简报，不包含 layout/type，也不是成品视觉评审。"});
        let id = crate::alchemy::brew_value(
            &crate::alchemy::BrewOpts {
                db: config.db.to_str().context("数据库路径不是 UTF-8")?,
                recipe: Path::new("<studio-local>"),
                pinned: Some(&pins),
                formula_id: None,
                allow_mixed: false,
                force: false,
                out_dir: None,
                quiet: true,
            },
            recipe,
            None,
        )?;
        let trace = json!({"source":"studio_local_controls","work_id":work,"palette_ratio":palette_ratio,
            "spacing_ratio":1.0-palette_ratio,"extract_ids":ids,"selection":"actual_refine_return_ids",
            "visual_evaluation":false,"personal_preference":false});
        return finish_brew(config, id, Some(trace));
    }
    if let StudioAction::BrewRecipe { path } = action {
        let recipe: Value =
            serde_json::from_str(&std::fs::read_to_string(path).context("读取已有配方失败")?)?;
        let id = crate::alchemy::brew_value(
            &crate::alchemy::BrewOpts {
                db: config.db.to_str().context("数据库路径不是 UTF-8")?,
                recipe: path,
                pinned: None,
                formula_id: None,
                allow_mixed: false,
                force: false,
                out_dir: None,
                quiet: true,
            },
            recipe,
            None,
        )?;
        return finish_brew(config, id, None);
    }
    if let StudioAction::ExportBrief { pill, out } = action {
        // The reused exporter may overwrite its own filenames, so never give it an
        // existing user directory. Only the new, exclusively-created child is writable.
        if !out.try_exists()? {
            std::fs::create_dir(out).context("导出目录父路径必须存在")?;
        }
        let root = out.canonicalize()?;
        ensure!(root.is_dir(), "导出路径必须是目录");
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let destination = root.join(format!("pill-{pill}-brief-{stamp}"));
        std::fs::create_dir(&destination).context("简报必须写入全新子目录")?;
        crate::forge::brief(
            config.db.to_str().context("数据库路径不是 UTF-8")?,
            *pill,
            &destination,
        )?;
        return Ok(
            json!({"message":"简报已导出至新子目录，未覆盖已有文件；不是生成页面", "pill_id":pill,
            "out_dir":destination,"files":[destination.join("brief.md"),destination.join("tokens.json")]}),
        );
    }
    if let StudioAction::RecordPreference {
        experiment,
        manifest_hash,
        winner,
        person,
        personal,
        demo,
    } = action
    {
        ensure!(
            ["A", "B", "tie", "abstain"].contains(&winner.as_str()),
            "winner 只能为 A/B/tie/abstain"
        );
        let conn = Connection::open_with_flags(&config.db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let demo = *demo || demo_database(&conn)?;
        drop(conn);
        let db = config.db.to_str().context("数据库路径不是 UTF-8")?;
        let reviewer = person.as_deref().map(str::trim).filter(|s| !s.is_empty());
        if !demo {
            ensure!(
                reviewer.is_some() && reviewer == person.as_deref(),
                "真实视觉评审请先设置稳定、非空且无首尾空格的本机 reviewer；不会伪造 human"
            );
        }
        let visual = crate::experiment::RecordOpts {
            db,
            experiment_id: *experiment,
            manifest_hash,
            source: if demo { "demo" } else { "human" },
            reviewer,
            dimension: "visual",
            winner,
            note: Some(if demo {
                "Studio demo only; excluded from human/private evidence"
            } else {
                "Explicit local GUI visual choice"
            }),
        };
        let mut result = if *personal && !demo {
            let person = reviewer.context("个人用途需要明确本机身份")?;
            // Do not even initialize consent for absent/tampered/mock screenshot evidence.
            crate::experiment::eligible_evidence(db, *experiment, manifest_hash)?;
            crate::personal::init(&crate::personal::InitOpts { db, person })?;
            crate::personal::record(&crate::personal::RecordOpts {
                visual,
                person,
                intent: "personal_preference",
            })?
        } else {
            crate::experiment::record(&visual)?
        };
        result["message"] = json!(if demo {
            "已记录演示评价，不进入真人或私有个人偏好"
        } else if *personal && result["bridge_created"] != true && result["already_bridged"] != true
        {
            "视觉评价已保存，但个人桥接未完成；请查看 bridge_reason，未自动拟合"
        } else if *personal {
            "已按明确本机个人用途记录；未自动拟合内核"
        } else {
            "已记录命名视觉评价，未用于私有个人偏好"
        });
        result["personal"] = json!(*personal && !demo);
        return Ok(result);
    }
    bail!("不支持的工作台操作")
}

fn finish_brew(config: &StudioConfig, id: i64, trace: Option<Value>) -> Result<Value> {
    let conn = Connection::open_with_flags(&config.db, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let raw: String = conn.query_row("SELECT output_json FROM pill WHERE id=?1", [id], |r| {
        r.get(0)
    })?;
    let mut output: Value = serde_json::from_str(&raw)?;
    if let Some(trace) = trace {
        output["studio_selection"] = trace;
        conn.execute(
            "UPDATE pill SET output_json=?1 WHERE id=?2",
            rusqlite::params![output.to_string(), id],
        )?;
    }
    Ok(json!({"message":"已严格炼成冻结规格；预检分不是成品视觉分", "pill_id":id,"output":output}))
}

fn refine_local(config: &StudioConfig, work: i64) -> Result<Vec<i64>> {
    let conn = Connection::open_with_flags(&config.db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    // Validate both contracts before invoking a routine capable of remote inference.
    for (key, dimension) in [("t.palette", "palette"), ("t.spacing", "spacing")] {
        let (actual, params): (String, String) = conn.query_row(
            "SELECT dimension,params_json FROM technique WHERE key=?1 AND status='active'",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(
            actual == dimension && serde_json::from_str::<Value>(&params)?["kind"] == "pixel",
            "本地提炼只允许原生 pixel {key}/{dimension} 手法，不调用模型"
        );
    }
    drop(conn);
    crate::alchemy::refine_selected(&crate::alchemy::RefineOpts {
        db: config.db.to_str().context("数据库路径不是 UTF-8")?,
        root: &config.root,
        work,
        techniques: vec!["t.palette".into(), "t.spacing".into()],
        provider: "local",
        model: "pixel-proxy",
        base_url: "",
        thinking: 0,
        to_bag: true,
        refresh: false,
    })
}

/// Called only after explicitly creating and migrating a new destination.
fn store_asset_root_hint(db: &Path, root: &Path) -> Result<PathBuf> {
    let root = std::path::absolute(root)?;
    let text = root.to_str().context("素材根路径不是 UTF-8")?;
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    conn.execute(
        "INSERT INTO schema_meta(key,value) VALUES('studio_asset_root',?1)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [text],
    )?;
    Ok(root)
}

fn reserve_database(path: &Path) -> Result<PathBuf> {
    let name = path.file_name().context("目标必须含数据库文件名")?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent
        .canonicalize()
        .context("目标父目录必须已存在且可靠")?;
    ensure!(parent.is_dir(), "目标父路径不是目录");
    let target = parent.join(name);
    // create_new rejects existing files, directories and dangling symlinks atomically.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .context("目标必须不存在，拒绝覆盖任何已有路径")?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        dir: PathBuf,
        config: StudioConfig,
    }
    impl Fixture {
        fn new() -> Result<Self> {
            let dir = std::env::temp_dir().canonicalize()?.join(format!(
                "ie-studio-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&dir)?;
            Ok(Self {
                config: StudioConfig {
                    db: dir.join("studio.db"),
                    root: dir.clone(),
                    recipes: dir.join("recipes"),
                },
                dir,
            })
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            // Only remove the unique directory allocated by this test fixture.
            if self.dir.canonicalize().ok().as_ref() == Some(&self.dir)
                && self.dir.parent() == std::env::temp_dir().canonicalize().ok().as_deref()
                && self
                    .dir
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("ie-studio-"))
            {
                let _ = std::fs::remove_dir_all(&self.dir);
            }
        }
    }

    fn database(fixture: &Fixture, version: u32) -> Result<rusqlite::Connection> {
        let mut conn = rusqlite::Connection::open(&fixture.config.db)?;
        crate::db::migrate_to(&mut conn, version)?;
        conn.execute_batch("INSERT INTO pack(id,title,version) VALUES('studio-fixture','Fixture','1');
            INSERT INTO domain(id,title,medium,pack_id) VALUES('studio-fixture','Fixture','image','studio-fixture');
            INSERT INTO source(id,kind,title,created_at) VALUES('studio-fixture','manual','Fixture','now');
            INSERT INTO work(id,source_id,domain_id,collection,slug,canonical_url,title,first_seen_at,last_seen_at)
            VALUES(1,'studio-fixture','studio-fixture','local','one','fixture://one','One','now','now');")?;
        Ok(conn)
    }

    fn png_work(fixture: &Fixture) -> Result<()> {
        let path = fixture.dir.join("pixels.png");
        let image = image::RgbImage::from_fn(160, 120, |x, y| {
            if (20..100).contains(&x) && (20..90).contains(&y) {
                image::Rgb([16, 16, 16])
            } else {
                image::Rgb([250, 250, 250])
            }
        });
        image.save(&path)?;
        let conn = Connection::open(&fixture.config.db)?;
        conn.execute(
            "INSERT INTO asset(work_id,role,local_path) VALUES(1,'poster','pixels.png')",
            [],
        )?;
        Ok(())
    }

    fn experiment_fixture(fixture: &Fixture) -> Result<i64> {
        let a = execute(
            &fixture.config,
            &StudioAction::BrewLocal {
                work: 1,
                palette_ratio: 0.6,
            },
        )?["pill_id"]
            .as_i64()
            .unwrap();
        let b = execute(
            &fixture.config,
            &StudioAction::BrewLocal {
                work: 1,
                palette_ratio: 0.4,
            },
        )?["pill_id"]
            .as_i64()
            .unwrap();
        Ok(crate::experiment::create(&crate::experiment::CreateOpts {
            db: fixture.config.db.to_str().unwrap(),
            pill_a: a,
            pill_b: b,
            content: &json!({"strings":["Fixture title","Fixture body"]}),
            conditions: &json!({"viewport":{"width":160,"height":120},"theme":"light",
                "task":"same fixture","generator":{"kind":"mock","version":"test-only"}}),
        })?["id"]
            .as_i64()
            .unwrap())
    }

    struct MockRenderer;
    impl crate::capture::Renderer for MockRenderer {
        fn render(
            &self,
            _html: &Path,
            screenshot: &Path,
            spec: &crate::capture::CaptureSpec,
        ) -> Result<Value> {
            image::RgbImage::from_fn(spec.width, spec.height, |x, y| {
                if (20..100).contains(&x) && (20..90).contains(&y) {
                    image::Rgb([16, 16, 16])
                } else {
                    image::Rgb([250, 250, 250])
                }
            })
            .save(screenshot)?;
            Ok(json!({"kind":"studio-test-mock","version":"test-only"}))
        }
    }

    fn blind_pair(fixture: &Fixture) -> Result<(i64, String)> {
        let experiment = experiment_fixture(fixture)?;
        let archive = fixture.dir.join("archive");
        for arm in ["a", "b"] {
            let html = fixture.dir.join(format!("{arm}.html"));
            std::fs::write(
                &html,
                "<!doctype html><html><body><h1>Fixture title</h1><p>Fixture body</p></body></html>",
            )?;
            crate::experiment::attach_with_renderer(
                &crate::experiment::AttachOpts {
                    db: fixture.config.db.to_str().unwrap(),
                    experiment_id: experiment,
                    arm,
                    html: &html,
                    out_dir: &archive,
                    chrome: None,
                },
                &MockRenderer,
            )?;
        }
        let blind = crate::experiment::blind(&crate::experiment::BlindOpts {
            db: fixture.config.db.to_str().unwrap(),
            experiment_id: experiment,
            out_dir: &fixture.dir.join("public"),
        })?;
        Ok((
            experiment,
            blind["manifest_hash"].as_str().unwrap().to_owned(),
        ))
    }

    #[test]
    fn local_refine_rejects_changed_remote_contract_before_any_partial_write() -> Result<()> {
        let fixture = Fixture::new()?;
        {
            let conn = database(&fixture, crate::db::LATEST_SCHEMA)?;
            conn.execute(
                "UPDATE technique SET params_json='{}' WHERE key='t.spacing'",
                [],
            )?;
        }
        png_work(&fixture)?;
        let before = std::fs::read(&fixture.config.db)?;
        let error = execute(&fixture.config, &StudioAction::RefineLocal { work: 1 }).unwrap_err();
        assert!(error.to_string().contains("pixel"));
        assert_eq!(snapshot(&fixture.config)?["counts"]["extracts"], 0);
        assert_eq!(before, std::fs::read(&fixture.config.db)?);
        Ok(())
    }

    #[test]
    fn asset_browser_reports_existing_pixels_and_missing_images_without_writing() -> Result<()> {
        let fixture = Fixture::new()?;
        let conn = database(&fixture, 21)?;
        png_work(&fixture)?;
        conn.execute("INSERT INTO asset(work_id,role,local_path,width,height) VALUES(1,'screenshot','absent.png',640,480)", [])?;
        drop(conn);
        let before = std::fs::read(&fixture.config.db)?;
        let view = snapshot(&fixture.config)?;
        let assets = view["assets"]
            .as_array()
            .expect("asset metadata remains browsable");
        assert_eq!(assets.len(), 2);
        assert!(assets[0]["image_path"].is_null());
        assert_eq!(assets[0]["width"], 640);
        assert_eq!(
            assets[1]["image_path"],
            fixture.dir.join("pixels.png").to_string_lossy().as_ref()
        );
        assert_eq!(before, std::fs::read(&fixture.config.db)?);
        Ok(())
    }

    #[test]
    fn failed_copy_migration_preserves_source_and_rejects_symlink_targets() -> Result<()> {
        let fixture = Fixture::new()?;
        {
            let conn = database(&fixture, 21)?;
            conn.pragma_update(None, "foreign_keys", false)?;
            conn.execute(
                "INSERT INTO inventory(extract_id,added_at) VALUES(999999,'invalid fixture')",
                [],
            )?;
        }
        let before = std::fs::read(&fixture.config.db)?;
        let target = fixture.dir.join("failed-copy.db");
        assert!(
            execute(
                &fixture.config,
                &StudioAction::CreateCopy {
                    target: target.clone()
                }
            )
            .is_err()
        );
        assert_eq!(snapshot(&fixture.config)?["schema_version"], 21);
        assert_eq!(
            snapshot(&StudioConfig {
                db: target,
                ..fixture.config.clone()
            })?["schema_version"],
            21
        );
        assert_eq!(before, std::fs::read(&fixture.config.db)?);
        #[cfg(unix)]
        {
            let absent = fixture.dir.join("never-create.db");
            let link = fixture.dir.join("dangling.db");
            std::os::unix::fs::symlink(&absent, &link)?;
            assert!(execute(&fixture.config, &StudioAction::CreateEmpty { target: link }).is_err());
            assert!(!absent.exists());
        }
        Ok(())
    }

    #[test]
    fn local_brew_rejects_nonfinite_ratios_before_writes_and_does_not_force_low_contrast()
    -> Result<()> {
        let fixture = Fixture::new()?;
        drop(database(&fixture, crate::db::LATEST_SCHEMA)?);
        png_work(&fixture)?;
        let before = std::fs::read(&fixture.config.db)?;
        for ratio in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01, 1.01] {
            assert!(
                execute(
                    &fixture.config,
                    &StudioAction::BrewLocal {
                        work: 1,
                        palette_ratio: ratio
                    }
                )
                .is_err()
            );
            assert_eq!(before, std::fs::read(&fixture.config.db)?);
        }
        for ratio in [0.0, 1.0] {
            execute(
                &fixture.config,
                &StudioAction::BrewLocal {
                    work: 1,
                    palette_ratio: ratio,
                },
            )?;
        }
        image::RgbImage::from_pixel(160, 120, image::Rgb([128u8, 128, 128]))
            .save(fixture.dir.join("pixels.png"))?;
        assert!(
            execute(
                &fixture.config,
                &StudioAction::BrewLocal {
                    work: 1,
                    palette_ratio: 0.5
                }
            )
            .is_err()
        );
        assert_eq!(snapshot(&fixture.config)?["counts"]["pills"], 2);
        Ok(())
    }

    #[test]
    fn schema21_through25_browse_versions_without_querying_newer_columns_or_writing() -> Result<()>
    {
        for version in 21..=25 {
            let fixture = Fixture::new()?;
            {
                let conn = database(&fixture, version)?;
                png_work(&fixture)?;
                conn.execute("INSERT INTO extract(work_id,technique_id,dimension,content_json,quality,fire_model,hash,created_at,technique_version)
                    SELECT 1,id,'palette','{\"legacy\":true}',0.7,'pixel-proxy','legacy','then',NULL FROM technique WHERE key='t.palette'", [])?;
                conn.execute(
                    "INSERT INTO pill(output_md,created_at) VALUES('legacy','then')",
                    [],
                )?;
            }
            let before = std::fs::read(&fixture.config.db)?;
            let view = snapshot(&fixture.config)?;
            assert_eq!(view["schema_version"], version);
            assert_eq!(view["needs_migration"], version < 25);
            assert_eq!(view["works"][0]["extract_count"], 1);
            assert!(view["works"][0]["image_path"].as_str().is_some());
            assert!(view["extracts"][0]["technique_version"].is_null());
            assert_eq!(view["extracts"][0]["current"], false);
            assert_eq!(view["extracts"][0]["content"], json!({"legacy":true}));
            assert_eq!(view["pills"][0]["snapshot"], false);
            if version < 25 {
                assert!(execute(&fixture.config, &StudioAction::RefineLocal { work: 1 }).is_err());
            }
            assert_eq!(before, std::fs::read(&fixture.config.db)?);
        }
        Ok(())
    }

    #[test]
    fn invalid_visual_evidence_or_missing_reviewer_never_records_or_initializes_person()
    -> Result<()> {
        let fixture = Fixture::new()?;
        drop(database(&fixture, crate::db::LATEST_SCHEMA)?);
        png_work(&fixture)?;
        let (experiment, hash) = blind_pair(&fixture)?;
        let action = |manifest_hash: String, person: Option<String>, personal, demo| {
            StudioAction::RecordPreference {
                experiment,
                manifest_hash,
                winner: "A".into(),
                person,
                personal,
                demo,
            }
        };
        let missing_actor =
            execute(&fixture.config, &action(hash.clone(), None, false, false)).unwrap_err();
        assert!(missing_actor.to_string().contains("reviewer"));
        // Mock screenshots are never elevated to formal personal evidence, even
        // after an explicit personal click. There is no synthetic human fixture.
        assert!(
            execute(
                &fixture.config,
                &action(hash.clone(), Some("test-only-person".into()), true, false)
            )
            .is_err()
        );
        assert!(
            execute(
                &fixture.config,
                &action("wrong-manifest".into(), None, false, true)
            )
            .is_err()
        );
        std::fs::write(fixture.dir.join("public/A.png"), b"not an image")?;
        assert!(execute(&fixture.config, &action(hash, None, false, true)).is_err());
        assert_eq!(snapshot(&fixture.config)?["people"], json!([]));
        let stats = crate::experiment::stats(&crate::experiment::StatsOpts {
            db: fixture.config.db.to_str().unwrap(),
        })?;
        assert_eq!(stats["history"], json!({"human":0,"demo":0,"ai":0}));
        Ok(())
    }

    #[test]
    fn marked_demo_workspace_forces_demo_even_when_personal_human_controls_are_requested()
    -> Result<()> {
        let fixture = Fixture::new()?;
        let conn = database(&fixture, crate::db::LATEST_SCHEMA)?;
        png_work(&fixture)?;
        let (experiment, manifest_hash) = blind_pair(&fixture)?;
        conn.execute(
            "INSERT INTO schema_meta(key,value) VALUES('studio_demo','1')",
            [],
        )?;
        let result = execute(
            &fixture.config,
            &StudioAction::RecordPreference {
                experiment,
                manifest_hash,
                winner: "A".into(),
                person: Some("must-not-be-created".into()),
                personal: true,
                demo: false,
            },
        )?;
        assert_eq!(result["source"], "demo");
        assert_eq!(result["formal_eligible"], false);
        let view = snapshot(&fixture.config)?;
        assert_eq!(view["people"], json!([]));
        assert_eq!(view["experiments"][0]["ready"], true);
        assert_eq!(view["experiments"][0]["formal"], false);
        assert!(
            view["experiments"][0]["A"]
                .as_str()
                .unwrap()
                .ends_with("A.png")
        );
        let stats = crate::experiment::stats(&crate::experiment::StatsOpts {
            db: fixture.config.db.to_str().unwrap(),
        })?;
        assert_eq!(stats["history"]["demo"], 1);
        assert_eq!(stats["history"]["human"], 0);
        assert_eq!(stats["formal_visual"]["evaluations"], 0);
        Ok(())
    }

    #[test]
    fn snapshot_separates_personal_raw_counts_from_mixed_source_audit_totals() -> Result<()> {
        let fixture = Fixture::new()?;
        let conn = database(&fixture, crate::db::LATEST_SCHEMA)?;
        crate::personal::init(&crate::personal::InitOpts {
            db: fixture.config.db.to_str().unwrap(),
            person: "snapshot-import-fixture",
        })?;
        conn.execute_batch("INSERT INTO work(id,source_id,domain_id,collection,slug,canonical_url,title,first_seen_at,last_seen_at)
            VALUES(2,'studio-fixture','studio-fixture','local','two','fixture://two','Two','then','then');
            WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<200)
            INSERT INTO judgment(work_id,opponent_work_id,kind,outcome,created_at,actor_id,visibility,source,intent,scope,note)
            SELECT 1,2,'pairwise','a_wins','then','snapshot-import-fixture','private','demo','personal_preference','ui-static','Synthetic import test only' FROM n;
            INSERT INTO judgment(work_id,opponent_work_id,kind,outcome,created_at,actor_id,visibility,source,intent,scope,note)
            VALUES(1,2,'pairwise','a_wins','then','snapshot-import-fixture','private','human','personal_preference','ui-static','Untrusted imported label; no visual evaluation or evidence');")?;
        drop(conn);
        let before = std::fs::read(&fixture.config.db)?;
        let view = snapshot(&fixture.config)?;
        let person = &view["people"][0];
        assert_eq!(person["raw"], 1);
        assert_eq!(person["all_source_records"], 201);
        assert_eq!(person["dataset_counts"]["excluded_demo"], 200);
        // A claimed human source alone never makes this import usable evidence.
        assert_eq!(person["dataset_counts"]["excluded_invalid"], 1);
        assert_eq!(person["usable"], 0);
        assert_eq!(person["status"], "cold_start");
        assert_eq!(person["active"], false);
        assert_eq!(before, std::fs::read(&fixture.config.db)?);
        Ok(())
    }

    #[test]
    fn current_snapshot_lists_recipe_experiment_and_person_metadata_without_fake_visual()
    -> Result<()> {
        let fixture = Fixture::new()?;
        let conn = database(&fixture, crate::db::LATEST_SCHEMA)?;
        png_work(&fixture)?;
        experiment_fixture(&fixture)?;
        conn.execute("INSERT INTO person(id,display_name,consent_json,created_at) VALUES('demo-person','Demo','{\"purpose\":\"demo\"}','then')", [])?;
        conn.execute(
            "INSERT INTO schema_meta(key,value) VALUES('studio_demo','1')",
            [],
        )?;
        std::fs::create_dir(&fixture.config.recipes)?;
        std::fs::write(fixture.config.recipes.join("fixture.recipe.json"), json!({
            "key":"recipe.fixture","name":"Fixture recipe","goal":"inspect only",
            "components":[{"dimension":"palette","required":true},{"dimension":"type","required":false}]
        }).to_string())?;
        let before = std::fs::read(&fixture.config.db)?;
        let view = snapshot(&fixture.config)?;
        assert_eq!(view["demo"], true);
        assert_eq!(view["counts"]["experiments"], 1);
        assert_eq!(view["experiments"][0]["ready"], false);
        assert_eq!(view["experiments"][0]["formal"], false);
        assert!(view["experiments"][0]["A"].is_null());
        assert!(view["experiments"][0]["B"].is_null());
        assert_eq!(view["recipes"][0]["key"], "recipe.fixture");
        assert_eq!(
            view["recipes"][0]["required_dimensions"],
            json!(["palette"])
        );
        assert!(Path::new(view["recipes"][0]["path"].as_str().unwrap()).is_absolute());
        assert_eq!(view["people"][0]["active"], false);
        assert_eq!(view["people"][0]["usable"], 0);
        assert_eq!(view["people"][0]["status"], "dataset_unavailable");
        assert_eq!(before, std::fs::read(&fixture.config.db)?);
        Ok(())
    }

    #[test]
    fn brief_export_uses_fresh_subdirectory_without_overwriting_unknown_files() -> Result<()> {
        let fixture = Fixture::new()?;
        drop(database(&fixture, crate::db::LATEST_SCHEMA)?);
        png_work(&fixture)?;
        let pill = execute(
            &fixture.config,
            &StudioAction::BrewLocal {
                work: 1,
                palette_ratio: 0.6,
            },
        )?["pill_id"]
            .as_i64()
            .unwrap();
        let out = fixture.dir.join("exports");
        std::fs::create_dir(&out)?;
        std::fs::write(out.join("brief.md"), "user-owned brief")?;
        std::fs::write(out.join("tokens.json"), "user-owned tokens")?;
        let first = execute(
            &fixture.config,
            &StudioAction::ExportBrief {
                pill,
                out: out.clone(),
            },
        )?;
        let second = execute(
            &fixture.config,
            &StudioAction::ExportBrief {
                pill,
                out: out.clone(),
            },
        )?;
        assert_ne!(first["out_dir"], second["out_dir"]);
        assert_eq!(
            std::fs::read_to_string(out.join("brief.md"))?,
            "user-owned brief"
        );
        assert_eq!(
            std::fs::read_to_string(out.join("tokens.json"))?,
            "user-owned tokens"
        );
        let exported = PathBuf::from(first["out_dir"].as_str().unwrap());
        assert_eq!(exported.parent(), Some(out.as_path()));
        assert!(exported.join("brief.md").is_file());
        let tokens: Value =
            serde_json::from_str(&std::fs::read_to_string(exported.join("tokens.json"))?)?;
        assert_eq!(tokens["provenance"]["historically_reproducible"], true);
        Ok(())
    }

    #[test]
    fn recipe_brew_returns_actual_pill_and_keeps_missing_dimension_constraints() -> Result<()> {
        let fixture = Fixture::new()?;
        drop(database(&fixture, crate::db::LATEST_SCHEMA)?);
        png_work(&fixture)?;
        execute(&fixture.config, &StudioAction::RefineLocal { work: 1 })?;
        let path = fixture.dir.join("test.recipe.json");
        let mut recipe = json!({"key":"studio.test","name":"Test","goal":"Fixture",
            "components":[{"slot":"base","dimension":"palette","ratio":1.0,"required":true}],
            "combination_prompt":"{{base.palette.content}}","constraints":{"contrast_min":4.5}});
        std::fs::write(&path, recipe.to_string())?;
        let result = execute(
            &fixture.config,
            &StudioAction::BrewRecipe { path: path.clone() },
        )?;
        assert_eq!(
            snapshot(&fixture.config)?["pills"][0]["id"],
            result["pill_id"]
        );
        assert_eq!(result["output"]["recipe_definition"], recipe);
        recipe["components"][0]["dimension"] = json!("layout");
        std::fs::write(&path, recipe.to_string())?;
        assert!(execute(&fixture.config, &StudioAction::BrewRecipe { path }).is_err());
        assert_eq!(snapshot(&fixture.config)?["counts"]["pills"], 1);
        Ok(())
    }

    #[test]
    fn local_brew_pins_this_refinement_not_largest_ids_and_freezes_trace() -> Result<()> {
        let fixture = Fixture::new()?;
        let conn = database(&fixture, crate::db::LATEST_SCHEMA)?;
        png_work(&fixture)?;
        let first = execute(&fixture.config, &StudioAction::RefineLocal { work: 1 })?;
        conn.execute(
            "UPDATE technique SET version='studio-new' WHERE key='t.palette'",
            [],
        )?;
        let newer = execute(&fixture.config, &StudioAction::RefineLocal { work: 1 })?;
        assert_ne!(first["extract_ids"], newer["extract_ids"]);
        conn.execute("UPDATE technique SET version='1' WHERE key='t.palette'", [])?;
        let result = execute(
            &fixture.config,
            &StudioAction::BrewLocal {
                work: 1,
                palette_ratio: 0.65,
            },
        )?;
        assert!(result["pill_id"].as_i64().unwrap() > 0);
        let out = &result["output"];
        assert_eq!(out["snapshot_version"], 1);
        assert_eq!(out["recipe_definition"]["key"], "studio.local-color-space");
        assert_eq!(out["components"].as_array().unwrap().len(), 2);
        for row in out["components"].as_array().unwrap() {
            assert_eq!(row["work_id"], 1);
            assert_eq!(row["fire"], "pixel-proxy");
            assert!(
                first["extract_ids"]
                    .as_array()
                    .unwrap()
                    .contains(&row["extract_id"])
            );
            assert!(!row["content"].is_null());
            assert!(!row["inputs"].is_null());
        }
        assert_eq!(out["studio_selection"]["palette_ratio"], 0.65);
        assert_eq!(out["studio_selection"]["extract_ids"], first["extract_ids"]);
        assert!(out["personal_selection"].is_null());
        let view = snapshot(&fixture.config)?;
        assert_eq!(view["pills"][0]["snapshot"], true);
        assert_eq!(view["pills"][0]["output"], *out);
        Ok(())
    }

    #[test]
    fn local_refine_produces_only_two_pixel_dimensions_and_reuses_exact_ids() -> Result<()> {
        let fixture = Fixture::new()?;
        drop(database(&fixture, crate::db::LATEST_SCHEMA)?);
        png_work(&fixture)?;
        let first = execute(&fixture.config, &StudioAction::RefineLocal { work: 1 })?;
        let second = execute(&fixture.config, &StudioAction::RefineLocal { work: 1 })?;
        assert_eq!(first["extract_ids"], second["extract_ids"]);
        assert_eq!(first["extract_ids"].as_array().unwrap().len(), 2);
        let view = snapshot(&fixture.config)?;
        assert_eq!(view["counts"]["extracts"], 2);
        for row in view["extracts"].as_array().unwrap() {
            assert!(["palette", "spacing"].contains(&row["dimension"].as_str().unwrap()));
            assert_eq!(row["current"], true);
        }
        assert_eq!(
            view["works"][0]["image_path"],
            fixture.dir.join("pixels.png").to_string_lossy().as_ref()
        );
        Ok(())
    }

    #[test]
    fn empty_workspace_requires_a_new_target_and_never_creates_source() -> Result<()> {
        let fixture = Fixture::new()?;
        let empty_root = fixture.dir.join("new-workspaces");
        std::fs::create_dir(&empty_root)?;
        let target = empty_root.join("empty.db");
        let result = execute(
            &fixture.config,
            &StudioAction::CreateEmpty {
                target: target.clone(),
            },
        )?;
        assert_eq!(result["switch_db"], target.to_string_lossy().as_ref());
        assert_eq!(result["switch_root"], empty_root.to_string_lossy().as_ref());
        let current = StudioConfig {
            db: target.clone(),
            ..fixture.config.clone()
        };
        assert_eq!(
            snapshot(&current)?["schema_version"],
            crate::db::LATEST_SCHEMA
        );
        assert_eq!(snapshot(&current)?["counts"]["works"], 0);
        assert_eq!(
            snapshot(&current)?["asset_root_hint"],
            empty_root.to_string_lossy().as_ref()
        );
        let before = std::fs::read(&target)?;
        assert!(
            execute(
                &fixture.config,
                &StudioAction::CreateEmpty {
                    target: target.clone()
                }
            )
            .is_err()
        );
        assert_eq!(before, std::fs::read(&target)?);
        assert!(
            execute(
                &fixture.config,
                &StudioAction::CreateEmpty {
                    target: fixture.dir.join("missing-parent/empty.db")
                }
            )
            .is_err()
        );
        assert!(
            execute(
                &fixture.config,
                &StudioAction::CreateCopy {
                    target: fixture.dir.join("absent-copy.db")
                }
            )
            .is_err()
        );
        assert!(!fixture.config.db.exists());
        assert!(!fixture.dir.join("absent-copy.db").exists());
        Ok(())
    }

    #[test]
    fn copied_workspace_exposes_asset_root_hint_without_overriding_explicit_root_or_source()
    -> Result<()> {
        use sha2::{Digest, Sha256};
        let fixture = Fixture::new()?;
        drop(database(&fixture, 21)?);
        png_work(&fixture)?;
        let original_hash = format!("{:x}", Sha256::digest(std::fs::read(&fixture.config.db)?));
        let copies = fixture.dir.join("copies");
        std::fs::create_dir(&copies)?;
        let target = copies.join("copy.db");
        let result = execute(
            &fixture.config,
            &StudioAction::CreateCopy {
                target: target.clone(),
            },
        )?;
        assert!(
            result.get("switch_root").is_none(),
            "copy preserves the current source root"
        );
        let reopened = StudioConfig {
            db: target,
            root: copies,
            ..fixture.config.clone()
        };
        let view = snapshot(&reopened)?;
        assert_eq!(
            view["asset_root_hint"],
            fixture.dir.to_string_lossy().as_ref()
        );
        assert!(Path::new(view["asset_root_hint"].as_str().unwrap()).is_absolute());
        assert!(
            view["works"][0]["image_path"].is_null(),
            "snapshot must not silently replace an explicit root"
        );
        let resolved = StudioConfig {
            root: PathBuf::from(view["asset_root_hint"].as_str().unwrap()),
            ..reopened
        };
        assert_eq!(
            snapshot(&resolved)?["works"][0]["image_path"],
            fixture.dir.join("pixels.png").to_string_lossy().as_ref()
        );
        assert_eq!(snapshot(&fixture.config)?["schema_version"], 21);
        assert!(snapshot(&fixture.config)?["asset_root_hint"].is_null());
        assert_eq!(
            original_hash,
            format!("{:x}", Sha256::digest(std::fs::read(&fixture.config.db)?))
        );
        Ok(())
    }

    #[test]
    fn copy_migrates_only_new_target_and_preserves_source_and_wal() -> Result<()> {
        let fixture = Fixture::new()?;
        let conn = database(&fixture, 21)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute(
            "INSERT INTO pill(output_md,created_at) VALUES('committed WAL evidence','then')",
            [],
        )?;
        let before = std::fs::read(&fixture.config.db)?;
        let target = fixture.dir.join("copy.db");
        let result = execute(
            &fixture.config,
            &StudioAction::CreateCopy {
                target: target.clone(),
            },
        )?;
        assert_eq!(result["switch_db"], target.to_string_lossy().as_ref());
        let copied = StudioConfig {
            db: target.clone(),
            ..fixture.config.clone()
        };
        let copy_view = snapshot(&copied)?;
        assert_eq!(copy_view["schema_version"], crate::db::LATEST_SCHEMA);
        assert_eq!(copy_view["counts"]["pills"], 1);
        assert_eq!(snapshot(&fixture.config)?["schema_version"], 21);
        assert_eq!(before, std::fs::read(&fixture.config.db)?);
        let backup = result["backup"]
            .as_str()
            .expect("migration keeps target pre-upgrade backup");
        let backup_view = snapshot(&StudioConfig {
            db: PathBuf::from(backup),
            ..copied
        })?;
        assert_eq!(backup_view["schema_version"], 21);
        assert!(execute(&fixture.config, &StudioAction::CreateCopy { target }).is_err());
        assert_eq!(before, std::fs::read(&fixture.config.db)?);
        Ok(())
    }

    #[test]
    fn schema21_snapshot_browses_without_migration_or_writes() -> Result<()> {
        let fixture = Fixture::new()?;
        {
            let conn = database(&fixture, 21)?;
            conn.execute(
                "INSERT INTO asset(work_id,role,local_path) VALUES(1,'poster','missing.png')",
                [],
            )?;
            conn.execute(
                "INSERT INTO pill(output_md,created_at) VALUES('legacy evidence','then')",
                [],
            )?;
            conn.execute(
                "INSERT INTO person(id,created_at) VALUES('legacy-person','then')",
                [],
            )?;
        }
        let original = std::fs::read(&fixture.config.db)?;
        let view = snapshot(&fixture.config)?;
        assert_eq!(view["schema_version"], 21);
        assert_eq!(view["needs_migration"], true);
        assert_eq!(view["counts"]["works"], 1);
        assert_eq!(view["works"][0]["title"], "One");
        assert!(view["works"][0]["image_path"].is_null());
        assert_eq!(view["pills"][0]["snapshot"], false);
        assert_eq!(view["people"][0]["usable"], 0);
        assert_eq!(view["experiments"], serde_json::json!([]));
        assert_eq!(original, std::fs::read(&fixture.config.db)?);
        Ok(())
    }

    #[test]
    fn missing_database_snapshot_does_not_create_any_file() -> Result<()> {
        let fixture = Fixture::new()?;
        let view = snapshot(&fixture.config)?;
        assert!(view["schema_version"].is_null());
        assert_eq!(view["needs_migration"], false);
        assert_eq!(view["works"], serde_json::json!([]));
        assert!(!fixture.config.db.exists());
        Ok(())
    }
}
