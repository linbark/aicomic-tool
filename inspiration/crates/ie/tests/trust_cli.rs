//! CLI-level regression tests: all files and databases live in owned temp dirs.

use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SERIAL: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ie-trust-cli-{}-{}-{stamp}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root.canonicalize().unwrap())
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ie"))
            .args(args)
            .current_dir(&self.0)
            .env_remove("GEMINI_API_KEY")
            .env_remove("IE_API_KEY")
            .output()
            .unwrap()
    }

    fn db(&self) -> PathBuf {
        self.0.join("fixture.db")
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        // Only remove the exact canonical directory this test created.
        let expected_parent = std::env::temp_dir().canonicalize().unwrap();
        if self.0.parent() == Some(expected_parent.as_path())
            && self
                .0
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("ie-trust-cli-"))
            && self.0.canonicalize().ok().as_ref() == Some(&self.0)
        {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn init(w: &Workspace) {
    success(w.run(&["migrate", "--db", w.db().to_str().unwrap()]));
}

fn seed_review(path: &Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute("INSERT INTO pill(id,output_md,created_at) VALUES(1,'first','fixture'),(2,'second','fixture')", []).unwrap();
    conn.execute("INSERT INTO judge_run(id,status,stage,created_at,updated_at) VALUES(1,'awaiting_human','panel','fixture','fixture')", []).unwrap();
    conn.execute("INSERT INTO judge_item(run_id,pill_id,label,display_order,precheck,machine_total,status) VALUES(1,1,'A',0,1,50,'human_needed'),(1,2,'B',1,1,50,'human_needed')", []).unwrap();
}

#[test]
fn migration_requires_explicit_path_without_creating_default_database() {
    let w = Workspace::new();
    let out = w.run(&["migrate"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--db"));
    assert!(!w.0.join("data").exists());
}

#[test]
fn migration_is_repeatable_through_the_cli() {
    let w = Workspace::new();
    init(&w);
    let conn = Connection::open(w.db()).unwrap();
    conn.execute(
        "INSERT INTO pill(output_md,created_at) VALUES('keep me','fixture')",
        [],
    )
    .unwrap();
    success(w.run(&["migrate", "--db", w.db().to_str().unwrap()]));
    let rows: i64 = conn
        .query_row(
            "SELECT count(*) FROM pill WHERE output_md='keep me'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1);
}

#[test]
fn legacy_schema_is_rejected_before_a_new_command_writes_scores() {
    let w = Workspace::new();
    init(&w);
    seed_review(&w.db());
    let conn = Connection::open(w.db()).unwrap();
    conn.execute(
        "UPDATE schema_meta SET value='21' WHERE key='schema_version'",
        [],
    )
    .unwrap();
    let out = w.run(&[
        "judge",
        "fill",
        "--db",
        w.db().to_str().unwrap(),
        "--run",
        "1",
        "--scores",
        "A=82,B=70",
    ]);
    assert!(
        !out.status.success(),
        "legacy schema should require explicit migration"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("migrate"));
    let written: i64 = conn
        .query_row(
            "SELECT count(*) FROM judge_item WHERE human_score IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(written, 0);
}

#[test]
fn invalid_batch_leaves_all_scores_and_run_status_unchanged() {
    let w = Workspace::new();
    init(&w);
    seed_review(&w.db());
    let out = w.run(&[
        "judge",
        "fill",
        "--db",
        w.db().to_str().unwrap(),
        "--run",
        "1",
        "--scores",
        "A=82,B=101",
    ]);
    assert!(!out.status.success());
    let conn = Connection::open(w.db()).unwrap();
    let written: i64 = conn
        .query_row(
            "SELECT count(*) FROM judge_item WHERE human_score IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let status: String = conn
        .query_row("SELECT status FROM judge_run WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(written, 0);
    assert_eq!(status, "awaiting_human");
}

#[test]
fn default_scores_are_demo_and_human_requires_explicit_reviewer() {
    let w = Workspace::new();
    init(&w);
    seed_review(&w.db());
    let db = w.db();
    success(w.run(&[
        "judge",
        "fill",
        "--db",
        db.to_str().unwrap(),
        "--run",
        "1",
        "--scores",
        "A=82,B=70",
    ]));
    let conn = Connection::open(&db).unwrap();
    let demo: i64 = conn
        .query_row(
            "SELECT count(*) FROM judge_item WHERE human_source='demo'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(demo, 2);
    let pending: String = conn
        .query_row("SELECT status FROM judge_run WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        pending, "awaiting_human",
        "demo scores must not complete real review work"
    );
    let failed = w.run(&[
        "judge",
        "fill",
        "--db",
        db.to_str().unwrap(),
        "--run",
        "1",
        "--scores",
        "A=90,B=60",
        "--source",
        "human",
    ]);
    assert!(!failed.status.success());
    let still_demo: i64 = conn
        .query_row(
            "SELECT count(*) FROM judge_item WHERE human_source='demo'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(still_demo, 2);
    // These are isolated test fixtures, never scores for real user data.
    success(w.run(&[
        "judge",
        "fill",
        "--db",
        db.to_str().unwrap(),
        "--run",
        "1",
        "--scores",
        "A=90,B=60",
        "--source",
        "human",
        "--reviewer",
        "fixture-user",
        "--dimension",
        "hierarchy",
    ]));
    let verified: i64 = conn
        .query_row(
            "SELECT count(*) FROM judge_item WHERE human_source='human' AND human_reviewer=?1",
            params!["fixture-user"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(verified, 2);
}
