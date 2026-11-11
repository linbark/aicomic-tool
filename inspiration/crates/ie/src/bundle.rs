//! 数据导入导出：把语料在库与库之间搬家。
//!
//! 设计取舍：
//! - 导出用**自然键**而不是自增 id（work = source+collection+slug，asset = work+role+sha256，
//!   observation = asset+extractor+version），所以能导入任意库、重复导入不产生副本。
//! - 图片默认不打包（只有 sha 与路径）；`--with-assets` 才把 blob 以 base64 塞进包，
//!   导入端按 sha 落地到 `<root>/blobs/`，天然去重。
//! - 个人数据（内核、判定、会话）默认不导出，需要 `--with-personal` 显式打开。

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct ExportOpts<'a> {
    pub db: &'a str,
    pub out: &'a Path,
    pub format: &'a str, // json | ndjson | auto
    pub with_assets: bool,
    pub with_personal: bool,
    pub collection: Option<&'a str>,
    pub domain: Option<&'a str>,
    pub since: Option<&'a str>,
}

pub struct ImportOpts<'a> {
    pub db: &'a str,
    pub input: &'a Path,
    pub root: &'a Path,
    pub dry_run: bool,
}

pub fn export(o: &ExportOpts<'_>) -> Result<()> {
    let conn = Connection::open(o.db)?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    let mut bundle = json!({
        "format": "ie-bundle",
        "version": 1,
        "schema_version": conn.query_row("SELECT value FROM schema_meta WHERE key='schema_version'", [], |r| r.get::<_, String>(0)).unwrap_or_default(),
        "exported_at": crate::now_iso(),
        "includes": {"assets": o.with_assets, "personal": o.with_personal},
    });

    bundle["packs"] = jrows(
        &conn,
        "SELECT id,title,version,spec_json,applied_at FROM pack ORDER BY id",
        &[],
    )?;
    bundle["domains"] = jrows(
        &conn,
        "SELECT id,title,medium,pack_id,note FROM domain ORDER BY id",
        &[],
    )?;
    bundle["domain_edges"] = jrows(
        &conn,
        "SELECT a,b,distance,rationale FROM domain_edge ORDER BY a,b",
        &[],
    )?;
    bundle["sources"] = jrows(
        &conn,
        "SELECT id,kind,title,homepage,licence_note,policy_json,created_at FROM source ORDER BY id",
        &[],
    )?;
    bundle["vocab_terms"] = jrows(
        &conn,
        "SELECT id,pack_id,kind,term,label,definition,parent_id,aliases,status FROM vocab_term ORDER BY id",
        &[],
    )?;

    // 作品：按可选过滤条件
    let mut where_parts: Vec<String> = Vec::new();
    if let Some(c) = o.collection {
        where_parts.push(format!("w.collection = '{}'", c.replace('\'', "''")));
    }
    if let Some(d) = o.domain {
        where_parts.push(format!("w.domain_id = '{}'", d.replace('\'', "''")));
    }
    if let Some(s) = o.since {
        where_parts.push(format!("w.last_seen_at >= '{}'", s.replace('\'', "''")));
    }
    let filter = if where_parts.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_parts.join(" AND "))
    };
    let works_sql = format!(
        "SELECT w.source_id,w.domain_id,w.collection,w.slug,w.canonical_url,w.title,w.subtitle_quoted,
                w.summary_derived,w.creator_name,w.creator_url,w.origin_url,w.created_year,w.licence,
                w.rights_holder,w.status,w.payload_json,w.first_seen_at,w.last_seen_at
         FROM work w {filter} ORDER BY w.id"
    );
    let works = rows(&conn, &works_sql, &[])?;
    let keys: Vec<(String, String, String)> = works
        .iter()
        .map(|w| (s(w, "source_id"), s(w, "collection"), s(w, "slug")))
        .collect();
    bundle["works"] = Value::Array(works.clone());

    // 依赖作品的表：逐条带上作品自然键
    let mut assets = Vec::new();
    let mut observations = Vec::new();
    let mut measures = Vec::new();
    let mut evidence = Vec::new();
    let mut usage = Vec::new();
    for (source_id, collection, slug) in &keys {
        let wkey = json!({"source_id": source_id, "collection": collection, "slug": slug});
        for a in rows(
            &conn,
            "SELECT a.role,a.url,a.local_path,a.sha256,a.width,a.height,a.bytes,a.fetched_at, a.id
             FROM asset a JOIN work w ON w.id=a.work_id
             WHERE w.source_id=?1 AND w.collection=?2 AND w.slug=?3 ORDER BY a.id",
            &[&source_id, &collection, &slug],
        )? {
            let asset_id = a["id"].as_i64().unwrap_or(0);
            let akey = json!({"work": wkey, "role": s(&a,"role"), "sha256": a.get("sha256").cloned().unwrap_or(Value::Null)});
            let mut row = a.clone();
            row["work"] = wkey.clone();
            if o.with_assets
                && let Some(lp) = a.get("local_path").and_then(|x| x.as_str())
            {
                let p = PathBuf::from("data/inspiration").join(lp);
                if let Ok(bytes) = std::fs::read(&p) {
                    use base64::Engine;
                    row["data_b64"] =
                        json!(base64::engine::general_purpose::STANDARD.encode(bytes));
                }
            }
            assets.push(row);
            for ob in rows(
                &conn,
                "SELECT id,extractor,extractor_version,payload_json,confidence,created_at FROM observation
                 WHERE asset_id=?1 ORDER BY id",
                &[&asset_id],
            )? {
                let obs_id = ob["id"].as_i64().unwrap_or(0);
                let mut orec = ob.clone();
                orec["asset"] = akey.clone();
                observations.push(orec);
                for m in rows(
                    &conn,
                    "SELECT vt.term AS metric, m.value, m.unit, m.confidence FROM measure m
                     JOIN vocab_term vt ON vt.id=m.metric_term_id WHERE m.observation_id=?1",
                    &[&obs_id],
                )? {
                    let mut mrec = m.clone();
                    mrec["observation"] = json!({"asset": akey, "extractor": s(&ob,"extractor"), "extractor_version": s(&ob,"extractor_version")});
                    measures.push(mrec);
                }
            }
        }
        let work_id: i64 = conn.query_row(
            "SELECT id FROM work WHERE source_id=?1 AND collection=?2 AND slug=?3",
            params![source_id, collection, slug],
            |r| r.get(0),
        )?;
        for e in rows(
            &conn,
            "SELECT vt.term AS primitive, e.source, e.score, e.confidence, e.extractor_version, e.updated_at
             FROM work_primitive_evidence e JOIN vocab_term vt ON vt.id=e.primitive_term_id
             WHERE e.work_id=?1",
            &[&work_id],
        )? {
            let mut r = e.clone();
            r["work"] = wkey.clone();
            evidence.push(r);
        }
        for u in rows(
            &conn,
            "SELECT vt.term AS device, d.strength, d.note FROM device_usage d
             JOIN vocab_term vt ON vt.id=d.device_term_id WHERE d.work_id=?1 AND d.fragment_id IS NULL",
            &[&work_id],
        )? {
            let mut r = u.clone();
            r["work"] = wkey.clone();
            usage.push(r);
        }
    }
    bundle["assets"] = Value::Array(assets);
    bundle["observations"] = Value::Array(observations);
    bundle["measures"] = Value::Array(measures);
    bundle["evidence"] = Value::Array(evidence);
    bundle["device_usage"] = Value::Array(usage);

    bundle["principles"] = jrows(
        &conn,
        "SELECT id,key,statement,rationale,context_json,boundary,counterexample,status,confidence,created_at,updated_at FROM principle ORDER BY id",
        &[],
    )?;
    let mut pev = Vec::new();
    for p in bundle["principles"].as_array().cloned().unwrap_or_default() {
        let pid = p["id"].as_i64().unwrap_or(0);
        for e in rows(
            &conn,
            "SELECT e.role, e.note, e.weight, w.source_id, w.collection, w.slug
             FROM principle_evidence e JOIN work w ON w.id=e.work_id WHERE e.principle_id=?1",
            &[&pid],
        )? {
            let mut r = e.clone();
            r["principle_key"] = p.get("key").cloned().unwrap_or(Value::Null);
            r["work"] = json!({"source_id": s(&e,"source_id"), "collection": s(&e,"collection"), "slug": s(&e,"slug")});
            pev.push(r);
        }
    }
    bundle["principle_evidence"] = Value::Array(pev);
    bundle["specs"] = jrows(
        &conn,
        "SELECT domain_id,name,profile_json,body_md,tokens_json,created_at,updated_at FROM spec ORDER BY id",
        &[],
    )?;

    if o.with_personal {
        bundle["persons"] = jrows(
            &conn,
            "SELECT id,display_name,locale,privacy_level,consent_json,reliability,created_at FROM person ORDER BY id",
            &[],
        )?;
        bundle["kernels"] = jrows(
            &conn,
            "SELECT person_id,version,model_kind,weights_json,prior_json,fit_json,maturity,status,created_at FROM kernel ORDER BY person_id,version",
            &[],
        )?;
        bundle["kernel_claims"] = jrows(
            &conn,
            "SELECT k.person_id, k.version, c.kind, c.statement, c.effect, c.confidence, c.evidence_count, c.counterexample FROM kernel_claim c JOIN kernel k ON k.id=c.kernel_id ORDER BY k.person_id, k.version",
            &[],
        )?;
    }

    if let Some(parent) = o.out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let ext = o
        .out
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let ndjson =
        o.format == "ndjson" || (o.format == "auto" && matches!(ext.as_str(), "ndjson" | "jsonl"));
    let text = if ndjson {
        write_ndjson(&bundle, o.out)?;
        String::new()
    } else {
        let t = serde_json::to_string(&bundle)?;
        std::fs::write(o.out, &t)?;
        t
    };
    let stats = json!({
        "works": keys.len(),
        "assets": bundle["assets"].as_array().map(|a| a.len()).unwrap_or(0),
        "observations": bundle["observations"].as_array().map(|a| a.len()).unwrap_or(0),
        "bytes": if ndjson { std::fs::metadata(o.out).map(|m| m.len()).unwrap_or(0) as usize } else { text.len() },
        "format": if ndjson { "ndjson" } else { "json" },
    });
    println!(
        "导出完成 {} 条作品 -> {}\n{}",
        keys.len(),
        o.out.display(),
        stats
    );
    Ok(())
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

fn jrows(conn: &Connection, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Result<Value> {
    Ok(Value::Array(rows(conn, sql, params)?))
}

fn rows(conn: &Connection, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(sql)?;
    let cols: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
    let mut out = Vec::new();
    let mut r = stmt.query(params)?;
    while let Some(row) = r.next()? {
        let mut obj = serde_json::Map::new();
        for (i, name) in cols.iter().enumerate() {
            let v = row.get_ref(i)?;
            let val = match v {
                rusqlite::types::ValueRef::Null => Value::Null,
                rusqlite::types::ValueRef::Integer(i) => json!(i),
                rusqlite::types::ValueRef::Real(f) => json!(f),
                rusqlite::types::ValueRef::Text(t) => {
                    json!(String::from_utf8_lossy(t).to_string())
                }
                rusqlite::types::ValueRef::Blob(b) => json!(format!("<blob {}B>", b.len())),
            };
            obj.insert(name.clone(), val);
        }
        out.push(Value::Object(obj));
    }
    Ok(out)
}

/// NDJSON：首行 header，其余按依赖顺序一行一条，导入端可单遍流式消费。
fn write_ndjson(b: &Value, out: &Path) -> Result<usize> {
    use std::io::Write;
    let f = std::fs::File::create(out)?;
    let mut w = std::io::BufWriter::new(f);
    let header = json!({
        "_type": "header",
        "format": "ie-bundle",
        "version": 1,
        "schema_version": b["schema_version"],
        "exported_at": b["exported_at"],
        "includes": b["includes"],
    });
    writeln!(w, "{}", header)?;
    const ORDER: &[(&str, &str)] = &[
        ("packs", "pack"),
        ("domains", "domain"),
        ("domain_edges", "domain_edge"),
        ("sources", "source"),
        ("vocab_terms", "vocab_term"),
        ("works", "work"),
        ("assets", "asset"),
        ("observations", "observation"),
        ("measures", "measure"),
        ("evidence", "evidence"),
        ("device_usage", "device_usage"),
        ("principles", "principle"),
        ("principle_evidence", "principle_evidence"),
        ("specs", "spec"),
        ("persons", "person"),
        ("kernels", "kernel"),
        ("kernel_claims", "kernel_claim"),
    ];
    let mut n = 0usize;
    for (key, kind) in ORDER {
        if let Some(a) = b.get(*key).and_then(|x| x.as_array()) {
            for row in a {
                let mut r = row.clone();
                r["_type"] = json!(kind);
                writeln!(w, "{}", r)?;
                n += 1;
            }
        }
    }
    w.flush()?;
    Ok(n)
}

// ---------------------------------------------------------------- 导入

const HANDLED: &[&str] = &[
    "pack",
    "domain",
    "domain_edge",
    "source",
    "vocab_term",
    "work",
    "asset",
    "observation",
    "measure",
    "evidence",
    "device_usage",
    "principle",
    "principle_evidence",
    "spec",
];

struct Imp<'a> {
    tx: rusqlite::Transaction<'a>,
    root: PathBuf,
    work_map: HashMap<String, i64>,
    term_map: HashMap<String, i64>,
    principle_map: HashMap<String, i64>,
    counts: HashMap<&'static str, usize>,
}

pub fn import(o: &ImportOpts<'_>) -> Result<()> {
    // 用第一行判断格式：NDJSON 的首行是 {"_type":"header",...}
    let ndjson = {
        use std::io::BufRead;
        let f = std::fs::File::open(o.input)
            .with_context(|| format!("读不到 {}", o.input.display()))?;
        let mut r = std::io::BufReader::new(f);
        let mut line = String::new();
        r.read_line(&mut line)?;
        serde_json::from_str::<Value>(line.trim())
            .ok()
            .and_then(|v| {
                v.get("_type")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
            })
            .as_deref()
            == Some("header")
    };

    let conn = Connection::open(o.db)?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    conn.busy_timeout(std::time::Duration::from_secs(20))?;
    let tx = conn.unchecked_transaction()?;
    let mut imp = Imp {
        tx,
        root: o.root.to_path_buf(),
        work_map: HashMap::new(),
        term_map: HashMap::new(),
        principle_map: HashMap::new(),
        counts: HashMap::new(),
    };
    if ndjson {
        imp.load_ndjson(o.input)?;
    } else {
        imp.load_bundle(o.input)?;
    }
    let counts = imp.counts.clone();
    if o.dry_run {
        imp.tx.rollback()?;
        println!("dry-run（已回滚）：{counts:?}");
    } else {
        imp.tx.commit()?;
        println!("导入完成：{counts:?}");
    }
    Ok(())
}

impl Imp<'_> {
    fn bump(&mut self, k: &'static str) {
        *self.counts.entry(k).or_insert(0) += 1;
    }

    /// JSON bundle：按依赖顺序遍历各数组
    fn load_bundle(&mut self, path: &Path) -> Result<()> {
        let text = std::fs::read_to_string(path)?;
        let b: Value = serde_json::from_str(&text).context("不是合法的 JSON")?;
        if b.get("format").and_then(|x| x.as_str()) != Some("ie-bundle") {
            anyhow::bail!("不是 ie-bundle 格式");
        }
        const ORDER: &[(&str, &str)] = &[
            ("packs", "pack"),
            ("domains", "domain"),
            ("domain_edges", "domain_edge"),
            ("sources", "source"),
            ("vocab_terms", "vocab_term"),
            ("works", "work"),
            ("assets", "asset"),
            ("observations", "observation"),
            ("measures", "measure"),
            ("evidence", "evidence"),
            ("device_usage", "device_usage"),
            ("principles", "principle"),
            ("principle_evidence", "principle_evidence"),
            ("specs", "spec"),
        ];
        for (key, kind) in ORDER {
            if let Some(a) = b.get(*key).and_then(|x| x.as_array()) {
                for row in a {
                    self.dispatch(kind, row)?;
                }
            }
        }
        let personal = ["persons", "kernels", "kernel_claims"]
            .iter()
            .map(|k| {
                b.get(*k)
                    .and_then(|x| x.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0)
            })
            .sum::<usize>();
        if personal > 0 {
            println!(
                "注意：包含 {personal} 条个人数据（内核/主张），当前版本不导入，如需迁移请用 --with-personal 的 JSON 包手工处理"
            );
        }
        Ok(())
    }

    /// NDJSON：逐行流式处理，内存只占一行
    fn load_ndjson(&mut self, path: &Path) -> Result<()> {
        use std::io::{BufRead, BufReader};
        let f = std::fs::File::open(path)?;
        let mut skipped = 0usize;
        for (i, line) in BufReader::new(f).lines().enumerate() {
            let line = line?;
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            let v: Value =
                serde_json::from_str(t).with_context(|| format!("第 {} 行不是 JSON", i + 1))?;
            let kind = v.get("_type").and_then(|x| x.as_str()).unwrap_or("");
            if kind.is_empty() || kind == "header" {
                continue;
            }
            if !HANDLED.contains(&kind) {
                skipped += 1;
                continue;
            }
            self.dispatch(kind, &v)?;
        }
        if skipped > 0 {
            println!("跳过 {skipped} 条未知类型（可能是 --with-personal 的个人数据）");
        }
        Ok(())
    }

    fn dispatch(&mut self, kind: &str, v: &Value) -> Result<()> {
        match kind {
            "pack" => self.pack(v),
            "domain" => self.domain(v),
            "domain_edge" => self.edge(v),
            "source" => self.source(v),
            "vocab_term" => self.vocab(v),
            "work" => self.work(v),
            "asset" => self.asset(v),
            "observation" => self.observation(v),
            "measure" => self.measure(v),
            "evidence" => self.evidence(v),
            "device_usage" => self.device_usage(v),
            "principle" => self.principle(v),
            "principle_evidence" => self.principle_evidence(v),
            "spec" => self.spec(v),
            _ => Ok(()),
        }
    }

    fn pack(&mut self, p: &Value) -> Result<()> {
        self.tx.execute(
            "INSERT INTO pack(id,title,version,spec_json,applied_at) VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(id) DO UPDATE SET title=excluded.title, version=excluded.version, spec_json=excluded.spec_json",
            params![s(p, "id"), s(p, "title"), s(p, "version"), s(p, "spec_json"), opt_s(p, "applied_at")],
        )?;
        self.bump("packs");
        Ok(())
    }

    fn domain(&mut self, d: &Value) -> Result<()> {
        self.tx.execute(
            "INSERT INTO domain(id,title,medium,pack_id,note) VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(id) DO UPDATE SET title=excluded.title, medium=excluded.medium, pack_id=excluded.pack_id",
            params![s(d, "id"), s(d, "title"), s(d, "medium"), s(d, "pack_id"), opt_s(d, "note")],
        )?;
        self.bump("domains");
        Ok(())
    }

    fn edge(&mut self, e: &Value) -> Result<()> {
        self.tx.execute(
            "INSERT OR REPLACE INTO domain_edge(a,b,distance,rationale) VALUES(?1,?2,?3,?4)",
            params![
                s(e, "a"),
                s(e, "b"),
                num(e, "distance", 0.5),
                opt_s(e, "rationale")
            ],
        )?;
        self.bump("domain_edges");
        Ok(())
    }

    fn source(&mut self, sc: &Value) -> Result<()> {
        self.tx.execute(
            "INSERT INTO source(id,kind,title,homepage,licence_note,policy_json,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(id) DO UPDATE SET title=excluded.title, homepage=excluded.homepage,
               licence_note=excluded.licence_note, policy_json=excluded.policy_json",
            params![
                s(sc, "id"),
                s(sc, "kind"),
                s(sc, "title"),
                opt_s(sc, "homepage"),
                opt_s(sc, "licence_note"),
                s(sc, "policy_json"),
                s(sc, "created_at")
            ],
        )?;
        self.bump("sources");
        Ok(())
    }

    fn vocab(&mut self, t: &Value) -> Result<()> {
        self.tx.execute(
            "INSERT INTO vocab_term(pack_id,kind,term,label,definition,parent_id,aliases,status)
             VALUES(?1,?2,?3,?4,?5,NULL,?6,?7)
             ON CONFLICT(pack_id,kind,term) DO UPDATE SET label=excluded.label, definition=excluded.definition,
               aliases=excluded.aliases, status=excluded.status",
            params![
                s(t, "pack_id"),
                s(t, "kind"),
                s(t, "term"),
                s(t, "label"),
                s(t, "definition"),
                s(t, "aliases"),
                s(t, "status")
            ],
        )?;
        let id: i64 = self.tx.query_row(
            "SELECT id FROM vocab_term WHERE pack_id=?1 AND kind=?2 AND term=?3",
            params![s(t, "pack_id"), s(t, "kind"), s(t, "term")],
            |r| r.get(0),
        )?;
        self.term_map.insert(
            format!("{}|{}|{}", s(t, "pack_id"), s(t, "kind"), s(t, "term")),
            id,
        );
        self.bump("vocab_terms");
        Ok(())
    }

    fn work(&mut self, w: &Value) -> Result<()> {
        self.tx.execute(
            "INSERT INTO work(source_id,domain_id,collection,slug,canonical_url,title,subtitle_quoted,summary_derived,
                creator_name,creator_url,origin_url,created_year,licence,rights_holder,status,payload_json,first_seen_at,last_seen_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
             ON CONFLICT(source_id,collection,slug) DO UPDATE SET
               title=excluded.title, subtitle_quoted=excluded.subtitle_quoted,
               summary_derived=excluded.summary_derived, payload_json=excluded.payload_json,
               status=excluded.status, last_seen_at=excluded.last_seen_at",
            params![
                s(w, "source_id"),
                s(w, "domain_id"),
                s(w, "collection"),
                s(w, "slug"),
                s(w, "canonical_url"),
                s(w, "title"),
                opt_s(w, "subtitle_quoted"),
                opt_s(w, "summary_derived"),
                opt_s(w, "creator_name"),
                opt_s(w, "creator_url"),
                opt_s(w, "origin_url"),
                w.get("created_year").and_then(|x| x.as_i64()),
                opt_s(w, "licence"),
                opt_s(w, "rights_holder"),
                s(w, "status"),
                s(w, "payload_json"),
                s(w, "first_seen_at"),
                s(w, "last_seen_at")
            ],
        )?;
        let id: i64 = self.tx.query_row(
            "SELECT id FROM work WHERE source_id=?1 AND collection=?2 AND slug=?3",
            params![s(w, "source_id"), s(w, "collection"), s(w, "slug")],
            |r| r.get(0),
        )?;
        self.work_map.insert(
            format!(
                "{}|{}|{}",
                s(w, "source_id"),
                s(w, "collection"),
                s(w, "slug")
            ),
            id,
        );
        self.bump("works");
        Ok(())
    }

    fn asset(&mut self, a: &Value) -> Result<()> {
        let wk = &a["work"];
        let Some(&wid) = self.work_map.get(&format!(
            "{}|{}|{}",
            s(wk, "source_id"),
            s(wk, "collection"),
            s(wk, "slug")
        )) else {
            return Ok(());
        };
        let sha = a.get("sha256").and_then(|x| x.as_str()).unwrap_or("");
        let role = s(a, "role");
        let mut local = a
            .get("local_path")
            .and_then(|x| x.as_str())
            .map(|x| x.to_string());
        if let Some(b64) = a.get("data_b64").and_then(|x| x.as_str()) {
            use base64::Engine;
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                let rel = format!("blobs/{}/{}", &sha[..2.min(sha.len())], sha);
                let full = self.root.join(&rel);
                if let Some(parent) = full.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&full, &bytes)?;
                local = Some(rel);
            }
        }
        self.tx.execute(
            "INSERT INTO asset(work_id,role,url,local_path,sha256,width,height,bytes,fetched_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(work_id,role,sha256) DO UPDATE SET local_path=coalesce(excluded.local_path, asset.local_path)",
            params![
                wid,
                role,
                opt_s(a, "url"),
                local,
                if sha.is_empty() { None } else { Some(sha.to_string()) },
                a.get("width").and_then(|x| x.as_i64()),
                a.get("height").and_then(|x| x.as_i64()),
                a.get("bytes").and_then(|x| x.as_i64()),
                s(a, "fetched_at")
            ],
        )?;
        self.bump("assets");
        Ok(())
    }

    fn observation(&mut self, ob: &Value) -> Result<()> {
        let ak = &ob["asset"];
        let Some(aid) = find_asset(&self.tx, &self.work_map, ak)? else {
            return Ok(());
        };
        self.tx.execute(
            "INSERT INTO observation(asset_id,extractor,extractor_version,payload_json,confidence,created_at)
             VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(asset_id,extractor,extractor_version) WHERE fragment_id IS NULL
             DO UPDATE SET payload_json=excluded.payload_json, confidence=excluded.confidence",
            params![
                aid,
                s(ob, "extractor"),
                s(ob, "extractor_version"),
                s(ob, "payload_json"),
                num(ob, "confidence", 0.5),
                s(ob, "created_at")
            ],
        )?;
        self.bump("observations");
        Ok(())
    }

    fn measure(&mut self, m: &Value) -> Result<()> {
        let ak = &m["observation"]["asset"];
        let Some(aid) = find_asset(&self.tx, &self.work_map, ak)? else {
            return Ok(());
        };
        let obs: Option<i64> = self
            .tx
            .query_row(
                "SELECT id FROM observation WHERE asset_id=?1 AND extractor=?2 AND extractor_version=?3 LIMIT 1",
                params![
                    aid,
                    s(&m["observation"], "extractor"),
                    s(&m["observation"], "extractor_version")
                ],
                |r| r.get(0),
            )
            .ok();
        let Some(oid) = obs else { return Ok(()) };
        let Some(&mid) = self
            .term_map
            .get(&format!("core|metric|{}", s(m, "metric")))
        else {
            return Ok(());
        };
        // measure 没有自然唯一键：同观察同指标已存在就跳过，保证重复导入不翻倍
        self.tx.execute(
            "INSERT INTO measure(observation_id,metric_term_id,value,unit,confidence)
             SELECT ?1,?2,?3,?4,?5
             WHERE NOT EXISTS (SELECT 1 FROM measure WHERE observation_id=?1 AND metric_term_id=?2)",
            params![
                oid,
                mid,
                num(m, "value", 0.0),
                opt_s(m, "unit"),
                num(m, "confidence", 0.5)
            ],
        )?;
        self.bump("measures");
        Ok(())
    }

    fn evidence(&mut self, e: &Value) -> Result<()> {
        let wk = &e["work"];
        let Some(&wid) = self.work_map.get(&format!(
            "{}|{}|{}",
            s(wk, "source_id"),
            s(wk, "collection"),
            s(wk, "slug")
        )) else {
            return Ok(());
        };
        let Some(&tid) = self
            .term_map
            .get(&format!("core|primitive|{}", s(e, "primitive")))
        else {
            return Ok(());
        };
        self.tx.execute(
            "INSERT INTO work_primitive_evidence(work_id,primitive_term_id,source,score,confidence,extractor_version,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(work_id,primitive_term_id,source) DO UPDATE SET
               score=excluded.score, confidence=excluded.confidence,
               extractor_version=excluded.extractor_version, updated_at=excluded.updated_at",
            params![
                wid,
                tid,
                s(e, "source"),
                num(e, "score", 0.0),
                num(e, "confidence", 0.5),
                opt_s(e, "extractor_version"),
                s(e, "updated_at")
            ],
        )?;
        self.tx.execute(
            "INSERT INTO work_primitive(work_id,primitive_term_id,score,confidence,source,extractor_version,updated_at)
             SELECT work_id,primitive_term_id,score,confidence,source,coalesce(extractor_version,''),updated_at
             FROM work_primitive_evidence WHERE work_id=?1 AND primitive_term_id=?2
             ORDER BY confidence DESC, score DESC LIMIT 1
             ON CONFLICT(work_id,primitive_term_id) DO UPDATE SET
               score=excluded.score, confidence=excluded.confidence, source=excluded.source,
               extractor_version=excluded.extractor_version, updated_at=excluded.updated_at",
            params![wid, tid],
        )?;
        self.bump("evidence");
        Ok(())
    }

    fn device_usage(&mut self, u: &Value) -> Result<()> {
        let wk = &u["work"];
        let Some(&wid) = self.work_map.get(&format!(
            "{}|{}|{}",
            s(wk, "source_id"),
            s(wk, "collection"),
            s(wk, "slug")
        )) else {
            return Ok(());
        };
        let Some(&did) = self.term_map.get(&format!("ui|device|{}", s(u, "device"))) else {
            return Ok(());
        };
        self.tx.execute(
            "INSERT OR IGNORE INTO device_usage(work_id,device_term_id,strength,note) VALUES(?1,?2,?3,?4)",
            params![wid, did, num(u, "strength", 0.5), opt_s(u, "note")],
        )?;
        self.bump("device_usage");
        Ok(())
    }

    fn principle(&mut self, p: &Value) -> Result<()> {
        self.tx.execute(
            "INSERT INTO principle(key,statement,rationale,context_json,boundary,counterexample,status,confidence,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(key) DO UPDATE SET statement=excluded.statement, rationale=excluded.rationale,
               context_json=excluded.context_json, boundary=excluded.boundary, counterexample=excluded.counterexample,
               status=excluded.status, confidence=excluded.confidence, updated_at=excluded.updated_at",
            params![
                s(p, "key"),
                s(p, "statement"),
                opt_s(p, "rationale"),
                s(p, "context_json"),
                opt_s(p, "boundary"),
                opt_s(p, "counterexample"),
                s(p, "status"),
                num(p, "confidence", 0.4),
                s(p, "created_at"),
                s(p, "updated_at")
            ],
        )?;
        let id: i64 = self.tx.query_row(
            "SELECT id FROM principle WHERE key=?1",
            params![s(p, "key")],
            |r| r.get(0),
        )?;
        self.principle_map.insert(s(p, "key"), id);
        self.bump("principles");
        Ok(())
    }

    fn principle_evidence(&mut self, e: &Value) -> Result<()> {
        let Some(&pid) = self.principle_map.get(&s(e, "principle_key")) else {
            return Ok(());
        };
        let wk = &e["work"];
        let Some(&wid) = self.work_map.get(&format!(
            "{}|{}|{}",
            s(wk, "source_id"),
            s(wk, "collection"),
            s(wk, "slug")
        )) else {
            return Ok(());
        };
        self.tx.execute(
            "INSERT OR IGNORE INTO principle_evidence(principle_id,work_id,role,note,weight) VALUES(?1,?2,?3,?4,?5)",
            params![pid, wid, s(e, "role"), opt_s(e, "note"), num(e, "weight", 1.0)],
        )?;
        self.bump("principle_evidence");
        Ok(())
    }

    fn spec(&mut self, sp: &Value) -> Result<()> {
        let exists: i64 = self.tx.query_row(
            "SELECT count(*) FROM spec WHERE name=?1 AND created_at=?2",
            params![s(sp, "name"), s(sp, "created_at")],
            |r| r.get(0),
        )?;
        if exists == 0 {
            self.tx.execute(
                "INSERT INTO spec(domain_id,name,profile_json,body_md,tokens_json,created_at,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    s(sp, "domain_id"),
                    s(sp, "name"),
                    s(sp, "profile_json"),
                    s(sp, "body_md"),
                    s(sp, "tokens_json"),
                    s(sp, "created_at"),
                    s(sp, "updated_at")
                ],
            )?;
            self.bump("specs");
        }
        Ok(())
    }
}

fn opt_s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string())
}

fn num(v: &Value, k: &str, d: f64) -> f64 {
    v.get(k).and_then(|x| x.as_f64()).unwrap_or(d)
}

fn find_asset(
    tx: &rusqlite::Transaction<'_>,
    work_map: &HashMap<String, i64>,
    ak: &Value,
) -> Result<Option<i64>> {
    let wk = &ak["work"];
    let Some(&wid) = work_map.get(&format!(
        "{}|{}|{}",
        s(wk, "source_id"),
        s(wk, "collection"),
        s(wk, "slug")
    )) else {
        return Ok(None);
    };
    let sha = ak.get("sha256").and_then(|x| x.as_str()).unwrap_or("");
    let id: Option<i64> = tx
        .query_row(
            "SELECT id FROM asset WHERE work_id=?1 AND role=?2 AND coalesce(sha256,'')=?3 LIMIT 1",
            params![wid, s(ak, "role"), sha],
            |r| r.get(0),
        )
        .ok();
    Ok(id)
}
