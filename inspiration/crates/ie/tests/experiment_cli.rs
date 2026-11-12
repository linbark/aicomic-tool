//! Experiment commands must not turn text/fixture data into real visual evidence.

use rusqlite::{Connection, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SERIAL: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ie-experiment-cli-{}-{}-{stamp}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ie"))
            .args(args)
            .arg("--db")
            .arg(self.0.join("fixture.db"))
            .current_dir(&self.0)
            .env_remove("GEMINI_API_KEY")
            .env_remove("IE_API_KEY")
            .output()
            .unwrap()
    }
    fn success(&self, args: &[&str]) -> Value {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn seed(&self) {
        let out = self.run(&["migrate"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let conn = Connection::open(self.0.join("fixture.db")).unwrap();
        let content = json!({"colors":[{"hex":"#FFFFFF","share":0.9},{"hex":"#000000","share":0.1}],
            "roles":{"bg":"#FFFFFF","text":"#000000","text_contrast":21.0},"confidence":0.9});
        let hash = format!("{:x}", Sha256::digest(content.to_string().as_bytes()));
        for id in [1, 2] {
            let snapshot = json!({"snapshot_version":1,"recipe":format!("recipe.fixture.{id}"),
                "recipe_definition":{"key":format!("recipe.fixture.{id}"),"version":"1","name":"fixture", "goal":"fixture",
                    "components":[{"slot":"base","dimension":"palette","ratio":1.0,"required":true}]},
                "components":[{"slot":"base","dimension":"palette","ratio":1.0,"quality":0.9,"fire":"pixel-proxy",
                    "extract_id":id,"extract_revision":1,"technique_version":"1","hash":hash,"content_hash":hash,"content":content}],
                "prompt":"test-only frozen specification", "conflicts":[],"spec_issues":[]});
            conn.execute("INSERT INTO pill(id,output_md,output_json,created_at) VALUES(?1,'fixture',?2,'fixture')",params![id,snapshot.to_string()]).unwrap();
        }
        std::fs::write(
            self.0.join("content.json"),
            json!({"strings":["The same content"],"media":[]}).to_string(),
        )
        .unwrap();
        std::fs::write(self.0.join("conditions.json"),json!({"task":"fixture", "viewport":{"width":640,"height":480},"theme":"light", "generator":{"kind":"coding-agent","model":"fixture","version":"1"}}).to_string()).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let (Ok(root), Ok(temp)) = (self.0.canonicalize(), std::env::temp_dir().canonicalize())
            && root == self.0
            && root.parent() == Some(temp.as_path())
            && root
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("ie-experiment-cli-"))
        {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

#[test]
fn text_specifications_cannot_produce_a_blind_visual_review_without_artifacts() {
    let f = Fixture::new();
    f.seed();
    let created = f.success(&[
        "experiment",
        "create",
        "--pills",
        "1,2",
        "--content",
        "content.json",
        "--conditions",
        "conditions.json",
    ]);
    let id = created["id"]
        .as_i64()
        .expect("created experiment has an ID")
        .to_string();
    let out = f.run(&[
        "experiment",
        "blind",
        "--experiment",
        &id,
        "--out",
        "review",
    ]);
    assert!(
        !out.status.success(),
        "no screenshots means no visual review"
    );
    let stats = f.success(&["experiment", "stats"]);
    for field in ["experiments", "reviewers", "evaluations"] {
        assert_eq!(stats["formal_visual"][field].as_u64(), Some(0), "{field}");
    }
}

#[test]
fn the_first_experiment_id_is_a_value_not_a_missing_option() {
    let f = Fixture::new();
    f.seed();
    let created = f.success(&[
        "experiment",
        "create",
        "--pills",
        "1,2",
        "--content",
        "content.json",
        "--conditions",
        "conditions.json",
    ]);
    assert_eq!(created["id"], 1);
    std::fs::write(f.0.join("page.html"),"<!doctype html><html><head><style>body{background:#FFFFFF;color:#000000}h1{font-size:32px}p{font-size:16px}</style></head><body><h1>The same content</h1></body></html>").unwrap();
    let out = f.run(&[
        "experiment",
        "attach",
        "--experiment",
        "1",
        "--arm",
        "a",
        "--html",
        "page.html",
        "--out",
        "artifact",
        "--chrome",
        "missing-fixture-browser",
    ]);
    assert!(!out.status.success());
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(
        error.contains("Chrome"),
        "ID=1 must reach explicit browser validation, got: {error}"
    );
    assert!(!error.contains("需要 --experiment"));
    let missing = f.run(&[
        "experiment",
        "attach",
        "--experiment",
        "--arm",
        "a",
        "--html",
        "page.html",
        "--out",
        "other",
        "--chrome",
        "missing-fixture-browser",
    ]);
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("需要 --experiment"),
        "a bare flag is not ID=1"
    );
}

#[test]
fn a_bare_reviewer_flag_never_becomes_a_human_identity() {
    let f = Fixture::new();
    f.seed();
    let created = f.success(&[
        "experiment",
        "create",
        "--pills",
        "1,2",
        "--content",
        "content.json",
        "--conditions",
        "conditions.json",
    ]);
    let id = created["id"].as_i64().unwrap().to_string();
    let out = f.run(&[
        "experiment",
        "record",
        "--experiment",
        &id,
        "--manifest-hash",
        "fixture",
        "--source",
        "human",
        "--reviewer",
        "--winner",
        "A",
    ]);
    assert!(!out.status.success());
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(
        error.contains("reviewer"),
        "a bare flag cannot be coerced to reviewer=1: {error}"
    );
}

#[test]
fn a_new_private_person_remains_honest_cold_start_without_a_kernel() {
    let f = Fixture::new();
    f.seed();
    let initialized = f.success(&["person", "init", "--person", "fixture-person"]);
    assert!(initialized.is_object());
    let fitted = f.success(&["fit", "--person", "fixture-person"]);
    assert_eq!(fitted["status"], "cold_start");
    assert_eq!(fitted["counts"]["raw"], 0);
    assert_eq!(fitted["fitted"], false);
    assert_eq!(fitted["activated"], false);
    let shown = f.success(&["kernel", "show", "--person", "fixture-person"]);
    assert_eq!(shown["counts"]["raw"], 0);
    assert_eq!(shown["counts"]["usable"], 0);
    assert_eq!(shown["kernels"].as_array().unwrap().len(), 0);
}

#[test]
fn personal_modes_cannot_be_silently_ignored_or_activated_with_false() {
    let f = Fixture::new();
    f.seed();
    let out = f.run(&[
        "brew",
        "--recipe",
        "missing.recipe.json",
        "--person",
        "fixture-person",
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("personal-candidates"));
    let out = f.run(&["fit", "--person", "fixture-person", "--activate", "false"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("activate"));
}

#[test]
fn malformed_candidate_ids_are_not_silently_dropped_by_cli() {
    let f = Fixture::new();
    f.seed();
    let out = f.run(&[
        "experiment",
        "create",
        "--pills",
        "1,bad,2",
        "--content",
        "content.json",
        "--conditions",
        "conditions.json",
    ]);
    assert!(!out.status.success());
    let out = f.run(&[
        "experiment",
        "create",
        "--pills",
        "1,2,3",
        "--content",
        "content.json",
        "--conditions",
        "conditions.json",
    ]);
    assert!(!out.status.success());
    let conn = Connection::open(f.0.join("fixture.db")).unwrap();
    let runs: i64 = conn
        .query_row("SELECT count(*) FROM experiment", [], |r| r.get(0))
        .unwrap();
    assert_eq!(runs, 0, "invalid input cannot create a partial experiment");
}
