//! Read-only preview audit using the exact native-studio public boundary.
//! Usage: cargo run -p ie --example preview_audit -- --db PATH --root DIR [--decode]
use anyhow::{Context, Result, ensure};
use ie::studio::{self, StudioConfig};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let required = |name: &str| -> Result<PathBuf> {
        args.windows(2)
            .find(|pair| pair[0] == name)
            .map(|pair| PathBuf::from(&pair[1]))
            .with_context(|| format!("需要显式 {name} <path>"))
    };
    let db = required("--db")?;
    let root = required("--root")?;
    let before = ie::capture::sha256_file(&db)?;
    let snapshot = studio::snapshot(&StudioConfig {
        db: db.clone(),
        root: root.clone(),
        recipes: root.join("recipes"),
    })?;
    let works = snapshot["works"].as_array().context("没有 works 列表")?;
    let available = works
        .iter()
        .filter(|work| work["image_path"].is_string())
        .count();
    let mut status = BTreeMap::<String, usize>::new();
    for work in works {
        let label = work["preview_status"]
            .as_str()
            .unwrap_or(if work["image_path"].is_string() {
                "available"
            } else {
                "unavailable"
            });
        *status.entry(label.to_owned()).or_default() += 1;
    }
    let decoded = if args.iter().any(|arg| arg == "--decode") {
        let mut decoded = 0;
        let mut failed = 0;
        for path in works.iter().filter_map(|work| work["image_path"].as_str()) {
            let readable = image::ImageReader::open(path)
                .and_then(|reader| reader.with_guessed_format())
                .ok()
                .and_then(|reader| reader.decode().ok())
                .is_some();
            if readable {
                decoded += 1;
            } else {
                failed += 1;
            }
        }
        json!({"decoded":decoded,"failed":failed})
    } else {
        serde_json::Value::Null
    };
    ensure!(
        before == ie::capture::sha256_file(&db)?,
        "只读审计后源数据库字节改变"
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema_version":snapshot["schema_version"],"works_audited":works.len(),
            "preview_available":available,"without_preview":works.len()-available,
            "preview_status":status,"full_decode":decoded,"source_database_unchanged":true,
            "network_or_model_requested":false
        }))?
    );
    Ok(())
}
