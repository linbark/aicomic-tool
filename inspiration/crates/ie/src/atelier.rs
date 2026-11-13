//! Versioned, local-only workshop. Reads never migrate or create a database.
use crate::studio::StudioConfig;
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn digest(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}
fn read_connection(config: &StudioConfig) -> Result<Connection> {
    let conn = Connection::open_with_flags(&config.db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    Ok(conn)
}
fn available(conn: &Connection) -> Result<bool> {
    let has: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_meta')",
        [],
        |r| r.get(0),
    )?;
    if !has {
        return Ok(false);
    }
    let version: Option<String> = conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(version.as_deref() == Some("26"))
}
fn decode_revision(body: String, hash: String) -> Result<Value> {
    let mut value: Value = serde_json::from_str(&body)?;
    ensure!(
        digest(&value) == hash,
        "资产版本 hash 校验失败；拒绝使用篡改版本"
    );
    value["hash"] = json!(hash);
    Ok(value)
}

pub fn snapshot(config: &StudioConfig) -> Result<Value> {
    let empty = json!({"available":false,"assets":[],"runs":[]});
    if !config.db.try_exists()? {
        return Ok(empty);
    }
    let conn = read_connection(config)?;
    if !available(&conn)? {
        return Ok(empty);
    }
    let mut assets: BTreeMap<(String, String), Value> = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT kind,asset_key,version,body_json,hash FROM atelier_revision ORDER BY kind,asset_key,version")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, u32>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    for row in rows {
        let (kind, key, version, body, hash) = row?;
        let revision = decode_revision(body, hash)?;
        ensure!(
            revision["kind"] == kind && revision["key"] == key && revision["version"] == version,
            "资产版本身份不匹配"
        );
        let asset = assets
            .entry((kind.clone(), key.clone()))
            .or_insert_with(|| json!({"kind":kind,"key":key,"revisions":[]}));
        asset["name"] = revision["name"].clone();
        asset["latest_version"] = json!(version);
        asset["revisions"].as_array_mut().unwrap().push(revision);
    }
    Ok(
        json!({"available":true,"assets":assets.into_values().collect::<Vec<_>>(),"runs":runtime::history(&conn)?}),
    )
}

fn revision(conn: &Connection, kind: &str, key: &str, version: u32) -> Result<Value> {
    let (body,hash): (String,String) = conn.query_row("SELECT body_json,hash FROM atelier_revision WHERE kind=?1 AND asset_key=?2 AND version=?3",params![kind,key,version],|r|Ok((r.get(0)?,r.get(1)?)))
        .with_context(||format!("不存在资产版本 {kind}/{key}@{version}"))?;
    let value = decode_revision(body, hash)?;
    ensure!(
        value["kind"] == kind && value["key"] == key && value["version"] == version,
        "资产版本身份不匹配"
    );
    Ok(value)
}

pub fn version(config: &StudioConfig, kind: &str, key: &str, version: u32) -> Result<Value> {
    let conn = read_connection(config)?;
    ensure!(
        available(&conn)?,
        "工坊需要显式创建 schema26 工作副本；原库不会自动迁移"
    );
    revision(&conn, kind, key, version)
}

pub fn save_asset(config: &StudioConfig, draft: &Value) -> Result<Value> {
    crate::db::require_current(&config.db)?;
    let mut conn = Connection::open_with_flags(&config.db, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let kind = draft["kind"].as_str().context("资产缺少 kind")?;
    ensure!(
        ["technique", "recipe", "formula"].contains(&kind),
        "未知资产类型"
    );
    let key = draft["key"].as_str().context("资产缺少 key")?;
    ensure!(
        !key.trim().is_empty() && key.len() <= 256 && !key.chars().any(char::is_control),
        "资产 key 无效"
    );
    let name = draft["name"].as_str().context("资产缺少 name")?;
    ensure!(!name.trim().is_empty() && name.len() <= 512, "资产名称无效");
    let prompt = draft["prompt"].as_str().context("资产缺少 prompt")?;
    ensure!(
        prompt.len() <= 256 * 1024 && (kind == "formula" || !prompt.trim().is_empty()),
        "Prompt 为空或过长"
    );
    let input_type = draft["input_type"]
        .as_str()
        .context("资产缺少 input_type")?;
    ensure!(
        ["text", "image", "json", "any"].contains(&input_type),
        "未知输入契约类型"
    );
    ensure!(
        draft["output_schema"].is_object(),
        "输出契约必须为 JSON Schema 对象"
    );
    ensure!(draft["definition"].is_object(), "definition 必须为对象");
    contract::schema(&draft["output_schema"], 0)?;
    if kind == "recipe" {
        contract::components(&draft["definition"])?;
    }
    if kind == "formula" {
        contract::resolve(&tx, &draft["definition"]["nodes"])?;
    }
    let next: u32 = tx.query_row(
        "SELECT coalesce(max(version),0)+1 FROM atelier_revision WHERE kind=?1 AND asset_key=?2",
        params![kind, key],
        |r| r.get(0),
    )?;
    let mut value = json!({"kind":kind,"key":key,"version":next,"name":name,"prompt":prompt,"input_type":input_type,"output_schema":draft["output_schema"],"definition":draft["definition"]});
    let hash = digest(&value);
    tx.execute("INSERT INTO atelier_revision(kind,asset_key,version,body_json,hash,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![kind,key,next,value.to_string(),hash,crate::now_iso()])?;
    tx.commit()?;
    value["hash"] = json!(hash);
    Ok(value)
}

pub(crate) fn seed(config: &StudioConfig) -> Result<()> {
    let conn = Connection::open_with_flags(&config.db, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let count: i64 = conn.query_row("SELECT count(*) FROM atelier_revision", [], |r| r.get(0))?;
    if count != 0 {
        return Ok(());
    }
    // These are real editable prompts, not fabricated materials, people or results.
    for mut value in [
        json!({"kind":"technique","key":"observe","version":1,"name":"观察与提取","prompt":"只根据输入记录可核对的要点，不推断不存在的事实。输出 JSON：{\"notes\":[\"要点\"]}。","input_type":"any","output_schema":{"type":"object","required":["notes"],"properties":{"notes":{"type":"array","items":{"type":"string"}}}},"definition":{}}),
        json!({"kind":"recipe","key":"synthesize","version":1,"name":"要点合成","prompt":"按照给定配比与约束合成可执行的设计简报。不评价美观，不冒充人工偏好。输出 JSON：{\"brief\":\"简报\"}。","input_type":"json","output_schema":{"type":"object","required":["brief"],"properties":{"brief":{"type":"string"}}},"definition":{"components":[{"input":"observe","ratio":1.0}],"constraints":{"facts_only":true}}}),
        json!({"kind":"formula","key":"local-brief","version":1,"name":"本地观察 → 简报","prompt":"","input_type":"any","output_schema":{"type":"object"},"definition":{"nodes":[{"id":"source","op":"source","inputs":[]},{"id":"observe","op":"technique","asset":{"kind":"technique","key":"observe","version":1},"inputs":["source"]},{"id":"synthesize","op":"recipe","asset":{"kind":"recipe","key":"synthesize","version":1},"inputs":["observe"]},{"id":"export","op":"export","inputs":["synthesize"]}]}}),
    ] {
        let hash = digest(&value);
        conn.execute("INSERT INTO atelier_revision(kind,asset_key,version,body_json,hash,created_at) VALUES(?1,?2,1,?3,?4,?5)",params![value["kind"].as_str(),value["key"].as_str(),value.to_string(),hash,crate::now_iso()])?;
        value["hash"] = json!(hash);
    }
    Ok(())
}

#[path = "atelier_contract.rs"]
mod contract;
#[path = "atelier_local.rs"]
mod local;

#[path = "atelier_runtime.rs"]
mod runtime;

/// Executes a frozen DAG, journaling real transitions; blocking HTTP cancellation is cooperative.
pub fn run(
    config: &StudioConfig,
    plan: &Value,
    model: &Value,
    cancel: &std::sync::atomic::AtomicBool,
    event: &mut dyn FnMut(Value),
) -> Result<Value> {
    runtime::run(config, plan, model, cancel, event)
}

/// Only loopback HTTP(S) services are accepted; no credentials or environment proxies.
pub fn discover_models(model: &Value) -> Result<Value> {
    let client = local::LocalModel::parse(model, false)?;
    Ok(match client.discover() {
        Ok(value) => value,
        Err(error) => json!({"connected":false,"models":[],"error":error.to_string()}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::{StudioAction, execute};
    use rusqlite::Connection;
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;

    struct Fixture {
        dir: PathBuf,
        config: StudioConfig,
    }
    impl Fixture {
        fn old() -> Result<Self> {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "ie-atelier-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_nanos(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            ));
            std::fs::create_dir(&dir)?;
            let config = StudioConfig {
                db: dir.join("old.sqlite"),
                root: dir.join("materials"),
                recipes: dir.join("recipes"),
            };
            std::fs::create_dir(&config.root)?;
            crate::db::migrate_to(&mut Connection::open(&config.db)?, 25)?;
            Ok(Self { dir, config })
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            // Only this exclusively-created, canonical temporary fixture is removed.
            let target = self.dir.canonicalize().unwrap();
            assert!(target.starts_with(std::env::temp_dir().canonicalize().unwrap()));
            assert!(
                target
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("ie-atelier-")
            );
            let _ = std::fs::remove_dir_all(target);
        }
    }
    fn file_hash(path: &std::path::Path) -> Result<String> {
        Ok(format!("{:x}", Sha256::digest(std::fs::read(path)?)))
    }
    impl Fixture {
        fn current() -> Result<Self> {
            let mut f = Self::old()?;
            let target = f.dir.join("workspace.sqlite");
            execute(
                &f.config,
                &StudioAction::CreateEmpty {
                    target: target.clone(),
                },
            )?;
            f.config.db = target;
            Ok(f)
        }
    }
    fn draft(key: &str, prompt: &str) -> Value {
        json!({"kind":"technique","key":key,"name":"可编辑观察","prompt":prompt,"input_type":"text","output_schema":{"type":"object","required":["notes"],"properties":{"notes":{"type":"array","items":{"type":"string"}}}},"definition":{}})
    }

    #[test]
    fn assets_append_versions_and_detect_tampered_history() -> Result<()> {
        let f = Fixture::current()?;
        let one = save_asset(&f.config, &draft("t.test", "FIRST"))?;
        let two = save_asset(&f.config, &draft("t.test", "SECOND"))?;
        assert_eq!(one["version"], 1);
        assert_eq!(two["version"], 2);
        assert_ne!(one["hash"], two["hash"]);
        assert_eq!(version(&f.config, "technique", "t.test", 1)?, one);
        assert_eq!(version(&f.config, "technique", "t.test", 2)?, two);
        assert!(version(&f.config, "technique", "t.test", 3).is_err());
        let conn = Connection::open(&f.config.db)?;
        assert!(
            conn.execute(
                "UPDATE atelier_revision SET hash='bad' WHERE asset_key='t.test'",
                []
            )
            .is_err()
        );
        assert!(
            conn.execute("DELETE FROM atelier_revision WHERE asset_key='t.test'", [])
                .is_err()
        );
        conn.execute_batch("DROP TRIGGER atelier_revision_no_update; UPDATE atelier_revision SET body_json=json_set(body_json,'$.prompt','TAMPERED') WHERE asset_key='t.test' AND version=1;")?;
        assert!(version(&f.config, "technique", "t.test", 1).is_err());
        assert!(snapshot(&f.config).is_err());
        let legacy = crate::studio::snapshot(&f.config)?;
        assert_eq!(legacy["atelier"]["available"], false);
        assert_eq!(legacy["atelier"]["integrity_error"], true);
        assert!(
            legacy["atelier"]["error"]
                .as_str()
                .unwrap()
                .contains("hash")
        );
        assert_eq!(legacy["counts"]["works"], 0);
        Ok(())
    }

    type MockServer = (String, std::thread::JoinHandle<Result<Vec<Value>>>);
    fn mock_server(responses: Vec<(u16, Value)>) -> Result<MockServer> {
        mock_server_hook(responses, || Ok(()))
    }
    fn mock_server_hook(
        responses: Vec<(u16, Value)>,
        mut hook: impl FnMut() -> Result<()> + Send + 'static,
    ) -> Result<MockServer> {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let endpoint = format!("http://{}", listener.local_addr()?);
        let handle = std::thread::spawn(move || -> Result<Vec<Value>> {
            let mut requests = Vec::new();
            for (status, body) in responses {
                let (mut stream, _) = listener.accept()?;
                stream.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;
                let mut bytes = Vec::new();
                let boundary;
                loop {
                    let mut chunk = [0u8; 4096];
                    let n = stream.read(&mut chunk)?;
                    ensure!(n > 0, "mock request ended before headers");
                    bytes.extend_from_slice(&chunk[..n]);
                    if let Some(i) = bytes.windows(4).position(|x| x == b"\r\n\r\n") {
                        boundary = i + 4;
                        break;
                    }
                    ensure!(bytes.len() < 1024 * 1024, "mock oversized request");
                }
                let header = String::from_utf8(bytes[..boundary].to_vec())?;
                let length = header
                    .lines()
                    .find_map(|l| {
                        l.split_once(':')
                            .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                            .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                while bytes.len() < boundary + length {
                    let mut chunk = [0u8; 4096];
                    let n = stream.read(&mut chunk)?;
                    ensure!(n > 0, "mock truncated body");
                    bytes.extend_from_slice(&chunk[..n]);
                }
                requests.push(json!({"headers":header,"body":if length>0 {serde_json::from_slice::<Value>(&bytes[boundary..boundary+length])?} else {Value::Null}}));
                hook()?;
                let body = body.to_string();
                write!(
                    stream,
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )?;
            }
            Ok(requests)
        });
        Ok((endpoint, handle))
    }

    #[test]
    fn discovery_reads_local_model_names_and_rejects_unsafe_urls() -> Result<()> {
        let (endpoint, server) =
            mock_server(vec![(200, json!({"models":[{"name":"local-qwen"}]}))])?;
        let found =
            discover_models(&json!({"protocol":"ollama","endpoint":endpoint,"timeout_seconds":1}))?;
        assert_eq!(found["connected"], true);
        assert_eq!(found["models"][0]["name"], "local-qwen");
        let requests = server.join().unwrap()?;
        assert!(
            requests[0]["headers"]
                .as_str()
                .unwrap()
                .starts_with("GET /api/tags ")
        );
        assert!(
            !requests[0]["headers"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains("authorization")
        );
        for url in [
            "https://api.openai.com",
            "http://192.168.1.2:11434",
            "http://0.0.0.0:11434",
            "http://user:SECRET@127.0.0.1",
            "http://localhost/#SECRET",
            "http://localhost/?key=SECRET",
            "file:///tmp/model",
            "http://localhost.evil.test",
        ] {
            let err = discover_models(&json!({"protocol":"ollama","endpoint":url}))
                .unwrap_err()
                .to_string();
            assert!(!err.contains("SECRET"));
        }
        let dead = std::net::TcpListener::bind("127.0.0.1:0")?;
        let endpoint = format!("http://{}", dead.local_addr()?);
        drop(dead);
        let absent =
            discover_models(&json!({"protocol":"ollama","endpoint":endpoint,"timeout_seconds":1}))?;
        assert_eq!(absent["connected"], false);
        assert_eq!(absent["models"], json!([]));
        Ok(())
    }

    #[test]
    fn formula_and_recipe_validation_reject_invalid_pins_dags_and_ratios() -> Result<()> {
        let f = Fixture::current()?;
        let base = version(&f.config, "formula", "local-brief", 1)?;
        let saved = save_asset(&f.config, &base)?;
        assert_eq!(saved["version"], 2);
        for problem in ["cycle", "dangling", "duplicate", "missing-version"] {
            let mut value = base.clone();
            value["key"] = json!(format!("invalid-{problem}"));
            match problem {
                "cycle" => value["definition"]["nodes"][1]["inputs"] = json!(["synthesize"]),
                "dangling" => value["definition"]["nodes"][1]["inputs"] = json!(["absent"]),
                "duplicate" => {
                    let duplicate = value["definition"]["nodes"][0].clone();
                    value["definition"]["nodes"]
                        .as_array_mut()
                        .unwrap()
                        .push(duplicate);
                }
                _ => value["definition"]["nodes"][1]["asset"]["version"] = json!(999),
            }
            assert!(save_asset(&f.config, &value).is_err(), "accepted {problem}");
        }
        let mut recipe = version(&f.config, "recipe", "synthesize", 1)?;
        for ratio in [
            json!(-0.1),
            json!(0),
            Value::Null,
            json!("NaN"),
            json!(1e308),
        ] {
            recipe["definition"]["components"][0]["ratio"] = ratio;
            if recipe["definition"]["components"][0]["ratio"] == json!(1e308) {
                recipe["definition"]["components"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"input":"second","ratio":1e308}));
            }
            assert!(save_asset(&f.config, &recipe).is_err());
        }
        let mut invalid_schema = draft("unsupported-schema", "TEST");
        invalid_schema["output_schema"] = json!({"$ref":"https://cloud/schema"});
        assert!(save_asset(&f.config, &invalid_schema).is_err());
        Ok(())
    }

    #[test]
    fn real_dag_freezes_versions_calls_prompts_and_exports_bound_artifacts() -> Result<()> {
        use std::sync::atomic::AtomicBool;
        let f = Fixture::current()?;
        let (endpoint, server) = mock_server(vec![
            (
                200,
                json!({"message":{"content":"{\"notes\":[\"REAL SOURCE NOTE\"]}"}}),
            ),
            (
                200,
                json!({"message":{"content":"{\"brief\":\"REAL BRIEF\"}"}}),
            ),
        ])?;
        let plan = json!({"name":"真实简报","formula":{"kind":"formula","key":"local-brief","version":1},"source":{"text":"ACTUAL INPUT TEXT"}});
        let model = json!({"protocol":"ollama","endpoint":endpoint,"model":"mock-local","timeout_seconds":2});
        let old = version(&f.config, "technique", "observe", 1)?;
        let mut edited = false;
        let mut events = Vec::new();
        let result = run(
            &f.config,
            &plan,
            &model,
            &AtomicBool::new(false),
            &mut |event| {
                if event["node_id"] == "observe" && event["status"] == "running" {
                    let mut draft = old.clone();
                    draft["prompt"] = json!("EDITED WHILE RUNNING");
                    save_asset(&f.config, &draft)
                        .expect("run must hold no write lock across callbacks/network");
                    edited = true;
                }
                events.push(event);
            },
        )?;
        assert!(edited);
        assert_eq!(result["status"], "completed");
        assert_eq!(result["snapshot"]["nodes"][1]["revision"], old);
        assert_eq!(result["snapshot"]["source"]["text"], "ACTUAL INPUT TEXT");
        let requests = server.join().unwrap()?;
        assert_eq!(
            result["snapshot"]["model"]["system_prompt"],
            requests[0]["body"]["messages"][0]["content"]
        );
        let first = requests[0]["body"]["messages"][1]["content"]
            .as_str()
            .unwrap();
        let decoded: Value = serde_json::from_str(first)?;
        assert_eq!(decoded["prompt"], old["prompt"]);
        assert!(first.contains("ACTUAL INPUT TEXT"));
        assert!(!first.contains("EDITED WHILE RUNNING"));
        let second = requests[1]["body"]["messages"][1]["content"]
            .as_str()
            .unwrap();
        assert!(
            second.contains("REAL SOURCE NOTE")
                && second.contains("effective_ratio")
                && second.contains("facts_only")
        );
        let artifact = &result["output"]["artifacts"][0];
        let path = std::path::Path::new(artifact["json_path"].as_str().unwrap());
        assert!(
            path.starts_with(
                f.config
                    .db
                    .parent()
                    .unwrap()
                    .canonicalize()?
                    .join("atelier-runs")
            )
        );
        let content: Value = serde_json::from_slice(&std::fs::read(path)?)?;
        assert_eq!(content["snapshot_hash"], result["snapshot_hash"]);
        assert!(content.to_string().contains("REAL BRIEF"));
        assert!(
            events
                .iter()
                .any(|v| v["node_id"] == "observe" && v["status"] == "running")
        );
        assert!(
            events
                .iter()
                .any(|v| v["node_id"] == "observe" && v["status"] == "completed")
        );
        assert_eq!(snapshot(&f.config)?["runs"][0]["output"], result["output"]);
        assert_eq!(crate::studio::snapshot(&f.config)?["counts"]["works"], 0);
        Ok(())
    }

    #[test]
    fn missing_real_source_is_a_readable_failed_run_not_a_fabricated_result() -> Result<()> {
        let f = Fixture::current()?;
        let plan = json!({"formula":{"kind":"formula","key":"local-brief","version":1},"source":{"work":99999}});
        let result = run(
            &f.config,
            &plan,
            &json!({"model":"local","timeout_seconds":1}),
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| {},
        )?;
        assert_eq!(result["status"], "failed");
        assert_eq!(result["stages"][0]["node_id"], "source");
        assert_eq!(result["stages"][0]["status"], "failed");
        assert!(result["output"].is_null());
        assert_eq!(snapshot(&f.config)?["runs"][0]["status"], "failed");
        Ok(())
    }

    #[test]
    fn model_http_non_json_and_contract_errors_fail_and_block_dependents() -> Result<()> {
        for (status, content) in [
            (200, "SENSITIVE_RESPONSE_NOT_JSON"),
            (200, "{\"notes\":[42]}"),
            (500, "SENSITIVE_HTTP_BODY"),
        ] {
            let f = Fixture::current()?;
            let (endpoint, server) =
                mock_server(vec![(status, json!({"message":{"content":content}}))])?;
            let plan = json!({"formula":{"kind":"formula","key":"local-brief","version":1},"source":{"text":"real text"}});
            let result = run(
                &f.config,
                &plan,
                &json!({"endpoint":endpoint,"model":"local","timeout_seconds":2}),
                &std::sync::atomic::AtomicBool::new(false),
                &mut |_| {},
            )?;
            assert_eq!(result["status"], "failed");
            assert_eq!(result["stages"][1]["status"], "failed");
            assert_eq!(result["stages"][2]["status"], "blocked");
            assert!(result["output"].is_null());
            assert!(!result.to_string().contains("SENSITIVE_"));
            assert!(!f.config.db.parent().unwrap().join("atelier-runs").exists());
            assert_eq!(server.join().unwrap()?.len(), 1);
            assert_eq!(snapshot(&f.config)?["runs"][0]["status"], "failed");
        }
        Ok(())
    }

    #[test]
    fn cancellation_is_honest_during_blocking_http_and_discards_response() -> Result<()> {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, mpsc};
        let f = Fixture::current()?;
        let (arrived, received) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let (endpoint, server) = mock_server_hook(
            vec![(
                200,
                json!({"message":{"content":"{\"notes\":[\"MUST NOT COMMIT\"]}"}}),
            )],
            move || {
                arrived.send(())?;
                wait.recv_timeout(std::time::Duration::from_secs(3))?;
                Ok(())
            },
        )?;
        let config = f.config.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let runner = std::thread::spawn(move || {
            let plan = json!({"formula":{"kind":"formula","key":"local-brief","version":1},"source":{"text":"real text"}});
            run(
                &config,
                &plan,
                &json!({"endpoint":endpoint,"model":"local","timeout_seconds":3}),
                &worker_cancel,
                &mut |_| {},
            )
        });
        received.recv_timeout(std::time::Duration::from_secs(3))?;
        cancel.store(true, Ordering::SeqCst);
        let during = snapshot(&f.config)?;
        assert_eq!(
            during["runs"][0]["status"], "running",
            "blocking HTTP has not returned: must not claim stopped"
        );
        assert_eq!(during["runs"][0]["stages"][1]["status"], "running");
        release.send(())?;
        let result = runner.join().unwrap()?;
        assert_eq!(result["status"], "cancelled");
        assert_eq!(result["stages"][1]["status"], "cancelled");
        assert_eq!(result["stages"][2]["status"], "blocked");
        assert!(!result.to_string().contains("MUST NOT COMMIT"));
        assert!(result["output"].is_null());
        assert_eq!(server.join().unwrap()?.len(), 1);
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let result = run(
            &f.config,
            &json!({"formula":{"kind":"formula","key":"local-brief","version":1},"source":{"text":"text"}}),
            &json!({"endpoint":format!("http://{}",listener.local_addr()?),"model":"local"}),
            &AtomicBool::new(true),
            &mut |_| {},
        )?;
        assert_eq!(result["status"], "cancelled");
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        Ok(())
    }

    fn export_plan() -> Value {
        json!({"name":"本地导出","nodes":[{"id":"source","op":"source","inputs":[]},{"id":"export","op":"export","inputs":["source"]}],"source":{"text":"real brief text, not generated code"}})
    }

    #[test]
    fn exports_are_exclusive_and_history_detects_modified_artifacts_or_journals() -> Result<()> {
        let f = Fixture::current()?;
        let model = json!({"model":"local"});
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let first = run(&f.config, &export_plan(), &model, &cancel, &mut |_| {})?;
        assert_eq!(first["status"], "completed");
        assert_eq!(first["artifacts_valid"], true);
        let path = std::path::Path::new(
            first["output"]["artifacts"][0]["json_path"]
                .as_str()
                .unwrap(),
        );
        let before = std::fs::read(path)?;
        let second = run(&f.config, &export_plan(), &model, &cancel, &mut |_| {})?;
        assert_ne!(
            first["output"]["artifacts"][0]["json_path"],
            second["output"]["artifacts"][0]["json_path"]
        );
        assert_eq!(std::fs::read(path)?, before);
        std::fs::write(path, b"TAMPERED ARTIFACT")?;
        let view = snapshot(&f.config)?;
        assert_eq!(view["runs"][0]["artifacts_valid"], true);
        assert_eq!(view["runs"][1]["artifacts_valid"], false);
        let conn = Connection::open(&f.config.db)?;
        assert!(
            conn.execute(
                "UPDATE atelier_run SET snapshot_json='{}' WHERE id=?1",
                [first["id"].as_i64().unwrap()]
            )
            .is_err()
        );
        assert!(
            conn.execute("UPDATE atelier_stage SET output_json='{}'", [])
                .is_err()
        );
        assert!(
            conn.execute(
                "UPDATE atelier_run SET output_json='{}' WHERE id=?1",
                [first["id"].as_i64().unwrap()]
            )
            .is_err(),
            "terminal run history must be immutable"
        );
        conn.execute_batch("DROP TRIGGER atelier_run_terminal;")?;
        conn.execute(
            "UPDATE atelier_run SET output_json='{}' WHERE id=?1",
            [first["id"].as_i64().unwrap()],
        )?;
        assert!(snapshot(&f.config).is_err());
        #[cfg(unix)]
        {
            let f = Fixture::current()?;
            std::fs::write(f.config.root.join("keep"), b"original")?;
            std::os::unix::fs::symlink(
                &f.config.root,
                f.config.db.parent().unwrap().join("atelier-runs"),
            )?;
            let result = run(&f.config, &export_plan(), &model, &cancel, &mut |_| {})?;
            assert_eq!(result["status"], "failed");
            assert_eq!(std::fs::read(f.config.root.join("keep"))?, b"original");
            assert_eq!(std::fs::read_dir(&f.config.root)?.count(), 1);
        }
        Ok(())
    }

    #[test]
    fn saved_array_schema_is_sent_without_forcing_object_in_either_protocol() -> Result<()> {
        for protocol in ["ollama", "openai"] {
            let f = Fixture::current()?;
            let schema = json!({"type":"array","items":{"type":"string"},"minItems":1});
            let mut technique = draft("array", "Return the real input as a JSON array");
            technique["output_schema"] = schema.clone();
            save_asset(&f.config, &technique)?;
            let response = if protocol == "ollama" {
                json!({"message":{"content":"[\"actual\"]"}})
            } else {
                json!({"choices":[{"message":{"content":"[\"actual\"]"}}]})
            };
            let (endpoint, server) = mock_server(vec![(200, response)])?;
            let plan = json!({"nodes":[{"id":"source","op":"source","inputs":[]},{"id":"array","op":"technique","asset":{"kind":"technique","key":"array","version":1},"inputs":["source"]},{"id":"export","op":"export","inputs":["array"]}],"source":{"text":"actual"}});
            let result = run(
                &f.config,
                &plan,
                &json!({"protocol":protocol,"endpoint":endpoint,"model":"local","timeout_seconds":2}),
                &std::sync::atomic::AtomicBool::new(false),
                &mut |_| {},
            )?;
            assert_eq!(result["status"], "completed");
            assert_eq!(result["output"]["nodes"]["array"], json!(["actual"]));
            let requests = server.join().unwrap()?;
            if protocol == "ollama" {
                assert_eq!(requests[0]["body"]["format"], schema);
            } else {
                assert!(requests[0]["body"].get("response_format").is_none());
            }
        }
        Ok(())
    }

    #[test]
    fn redirects_are_not_followed_and_invalid_runtime_graphs_send_no_http() -> Result<()> {
        use std::io::{Read, Write};
        let destination = std::net::TcpListener::bind("127.0.0.1:0")?;
        destination.set_nonblocking(true)?;
        let redirect = std::net::TcpListener::bind("127.0.0.1:0")?;
        let endpoint = format!("http://{}", redirect.local_addr()?);
        let target = format!("http://{}/cloud", destination.local_addr()?);
        let server = std::thread::spawn(move || -> Result<()> {
            let (mut stream, _) = redirect.accept()?;
            stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
            let mut buffer = [0; 8192];
            let n = stream.read(&mut buffer)?;
            ensure!(n > 0, "redirect mock received no request");
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )?;
            Ok(())
        });
        let result = discover_models(&json!({"endpoint":endpoint,"timeout_seconds":2}))?;
        assert_eq!(result["connected"], false);
        assert!(result["error"].as_str().unwrap().contains("302"));
        server.join().unwrap()?;
        assert_eq!(
            destination.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        let f = Fixture::current()?;
        let base = version(&f.config, "formula", "local-brief", 1)?;
        let before = file_hash(&f.config.db)?;
        for problem in ["cycle", "dangling", "duplicate", "pin", "formula-override"] {
            let mut nodes = base["definition"]["nodes"].clone();
            match problem {
                "cycle" => nodes[1]["inputs"] = json!(["synthesize"]),
                "dangling" => nodes[1]["inputs"] = json!(["missing"]),
                "duplicate" => {
                    let n = nodes[0].clone();
                    nodes.as_array_mut().unwrap().push(n);
                }
                _ => nodes[1]["asset"]["version"] = json!(999),
            }
            let mut plan = json!({"nodes":nodes,"source":{"text":"real"}});
            if problem == "formula-override" {
                plan["formula"] = json!({"kind":"formula","key":"local-brief","version":1});
            }
            assert!(run(&f.config,&plan,&json!({"endpoint":format!("http://{}",destination.local_addr()?),"model":"local"}),&std::sync::atomic::AtomicBool::new(false),&mut |_|{}).is_err());
        }
        assert_eq!(file_hash(&f.config.db)?, before);
        assert_eq!(snapshot(&f.config)?["runs"], json!([]));
        assert_eq!(
            destination.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        Ok(())
    }

    #[test]
    fn proxy_probe() -> Result<()> {
        let Ok(endpoint) = std::env::var("IE_ATELIER_PROXY_PROBE") else {
            return Ok(());
        };
        let result =
            discover_models(&json!({"protocol":"openai","endpoint":endpoint,"timeout_seconds":2}))?;
        assert_eq!(result["connected"], true);
        assert_eq!(result["models"][0]["name"], "local-only");
        Ok(())
    }
    #[test]
    fn environment_proxies_and_cloud_keys_cannot_cross_local_transport_boundary() -> Result<()> {
        let (endpoint, server) = mock_server(vec![(200, json!({"data":[{"id":"local-only"}]}))])?;
        let trap = std::net::TcpListener::bind("127.0.0.1:0")?;
        trap.set_nonblocking(true)?;
        let proxy = format!("http://{}", trap.local_addr()?);
        // Isolated child environment, never mutate process-global env during parallel tests.
        let mut child = std::process::Command::new(std::env::current_exe()?);
        child.args(["--exact", "atelier::tests::proxy_probe"]).env(
            "IE_ATELIER_PROXY_PROBE",
            endpoint.replace("127.0.0.1", "localhost"),
        );
        for key in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
        ] {
            child.env(key, &proxy);
        }
        child
            .env("NO_PROXY", "")
            .env("no_proxy", "")
            .env("IE_API_KEY", "DUMMY_SENSITIVE_KEY")
            .env("GEMINI_API_KEY", "DUMMY_SENSITIVE_KEY");
        let output = child.output()?;
        ensure!(
            output.status.success(),
            "proxy-isolation child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let requests = server.join().unwrap()?;
        let header = requests[0]["headers"].as_str().unwrap();
        assert!(header.starts_with("GET /v1/models "));
        assert!(!header.to_lowercase().contains("authorization"));
        assert!(!header.contains("DUMMY_SENSITIVE_KEY"));
        assert_eq!(
            trap.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        Ok(())
    }

    #[test]
    fn image_sources_are_content_sniffed_frozen_bounded_and_require_explicit_vision() -> Result<()>
    {
        use base64::Engine;
        let f = Fixture::current()?;
        let path = f.config.root.join("incorrect.jpg");
        image::RgbImage::from_pixel(4, 3, image::Rgb([10, 20, 30]))
            .save_with_format(&path, image::ImageFormat::Png)?;
        let original = std::fs::read(&path)?;
        let conn = Connection::open(&f.config.db)?;
        conn.execute_batch("INSERT INTO pack(id,title,version) VALUES('fixture','Fixture','1');INSERT INTO domain(id,title,medium,pack_id) VALUES('fixture','Fixture','image','fixture');INSERT INTO source(id,kind,title,created_at) VALUES('fixture','manual','Fixture','fixture');INSERT INTO work(id,source_id,domain_id,collection,slug,canonical_url,title,first_seen_at,last_seen_at) VALUES(42,'fixture','fixture','fixture','actual','fixture://actual','Actual local image','fixture','fixture');INSERT INTO asset(work_id,role,local_path) VALUES(42,'screenshot','incorrect.jpg');")?;
        let mut technique = draft("vision", "Read the actual local image");
        technique["input_type"] = json!("image");
        save_asset(&f.config, &technique)?;
        let plan = json!({"nodes":[{"id":"source","op":"source","inputs":[]},{"id":"vision","op":"technique","asset":{"kind":"technique","key":"vision","version":1},"inputs":["source"]},{"id":"export","op":"export","inputs":["vision"]}],"source":{"work":42}});
        let (endpoint, server) = mock_server(vec![(
            200,
            json!({"choices":[{"message":{"content":"{\"notes\":[\"actual pixels\"]}"}}]}),
        )])?;
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let result = run(
            &f.config,
            &plan,
            &json!({"protocol":"openai","endpoint":endpoint,"model":"local-vl","vision":true,"timeout_seconds":2}),
            &cancel,
            &mut |event| {
                if event["node_id"] == "source" && event["status"] == "completed" {
                    std::fs::write(&path, b"CHANGED AFTER FREEZE").unwrap();
                }
            },
        )?;
        assert_eq!(result["status"], "completed");
        assert_eq!(result["snapshot"]["source"]["mime"], "image/png");
        let requests = server.join().unwrap()?;
        let uri = requests[0]["body"]["messages"][1]["content"][1]["image_url"]["url"]
            .as_str()
            .unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(uri.strip_prefix("data:image/png;base64,").unwrap())?,
            original
        );
        // Restore our fixture, then prove a text-only model never gets an HTTP request.
        let extensionless = f.config.root.join("blob");
        std::fs::write(&extensionless, &original)?;
        conn.execute("UPDATE asset SET local_path='blob' WHERE work_id=42", [])?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let model =
            json!({"model":"text-only","endpoint":format!("http://{}",listener.local_addr()?)});
        let result = run(&f.config, &plan, &model, &cancel, &mut |_| {})?;
        assert_eq!(result["status"], "failed");
        assert!(result["error"].as_str().unwrap().contains("视觉"));
        std::fs::OpenOptions::new()
            .write(true)
            .open(&extensionless)?
            .set_len(16 * 1024 * 1024 + 1)?;
        let result = run(&f.config, &plan, &model, &cancel, &mut |_| {})?;
        assert_eq!(result["stages"][0]["status"], "failed");
        assert!(result["error"].as_str().unwrap().contains("16 MiB"));
        image::RgbImage::from_pixel(8193, 1, image::Rgb([1, 2, 3]))
            .save_with_format(&extensionless, image::ImageFormat::Png)?;
        let result = run(&f.config, &plan, &model, &cancel, &mut |_| {})?;
        assert_eq!(result["stages"][0]["status"], "failed");
        assert!(result["error"].as_str().unwrap().contains("尺寸"));
        conn.execute(
            "UPDATE asset SET local_path='../outside' WHERE work_id=42",
            [],
        )?;
        std::fs::write(f.dir.join("outside"), original)?;
        let result = run(&f.config, &plan, &model, &cancel, &mut |_| {})?;
        assert_eq!(result["stages"][0]["status"], "failed");
        assert!(result["error"].as_str().unwrap().contains("越界"));
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        Ok(())
    }

    #[test]
    fn relative_recipe_weights_are_explicitly_frozen_and_sent_with_each_real_input() -> Result<()> {
        let f = Fixture::current()?;
        let mut recipe = version(&f.config, "recipe", "synthesize", 1)?;
        recipe["key"] = json!("weighted");
        recipe["definition"]["components"] =
            json!([{"input":"one","ratio":2.0},{"input":"two","ratio":1.0}]);
        save_asset(&f.config, &recipe)?;
        let plan = json!({"nodes":[{"id":"source","op":"source","inputs":[]},{"id":"one","op":"technique","inputs":["source"],"asset":{"kind":"technique","key":"observe","version":1}},{"id":"two","op":"technique","inputs":["source"],"asset":{"kind":"technique","key":"observe","version":1}},{"id":"recipe","op":"recipe","inputs":["one","two"],"asset":{"kind":"recipe","key":"weighted","version":1}},{"id":"export","op":"export","inputs":["recipe"]}],"source":{"text":"real input"}});
        let (endpoint, server) = mock_server(vec![
            (
                200,
                json!({"message":{"content":"{\"notes\":[\"FIRST\"]}"}}),
            ),
            (
                200,
                json!({"message":{"content":"{\"notes\":[\"SECOND\"]}"}}),
            ),
            (
                200,
                json!({"message":{"content":"{\"brief\":\"weighted inputs\"}"}}),
            ),
        ])?;
        let result = run(
            &f.config,
            &plan,
            &json!({"model":"local","endpoint":endpoint,"timeout_seconds":2}),
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| {},
        )?;
        assert_eq!(result["status"], "completed");
        let frozen = &result["snapshot"]["nodes"][3]["components"];
        assert_eq!(frozen[0]["ratio"], 2.0);
        assert_eq!(frozen[0]["effective_ratio"], 2.0 / 3.0);
        assert_eq!(frozen[1]["effective_ratio"], 1.0 / 3.0);
        let requests = server.join().unwrap()?;
        let prompt: Value = serde_json::from_str(
            requests[2]["body"]["messages"][1]["content"]
                .as_str()
                .unwrap(),
        )?;
        assert_eq!(prompt["components"], *frozen);
        assert_eq!(prompt["inputs"]["one"]["notes"][0], "FIRST");
        assert_eq!(prompt["inputs"]["two"]["notes"][0], "SECOND");
        let mut mismatched = plan.clone();
        mismatched["nodes"][3]["inputs"] = json!(["one"]);
        assert!(
            run(
                &f.config,
                &mismatched,
                &json!({"model":"local"}),
                &std::sync::atomic::AtomicBool::new(false),
                &mut |_| {}
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn schema25_person_metadata_stays_readable_without_migration_or_writes() -> Result<()> {
        let f = Fixture::old()?;
        // Explicit synthetic fixture consent only; snapshot itself must never create it.
        crate::personal::init(&crate::personal::InitOpts {
            db: f.config.db.to_str().unwrap(),
            person: "fixture-synthetic",
        })?;
        let before = file_hash(&f.config.db)?;
        let view = crate::studio::snapshot(&f.config)?;
        assert_eq!(view["schema_version"], 25);
        assert_eq!(view["atelier"]["available"], false);
        assert_eq!(view["people"][0]["status"], "cold_start");
        assert_eq!(view["people"][0]["usable"], 0);
        assert_eq!(view["people"][0]["active"], false);
        assert!(save_asset(&f.config, &draft("old-must-not-write", "REAL PROMPT")).is_err());
        assert!(
            run(
                &f.config,
                &export_plan(),
                &json!({"model":"local"}),
                &std::sync::atomic::AtomicBool::new(false),
                &mut |_| {}
            )
            .is_err()
        );
        assert_eq!(file_hash(&f.config.db)?, before);
        Ok(())
    }

    #[test]
    fn old_snapshot_is_readonly_and_explicit_new_workspace_has_editable_starters() -> Result<()> {
        let f = Fixture::old()?;
        let before = file_hash(&f.config.db)?;
        let old = snapshot(&f.config)?;
        assert_eq!(old["available"], false);
        assert_eq!(old["assets"], serde_json::json!([]));
        assert_eq!(crate::studio::snapshot(&f.config)?["atelier"], old);
        assert_eq!(file_hash(&f.config.db)?, before);
        let target = f.dir.join("workspace.sqlite");
        execute(
            &f.config,
            &StudioAction::CreateEmpty {
                target: target.clone(),
            },
        )?;
        let current = StudioConfig {
            db: target,
            ..f.config.clone()
        };
        let view = snapshot(&current)?;
        assert_eq!(view["available"], true);
        for kind in ["technique", "recipe", "formula"] {
            assert!(
                view["assets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a["kind"] == kind)
            );
        }
        assert_eq!(crate::studio::snapshot(&current)?["counts"]["works"], 0);
        assert_eq!(file_hash(&f.config.db)?, before);
        Ok(())
    }
}
