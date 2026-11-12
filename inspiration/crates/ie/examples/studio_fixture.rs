//! Explicit, isolated native-studio demo. Never opens or changes the user's database.
//! cargo run -p ie --example studio_fixture -- --out /new/owned/directory [--no-browser]
use anyhow::{Context, Result, ensure};
use ie::{
    experiment,
    studio::{self, StudioAction, StudioConfig},
};
use image::{Rgb, RgbImage};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args
        .windows(2)
        .find(|p| p[0] == "--out")
        .map(|p| PathBuf::from(&p[1]))
        .context("请显式指定 --out <不存在的新目录>；不会使用默认用户数据")?;
    ensure!(!out.exists(), "演示目录已存在，不覆盖或删除任何文件");
    let parent = out.parent().context("演示目录需要父目录")?;
    ensure!(parent.is_dir(), "请先选择已存在的父目录");
    std::fs::create_dir(&out)?;
    let out = out.canonicalize()?;
    let cfg = StudioConfig {
        db: out.join("studio-demo.db"),
        root: out.clone(),
        recipes: out.join("recipes"),
    };
    studio::execute(
        &cfg,
        &StudioAction::CreateEmpty {
            target: cfg.db.clone(),
        },
    )?;
    std::fs::create_dir(out.join("assets"))?;
    std::fs::create_dir(&cfg.recipes)?;
    let conn = Connection::open(&cfg.db)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.execute(
        "INSERT OR REPLACE INTO schema_meta(key,value) VALUES('studio_demo','1')",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO pack(id,title,version) VALUES('ui','界面','1')",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO domain(id,title,medium,pack_id) VALUES('ui','界面','image','ui')",
        [],
    )?;
    conn.execute("INSERT INTO source(id,kind,title,policy_json,created_at) VALUES('studio-demo','manual','明确标注的生成演示素材','{\"demo\":true}',?1)", [ie::now_iso()])?;
    let tones = [
        [39, 74, 101],
        [159, 77, 48],
        [63, 98, 84],
        [125, 98, 58],
        [92, 78, 110],
        [58, 95, 117],
    ];
    let titles = [
        "演示 · 色彩与留白",
        "演示 · 暖色节奏",
        "演示 · 静谧秩序",
        "演示 · 纸页层次",
        "演示 · 几何结构",
        "演示 · 轻重平衡",
    ];
    for (index, tone) in tones.into_iter().enumerate() {
        let path = out
            .join("assets")
            .join(format!("material-{}.png", index + 1));
        pattern(&path, tone, index)?;
        conn.execute("INSERT INTO work(source_id,domain_id,collection,slug,canonical_url,title,summary_derived,payload_json,first_seen_at,last_seen_at)
            VALUES('studio-demo','ui','studio-demo',?1,?2,?3,'隔离生成图案，用于界面和流程验证，不是真实参考作品','{\"studio_demo\":true}',?4,?4)",
            params![format!("material-{}",index+1),format!("urn:studio-demo:{}",index+1),titles[index],ie::now_iso()])?;
        let id = conn.last_insert_rowid();
        conn.execute("INSERT INTO asset(work_id,role,local_path,width,height,bytes,sha256) VALUES(?1,'poster',?2,480,640,?3,?4)",
            params![id,format!("assets/material-{}.png",index+1),std::fs::metadata(&path)?.len(),ie::capture::sha256_file(&path)?])?;
    }
    drop(conn);
    studio::execute(&cfg, &StudioAction::RefineLocal { work: 1 })?;
    let a = studio::execute(
        &cfg,
        &StudioAction::BrewLocal {
            work: 1,
            palette_ratio: 0.7,
        },
    )?;
    let b = studio::execute(
        &cfg,
        &StudioAction::BrewLocal {
            work: 1,
            palette_ratio: 0.45,
        },
    )?;
    let pill_a = pill_id(&a)?;
    let pill_b = pill_id(&b)?;
    let conn = Connection::open(&cfg.db)?;
    let raw: String =
        conn.query_row("SELECT output_json FROM pill WHERE id=?1", [pill_a], |r| {
            r.get(0)
        })?;
    let frozen: Value = serde_json::from_str(&raw)?;
    std::fs::write(
        cfg.recipes.join("local-color-space.json"),
        serde_json::to_string_pretty(&frozen["recipe_definition"])?,
    )?;
    drop(conn);
    if !args.iter().any(|a| a == "--no-browser") {
        let content = json!({"strings":["借一束光，把想法炼成作品。","每一次取舍，留下更清晰的表达。","这是隔离演示，不作为真人审美数据。"]});
        let conditions = json!({"viewport":{"width":960,"height":1200},"theme":"light","task":"演示同内容匿名双图审阅", "generator":{"kind":"fixture","version":"studio-demo-v1"}});
        let db = cfg.db.to_str().context("数据库路径不是 UTF-8")?;
        let created = experiment::create(&experiment::CreateOpts {
            db,
            pill_a,
            pill_b,
            content: &content,
            conditions: &conditions,
        })?;
        let id = created["id"].as_i64().context("实验没有 id")?;
        for (arm, alternative) in [("a", false), ("b", true)] {
            let html = out.join(format!("demo-{arm}.html"));
            std::fs::write(&html, page(&frozen, alternative))?;
            experiment::attach(&experiment::AttachOpts {
                db,
                experiment_id: id,
                arm,
                html: &html,
                out_dir: &out.join(format!("artifact-{arm}")),
                chrome: None,
            })?;
        }
        experiment::blind(&experiment::BlindOpts {
            db,
            experiment_id: id,
            out_dir: &out.join("anonymous-review"),
        })?;
    }
    let view = studio::snapshot(&cfg)?;
    ensure!(view["demo"] == true, "隔离演示标记未被 studio 识别");
    ensure!(
        view["people"].as_array().is_some_and(Vec::is_empty),
        "演示不得创建真人身份"
    );
    println!(
        "隔离体验数据已准备：{}\n作品/丹药/实验：{}/{}/{}\n标记 demo=true；未记录真人偏好。",
        out.display(),
        view["counts"]["works"],
        view["counts"]["pills"],
        view["counts"]["experiments"]
    );
    Ok(())
}
fn pill_id(value: &Value) -> Result<i64> {
    value["pill_id"]
        .as_i64()
        .context("炼制动作没有返回 pill_id")
}
fn rect(image: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, c: [u8; 3]) {
    for yy in y..(y + h).min(image.height()) {
        for xx in x..(x + w).min(image.width()) {
            image.put_pixel(xx, yy, Rgb(c));
        }
    }
}
fn pattern(path: &Path, tone: [u8; 3], index: usize) -> Result<()> {
    let mut img = RgbImage::from_pixel(480, 640, Rgb([246, 243, 236]));
    let ink = [29, 33, 34];
    rect(&mut img, 36, 34, 32, 5, ink);
    rect(&mut img, 76, 34, 64, 5, ink);
    if index.is_multiple_of(2) {
        rect(&mut img, 36, 112, 408, 244, tone);
        rect(&mut img, 66, 144, 88, 180, [230, 226, 214]);
        rect(&mut img, 182, 186, 226, 46, [246, 243, 236]);
        rect(&mut img, 182, 248, 168, 12, [246, 243, 236]);
    } else {
        rect(&mut img, 236, 84, 208, 344, tone);
        rect(&mut img, 36, 136, 286, 18, ink);
        rect(&mut img, 36, 170, 232, 18, ink);
        rect(&mut img, 36, 224, 148, 8, ink);
        rect(&mut img, 36, 246, 120, 8, ink);
    }
    for i in 0..5 {
        rect(
            &mut img,
            36,
            470 + i * 18,
            if i == 4 { 192 } else { 348 },
            3,
            [105, 106, 100],
        );
    }
    rect(&mut img, 36, 594, 408, 1, [196, 193, 183]);
    img.save(path)?;
    Ok(())
}
fn page(snapshot: &Value, alternative: bool) -> String {
    let palette = snapshot["components"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["dimension"] == "palette"))
        .map(|c| c["content"]["roles"].clone())
        .unwrap_or(json!({}));
    let color = |key: &str, fallback: &str| palette[key].as_str().unwrap_or(fallback).to_string();
    let bg = color("bg", "#FFFFFF");
    let ink = color("text", "#111111");
    let accent = color("accent", "#274A65");
    let border = color("border", "#D0CCC3");
    let muted = color("muted", "#505050");
    let (width, top, size, gap) = if alternative {
        (570, 112, 48, 88)
    } else {
        (800, 88, 76, 64)
    };
    format!(
        "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><title>隔离演示</title><style>:root{{--bg:{bg};--ink:{ink};--accent:{accent};--border:{border};--muted:{muted};}}*{{box-sizing:border-box}}html,body{{margin:0}}body{{background:var(--bg);color:var(--ink);font-family:Arial,'PingFang SC',sans-serif}}main{{max-width:{width}px;margin:{top}px auto;padding:0 40px}}small{{font-size:13px;letter-spacing:2px}}h1{{font-size:{size}px;line-height:1.2;letter-spacing:-2px;font-weight:600;margin:56px 0 32px}}p{{font-size:20px;line-height:1.8;max-width:540px}}section{{margin-top:{gap}px;padding-top:32px;border-top:1px solid var(--border)}}.shape{{height:224px;background:var(--accent);margin-top:40px}}</style></head><body><main><small>STUDIO / DEMO</small><h1>借一束光，把想法炼成作品。</h1><p>每一次取舍，留下更清晰的表达。</p><div class=\"shape\"></div><section><p>这是隔离演示，不作为真人审美数据。</p></section></main></body></html>"
    )
}
