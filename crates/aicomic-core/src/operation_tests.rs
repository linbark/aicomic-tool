//! Real process termination: no unwinding and no Drop-based recovery.
use crate::*;
use std::io::{BufRead, Write};
use std::process::{Command, Stdio};

pub(crate) fn error_at(point: &str) -> bool {
    std::env::var("AICOMIC_TEST_ERROR").ok().as_deref() == Some(point)
}

pub(crate) fn hit(point: &str) {
    if std::env::var("AICOMIC_TEST_POINT").ok().as_deref() == Some(point) {
        static HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let occurrence = std::env::var("AICOMIC_TEST_POINT_OCCURRENCE")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1);
        if HITS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1 != occurrence {
            return;
        }
        println!("AICOMIC_READY:{point}");
        std::io::stdout().flush().unwrap();
        // Child::wait closes stdin; never turn that EOF into a panic/unwind
        // racing SIGKILL. Park until the controller really terminates us.
        loop {
            std::thread::park();
        }
    }
}

#[test]
#[ignore = "subprocess helper, invoked only by crash recovery tests"]
fn subprocess() {
    let root = std::env::var_os("AICOMIC_TEST_ROOT").unwrap();
    let store = Store::open(&root).unwrap();
    match std::env::var("AICOMIC_TEST_ACTION").unwrap().as_str() {
        "asset" => store.delete_asset(1).unwrap(),
        "import" => {
            store
                .import_asset(&Path::new(&root).join("source.txt"), AssetTarget::Item(1))
                .unwrap();
        }
        "project" => store.delete_project(1).unwrap(),
        "chained" => {
            assert!(matches!(
                store.import_shot_video(1, &Path::new(&root).join("source.mp4")),
                Err(Error::CommittedPending { .. })
            ));
            store.delete_shot(1).unwrap();
        }
        "video" => {
            store
                .import_shot_video(1, &Path::new(&root).join("source.mp4"))
                .unwrap();
        }
        "intent_error" => {
            assert!(matches!(store.delete_asset(1), Err(Error::Io(_))));
            assert_eq!(store.assets_for_item(1).unwrap().len(), 1);
            assert!(
                !operations::has_pending(store.paths()).unwrap(),
                "failed intent publication must not strand an unusable journal"
            );
        }
        "project_error" => {
            assert!(matches!(store.delete_project(1), Err(Error::Io(_))));
            assert!(store.project(1).is_ok());
            let asset = store.assets_for_item(1).unwrap().remove(0);
            assert!(store.asset_path(&asset.file_path).unwrap().is_file());
            assert_eq!(
                store.read_context(1, "legacy.json").unwrap(),
                Some(serde_json::json!({"keep":true}))
            );
            assert!(!operations::has_pending(store.paths()).unwrap());
        }
        "copy_error" => {
            assert!(matches!(
                store.import_asset(&Path::new(&root).join("source.txt"), AssetTarget::Item(1)),
                Err(Error::Io(_))
            ));
            assert_eq!(store.assets_for_item(1).unwrap().len(), 1);
            assert!(!operations::has_pending(store.paths()).unwrap());
        }
        "pending" => {
            let error = store.delete_project(1).unwrap_err();
            assert!(
                matches!(error, Error::CommittedPending { operation_id, .. } if operation_id.len() == 32)
            );
            assert!(store.project(1).is_err());
            assert!(operations::has_pending(store.paths()).unwrap());
        }
        "recover" => {}
        action => panic!("unknown action {action}"),
    }
}

fn kill_at(root: &Path, action: &str, point: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "operation_tests::subprocess",
            "--ignored",
            "--nocapture",
        ])
        .env("AICOMIC_TEST_ROOT", root)
        .env("AICOMIC_TEST_ACTION", action)
        .env("AICOMIC_TEST_POINT", point)
        .env(
            "AICOMIC_TEST_POINT_OCCURRENCE",
            if action == "chained" { "2" } else { "1" },
        )
        .env(
            "AICOMIC_TEST_ERROR",
            if action == "chained" { "purge" } else { "" },
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let expected = format!("AICOMIC_READY:{point}");
    let (sender, receiver) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            if line.unwrap() == expected {
                let _ = sender.send(());
                return;
            }
        }
    });
    let ready = receiver.recv_timeout(Duration::from_secs(30));
    let killed = child.kill();
    let status = child.wait().unwrap();
    reader.join().unwrap();
    assert!(
        ready.is_ok(),
        "subprocess never reached {point}; status {status}"
    );
    killed.unwrap();
    assert!(!status.success());
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            status.signal(),
            Some(9),
            "must be SIGKILL, never panic/Drop"
        );
    }
}

fn run_with_error(root: &Path, action: &str, error: &str) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "operation_tests::subprocess",
            "--ignored",
            "--nocapture",
        ])
        .env("AICOMIC_TEST_ROOT", root)
        .env("AICOMIC_TEST_ACTION", action)
        .env("AICOMIC_TEST_ERROR", error)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn intent_publication_error_does_not_strand_an_operation() {
    let root = tempfile::tempdir().unwrap();
    let (store, asset) = media_fixture(root.path());
    drop(store);
    run_with_error(root.path(), "intent_error", "intent_publish");
    run_with_error(root.path(), "intent_error", "intent_sync");
    let store = Store::open(root.path()).unwrap();
    assert!(store.asset_path(&asset.file_path).unwrap().is_file());
    store.delete_asset(asset.id).unwrap();
}

fn media_fixture(root: &Path) -> (Store, Asset) {
    let store = Store::open(root).unwrap();
    let project = store.create_project("crash fixture", None).unwrap();
    let item = store
        .create_asset_item(project.id, "hero", "persona", None)
        .unwrap();
    let source = root.join("source.txt");
    std::fs::write(&source, b"original media").unwrap();
    let asset = store
        .import_asset(&source, AssetTarget::Item(item.id))
        .unwrap();
    (store, asset)
}

fn project_memory(root: &Path) -> rusqlite::Connection {
    let memory = Connection::open(root.join("memory_store.db")).unwrap();
    memory.execute_batch("CREATE TABLE memory_records(id TEXT PRIMARY KEY,project_id INTEGER,payload_json TEXT); CREATE TABLE rust_memory_vectors(record_id TEXT,vector BLOB); INSERT INTO memory_records VALUES('old',1,'{\"shot_id\":9000}'); INSERT INTO rust_memory_vectors VALUES('old',X'1234');").unwrap();
    memory
}

#[test]
fn main_receipt_recovers_uncommitted_memory_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let (store, asset) = media_fixture(root.path());
    store
        .write_context(
            1,
            "legacy.json",
            &serde_json::json!({"keep":"until commit"}),
        )
        .unwrap();
    let memory = project_memory(root.path());
    drop(store);
    kill_at(root.path(), "project", "main_committed");
    assert_eq!(
        memory
            .query_row("SELECT count(*) FROM memory_records", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1,
        "memory must not commit before main"
    );
    let reopened = Store::open(root.path()).unwrap();
    assert!(reopened.project(1).is_err());
    assert!(!reopened.asset_path(&asset.file_path).unwrap().exists());
    assert!(!root.path().join("projects/1").exists());
    assert_eq!(
        memory
            .query_row("SELECT count(*) FROM memory_records", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        memory
            .query_row("SELECT count(*) FROM rust_memory_vectors", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let project = reopened.create_project("next", None).unwrap();
    let episode = reopened.create_episode(project.id, "next", None).unwrap();
    let scene = reopened.create_scene(episode.id, "next", 1).unwrap();
    assert!(reopened.create_shot(scene.id, "next", 1).unwrap().id > 9000);
}

fn video_fixture(root: &Path) -> (Store, Shot) {
    let store = Store::open(root).unwrap();
    let project = store.create_project("video", None).unwrap();
    let episode = store.create_episode(project.id, "one", None).unwrap();
    let scene = store.create_scene(episode.id, "one", 1).unwrap();
    let shot = store.create_shot(scene.id, "one", 1).unwrap();
    let source = root.join("source.mp4");
    std::fs::write(&source, b"old video").unwrap();
    let shot = store.import_shot_video(shot.id, &source).unwrap();
    std::fs::write(&source, b"replacement video").unwrap();
    (store, shot)
}
fn data_files(root: &Path) -> Vec<std::path::PathBuf> {
    fn visit(path: &Path, files: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(&entry.path(), files);
            } else {
                files.push(entry.path());
            }
        }
    }
    let mut files = Vec::new();
    visit(&root.join("data"), &mut files);
    files.sort();
    files
}
#[test]
fn killed_video_replacement_preserves_old_clip_and_discards_copy() {
    let root = tempfile::tempdir().unwrap();
    let (store, shot) = video_fixture(root.path());
    let before = data_files(root.path());
    drop(store);
    kill_at(root.path(), "video", "quarantined");
    let reopened = Store::open(root.path()).unwrap();
    assert_eq!(reopened.shot(shot.id).unwrap(), shot);
    assert_eq!(
        std::fs::read(
            reopened
                .asset_path(shot.video_path.as_ref().unwrap())
                .unwrap()
        )
        .unwrap(),
        b"old video"
    );
    assert_eq!(data_files(root.path()), before);
    assert_eq!(
        std::fs::read(root.path().join("source.mp4")).unwrap(),
        b"replacement video"
    );
}

#[test]
fn killed_asset_copy_leaves_no_unregistered_media() {
    let root = tempfile::tempdir().unwrap();
    let (store, asset) = media_fixture(root.path());
    let before = data_files(root.path());
    drop(store);
    kill_at(root.path(), "import", "new_copy_partial");
    let reopened = Store::open(root.path()).unwrap();
    assert_eq!(reopened.assets_for_item(1).unwrap(), vec![asset]);
    assert_eq!(data_files(root.path()), before);
}

#[test]
fn late_promotion_does_not_publish_unreferenced_video() {
    for replace in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (other_store, shot) = video_fixture(root.path());
        kill_at(root.path(), "video", "main_committed");
        if replace {
            other_store
                .import_shot_video(shot.id, &root.path().join("source.mp4"))
                .unwrap();
        } else {
            other_store.delete_shot(shot.id).unwrap();
        }
        drop(other_store);
        let reopened = Store::open(root.path()).unwrap();
        assert_eq!(
            data_files(root.path()).len(),
            usize::from(replace),
            "late promote must discard its private copy when its reference was consumed"
        );
        if replace {
            assert!(
                reopened
                    .asset_path(reopened.shot(shot.id).unwrap().video_path.as_ref().unwrap())
                    .unwrap()
                    .is_file()
            );
        }
    }
}

#[test]
fn recovery_accepts_only_empty_manifestless_operation_directories() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let empty = root
        .path()
        .join(".aicomic-operations/0123456789abcdef0123456789abcdef");
    std::fs::create_dir_all(&empty).unwrap();
    store.recover_pending_operations().unwrap();
    assert!(!empty.exists());
    std::fs::create_dir_all(&empty).unwrap();
    std::fs::write(empty.join("removed-0"), b"only surviving copy").unwrap();
    assert!(matches!(
        store.recover_pending_operations(),
        Err(Error::RecoveryRequired { .. })
    ));
    assert_eq!(
        std::fs::read(empty.join("removed-0")).unwrap(),
        b"only surviving copy"
    );
}

fn operation_directory(root: &Path) -> std::path::PathBuf {
    let mut entries = std::fs::read_dir(root.join(".aicomic-operations")).unwrap();
    let path = entries.next().unwrap().unwrap().path();
    assert!(entries.next().is_none());
    path
}
#[test]
fn committed_private_copy_with_wrong_type_is_preserved() {
    let root = tempfile::tempdir().unwrap();
    let (store, _) = video_fixture(root.path());
    drop(store);
    kill_at(root.path(), "video", "main_committed");
    let directory = operation_directory(root.path());
    let copy = directory.join("new-0");
    std::fs::remove_file(&copy).unwrap();
    std::fs::create_dir(&copy).unwrap();
    std::fs::write(copy.join("do-not-promote"), b"untrusted directory").unwrap();
    assert!(
        Store::open(root.path()).is_err(),
        "a directory must never become a video file"
    );
    assert!(copy.join("do-not-promote").is_file());
    assert!(directory.join("manifest.json").is_file());
}

#[test]
fn unresolved_rollback_blocks_new_destructive_operations_on_existing_store() {
    let root = tempfile::tempdir().unwrap();
    let (store, asset) = media_fixture(root.path());
    kill_at(root.path(), "asset", "quarantined");
    assert!(
        matches!(
            store.delete_asset(asset.id),
            Err(Error::RecoveryRequired { .. })
        ),
        "a retry must not delete the DB fact needed by an unresolved rollback"
    );
    store.recover_pending_operations().unwrap();
    assert_eq!(store.assets_for_item(1).unwrap(), vec![asset.clone()]);
    assert!(store.asset_path(&asset.file_path).unwrap().is_file());
}

#[test]
fn ordinary_copy_and_second_rename_errors_restore_everything() {
    for error in ["copy", "second_rename"] {
        let root = tempfile::tempdir().unwrap();
        let (store, asset) = media_fixture(root.path());
        store
            .write_context(1, "legacy.json", &serde_json::json!({"keep":true}))
            .unwrap();
        let memory = project_memory(root.path());
        let before = data_files(root.path());
        drop(store);
        run_with_error(
            root.path(),
            if error == "copy" {
                "copy_error"
            } else {
                "project_error"
            },
            error,
        );
        let store = Store::open(root.path()).unwrap();
        assert!(store.project(1).is_ok());
        assert!(store.asset_path(&asset.file_path).unwrap().is_file());
        assert_eq!(data_files(root.path()), before);
        assert_eq!(
            memory
                .query_row("SELECT count(*) FROM memory_records", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            memory
                .query_row("SELECT count(*) FROM rust_memory_vectors", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
#[test]
fn postcommit_errors_report_pending_and_recover_without_reviving_project() {
    for error in ["memory_commit", "purge"] {
        let root = tempfile::tempdir().unwrap();
        let (store, asset) = media_fixture(root.path());
        store
            .write_context(1, "legacy.json", &serde_json::json!({"keep":true}))
            .unwrap();
        let memory = project_memory(root.path());
        drop(store);
        run_with_error(root.path(), "pending", error);
        let store = Store::open(root.path()).unwrap();
        assert!(store.project(1).is_err());
        assert!(!store.asset_path(&asset.file_path).unwrap().exists());
        assert_eq!(
            memory
                .query_row("SELECT count(*) FROM memory_records", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(!operations::has_pending(store.paths()).unwrap());
        store.recover_pending_operations().unwrap();
    }
}
#[test]
fn intent_kill_keeps_original_paths_without_assuming_rename_happened() {
    let root = tempfile::tempdir().unwrap();
    let (store, asset) = media_fixture(root.path());
    drop(store);
    kill_at(root.path(), "asset", "intent_durable");
    let store = Store::open(root.path()).unwrap();
    assert_eq!(store.assets_for_item(1).unwrap(), vec![asset.clone()]);
    assert_eq!(
        std::fs::read(store.asset_path(&asset.file_path).unwrap()).unwrap(),
        b"original media"
    );
}
#[test]
fn project_quarantine_and_interrupted_rollback_preserve_context_runs_and_memory() {
    let root = tempfile::tempdir().unwrap();
    let (store, asset) = media_fixture(root.path());
    let other = store.create_project("untouched", None).unwrap();
    store
        .write_context(1, "legacy.json", &serde_json::json!({"keep":true}))
        .unwrap();
    store
        .write_run(
            1,
            "legacy_run",
            "stages/old.json",
            &serde_json::json!({"legacy":true}),
        )
        .unwrap();
    let memory = project_memory(root.path());
    drop(store);
    kill_at(root.path(), "project", "quarantined");
    assert!(
        !root.path().join("projects/1").exists(),
        "kill occurs after the entire project directory moved"
    );
    kill_at(root.path(), "recover", "recovery_step");
    for _ in 0..2 {
        let store = Store::open(root.path()).unwrap();
        assert!(store.project(1).is_ok());
        assert!(store.project(other.id).is_ok());
        assert!(store.asset_path(&asset.file_path).unwrap().is_file());
        assert_eq!(
            store.read_context(1, "legacy.json").unwrap(),
            Some(serde_json::json!({"keep":true}))
        );
        assert_eq!(
            store.read_run(1, "legacy_run", "stages/old.json").unwrap(),
            Some(serde_json::json!({"legacy":true}))
        );
        assert_eq!(
            memory
                .query_row("SELECT count(*) FROM memory_records", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            memory
                .query_row("SELECT count(*) FROM rust_memory_vectors", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
#[test]
fn partial_video_copy_kill_discards_only_private_bytes() {
    let root = tempfile::tempdir().unwrap();
    let (store, shot) = video_fixture(root.path());
    let before = data_files(root.path());
    std::fs::write(root.path().join("source.mp4"), vec![7u8; 200_000]).unwrap();
    drop(store);
    kill_at(root.path(), "video", "new_copy_partial");
    let private = operation_directory(root.path()).join("new-0");
    let size = std::fs::metadata(&private).unwrap().len();
    assert!(
        size > 0 && size < 200_000,
        "copy really is partial at the kill point"
    );
    let store = Store::open(root.path()).unwrap();
    assert_eq!(store.shot(shot.id).unwrap(), shot);
    assert_eq!(data_files(root.path()), before);
    assert_eq!(
        std::fs::metadata(root.path().join("source.mp4"))
            .unwrap()
            .len(),
        200_000
    );
}
#[test]
fn interrupted_committed_recovery_finishes_promotion_idempotently() {
    let root = tempfile::tempdir().unwrap();
    let (store, shot) = video_fixture(root.path());
    drop(store);
    kill_at(root.path(), "video", "main_committed");
    kill_at(root.path(), "recover", "recovery_step");
    for _ in 0..2 {
        let store = Store::open(root.path()).unwrap();
        let updated = store.shot(shot.id).unwrap();
        assert_ne!(updated.video_path, shot.video_path);
        assert_eq!(
            std::fs::read(
                store
                    .asset_path(updated.video_path.as_ref().unwrap())
                    .unwrap()
            )
            .unwrap(),
            b"replacement video"
        );
        assert!(
            !store
                .asset_path(shot.video_path.as_ref().unwrap())
                .unwrap()
                .exists()
        );
        assert_eq!(data_files(root.path()).len(), 1);
    }
}

#[test]
fn context_and_run_writes_cannot_recreate_an_unresolved_project_directory() {
    let root = tempfile::tempdir().unwrap();
    let (store, _) = media_fixture(root.path());
    let episode = store.create_episode(1, "legacy", None).unwrap();
    store
        .write_context(1, "legacy.json", &serde_json::json!({"keep":true}))
        .unwrap();
    kill_at(root.path(), "project", "quarantined");
    assert!(matches!(
        store.write_context(1, "new.json", &serde_json::json!({})),
        Err(Error::RecoveryRequired { .. })
    ));
    assert!(matches!(
        store.write_run(1, "new_run", "request.json", &serde_json::json!({})),
        Err(Error::RecoveryRequired { .. })
    ));
    assert!(matches!(
        store.write_json("projects/1/context/direct.json", &serde_json::json!({})),
        Err(Error::RecoveryRequired { .. })
    ));
    assert!(matches!(
        store.apply_workflow_artifacts(
            1,
            episode.id,
            "run",
            &serde_json::json!({"approved_asset_ingest_plan":{"items":[],"series_style":{"x":1}}})
        ),
        Err(Error::RecoveryRequired { .. })
    ));
    assert!(!root.path().join("projects/1").exists());
    store.recover_pending_operations().unwrap();
    assert_eq!(
        store.read_context(1, "legacy.json").unwrap(),
        Some(serde_json::json!({"keep":true}))
    );
}

#[test]
fn deferred_memory_constraints_abort_before_main_commit_without_rejecting_old_orphans() {
    let root = tempfile::tempdir().unwrap();
    let (store, asset) = media_fixture(root.path());
    let memory = project_memory(root.path());
    memory.execute_batch("PRAGMA foreign_keys=OFF; CREATE TABLE legacy_links(parent_id TEXT REFERENCES memory_records(id)); INSERT INTO legacy_links VALUES('missing'),('old'); PRAGMA foreign_keys=ON;").unwrap();
    assert!(matches!(store.delete_project(1), Err(Error::Invalid(_))));
    assert!(store.project(1).is_ok());
    assert!(store.asset_path(&asset.file_path).unwrap().is_file());
    assert_eq!(
        memory
            .query_row("SELECT count(*) FROM memory_records", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        memory
            .query_row("SELECT count(*) FROM rust_memory_vectors", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    memory
        .execute("DELETE FROM legacy_links WHERE parent_id='old'", [])
        .unwrap();
    store.delete_project(1).unwrap();
    assert_eq!(
        memory
            .query_row("SELECT count(*) FROM legacy_links", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1,
        "an unrelated old orphan does not block deletion"
    );
}
#[test]
fn sql_abort_leaves_no_import_copy_and_preserves_old_video() {
    let root = tempfile::tempdir().unwrap();
    let (store, asset) = media_fixture(root.path());
    let db = Connection::open(store.paths().database_path.clone()).unwrap();
    db.execute_batch("CREATE TRIGGER reject_import BEFORE INSERT ON assets BEGIN SELECT RAISE(ABORT,'fixture');END;").unwrap();
    let before = data_files(root.path());
    assert!(matches!(
        store.import_asset(&root.path().join("source.txt"), AssetTarget::Item(1)),
        Err(Error::Database(_))
    ));
    assert_eq!(store.assets_for_item(1).unwrap(), vec![asset]);
    assert_eq!(data_files(root.path()), before);
    assert!(!operations::has_pending(store.paths()).unwrap());
    let video_root = tempfile::tempdir().unwrap();
    let (store, shot) = video_fixture(video_root.path());
    let db = Connection::open(store.paths().database_path.clone()).unwrap();
    db.execute_batch("CREATE TRIGGER reject_video BEFORE UPDATE OF video_path ON shots BEGIN SELECT RAISE(ABORT,'fixture');END;").unwrap();
    let before = data_files(video_root.path());
    assert!(matches!(
        store.import_shot_video(1, &video_root.path().join("source.mp4")),
        Err(Error::Database(_))
    ));
    assert_eq!(store.shot(1).unwrap(), shot);
    assert_eq!(data_files(video_root.path()), before);
}
#[test]
fn invalid_manifests_never_mutate_originals_or_quarantined_payload() {
    for mutation in [
        "json",
        "version",
        "escape",
        "database",
        "identity",
        "duplicate",
        "redirect",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (store, asset) = media_fixture(root.path());
        drop(store);
        kill_at(root.path(), "asset", "quarantined");
        let directory = operation_directory(root.path());
        let path = directory.join("manifest.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        match mutation {
            "version" => manifest["version"] = serde_json::json!(99),
            "escape" => manifest["moves"] = serde_json::json!(["data/../../outside"]),
            "database" => manifest["moves"] = serde_json::json!(["database.db"]),
            "redirect" => manifest["moves"] = serde_json::json!(["data/unowned.txt"]),
            "identity" => manifest["database_id"] = serde_json::json!("different database"),
            "duplicate" => {
                manifest["moves"] = serde_json::json!([
                    format!("data/{}", asset.file_path),
                    format!("data/{}", asset.file_path)
                ])
            }
            _ => {}
        }
        std::fs::write(
            &path,
            if mutation == "json" {
                b"{broken".to_vec()
            } else {
                serde_json::to_vec(&manifest).unwrap()
            },
        )
        .unwrap();
        assert!(
            matches!(
                Store::open(root.path()),
                Err(Error::RecoveryRequired { .. })
            ),
            "{mutation}"
        );
        assert_eq!(
            std::fs::read(directory.join("removed-0")).unwrap(),
            b"original media"
        );
        assert_eq!(
            std::fs::read(root.path().join("source.txt")).unwrap(),
            b"original media"
        );
        assert!(path.is_file());
    }
}
#[test]
fn uncommitted_created_destination_is_never_authority_to_delete_existing_media() {
    let root = tempfile::tempdir().unwrap();
    let (store, shot) = video_fixture(root.path());
    kill_at(root.path(), "video", "new_copy_partial");
    let manifest_path = operation_directory(root.path()).join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["created"] = serde_json::json!("data/keep.txt");
    std::fs::write(
        root.path().join("data/keep.txt"),
        b"not this operation's file",
    )
    .unwrap();
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(matches!(
        Store::open(root.path()),
        Err(Error::RecoveryRequired { .. })
    ));
    assert!(manifest_path.is_file());
    assert!(manifest_path.parent().unwrap().join("new-0").is_file());
    assert_eq!(store.shot(1).unwrap(), shot);
    assert_eq!(
        std::fs::read(root.path().join("data/keep.txt")).unwrap(),
        b"not this operation's file"
    );
}
#[test]
fn missing_main_database_never_means_an_uncommitted_operation() {
    let root = tempfile::tempdir().unwrap();
    let (store, _) = media_fixture(root.path());
    drop(store);
    kill_at(root.path(), "asset", "quarantined");
    let directory = operation_directory(root.path());
    std::fs::rename(
        root.path().join("database.db"),
        root.path().join("database.saved"),
    )
    .unwrap();
    assert!(matches!(
        Store::open(root.path()),
        Err(Error::RecoveryRequired { .. })
    ));
    assert!(!root.path().join("database.db").exists());
    assert!(directory.join("removed-0").is_file());
}
#[test]
fn rollback_conflict_preserves_both_copies_and_blocks_new_writes() {
    let root = tempfile::tempdir().unwrap();
    let (store, asset) = media_fixture(root.path());
    let original = store.asset_path(&asset.file_path).unwrap();
    kill_at(root.path(), "asset", "quarantined");
    std::fs::write(&original, b"later conflicting file").unwrap();
    assert!(matches!(
        store.recover_pending_operations(),
        Err(Error::RecoveryRequired { .. })
    ));
    assert!(matches!(
        store.delete_asset(asset.id),
        Err(Error::RecoveryRequired { .. })
    ));
    assert_eq!(std::fs::read(&original).unwrap(), b"later conflicting file");
    assert_eq!(
        std::fs::read(operation_directory(root.path()).join("removed-0")).unwrap(),
        b"original media"
    );
}

#[test]
fn legacy_manifestless_quarantine_is_warned_and_preserved() {
    let root = tempfile::tempdir().unwrap();
    drop(Store::open(root.path()).unwrap());
    let legacy = root.path().join(".aicomic-trash-legacy");
    std::fs::create_dir(&legacy).unwrap();
    std::fs::write(legacy.join("0"), b"unknown original path").unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "operation_tests::subprocess",
            "--ignored",
            "--nocapture",
        ])
        .env("AICOMIC_TEST_ROOT", root.path())
        .env("AICOMIC_TEST_ACTION", "recover")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("legacy quarantine"),
        "opening must warn instead of silently ignoring unknown legacy data"
    );
    assert_eq!(
        std::fs::read(legacy.join("0")).unwrap(),
        b"unknown original path"
    );
}

#[test]
fn live_delete_reserves_last_memory_identifier_references_before_erasing_them() {
    let root = tempfile::tempdir().unwrap();
    let (store, _) = media_fixture(root.path());
    let _memory = project_memory(root.path()); // Created after this Store initialized.
    store.delete_project(1).unwrap();
    let project = store.create_project("next", None).unwrap();
    let episode = store.create_episode(project.id, "next", None).unwrap();
    let scene = store.create_scene(episode.id, "next", 1).unwrap();
    assert!(store.create_shot(scene.id, "next", 1).unwrap().id > 9000);
}
#[test]
fn version_two_upgrade_adds_receipts_once_without_project_foreign_keys() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let project = store.create_project("v2", None).unwrap();
    drop(store);
    let db = Connection::open(root.path().join("database.db")).unwrap();
    db.execute_batch("DROP TABLE aicomic_operation_receipts; DROP TABLE aicomic_storage_identity; DELETE FROM aicomic_schema_migrations WHERE version>=3;").unwrap();
    drop(db);
    for _ in 0..2 {
        let store = Store::open(root.path()).unwrap();
        assert_eq!(store.project(project.id).unwrap(), project);
        assert_eq!(
            std::fs::read_dir(root.path().join("backups"))
                .unwrap()
                .count(),
            1
        );
        let db = Connection::open(store.paths().database_path.clone()).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT MAX(version) FROM aicomic_schema_migrations",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            4
        );
        assert!(
            !db.prepare("PRAGMA foreign_key_list(aicomic_operation_receipts)")
                .unwrap()
                .exists([])
                .unwrap()
        );
    }
}
#[cfg(unix)]
#[test]
fn staged_symlink_never_promotes_or_touches_its_external_target() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let target = external.path().join("keep");
    std::fs::write(&target, b"external sentinel").unwrap();
    let (store, _) = video_fixture(root.path());
    drop(store);
    kill_at(root.path(), "video", "main_committed");
    let copy = operation_directory(root.path()).join("new-0");
    std::fs::remove_file(&copy).unwrap();
    std::os::unix::fs::symlink(&target, &copy).unwrap();
    assert!(matches!(
        Store::open(root.path()),
        Err(Error::RecoveryRequired { .. })
    ));
    assert!(
        std::fs::symlink_metadata(copy)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(target).unwrap(), b"external sentinel");
}
#[test]
fn staged_media_directory_is_not_restored_as_a_file() {
    let root = tempfile::tempdir().unwrap();
    let (store, _) = media_fixture(root.path());
    drop(store);
    kill_at(root.path(), "asset", "quarantined");
    let staged = operation_directory(root.path()).join("removed-0");
    std::fs::remove_file(&staged).unwrap();
    std::fs::create_dir(&staged).unwrap();
    std::fs::write(staged.join("keep"), b"wrong type").unwrap();
    assert!(matches!(
        Store::open(root.path()),
        Err(Error::RecoveryRequired { .. })
    ));
    assert_eq!(std::fs::read(staged.join("keep")).unwrap(), b"wrong type");
}
#[test]
fn committed_receipt_rejects_a_safe_looking_but_modified_manifest() {
    let root = tempfile::tempdir().unwrap();
    let (store, _) = video_fixture(root.path());
    drop(store);
    kill_at(root.path(), "video", "main_committed");
    let directory = operation_directory(root.path());
    let path = directory.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest["created"] = serde_json::json!("data/forged.mp4");
    std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(matches!(
        Store::open(root.path()),
        Err(Error::RecoveryRequired { .. })
    ));
    assert!(directory.join("new-0").is_file());
    assert!(directory.join("removed-0").is_file());
    assert!(!root.path().join("data/forged.mp4").exists());
}

#[test]
fn recovery_rolls_back_consumers_before_finalizing_older_promotions() {
    let root = tempfile::tempdir().unwrap();
    let (store, shot) = video_fixture(root.path());
    drop(store);
    kill_at(root.path(), "chained", "quarantined");
    assert_eq!(
        std::fs::read_dir(root.path().join(".aicomic-operations"))
            .unwrap()
            .count(),
        2
    );
    // Deliberately enumerate the older committed manifest first. Recovery must
    // still restore the uncommitted consumer which currently owns its new clip.
    run_with_error(root.path(), "recover", "committed_first");
    for _ in 0..2 {
        let store = Store::open(root.path()).unwrap();
        let updated = store.shot(shot.id).unwrap();
        assert_ne!(updated.video_path, shot.video_path);
        assert_eq!(
            std::fs::read(
                store
                    .asset_path(updated.video_path.as_ref().unwrap())
                    .unwrap()
            )
            .unwrap(),
            b"replacement video"
        );
        assert_eq!(data_files(root.path()).len(), 1);
    }
}

#[test]
fn project_directory_cannot_be_redirected_to_an_existing_project_without_a_directory() {
    let root = tempfile::tempdir().unwrap();
    let (store, _) = media_fixture(root.path());
    let other = store.create_project("unrelated owner", None).unwrap();
    store
        .write_context(
            1,
            "legacy.json",
            &serde_json::json!({"secret":"project one only"}),
        )
        .unwrap();
    drop(store);
    kill_at(root.path(), "project", "quarantined");
    let directory = operation_directory(root.path());
    let manifest_path = directory.join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["project_id"] = serde_json::json!(other.id);
    for original in manifest["moves"].as_array_mut().unwrap() {
        if original == "projects/1" {
            *original = serde_json::json!(format!("projects/{}", other.id));
        }
    }
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(
        matches!(
            Store::open(root.path()),
            Err(Error::RecoveryRequired { .. })
        ),
        "existence of project 2 is not proof this directory belongs to it"
    );
    assert!(!root.path().join(format!("projects/{}", other.id)).exists());
    assert!(manifest_path.is_file());
    assert!(directory.join("removed-1/context/legacy.json").is_file());
}

#[test]
fn media_cannot_be_redirected_to_a_missing_file_with_a_different_valid_owner() {
    let root = tempfile::tempdir().unwrap();
    let (store, original) = media_fixture(root.path());
    let other = store.create_project("unrelated owner", None).unwrap();
    let item = store
        .create_asset_item(other.id, "other", "persona", None)
        .unwrap();
    let second = store
        .import_asset(&root.path().join("source.txt"), AssetTarget::Item(item.id))
        .unwrap();
    let destination = store.asset_path(&second.file_path).unwrap();
    std::fs::remove_file(&destination).unwrap();
    drop(store);
    kill_at(root.path(), "asset", "quarantined");
    let directory = operation_directory(root.path());
    let manifest_path = directory.join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["moves"] = serde_json::json!([format!("data/{}", second.file_path)]);
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(
        matches!(
            Store::open(root.path()),
            Err(Error::RecoveryRequired { .. })
        ),
        "a different asset's missing path must not gain this asset's bytes"
    );
    assert!(!destination.exists());
    assert!(manifest_path.is_file());
    assert_eq!(
        std::fs::read(directory.join("removed-0")).unwrap(),
        b"original media"
    );
    assert!(!root.path().join("data").join(original.file_path).exists());
}

fn journal_auth_key(root: &Path) -> Vec<u8> {
    Connection::open(root.join("database.db"))
        .unwrap()
        .query_row(
            "SELECT journal_auth_key FROM aicomic_storage_identity WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn authentication_key_persists_without_leaking_into_manifests_receipts_or_errors() {
    let root = tempfile::tempdir().unwrap();
    let (store, asset) = media_fixture(root.path());
    let key = journal_auth_key(root.path());
    assert!(
        key.len() == 32,
        "authentication key must have the expected length"
    );
    let key_json = serde_json::to_string(&key).unwrap();
    let key_hex = key
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    drop(store);
    kill_at(root.path(), "asset", "intent_durable");
    let manifest_path = operation_directory(root.path()).join("manifest.json");
    let bytes = std::fs::read(&manifest_path).unwrap();
    let manifest: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(manifest["version"], 2);
    assert_eq!(manifest["auth_tag"].as_array().unwrap().len(), 32);
    let canonical = serde_json::to_string(&manifest).unwrap();
    assert!(
        !canonical.contains(&key_json) && !canonical.contains(&key_hex),
        "manifest must not contain the database authentication key"
    );
    let db = Connection::open(root.path().join("database.db")).unwrap();
    let receipt: String = db
        .query_row(
            "SELECT manifest_json FROM aicomic_operation_receipts LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        !receipt.contains(&key_json) && !receipt.contains(&key_hex),
        "receipt must not contain the key"
    );
    drop(db);
    // Whitespace and JSON map ordering are not authenticated intent changes.
    std::fs::write(&manifest_path, canonical).unwrap();
    for _ in 0..2 {
        let store = Store::open(root.path()).unwrap();
        assert!(store.asset_path(&asset.file_path).unwrap().is_file());
        assert!(
            journal_auth_key(root.path()) == key,
            "opening must retain the key, not regenerate it"
        );
    }
    kill_at(root.path(), "asset", "quarantined");
    let manifest_path = operation_directory(root.path()).join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    let first = manifest["auth_tag"][0].as_u64().unwrap();
    manifest["auth_tag"][0] = serde_json::json!(first ^ 1);
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let error = match Store::open(root.path()) {
        Err(error) => error,
        Ok(_) => panic!("tampered tag must fail"),
    };
    let diagnostic = format!("{error}; {error:?}");
    assert!(matches!(error, Error::RecoveryRequired { .. }));
    assert!(
        !diagnostic.contains(&key_json) && !diagnostic.contains(&key_hex),
        "diagnostics must not contain the key"
    );
    assert!(manifest_path.is_file());
}

#[test]
fn missing_or_changed_authentication_tags_preserve_committed_and_uncommitted_payloads() {
    for point in ["quarantined", "main_committed"] {
        for mutation in ["missing", "bitflip", "short", "zero"] {
            let root = tempfile::tempdir().unwrap();
            let (store, _) = media_fixture(root.path());
            drop(store);
            kill_at(root.path(), "asset", point);
            let directory = operation_directory(root.path());
            let path = directory.join("manifest.json");
            let mut manifest: Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            match mutation {
                "missing" => {
                    manifest.as_object_mut().unwrap().remove("auth_tag");
                }
                "bitflip" => {
                    let byte = manifest["auth_tag"][0].as_u64().unwrap();
                    manifest["auth_tag"][0] = serde_json::json!(byte ^ 1);
                }
                "short" => manifest["auth_tag"] = serde_json::json!([0]),
                "zero" => manifest["auth_tag"] = serde_json::json!(vec![0u8; 32]),
                _ => unreachable!(),
            }
            std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            assert!(
                matches!(Store::open(root.path()), Err(Error::RecoveryRequired { reason, .. }) if reason.contains("authentication")),
                "{point}/{mutation}"
            );
            assert_eq!(
                std::fs::read(directory.join("removed-0")).unwrap(),
                b"original media"
            );
            assert!(path.is_file());
        }
    }
}

#[test]
fn unsigned_uncommitted_legacy_intent_is_preserved_instead_of_guessed() {
    let root = tempfile::tempdir().unwrap();
    let (store, _) = media_fixture(root.path());
    drop(store);
    kill_at(root.path(), "asset", "quarantined");
    let directory = operation_directory(root.path());
    let path = directory.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest["version"] = serde_json::json!(1);
    manifest.as_object_mut().unwrap().remove("auth_tag");
    std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(
        matches!(Store::open(root.path()), Err(Error::RecoveryRequired { reason, .. }) if reason.contains("unsigned"))
    );
    assert_eq!(
        std::fs::read(directory.join("removed-0")).unwrap(),
        b"original media"
    );
    assert!(path.is_file());
}

#[test]
fn unsigned_committed_legacy_intent_requires_an_exact_trusted_main_receipt() {
    for tamper in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (store, _) = media_fixture(root.path());
        drop(store);
        kill_at(root.path(), "asset", "main_committed");
        let directory = operation_directory(root.path());
        let path = directory.join("manifest.json");
        let manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let id = manifest["operation_id"].as_str().unwrap();
        let db = Connection::open(root.path().join("database.db")).unwrap();
        let signed: String = db
            .query_row(
                "SELECT manifest_json FROM aicomic_operation_receipts WHERE operation_id=?",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        // Reproduce the original v1 serializer's exact field order, not a new
        // self-asserted sidecar. The fixture deliberately installs a trusted
        // main receipt as if written by the previous, unpublished protocol.
        let prefix = signed.split_once(",\"auth_tag\":").unwrap().0;
        let legacy = format!("{prefix}}}").replacen("\"version\":2", "\"version\":1", 1);
        db.execute(
            "UPDATE aicomic_operation_receipts SET manifest_json=? WHERE operation_id=?",
            params![legacy, id],
        )
        .unwrap();
        drop(db);
        let file = if tamper {
            let mut value: Value = serde_json::from_str(&legacy).unwrap();
            value["moves"] = serde_json::json!(["data/forged.txt"]);
            serde_json::to_string(&value).unwrap()
        } else {
            legacy
        };
        std::fs::write(&path, file).unwrap();
        if tamper {
            assert!(matches!(
                Store::open(root.path()),
                Err(Error::RecoveryRequired { .. })
            ));
            assert_eq!(
                std::fs::read(directory.join("removed-0")).unwrap(),
                b"original media"
            );
        } else {
            let store = Store::open(root.path()).unwrap();
            assert!(store.assets_for_item(1).unwrap().is_empty());
            assert!(!directory.exists());
        }
    }
}

#[test]
fn version_three_upgrade_persists_a_key_without_changing_database_identity_or_context() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let project = store.create_project("v3", None).unwrap();
    store
        .write_context(
            project.id,
            "legacy.json",
            &serde_json::json!({"legacy":"unchanged"}),
        )
        .unwrap();
    drop(store);
    let db = Connection::open(root.path().join("database.db")).unwrap();
    let identity: String = db
        .query_row(
            "SELECT database_id FROM aicomic_storage_identity",
            [],
            |r| r.get(0),
        )
        .unwrap();
    db.execute_batch("ALTER TABLE aicomic_storage_identity DROP COLUMN journal_auth_key; DELETE FROM aicomic_schema_migrations WHERE version=4;").unwrap();
    drop(db);
    let store = Store::open(root.path()).unwrap();
    let key = journal_auth_key(root.path());
    assert!(key.len() == 32);
    for _ in 0..2 {
        let reopened = Store::open(root.path()).unwrap();
        assert!(
            journal_auth_key(root.path()) == key,
            "v3 upgrade must generate only once"
        );
        assert_eq!(reopened.project(project.id).unwrap(), project);
        assert_eq!(
            reopened.read_context(project.id, "legacy.json").unwrap(),
            Some(serde_json::json!({"legacy":"unchanged"}))
        );
        assert_eq!(
            std::fs::read_dir(root.path().join("backups"))
                .unwrap()
                .count(),
            1
        );
        let db = Connection::open(root.path().join("database.db")).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT database_id FROM aicomic_storage_identity",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            identity
        );
    }
    store.delete_project(project.id).unwrap();
}

#[test]
fn killed_quarantine_restores_asset_on_reopen() {
    let root = tempfile::tempdir().unwrap();
    let (store, asset) = media_fixture(root.path());
    drop(store);
    kill_at(root.path(), "asset", "quarantined");
    let reopened = Store::open(root.path()).unwrap();
    assert_eq!(reopened.assets_for_item(1).unwrap(), vec![asset.clone()]);
    assert_eq!(
        std::fs::read(reopened.asset_path(&asset.file_path).unwrap()).unwrap(),
        b"original media"
    );
}
