//! Frozen execution and append-only transition journal. No transaction crosses HTTP.
use super::{contract, digest, local::LocalModel, read_connection, revision};
use crate::studio::StudioConfig;
use anyhow::{Context, Result, ensure};
use base64::Engine;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

fn bytes_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn source(conn: &Connection, config: &StudioConfig, selection: &Value) -> Result<Value> {
    ensure!(selection.is_object(), "source 必须选择 work 或输入 text");
    if let Some(text) = selection.get("text") {
        ensure!(
            selection.get("work").is_none(),
            "source 不得同时指定 work 与 text"
        );
        let text = text.as_str().context("source.text 必须为字符串")?;
        ensure!(
            !text.trim().is_empty() && text.len() <= 256 * 1024,
            "输入文本须非空且不超过 256 KiB"
        );
        return Ok(json!({"kind":"text","text":text,"sha256":bytes_hash(text.as_bytes())}));
    }
    let work = selection["work"]
        .as_i64()
        .filter(|n| *n > 0)
        .context("source.work 必须为已选择的作品 ID")?;
    let (title, summary, quoted): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT title,summary_derived,subtitle_quoted FROM work WHERE id=?1",
            [work],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .context("所选作品不存在")?;
    let asset: Option<(i64,String)>=conn.query_row("SELECT id,local_path FROM asset WHERE work_id=?1 AND local_path IS NOT NULL ORDER BY id LIMIT 1",[work],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let Some((asset_id, raw)) = asset else {
        let text = summary
            .or(quoted)
            .filter(|s| !s.trim().is_empty())
            .context("所选作品没有真实本地图片或文字")?;
        ensure!(text.len() <= 256 * 1024, "作品文字超过 256 KiB");
        return Ok(
            json!({"kind":"text","work":work,"title":title,"text":text,"sha256":bytes_hash(text.as_bytes())}),
        );
    };
    let root = config.root.canonicalize().context("素材根目录不可读")?;
    let path = root
        .join(raw)
        .canonicalize()
        .context("所选本地素材不存在")?;
    ensure!(
        path.starts_with(&root) && path.is_file(),
        "素材路径必须为素材根目录内的常规文件；拒绝越界链接"
    );
    let file = std::fs::File::open(&path)?;
    ensure!(
        file.metadata()?.len() <= 16 * 1024 * 1024,
        "本地素材超过 16 MiB"
    );
    let mut bytes = Vec::new();
    file.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 16 * 1024 * 1024, "本地素材超过 16 MiB");
    if let Ok(format) = image::guess_format(&bytes) {
        ensure!(
            [
                image::ImageFormat::Png,
                image::ImageFormat::Jpeg,
                image::ImageFormat::WebP
            ]
            .contains(&format),
            "仅支持按内容识别的 PNG/JPEG/WebP"
        );
        let (width, height) = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format)
            .into_dimensions()
            .context("图片尺寸无法读取")?;
        ensure!(
            width > 0
                && height > 0
                && width <= 8192
                && height <= 8192
                && u64::from(width) * u64::from(height) <= 32_000_000,
            "图片尺寸超过 8192 边长或 3200 万像素限制"
        );
        let mut reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        reader.decode().context("图片内容损坏或解码超限")?;
        let mime = match format {
            image::ImageFormat::Png => "image/png",
            image::ImageFormat::Jpeg => "image/jpeg",
            _ => "image/webp",
        };
        return Ok(
            json!({"kind":"image","work":work,"asset_id":asset_id,"title":title,"mime":mime,"width":width,"height":height,"bytes":bytes.len(),"sha256":bytes_hash(&bytes),"data_base64":base64::engine::general_purpose::STANDARD.encode(&bytes)}),
        );
    }
    ensure!(bytes.len() <= 256 * 1024, "本地文字超过 256 KiB");
    let text =
        std::str::from_utf8(&bytes).context("本地素材既不是支持的图片，也不是 UTF-8 文本")?;
    ensure!(
        !text.trim().is_empty() && !text.contains('\0'),
        "本地文本为空或含二进制内容"
    );
    Ok(
        json!({"kind":"text","work":work,"asset_id":asset_id,"title":title,"text":text,"sha256":bytes_hash(&bytes)}),
    )
}

fn freeze(config: &StudioConfig, plan: &Value, model: &LocalModel) -> Result<Value> {
    let mut conn = read_connection(config)?;
    let tx = conn.transaction()?;
    let formula = if let Some(reference) = plan.get("formula").filter(|v| !v.is_null()) {
        ensure!(
            reference["kind"] == "formula",
            "plan.formula 必须绑定 formula 资产"
        );
        let key = reference["key"].as_str().context("formula 缺少 key")?;
        let version = reference["version"]
            .as_u64()
            .filter(|n| *n > 0 && *n <= u32::MAX as u64)
            .context("formula 缺少明确版本")? as u32;
        revision(&tx, "formula", key, version)?
    } else {
        Value::Null
    };
    let raw = if !formula.is_null() {
        if let Some(nodes) = plan.get("nodes") {
            ensure!(
                nodes == &formula["definition"]["nodes"],
                "绑定丹方后 nodes 与该固定版本不一致；请保存新版本或解除绑定"
            );
        }
        &formula["definition"]["nodes"]
    } else {
        &plan["nodes"]
    };
    let nodes = contract::resolve(&tx, raw)?;
    let name = plan["name"].as_str().unwrap_or("本地工坊运行");
    ensure!(
        !name.trim().is_empty() && name.len() <= 512,
        "运行名称为空或过长"
    );
    let db = config.db.canonicalize()?;
    let artifact_root = db
        .parent()
        .context("工作库缺少父目录")?
        .join("atelier-runs");
    let (source, source_error) = match source(&tx, config, &plan["source"]) {
        Ok(value) => (value, None),
        Err(error) => (Value::Null, Some(error.to_string())),
    };
    let selection = json!({"work":plan["source"]["work"],"text":plan["source"]["text"]});
    Ok(
        json!({"snapshot_version":1,"name":name,"nodes":nodes,"formula":formula,"artifact_root":artifact_root,"source":source,"source_error":source_error,"model":model.value,"selection":selection,"cancellation":"cooperative: blocking HTTP must return or time out before cancellation is observed; then no output commit or subsequent request"}),
    )
}

struct Journal<'a> {
    conn: &'a Connection,
    run_id: i64,
    snapshot_hash: &'a str,
    event: &'a mut dyn FnMut(Value),
}
impl Journal<'_> {
    fn stage(
        &mut self,
        node: &Value,
        status: &str,
        input: &Value,
        output: Option<&Value>,
        error: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn;
        let run_id = self.run_id;
        let snapshot_hash = self.snapshot_hash;
        let now = crate::now_iso();
        let value = json!({"run_id":run_id,"snapshot_hash":snapshot_hash,"node_id":node["id"],"op":node["op"],"status":status,"input":input,"output":output,"error":error,"created_at":now});
        let hash = digest(&value);
        conn.execute("INSERT INTO atelier_stage(run_id,node_id,op,status,input_json,output_json,hash,error,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![run_id,node["id"].as_str(),node["op"].as_str(),status,input.to_string(),output.map(Value::to_string),hash,error,now])?;
        (self.event)(value);
        Ok(())
    }
}
fn finish(
    conn: &Connection,
    id: i64,
    hash: &str,
    status: &str,
    output: Option<&Value>,
    error: Option<&str>,
) -> Result<()> {
    let output_hash = output.map(|v| digest(&json!({"snapshot_hash":hash,"output":v})));
    conn.execute("UPDATE atelier_run SET status=?1,output_json=?2,output_hash=?3,error=?4,updated_at=?5 WHERE id=?6",params![status,output.map(Value::to_string),output_hash,error,crate::now_iso(),id])?;
    Ok(())
}
fn exclusive_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .context("产物只允许创建新文件；拒绝覆盖或链接")?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn export(root: &Path, id: i64, hash: &str, node: &Value, input: &Value) -> Result<Value> {
    match std::fs::create_dir(root) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    let meta = std::fs::symlink_metadata(root)?;
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink() && root.canonicalize()? == root,
        "atelier-runs 必须为工作库同级真实目录，拒绝链接"
    );
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let destination = root.join(format!(
        "run-{id}-{stamp}-{}",
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&destination).context("产物目录必须全新；不覆盖任何路径")?;
    ensure!(
        destination.canonicalize()? == destination,
        "产物目录不能是链接"
    );
    let content = json!({"run_id":id,"snapshot_hash":hash,"node_id":node["id"],"output":input});
    let bytes = serde_json::to_vec_pretty(&content)?;
    let brief = format!(
        "# 本地工坊简报\n\nRun {id} · snapshot `{hash}`\n\n仅记录真实节点输出；未评估美观，也不代表人工偏好。模型输出不会作为代码执行。\n\n```json\n{}\n```\n",
        serde_json::to_string_pretty(input)?
    );
    let json_path = destination.join("result.json");
    let brief_path = destination.join("brief.md");
    exclusive_file(&json_path, &bytes)?;
    exclusive_file(&brief_path, brief.as_bytes())?;
    Ok(
        json!({"run_id":id,"snapshot_hash":hash,"node_id":node["id"],"json_path":json_path,"brief_path":brief_path,"json_sha256":bytes_hash(&bytes),"brief_sha256":bytes_hash(brief.as_bytes())}),
    )
}

pub(super) fn run(
    config: &StudioConfig,
    plan: &Value,
    model: &Value,
    cancel: &AtomicBool,
    event: &mut dyn FnMut(Value),
) -> Result<Value> {
    crate::db::require_current(&config.db)?;
    let model = LocalModel::parse(model, true)?;
    let frozen = freeze(config, plan, &model)?;
    let hash = digest(&frozen);
    let conn = Connection::open_with_flags(&config.db, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    let now = crate::now_iso();
    conn.execute("INSERT INTO atelier_run(name,status,snapshot_json,snapshot_hash,created_at,updated_at) VALUES(?1,'running',?2,?3,?4,?4)",params![frozen["name"].as_str(),frozen.to_string(),hash,now])?;
    let id = conn.last_insert_rowid();
    event(json!({"run_id":id,"status":"running","snapshot_hash":hash}));
    let mut journal = Journal {
        conn: &conn,
        run_id: id,
        snapshot_hash: &hash,
        event,
    };
    let nodes = frozen["nodes"].as_array().unwrap();
    let source_ids: Vec<&str> = nodes
        .iter()
        .filter(|n| n["op"] == "source")
        .map(|n| n["id"].as_str().unwrap())
        .collect();
    let mut outputs = serde_json::Map::new();
    let mut artifacts = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        let node_id = node["id"].as_str().unwrap();
        let inputs = node["inputs"].as_array().unwrap();
        let input = Value::Object(
            inputs
                .iter()
                .filter_map(|i| {
                    let key = i.as_str().unwrap();
                    outputs.get(key).cloned().map(|v| (key.to_owned(), v))
                })
                .collect(),
        );
        journal.stage(node, "running", &input, None, None)?;
        let result = if cancel.load(Ordering::SeqCst) {
            Err(anyhow::anyhow!(
                "已取消；未提交当前节点输出、不会发起后续请求"
            ))
        } else {
            match node["op"].as_str().unwrap() {
                "source" => {
                    if let Some(error) = frozen["source_error"].as_str() {
                        Err(anyhow::anyhow!(error.to_owned()))
                    } else {
                        Ok(frozen["source"].clone())
                    }
                }
                "export" => export(
                    Path::new(frozen["artifact_root"].as_str().unwrap()),
                    id,
                    &hash,
                    node,
                    &input,
                ),
                _ => {
                    let mut projected = input.clone();
                    let mut images = Vec::new();
                    for i in inputs {
                        let key = i.as_str().unwrap();
                        if source_ids.contains(&key) && input[key]["kind"] == "image" {
                            images.push(input[key].clone());
                            projected[key]
                                .as_object_mut()
                                .unwrap()
                                .remove("data_base64");
                        }
                    }
                    let asset = &node["revision"];
                    let valid = match asset["input_type"].as_str().unwrap_or("invalid") {
                        "any" => true,
                        "json" => images.is_empty(),
                        "image" => !images.is_empty() && images.len() == inputs.len(),
                        "text" => {
                            images.is_empty()
                                && input
                                    .as_object()
                                    .unwrap()
                                    .values()
                                    .all(|v| v.is_string() || v["kind"] == "text")
                        }
                        _ => false,
                    };
                    if !valid {
                        Err(anyhow::anyhow!("上游输入不符合手法/配方 input_type 契约"))
                    } else {
                        model
                            .infer(asset, &projected, &images, &node["components"])
                            .and_then(|value| {
                                contract::validate(&value, &asset["output_schema"], 0)?;
                                Ok(value)
                            })
                    }
                }
            }
        };
        // Observe cancellation after blocking HTTP, before any successful stage/result commit.
        let cancelled = cancel.load(Ordering::SeqCst);
        match result {
            Ok(value) if !cancelled => {
                journal.stage(node, "completed", &input, Some(&value), None)?;
                if node["op"] == "export" {
                    artifacts.push(value.clone());
                }
                outputs.insert(node_id.to_owned(), value);
            }
            result => {
                let status = if cancelled { "cancelled" } else { "failed" };
                let error = if cancelled {
                    "取消已生效：阻塞 HTTP 已返回/超时；当前输出未提交，后续请求已阻止".to_owned()
                } else {
                    result.err().unwrap().to_string()
                };
                journal.stage(node, status, &input, None, Some(&error))?;
                for next in &nodes[index + 1..] {
                    journal.stage(
                        next,
                        "blocked",
                        &json!({}),
                        None,
                        Some("前序失败或取消；未执行"),
                    )?;
                }
                finish(&conn, id, &hash, status, None, Some(&error))?;
                event(json!({"run_id":id,"status":status,"error":error}));
                return record(&conn, id);
            }
        }
    }
    if cancel.load(Ordering::SeqCst) {
        finish(
            &conn,
            id,
            &hash,
            "cancelled",
            None,
            Some("已取消；最终运行输出未提交"),
        )?;
        event(json!({"run_id":id,"status":"cancelled"}));
    } else {
        let output =
            json!({"run_id":id,"snapshot_hash":hash,"nodes":outputs,"artifacts":artifacts});
        finish(&conn, id, &hash, "completed", Some(&output), None)?;
        event(json!({"run_id":id,"status":"completed","output":output}));
    }
    record(&conn, id)
}

fn optional_json(raw: Option<String>) -> Result<Value> {
    match raw {
        Some(v) => Ok(serde_json::from_str(&v)?),
        None => Ok(Value::Null),
    }
}
fn record(conn: &Connection, id: i64) -> Result<Value> {
    let mut value=conn.query_row("SELECT name,status,snapshot_json,snapshot_hash,output_json,output_hash,error,created_at,updated_at FROM atelier_run WHERE id=?1",[id],|r|Ok(json!({"id":id,"name":r.get::<_,String>(0)?,"status":r.get::<_,String>(1)?,"snapshot_raw":r.get::<_,String>(2)?,"snapshot_hash":r.get::<_,String>(3)?,"output_raw":r.get::<_,Option<String>>(4)?,"output_hash":r.get::<_,Option<String>>(5)?,"error":r.get::<_,Option<String>>(6)?,"created_at":r.get::<_,String>(7)?,"updated_at":r.get::<_,String>(8)?})))?;
    let snapshot: Value = serde_json::from_str(value["snapshot_raw"].as_str().unwrap())?;
    ensure!(
        digest(&snapshot) == value["snapshot_hash"],
        "运行快照 hash 校验失败"
    );
    let output = optional_json(value["output_raw"].as_str().map(str::to_owned))?;
    if !output.is_null() {
        ensure!(
            digest(&json!({"snapshot_hash":value["snapshot_hash"],"output":output}))
                == value["output_hash"],
            "运行输出与快照绑定 hash 校验失败"
        );
    }
    let mut stmt=conn.prepare("SELECT node_id,op,status,input_json,output_json,hash,error,created_at FROM atelier_stage WHERE run_id=?1 ORDER BY id")?;
    let rows = stmt.query_map([id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, String>(7)?,
        ))
    })?;
    let mut events = Vec::new();
    let mut latest = BTreeMap::new();
    for row in rows {
        let (node_id, op, status, input, output, hash, error, created) = row?;
        let event = json!({"run_id":id,"snapshot_hash":value["snapshot_hash"],"node_id":node_id,"op":op,"status":status,"input":serde_json::from_str::<Value>(&input)?,"output":optional_json(output)?,"error":error,"created_at":created});
        ensure!(digest(&event) == hash, "节点 journal hash 校验失败");
        latest.insert(node_id, event.clone());
        events.push(event);
    }
    let stages: Vec<Value> = snapshot["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| latest.remove(n["id"].as_str().unwrap()))
        .collect();
    let mut artifact_integrity = Vec::new();
    if let Some(artifacts) = output["artifacts"].as_array() {
        for artifact in artifacts {
            let checked = verify_artifact(
                artifact,
                id,
                value["snapshot_hash"].as_str().unwrap(),
                &snapshot,
                &stages,
            );
            artifact_integrity.push(json!({"node_id":artifact["node_id"],"json_path":artifact["json_path"],"brief_path":artifact["brief_path"],"valid":checked.is_ok(),"error":checked.err().map(|e|e.to_string())}));
        }
    }
    value["artifacts_valid"] = if output.is_null() {
        Value::Null
    } else {
        json!(artifact_integrity.iter().all(|v| v["valid"] == true))
    };
    value["artifact_integrity"] = json!(artifact_integrity);
    value["snapshot"] = snapshot;
    value["output"] = output;
    value["stages"] = json!(stages);
    value["events"] = json!(events);
    value.as_object_mut().unwrap().remove("snapshot_raw");
    value.as_object_mut().unwrap().remove("output_raw");
    Ok(value)
}
fn artifact_bytes(path: &Path, root: &Path) -> Result<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path).context("产物文件已丢失或不可读")?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 64 * 1024 * 1024,
        "产物不是受限常规文件或已变成链接"
    );
    ensure!(
        root.canonicalize()? == root && path.canonicalize()? == path && path.starts_with(root),
        "产物路径越界或包含链接"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 64 * 1024 * 1024, "产物超过验证字节上限");
    Ok(bytes)
}
fn verify_artifact(
    artifact: &Value,
    id: i64,
    hash: &str,
    snapshot: &Value,
    stages: &[Value],
) -> Result<()> {
    ensure!(
        artifact["run_id"] == id && artifact["snapshot_hash"] == hash,
        "产物引用没有绑定当前运行快照"
    );
    let root = Path::new(
        snapshot["artifact_root"]
            .as_str()
            .context("快照缺少产物根目录")?,
    );
    let json_path = Path::new(
        artifact["json_path"]
            .as_str()
            .context("产物缺少 JSON 路径")?,
    );
    let brief_path = Path::new(
        artifact["brief_path"]
            .as_str()
            .context("产物缺少简报路径")?,
    );
    let bytes = artifact_bytes(json_path, root)?;
    let brief = artifact_bytes(brief_path, root)?;
    ensure!(
        bytes_hash(&bytes) == artifact["json_sha256"]
            && bytes_hash(&brief) == artifact["brief_sha256"],
        "产物字节 hash 不匹配；不可作为原运行产物使用"
    );
    let content: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        content["run_id"] == id
            && content["snapshot_hash"] == hash
            && content["node_id"] == artifact["node_id"],
        "JSON 产物运行快照绑定不一致"
    );
    let stage = stages
        .iter()
        .find(|s| {
            s["node_id"] == artifact["node_id"] && s["op"] == "export" && s["status"] == "completed"
        })
        .context("产物没有已完成的 export journal")?;
    ensure!(
        content["output"] == stage["input"] && stage["output"] == *artifact,
        "产物内容与 export journal 不一致"
    );
    Ok(())
}

pub(super) fn history(conn: &Connection) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare("SELECT id FROM atelier_run ORDER BY id DESC LIMIT 20")?;
    let ids = stmt
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.into_iter().map(|id| record(conn, id)).collect()
}
