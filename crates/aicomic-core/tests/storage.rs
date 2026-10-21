use aicomic_core::*;
use serde_json::json;

#[test]
fn opens_a_fresh_isolated_database() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Store::open(dir.path()).is_ok(), "fresh database must open");
    assert!(dir.path().join("database.db").is_file());
}

#[test]
fn migrates_legacy_sqlalchemy_tables_without_losing_ids_or_extra_fields() {
    let dir = tempfile::tempdir().unwrap();
    let db = rusqlite::Connection::open(dir.path().join("database.db")).unwrap();
    db.execute_batch("CREATE TABLE projects(id INTEGER PRIMARY KEY,name TEXT,description TEXT,created_at TEXT,extra TEXT); INSERT INTO projects VALUES(42,'旧项目','内容','2020-01-01','keep'); CREATE TABLE characters(id INTEGER PRIMARY KEY,project_id INTEGER,name TEXT,description TEXT,base_prompt TEXT,negative_prompt TEXT,avatar_asset_id INTEGER); INSERT INTO characters VALUES(7,42,'主角',NULL,'prompt',NULL,NULL);").unwrap();
    drop(db);
    let store = Store::open(dir.path()).unwrap();
    let p = store.resolve_project("42").unwrap();
    assert_eq!(p.id, 42);
    assert_eq!(p.uuid.len(), 32);
    assert_eq!(store.resolve_project(&p.uuid).unwrap().id, 42);
    assert_eq!(store.asset_items(42).unwrap()[0].category, "persona_visual");
    assert!(
        dir.path()
            .join("backups")
            .read_dir()
            .unwrap()
            .next()
            .is_some()
    );
    let db = rusqlite::Connection::open(dir.path().join("database.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT extra FROM projects WHERE id=42", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "keep"
    );
    assert_eq!(
        Store::open(dir.path()).unwrap().projects().unwrap()[0].uuid,
        p.uuid
    );
}

#[test]
fn persists_full_script_and_removes_polymorphic_descendants() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    let p = s.create_project("作品", Some("描述")).unwrap();
    let ep = s.create_episode(p.id, "第一集", Some("原剧本")).unwrap();
    let sc = s.create_scene(ep.id, "开场", 1).unwrap();
    let sh = s.create_shot(sc.id, "远景", 1).unwrap();
    s.update_episode(ep.id, "改名", Some("新剧本")).unwrap();
    s.patch_episode(
        ep.id,
        EpisodeUpdate {
            action_text: Some("动作".into()),
            prompt: Some("cinematic".into()),
            exec_artifacts: Some(json!({"step":2})),
            ..Default::default()
        },
    )
    .unwrap();
    s.update_scene(
        sc.id,
        SceneUpdate {
            dialogue: Some("你好".into()),
            ..Default::default()
        },
    )
    .unwrap();
    s.update_shot(
        sh.id,
        ShotUpdate {
            negative_prompt: Some("blur".into()),
            status: Some("ready".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        s.script(p.id).unwrap()[0].scenes[0].shots[0].shot.status,
        "ready"
    );
    assert_eq!(
        s.episodes(p.id).unwrap()[0].exec_artifacts,
        Some(json!({"step":2}))
    );
    let ev = s.create_event(p.id, "相遇", "#ffffff").unwrap();
    s.upsert_event_node(ev.id, "shot", sh.id, "关联").unwrap();
    s.delete_episode(ep.id).unwrap();
    assert!(s.episodes(p.id).unwrap().is_empty());
    assert!(s.event_matrix(p.id).unwrap().nodes.is_empty());
    assert!(s.shot(sh.id).is_err());
    assert!(s.create_scene(ep.id, "孤儿", 2).is_err());
}

#[test]
fn imports_media_without_overwrite_and_cleans_selection_on_delete() {
    let dir = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let file = source.path().join("画稿.txt");
    std::fs::write(&file, "first").unwrap();
    let s = Store::open(dir.path()).unwrap();
    let p = s.create_project("../../不可做目录", None).unwrap();
    let item = s.create_asset_item(p.id, "主角", "persona", None).unwrap();
    assert_eq!(item.category, "persona_visual");
    let a = s.import_asset(&file, AssetTarget::Item(item.id)).unwrap();
    std::fs::write(&file, "second").unwrap();
    let b = s.import_asset(&file, AssetTarget::Item(item.id)).unwrap();
    assert_ne!(a.file_path, b.file_path);
    assert_eq!(
        std::fs::read_to_string(s.asset_path(&a.file_path).unwrap()).unwrap(),
        "first"
    );
    s.set_item_avatar(item.id, Some(a.id)).unwrap();
    s.delete_asset(a.id).unwrap();
    assert!(s.asset_item(item.id).unwrap().avatar_asset_id.is_none());
    assert!(!s.asset_path(&a.file_path).unwrap().exists());
    s.delete_asset_item(item.id).unwrap();
    assert!(!s.asset_path(&b.file_path).unwrap().exists());
    assert_eq!(std::fs::read_to_string(file).unwrap(), "second");
}

#[test]
fn rejects_cross_project_event_and_asset_links() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    let a = s.create_project("A", None).unwrap();
    let b = s.create_project("B", None).unwrap();
    let ep = s.create_episode(b.id, "B1", None).unwrap();
    let ev = s.create_event(a.id, "A事件", "#000").unwrap();
    assert!(
        s.upsert_event_node(ev.id, "episode", ep.id, "错误")
            .is_err()
    );
    assert!(
        s.upsert_event_node(ev.id, "invalid", ep.id, "错误")
            .is_err()
    );
    let node = s
        .upsert_event_node(
            ev.id,
            "episode",
            s.create_episode(a.id, "A1", None).unwrap().id,
            "一",
        )
        .unwrap();
    let node2 = s
        .upsert_event_node(ev.id, "episode", node.target_id, "二")
        .unwrap();
    assert_eq!(node.id, node2.id);
    assert_eq!(s.event_matrix(a.id).unwrap().nodes.len(), 1);
    s.delete_event(ev.id).unwrap();
    assert!(s.event_matrix(a.id).unwrap().nodes.is_empty());
}

#[test]
fn safe_json_roundtrips_unicode_and_rejects_both_platform_traversals() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    s.write_json("ai_settings.json", &json!({"model":"模型"}))
        .unwrap();
    assert_eq!(
        s.read_json("ai_settings.json").unwrap(),
        Some(json!({"model":"模型"}))
    );
    assert!(s.read_json("missing.json").unwrap().is_none());
    for path in [
        "../outside.json",
        "..\\outside.json",
        "/tmp/out.json",
        "C:\\out.json",
    ] {
        assert!(s.write_json(path, &json!({})).is_err(), "{path}");
    }
    let p = s.create_project("作品", None).unwrap();
    s.write_context(p.id, "series_bible.v1.json", &json!({"世界":"规则"}))
        .unwrap();
    assert_eq!(
        s.read_context(p.id, "series_bible.v1.json").unwrap(),
        Some(json!({"世界":"规则"}))
    );
    s.write_run(
        p.id,
        "run_1",
        "stages/chat.interrupt.json",
        &json!({"resume_state":{"step":3}}),
    )
    .unwrap();
    assert_eq!(
        s.read_run(p.id, "run_1", "stages/chat.interrupt.json")
            .unwrap()
            .unwrap()["resume_state"]["step"],
        3
    );
    assert!(
        s.write_run(p.id, "../escape", "meta.json", &json!({}))
            .is_err()
    );
    assert!(s.write_context(p.id, "../escape.json", &json!({})).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_symlink_escape_without_touching_external_files() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();
    assert!(s.write_json("escape/private.json", &json!({})).is_err());
    assert!(!outside.path().join("private.json").exists());
}

#[test]
fn run_history_and_project_deletion_preserve_other_projects() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    let a = s.create_project("A", None).unwrap();
    let b = s.create_project("B", None).unwrap();
    let ep = s.create_episode(a.id, "A1", None).unwrap();
    let run = s
        .create_ai_run(
            a.id,
            ep.id,
            "generate_script",
            Some("input"),
            "output",
            Some(json!({"run_id":"run1"})),
        )
        .unwrap();
    assert_eq!(
        s.ai_runs(a.id, Some(ep.id), Some("generate_script"), 10)
            .unwrap()[0]
            .id,
        run.id
    );
    assert!(
        s.create_ai_run(b.id, ep.id, "x", None, "out", None)
            .is_err()
    );
    s.write_context(a.id, "project_outline.v1.json", &json!({"x":1}))
        .unwrap();
    s.delete_project(a.id).unwrap();
    assert_eq!(s.projects().unwrap()[0].id, b.id);
    assert!(!dir.path().join(format!("projects/{}", a.id)).exists());
    let db = rusqlite::Connection::open(dir.path().join("database.db")).unwrap();
    for table in [
        "episodes",
        "scenes",
        "shots",
        "assets",
        "characters",
        "events",
        "event_nodes",
        "ai_action_runs",
    ] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0,
            "{table}"
        );
    }
}

#[test]
fn cloned_store_serializes_concurrent_writes() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    let threads: Vec<_> = (0..8)
        .map(|n| {
            let c = s.clone();
            std::thread::spawn(move || c.create_project(&format!("P{n}"), None).unwrap())
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(s.projects().unwrap().len(), 8);
}

#[test]
fn workflow_application_is_idempotent_per_run_action_and_writes_visual_dna() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    let p = s.create_project("作品", None).unwrap();
    let ep = s.create_episode(p.id, "源剧本", None).unwrap();
    let split = json!({"approved_split_episodes":{"episodes":[{"title":"新集","script":"剧本","outline":"提纲"}]}});
    s.apply_workflow_artifacts(p.id, ep.id, "run1", &split)
        .unwrap();
    let both = json!({"approved_split_episodes":split["approved_split_episodes"],"approved_asset_ingest_plan":{"items":[{"name":"主角","kind":"character","category":"persona_visual","description":"勇敢","base_prompt":"hero","visual_dna":{"hair":"black"},"stable_diffusion_tags":"hero"}],"series_style":{"lighting":"soft"}}});
    s.apply_workflow_artifacts(p.id, ep.id, "run1", &both)
        .unwrap();
    s.apply_workflow_artifacts(p.id, ep.id, "run1", &both)
        .unwrap();
    assert_eq!(s.episodes(p.id).unwrap().len(), 2);
    assert_eq!(
        s.episodes(p.id).unwrap()[1].exec_artifacts,
        Some(json!({"outline":"提纲"}))
    );
    let items = s.asset_items(p.id).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].base_prompt.as_deref(), Some("hero"));
    let dna = s
        .read_context(
            p.id,
            &format!("visual_dna.asset_item_{}.v1.json", items[0].id),
        )
        .unwrap()
        .unwrap();
    assert_eq!(dna["visual_dna"]["hair"], "black");
    assert_eq!(
        s.read_context(p.id, "series_bible.v1.json")
            .unwrap()
            .unwrap()["series_style"]["lighting"],
        "soft"
    );
}

#[test]
fn workflow_failure_rolls_back_split_assets_and_context_then_can_retry() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    let p = s.create_project("作品", None).unwrap();
    let ep = s.create_episode(p.id, "源", None).unwrap();
    let item = s
        .create_asset_item(p.id, "角色", "persona_visual", Some("before"))
        .unwrap();
    let filename = format!("visual_dna.asset_item_{}.v1.json", item.id);
    s.write_context(p.id, &filename, &json!({"original":true}))
        .unwrap();
    // A second context target being a directory forces a real filesystem error.
    let blocked = dir
        .path()
        .join(format!("projects/{}/context/series_bible.v1.json", p.id));
    std::fs::create_dir_all(&blocked).unwrap();
    let artifacts = json!({"approved_split_episodes":{"episodes":[{"title":"新","script":"x"}]},"approved_asset_ingest_plan":{"items":[{"name":"角色","kind":"character","description":"after","visual_dna":{"hair":"new"}}],"series_style":{"x":1}}});
    assert!(
        s.apply_workflow_artifacts(p.id, ep.id, "retry", &artifacts)
            .is_err()
    );
    assert_eq!(s.episodes(p.id).unwrap().len(), 1);
    assert_eq!(
        s.asset_item(item.id).unwrap().description.as_deref(),
        Some("before")
    );
    assert_eq!(
        s.read_context(p.id, &filename).unwrap(),
        Some(json!({"original":true}))
    );
    std::fs::remove_dir(blocked).unwrap();
    s.apply_workflow_artifacts(p.id, ep.id, "retry", &artifacts)
        .unwrap();
    assert_eq!(s.episodes(p.id).unwrap().len(), 2);
}

#[test]
fn imported_png_preserves_stable_diffusion_metadata_for_plain_and_compressed_text() {
    let dir = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    let p = s.create_project("作品", None).unwrap();
    let item = s
        .create_asset_item(p.id, "角色", "persona_visual", None)
        .unwrap();
    let parameters = "hero, cinematic\nNegative prompt: blur\nSteps: 20, Sampler: Euler a, CFG scale: 7, Seed: 12345, Size: 1x1, Model hash: abc123";
    for compressed in [false, true] {
        let path = source.path().join(format!("{compressed}.png"));
        let output = std::fs::File::create(&path).unwrap();
        let mut encoder = png::Encoder::new(output, 1, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        if compressed {
            encoder
                .add_ztxt_chunk("parameters".into(), parameters.into())
                .unwrap();
        } else {
            encoder
                .add_text_chunk("parameters".into(), parameters.into())
                .unwrap();
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255, 0, 0]).unwrap();
        writer.finish().unwrap();
        let asset = s.import_asset(&path, AssetTarget::Item(item.id)).unwrap();
        let meta = asset.meta_data.unwrap();
        assert_eq!(meta["width"], 1);
        assert_eq!(meta["height"], 1);
        assert_eq!(meta["raw_parameters"], parameters);
        assert_eq!(meta["prompt"], "hero, cinematic");
        assert_eq!(meta["negative_prompt"], "blur");
        assert_eq!(meta["seed"], 12345);
        assert_eq!(meta["sampler"], "Euler a");
        assert_eq!(meta["cfg_scale"], 7.0);
        assert_eq!(meta["model_hash"], "abc123");
    }
}

#[test]
fn deleting_legacy_windows_path_keeps_a_file_referenced_with_slashes() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    let p = s.create_project("作品", None).unwrap();
    let item = s
        .create_asset_item(p.id, "角色", "persona_visual", None)
        .unwrap();
    std::fs::create_dir_all(dir.path().join("data/legacy")).unwrap();
    std::fs::write(dir.path().join("data/legacy/shared.txt"), "keep").unwrap();
    let db = rusqlite::Connection::open(dir.path().join("database.db")).unwrap();
    db.execute("INSERT INTO assets(character_id,file_path,file_type) VALUES(?,'legacy\\shared.txt','text')",[item.id]).unwrap();
    let old = db.last_insert_rowid();
    db.execute(
        "INSERT INTO assets(character_id,file_path,file_type) VALUES(?,'legacy/shared.txt','text')",
        [item.id],
    )
    .unwrap();
    drop(db);
    s.delete_asset(old).unwrap();
    assert!(
        dir.path().join("data/legacy/shared.txt").is_file(),
        "the remaining asset still owns this file"
    );
}

#[test]
fn deletion_error_restores_already_quarantined_media_and_database_rows() {
    let dir = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    let p = s.create_project("作品", None).unwrap();
    let item = s
        .create_asset_item(p.id, "角色", "persona_visual", None)
        .unwrap();
    let file = source.path().join("safe.txt");
    std::fs::write(&file, "safe").unwrap();
    let asset = s.import_asset(&file, AssetTarget::Item(item.id)).unwrap();
    let db = rusqlite::Connection::open(dir.path().join("database.db")).unwrap();
    db.execute(
        "INSERT INTO assets(character_id,file_path,file_type) VALUES(?,'../unsafe.txt','text')",
        [item.id],
    )
    .unwrap();
    drop(db);
    assert!(s.delete_asset_item(item.id).is_err());
    assert_eq!(s.assets_for_item(item.id).unwrap().len(), 2);
    assert_eq!(
        std::fs::read_to_string(s.asset_path(&asset.file_path).unwrap()).unwrap(),
        "safe"
    );
}

#[test]
fn importing_video_replaces_the_old_clip_and_shot_deletion_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    let p = s.create_project("作品", None).unwrap();
    let ep = s.create_episode(p.id, "一", None).unwrap();
    let scene = s.create_scene(ep.id, "场", 1).unwrap();
    let shot = s.create_shot(scene.id, "镜", 1).unwrap();
    let video = source.path().join("clip.mp4");
    std::fs::write(&video, b"opaque uploaded video").unwrap();
    let first = s
        .import_shot_video(shot.id, &video)
        .unwrap()
        .video_path
        .unwrap();
    let second = s
        .import_shot_video(shot.id, &video)
        .unwrap()
        .video_path
        .unwrap();
    assert_ne!(first, second);
    assert!(!s.asset_path(&first).unwrap().exists());
    assert!(s.asset_path(&second).unwrap().exists());
    s.delete_shot(shot.id).unwrap();
    assert!(!s.asset_path(&second).unwrap().exists());
    assert!(video.exists());
}

#[test]
fn recreating_a_deleted_project_cannot_inherit_its_memory_records() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    let original = s.create_project("旧项目", None).unwrap();
    let memory = rusqlite::Connection::open(dir.path().join("memory_store.db")).unwrap();
    memory.execute_batch("CREATE TABLE memory_records(id TEXT PRIMARY KEY,project_id INTEGER,content TEXT);CREATE TABLE rust_memory_vectors(record_id TEXT,vector BLOB);").unwrap();
    memory
        .execute(
            "INSERT INTO memory_records VALUES('old',?,'旧项目秘密')",
            [original.id],
        )
        .unwrap();
    memory
        .execute("INSERT INTO rust_memory_vectors VALUES('old',X'1234')", [])
        .unwrap();
    s.delete_project(original.id).unwrap();
    let replacement = s.create_project("新项目", None).unwrap();
    assert!(
        replacement.id > original.id,
        "persistent project identifiers must never be recycled"
    );
    let inherited: i64 = memory
        .query_row(
            "SELECT count(*) FROM memory_records WHERE project_id=?",
            [replacement.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        inherited, 0,
        "new project must not inherit old project memory"
    );
    assert_eq!(
        memory
            .query_row("SELECT count(*) FROM rust_memory_vectors", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn all_domain_identifiers_keep_their_high_water_after_delete_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    let p = s.create_project("旧", None).unwrap();
    let ep = s.create_episode(p.id, "旧集", None).unwrap();
    let sc = s.create_scene(ep.id, "旧场", 1).unwrap();
    let sh = s.create_shot(sc.id, "旧镜", 1).unwrap();
    s.delete_project(p.id).unwrap();
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    let next = s.create_project("新", None).unwrap();
    let next_ep = s.create_episode(next.id, "新集", None).unwrap();
    let next_sc = s.create_scene(next_ep.id, "新场", 1).unwrap();
    let next_sh = s.create_shot(next_sc.id, "新镜", 1).unwrap();
    for (new, old, kind) in [
        (next.id, p.id, "project"),
        (next_ep.id, ep.id, "episode"),
        (next_sc.id, sc.id, "scene"),
        (next_sh.id, sh.id, "shot"),
    ] {
        assert!(new > old, "{kind} reused {old}");
    }
    assert!(
        !dir.path().join("memory_store.db").exists(),
        "deleting a project must not create an absent memory database"
    );
}

#[test]
fn initializes_allocators_from_orphaned_legacy_memory_columns_and_json_references() {
    let dir = tempfile::tempdir().unwrap();
    let main = rusqlite::Connection::open(dir.path().join("database.db")).unwrap();
    main.execute_batch("CREATE TABLE projects(id INTEGER PRIMARY KEY,name TEXT);INSERT INTO projects VALUES(42,'历史');").unwrap();
    drop(main);
    let memory = rusqlite::Connection::open(dir.path().join("memory_store.db")).unwrap();
    memory.execute_batch("CREATE TABLE canonical_evidences(evidence_id TEXT,project_id INTEGER,episode_id INTEGER,scene_id INTEGER);INSERT INTO canonical_evidences VALUES('ev',900,1200,1300);CREATE TABLE memory_records(id TEXT,project_id INTEGER,payload_json TEXT);INSERT INTO memory_records VALUES('m',800,'{\"source\":{\"shot_id\":1400},\"episode_id\":1250}');").unwrap();
    drop(memory);
    let s = Store::open(dir.path()).unwrap();
    let p = s.create_project("新", None).unwrap();
    let ep = s.create_episode(p.id, "新", None).unwrap();
    let sc = s.create_scene(ep.id, "新", 1).unwrap();
    let sh = s.create_shot(sc.id, "新", 1).unwrap();
    assert!(p.id > 900);
    assert!(ep.id > 1250);
    assert!(sc.id > 1300);
    assert!(sh.id > 1400);
}

#[test]
fn project_memory_cleanup_and_files_roll_back_as_one_operation() {
    let dir = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    let p = s.create_project("删除", None).unwrap();
    let other = s.create_project("保留", None).unwrap();
    let item = s
        .create_asset_item(p.id, "角色", "persona_visual", None)
        .unwrap();
    let file = source.path().join("safe.txt");
    std::fs::write(&file, "safe").unwrap();
    let asset = s.import_asset(&file, AssetTarget::Item(item.id)).unwrap();
    s.write_context(p.id, "series_bible.v1.json", &json!({"keep":true}))
        .unwrap();
    let memory = rusqlite::Connection::open(dir.path().join("memory_store.db")).unwrap();
    memory.execute_batch("CREATE TABLE memory_records(id TEXT PRIMARY KEY,project_id INTEGER);CREATE TABLE episodic_memories(id TEXT PRIMARY KEY,project_id INTEGER,FOREIGN KEY(id) REFERENCES memory_records(id));CREATE TABLE rust_memory_vectors(record_id TEXT,vector BLOB);CREATE TABLE canonical_changesets(changeset_id TEXT PRIMARY KEY,project_id INTEGER);CREATE TABLE canonical_conflicts(conflict_id TEXT PRIMARY KEY,project_id INTEGER);CREATE TABLE rust_memory_run_changesets(project_id INTEGER,run_id TEXT);").unwrap();
    for (id, pid) in [("old", p.id), ("keep", other.id)] {
        memory
            .execute(
                "INSERT INTO memory_records VALUES(?,?)",
                rusqlite::params![id, pid],
            )
            .unwrap();
        memory
            .execute(
                "INSERT INTO episodic_memories VALUES(?,?)",
                rusqlite::params![id, pid],
            )
            .unwrap();
        memory
            .execute("INSERT INTO rust_memory_vectors VALUES(?,X'1234')", [id])
            .unwrap();
        memory
            .execute(
                "INSERT INTO canonical_changesets VALUES(?,?)",
                rusqlite::params![id, pid],
            )
            .unwrap();
        memory
            .execute(
                "INSERT INTO canonical_conflicts VALUES(?,?)",
                rusqlite::params![id, pid],
            )
            .unwrap();
        memory
            .execute(
                "INSERT INTO rust_memory_run_changesets VALUES(?,'run')",
                [pid],
            )
            .unwrap();
    }
    memory.execute_batch("CREATE TRIGGER block_delete BEFORE DELETE ON canonical_conflicts BEGIN SELECT RAISE(ABORT,'fixture memory cleanup failed');END;").unwrap();
    assert!(
        s.delete_project(p.id).is_err(),
        "memory error must abort the complete delete"
    );
    assert!(s.project(p.id).is_ok());
    assert!(s.asset_path(&asset.file_path).unwrap().is_file());
    assert_eq!(
        s.read_context(p.id, "series_bible.v1.json").unwrap(),
        Some(json!({"keep":true}))
    );
    assert_eq!(
        memory
            .query_row("SELECT count(*) FROM memory_records", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        memory
            .query_row("SELECT count(*) FROM rust_memory_vectors", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    memory.execute_batch("DROP TRIGGER block_delete;").unwrap();
    s.delete_project(p.id).unwrap();
    assert!(s.project(p.id).is_err());
    assert!(s.project(other.id).is_ok());
    assert!(!s.asset_path(&asset.file_path).unwrap().exists());
    for table in [
        "memory_records",
        "episodic_memories",
        "canonical_changesets",
        "canonical_conflicts",
        "rust_memory_run_changesets",
    ] {
        assert_eq!(
            memory
                .query_row(
                    &format!("SELECT count(*) FROM {table} WHERE project_id=?"),
                    [p.id],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0,
            "{table}"
        );
        assert_eq!(
            memory
                .query_row(
                    &format!("SELECT count(*) FROM {table} WHERE project_id=?"),
                    [other.id],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1,
            "{table}"
        );
    }
    assert_eq!(
        memory
            .query_row(
                "SELECT count(*) FROM rust_memory_vectors WHERE record_id='keep'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
}

#[test]
fn migrated_database_reopens_without_accumulating_another_backup() {
    let dir = tempfile::tempdir().unwrap();
    let main = rusqlite::Connection::open(dir.path().join("database.db")).unwrap();
    main.execute_batch("CREATE TABLE projects(id INTEGER PRIMARY KEY,name TEXT);INSERT INTO projects VALUES(5,'历史');").unwrap();
    drop(main);
    let count = || {
        std::fs::read_dir(dir.path().join("backups"))
            .unwrap()
            .count()
    };
    drop(Store::open(dir.path()).unwrap());
    assert_eq!(count(), 1);
    for _ in 0..3 {
        drop(Store::open(dir.path()).unwrap());
    }
    assert_eq!(
        count(),
        1,
        "already migrated databases must not create startup backups"
    );
}

#[test]
fn failed_memory_high_water_scan_rolls_back_the_entire_main_schema_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("database.db");
    let main = rusqlite::Connection::open(&path).unwrap();
    main.execute_batch("CREATE TABLE projects(id INTEGER PRIMARY KEY,name TEXT,extra TEXT);INSERT INTO projects VALUES(5,'历史','preserve');").unwrap();
    let before: String = main
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name='projects'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    drop(main);
    std::fs::write(dir.path().join("memory_store.db"), b"not a sqlite database").unwrap();
    assert!(Store::open(dir.path()).is_err());
    let main = rusqlite::Connection::open(&path).unwrap();
    let after: String = main
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name='projects'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        after, before,
        "any upgrade failure must leave the legacy schema intact"
    );
    assert_eq!(
        main.query_row("SELECT extra FROM projects WHERE id=5", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "preserve"
    );
    assert_eq!(
        main.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    let backup = std::fs::read_dir(dir.path().join("backups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let saved = rusqlite::Connection::open(backup).unwrap();
    assert_eq!(
        saved
            .query_row("SELECT extra FROM projects WHERE id=5", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "preserve"
    );
}
