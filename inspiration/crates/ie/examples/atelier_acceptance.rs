//! Explicit isolated acceptance fixture for the native atelier.
//! Uses a real loopback HTTP mock, never a user's database or a paid model.
//! cargo run -p ie --example atelier_acceptance -- --out NEW_DIRECTORY
use anyhow::{Context, Result, ensure};
use ie::atelier;
use ie::studio::{self, StudioAction, StudioConfig};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let protocol = args
        .windows(2)
        .find(|p| p[0] == "--protocol")
        .map(|p| p[1].as_str())
        .unwrap_or("ollama");
    ensure!(
        ["ollama", "openai"].contains(&protocol),
        "--protocol 只接受 ollama / openai"
    );
    let out = args
        .windows(2)
        .find(|p| p[0] == "--out")
        .map(|p| PathBuf::from(&p[1]))
        .context("需要显式 --out <不存在的新目录>")?;
    ensure!(!out.exists() && !out.is_symlink(), "不覆盖已有目录或链接");
    ensure!(out.parent().is_some_and(|p| p.is_dir()), "请选择已有父目录");
    std::fs::create_dir(&out)?;
    let out = out.canonicalize()?;
    let config = StudioConfig {
        db: out.join("atelier-acceptance.db"),
        root: out.clone(),
        recipes: out.join("recipes"),
    };
    studio::execute(
        &config,
        &StudioAction::CreateEmpty {
            target: config.db.clone(),
        },
    )?;
    let conn = Connection::open(&config.db)?;
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
    conn.execute("INSERT INTO source(id,kind,title,policy_json,created_at) VALUES('atelier-acceptance','manual','隔离验收素材：不是用户收藏','{\"demo\":true}',?1)", [ie::now_iso()])?;
    conn.execute("INSERT INTO work(source_id,domain_id,collection,slug,canonical_url,title,summary_derived,payload_json,first_seen_at,last_seen_at) VALUES('atelier-acceptance','ui','atelier-acceptance','one','fixture://atelier/one','验收 · 无扩展名素材','仅供本地HTTP模拟验收，不是真实模型输出','{\"demo\":true}',?1,?1)", [ie::now_iso()])?;
    let work = conn.last_insert_rowid();
    std::fs::create_dir(out.join("blobs"))?;
    let picture = out.join("blobs/content-addressed-example");
    image::RgbImage::from_fn(240, 320, |x, y| {
        if (30..210).contains(&x) && (45..125).contains(&y) {
            image::Rgb([65, 109, 96])
        } else if (35..175).contains(&x) && (160..175).contains(&y) {
            image::Rgb([166, 133, 77])
        } else {
            image::Rgb([238, 229, 210])
        }
    })
    .save_with_format(&picture, image::ImageFormat::Png)?;
    conn.execute("INSERT INTO asset(work_id,role,local_path,width,height,bytes,sha256) VALUES(?1,'poster','blobs/content-addressed-example',240,320,?2,?3)", params![work,std::fs::metadata(&picture)?.len(),ie::capture::sha256_file(&picture)?])?;
    drop(conn);

    let schema = json!({"type":"object","required":["summary"],"properties":{"summary":{"type":"string"}},"additionalProperties":false});
    for (key, name, prompt) in [
        (
            "acceptance.observe",
            "验收 · 观形",
            "读取输入，只返回带 summary 的 JSON 对象。此为隔离验收。",
        ),
        (
            "acceptance.constraints",
            "验收 · 约束",
            "整理输入约束，只返回带 summary 的 JSON 对象。禁止审美评分。",
        ),
    ] {
        atelier::save_asset(
            &config,
            &json!({"kind":"technique","key":key,"name":name,"prompt":prompt,"input_type":"any","output_schema":schema,"definition":{}}),
        )?;
    }
    atelier::save_asset(
        &config,
        &json!({"kind":"recipe","key":"acceptance.compose","name":"验收 · 配方","prompt":"结合输入与结构化比例，只返回带 summary 的 JSON 对象。不添加未观察到的事实。","input_type":"json","output_schema":schema,"definition":{"components":[{"input":"observe","ratio":0.65},{"input":"constraints","ratio":0.35}],"constraints":{"purpose":"isolated mock acceptance only"}}}),
    )?;
    let nodes = json!([
        {"id":"source","op":"source","inputs":[]},
        {"id":"observe","op":"technique","inputs":["source"],"asset":{"kind":"technique","key":"acceptance.observe","version":1}},
        {"id":"constraints","op":"technique","inputs":["source"],"asset":{"kind":"technique","key":"acceptance.constraints","version":1}},
        {"id":"compose","op":"recipe","inputs":["observe","constraints"],"asset":{"kind":"recipe","key":"acceptance.compose","version":1}},
        {"id":"export","op":"export","inputs":["compose"]}
    ]);
    atelier::save_asset(
        &config,
        &json!({"kind":"formula","key":"acceptance.pipeline","name":"验收 · 青瓷本地流水线","prompt":"隔离验收的任务备注，不是执行 Prompt。","input_type":"any","output_schema":schema,"definition":{"nodes":nodes}}),
    )?;

    let requests = Arc::new(AtomicUsize::new(0));
    let received = Arc::new(Mutex::new(Vec::<Value>::new()));
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let count = requests.clone();
    let bodies = received.clone();
    std::thread::spawn(move || {
        for accepted in listener.incoming() {
            let Ok(mut stream) = accepted else { break };
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 4096];
            let Some((header_end, length)) = (loop {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break None,
                    Ok(n) => bytes.extend_from_slice(&buffer[..n]),
                }
                if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&bytes[..at]);
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.split_once(':')
                                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= at + 4 + length {
                        break Some((at + 4, length));
                    }
                }
            }) else {
                continue;
            };
            let head = String::from_utf8_lossy(&bytes[..header_end]);
            let path = head
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("");
            let response = if path.ends_with("/tags") {
                json!({"models":[{"name":"fixture-http-not-a-model"}]})
            } else if path.ends_with("/models") {
                json!({"data":[{"id":"fixture-http-not-a-model"}]})
            } else {
                count.fetch_add(1, Ordering::SeqCst);
                if let Ok(body) =
                    serde_json::from_slice::<Value>(&bytes[header_end..header_end + length])
                    && let Ok(mut values) = bodies.lock()
                {
                    values.push(body);
                }
                let content = "{\"summary\":\"隔离HTTP模拟结果：不是实际推理，不是人类偏好。\"}";
                if path.ends_with("/chat/completions") {
                    json!({"choices":[{"message":{"role":"assistant","content":content}}]})
                } else {
                    json!({"model":"fixture-http-not-a-model","message":{"role":"assistant","content":content},"done":true})
                }
            };
            let body = serde_json::to_vec(&response).unwrap_or_default();
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    let model = json!({"protocol":protocol,"endpoint":endpoint,"model":"fixture-http-not-a-model","temperature":0.2,"context":2048,"max_tokens":256,"timeout_seconds":15,"vision":false});
    let discovery = atelier::discover_models(&model)?;
    ensure!(discovery["connected"] == true, "模拟服务发现失败");
    let cancel = AtomicBool::new(false);
    let mut events = Vec::new();
    let plan = json!({"name":"隔离验收 · 文本真实HTTP链路","nodes":nodes,"source":{"text":"只检验工作流：青瓷色片、纸色背景、留白层级。所有数据为验收样例。"}});
    let text = atelier::run(&config, &plan, &model, &cancel, &mut |event| {
        events.push(event)
    })?;
    ensure!(
        text["status"] == "completed",
        "文本链路未完成：{}",
        text["error"]
    );
    ensure!(
        requests.load(Ordering::SeqCst) == 3,
        "应有三个真实本地HTTP模型节点"
    );
    let mut vision = model.clone();
    vision["vision"] = json!(true);
    let image_plan = json!({"name":"隔离验收 · 无扩展名图像","nodes":nodes,"source":{"work":work}});
    let image_run = atelier::run(&config, &image_plan, &vision, &cancel, &mut |event| {
        events.push(event)
    })?;
    ensure!(
        image_run["status"] == "completed",
        "图像链路未完成：{}",
        image_run["error"]
    );
    ensure!(
        requests.load(Ordering::SeqCst) == 6,
        "两轮应有六个真实本地HTTP模型节点"
    );
    let all = atelier::snapshot(&config)?;
    let conn = Connection::open_with_flags(&config.db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let people: i64 = conn.query_row("SELECT count(*) FROM person", [], |row| row.get(0))?;
    ensure!(people == 0, "验收不允许创建真人身份");
    let image_requests = received
        .lock()
        .map(|values| {
            values
                .iter()
                .filter(|value| {
                    let encoded = value.to_string();
                    encoded.contains("\"images\"") || encoded.contains("\"image_url\"")
                })
                .count()
        })
        .unwrap_or(0);
    ensure!(image_requests >= 2, "图像节点没有携带真实图像输入");
    let report = json!({"database":config.db,"asset_root":config.root,"http_mock_only":true,"protocol":protocol,"real_inference_requested":false,"requests":requests.load(Ordering::SeqCst),"image_requests":image_requests,"events":events.len(),"runs":all["runs"].as_array().map(Vec::len),"human_people":people,"text_status":text["status"],"image_status":image_run["status"]});
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out.join("acceptance-report.json"))?;
    serde_json::to_writer_pretty(file, &report)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
