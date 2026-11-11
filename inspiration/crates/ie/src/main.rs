//! 灵感引擎研究原型 CLI。
//!
//! 只做 M1（抽取）与 P0（个人层闭环）需要的最小闭环：
//!   ie extract  --db <db> --root <data dir> [--limit N]
//!   ie simulate --db <db> --person demo --n 200 [--seed 7]
//!   ie fit      --db <db> --person demo
//!   ie stats    --db <db>

mod bundle;
mod distill;
mod extract;
mod fit;
mod metrics;
mod vector;
mod vision;

use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    let mut opts: HashMap<String, String> = HashMap::new();
    while let Some(a) = args.next() {
        if let Some(k) = a.strip_prefix("--") {
            let v = args.next().unwrap_or_default();
            opts.insert(k.to_string(), v);
        }
    }
    let db = opts
        .get("db")
        .cloned()
        .unwrap_or_else(|| "data/inspiration/inspiration.db".to_string());
    match cmd.as_str() {
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
            let person = opts.get("person").cloned().unwrap_or_else(|| "demo".into());
            let seed: u64 = opts.get("seed").and_then(|v| v.parse().ok()).unwrap_or(7);
            fit::run(&db, &person, seed)
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
        "stats" => stats(&db),
        _ => {
            eprintln!(
                "用法：ie <extract|vision|distill|export|import|simulate|fit|stats> --db <path> [--root DIR] [--limit N] [--person ID] [--n N] [--seed S] [--model M]"
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

/// ISO-8601 UTC 时间戳，避免为原型引入时间库。
pub fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
