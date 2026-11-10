-- 灵感引擎 · 本地数据库 schema v2（领域无关）
-- 设计说明见 ../../灵感引擎设计.md。用法：sqlite3 inspiration.db < schema.v2.sql
--
-- 与 v1（UI 专用）的区别：核心层不含任何 UI 名词。
--   · 审美基元（primitive）、度量（metric）、语境（context）属于 core 包，跨领域共享；
--   · 领域知识（device/style）全部装在可插拔的 domain pack 里；
--   · 新增艺术/建筑/音乐等领域时只加数据，不改表结构。
--
-- 三条硬约束：
--   1. primitive 只能属于 core 包，device 只能属于领域包（CHECK 强制），防止词表漂移；
--   2. 手法必须挂到基元上（trigger 校验 kind），不允许孤立标签；
--   3. 原则（principle）发布前必须有反例或明确边界，见 v_principle_card。

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS schema_meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version','2');

-- ============================================================ 0. 领域包与词表

CREATE TABLE IF NOT EXISTS pack (
  id          TEXT PRIMARY KEY,              -- core | ui | architecture | music ...
  title       TEXT NOT NULL,
  version     TEXT NOT NULL,
  spec_json   TEXT NOT NULL DEFAULT '{}',    -- 包声明：词表、抽取器、模板、数据源
  applied_at  TEXT
);

CREATE TABLE IF NOT EXISTS vocab_term (
  id          INTEGER PRIMARY KEY,
  pack_id     TEXT NOT NULL REFERENCES pack(id),
  kind        TEXT NOT NULL CHECK (kind IN
              ('primitive','device','metric','context','style','medium','outcome')),
  term        TEXT NOT NULL,                 -- 稳定标识：'rhythm','negative-space'
  label       TEXT NOT NULL,                 -- 展示名：'节奏'
  definition  TEXT NOT NULL,                 -- 一句可判定的定义
  parent_id   INTEGER REFERENCES vocab_term(id),
  aliases     TEXT NOT NULL DEFAULT '',
  status      TEXT NOT NULL DEFAULT 'active'
              CHECK (status IN ('active','deprecated','proposed')),
  CHECK (kind <> 'primitive' OR pack_id = 'core'),
  CHECK (kind <> 'device'    OR pack_id <> 'core'),
  UNIQUE (pack_id, kind, term)
);

CREATE INDEX IF NOT EXISTS idx_vocab_kind ON vocab_term(kind, status);

-- 未入词表的自由标签只能进提案队列，不允许直接写进 work_tag
CREATE TABLE IF NOT EXISTS term_proposal (
  id            INTEGER PRIMARY KEY,
  pack_id       TEXT NOT NULL REFERENCES pack(id),
  kind          TEXT NOT NULL,
  raw_label     TEXT NOT NULL,
  sample_work_id INTEGER,
  rationale     TEXT,
  status        TEXT NOT NULL DEFAULT 'pending'
                CHECK (status IN ('pending','accepted','rejected')),
  proposed_at   TEXT NOT NULL
);

-- ============================================================ 1. 领域与距离

CREATE TABLE IF NOT EXISTS domain (
  id        TEXT PRIMARY KEY,                -- ui | architecture | music | photography
  title     TEXT NOT NULL,
  medium    TEXT NOT NULL CHECK (medium IN ('space','image','time','text','object')),
  pack_id   TEXT NOT NULL UNIQUE REFERENCES pack(id),  -- 包与领域 1:1，避免视图多路放大
  note      TEXT
);

CREATE TABLE IF NOT EXISTS domain_edge (
  a          TEXT NOT NULL REFERENCES domain(id),
  b          TEXT NOT NULL REFERENCES domain(id),
  distance   REAL NOT NULL CHECK (distance BETWEEN 0 AND 1),
  rationale  TEXT,
  PRIMARY KEY (a, b),
  CHECK (a < b)
);

-- ============================================================ 2. 来源与抓取

CREATE TABLE IF NOT EXISTS source (
  id           TEXT PRIMARY KEY,
  kind         TEXT NOT NULL CHECK (kind IN ('catalogue','museum','api','capture','repo','manual')),
  title        TEXT NOT NULL,
  homepage     TEXT,
  licence_note TEXT,
  policy_json  TEXT NOT NULL DEFAULT '{}',   -- 限速、允许端点、可达性、偏差说明
  created_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS fetch_run (
  id          INTEGER PRIMARY KEY,
  source_id   TEXT NOT NULL REFERENCES source(id),
  started_at  TEXT NOT NULL,
  finished_at TEXT,
  mode        TEXT NOT NULL CHECK (mode IN ('auto','browser_assisted','manual','import')),
  stats_json  TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS capture (
  id             INTEGER PRIMARY KEY,
  run_id         INTEGER NOT NULL REFERENCES fetch_run(id),
  source_id      TEXT NOT NULL REFERENCES source(id),
  url            TEXT NOT NULL,
  http_status    INTEGER,
  etag           TEXT,
  media_type     TEXT,
  content_sha256 TEXT,
  body_path      TEXT,
  fetched_at     TEXT NOT NULL,
  UNIQUE (source_id, url, content_sha256)
);

-- ============================================================ 3. 作品、片段、素材

CREATE TABLE IF NOT EXISTS work (
  id             INTEGER PRIMARY KEY,
  source_id      TEXT NOT NULL REFERENCES source(id),
  domain_id      TEXT NOT NULL REFERENCES domain(id),
  collection     TEXT NOT NULL,
  slug           TEXT NOT NULL,
  canonical_url  TEXT NOT NULL UNIQUE,
  title          TEXT NOT NULL,
  subtitle_quoted TEXT,
  summary_derived TEXT,
  creator_name   TEXT,
  creator_url    TEXT,
  origin_url     TEXT,
  created_year   INTEGER,
  licence        TEXT,
  rights_holder  TEXT,
  status         TEXT NOT NULL DEFAULT 'active'
                 CHECK (status IN ('active','gone','blocked','draft','retired')),
  payload_json   TEXT NOT NULL DEFAULT '{}',
  first_seen_at  TEXT NOT NULL,
  last_seen_at   TEXT NOT NULL,
  UNIQUE (source_id, collection, slug)  -- 同一源的不同集合可以复用 slug（Designeer 实测存在）
);

CREATE INDEX IF NOT EXISTS idx_work_domain ON work(domain_id, status);

CREATE TABLE IF NOT EXISTS asset (
  id         INTEGER PRIMARY KEY,
  work_id    INTEGER REFERENCES work(id) ON DELETE CASCADE,
  capture_id INTEGER REFERENCES capture(id),
  role       TEXT NOT NULL,                  -- poster|screenshot|scan|score|cover
  url        TEXT,
  local_path TEXT,
  sha256     TEXT,
  width      INTEGER,
  height     INTEGER,
  bytes      INTEGER,
  fetched_at TEXT,
  UNIQUE (work_id, role, sha256)
);

-- 片段：海报里的一个按钮、建筑里的一个檐口、乐谱里的一小节。
-- 基元经常只在局部成立，没有片段层就只能给整张图贴标签。
CREATE TABLE IF NOT EXISTS fragment (
  id         INTEGER PRIMARY KEY,
  work_id    INTEGER NOT NULL REFERENCES work(id) ON DELETE CASCADE,
  asset_id   INTEGER REFERENCES asset(id),
  bbox_json  TEXT,                           -- {"x":0.1,"y":0.2,"w":0.3,"h":0.1} 归一化
  region     TEXT,                           -- 'nav' | 'cornice' | 'bar-9-16' 等包内语境
  caption    TEXT
);

CREATE TABLE IF NOT EXISTS work_tag (
  work_id       INTEGER NOT NULL REFERENCES work(id) ON DELETE CASCADE,
  vocab_term_id INTEGER NOT NULL REFERENCES vocab_term(id),
  weight        REAL NOT NULL DEFAULT 1.0,
  origin        TEXT NOT NULL DEFAULT 'source'
                CHECK (origin IN ('source','extracted','manual')),
  PRIMARY KEY (work_id, vocab_term_id)
);

-- ============================================================ 4. 观察与度量

CREATE TABLE IF NOT EXISTS observation (
  id                INTEGER PRIMARY KEY,
  asset_id          INTEGER REFERENCES asset(id) ON DELETE CASCADE,
  fragment_id       INTEGER REFERENCES fragment(id) ON DELETE CASCADE,
  extractor         TEXT NOT NULL,           -- vision-llm | css-probe | audio-probe | manual
  extractor_version TEXT NOT NULL,
  payload_json      TEXT NOT NULL,
  confidence        REAL NOT NULL DEFAULT 0.5 CHECK (confidence BETWEEN 0 AND 1),
  supersedes_id     INTEGER REFERENCES observation(id),
  created_at        TEXT NOT NULL,
  CHECK (asset_id IS NOT NULL OR fragment_id IS NOT NULL)
);
-- SQLite 里 UNIQUE(..., fragment_id, ...) 对 NULL 不生效（NULL 互不相等），
-- 所以用两个部分唯一索引代替表级约束。
CREATE UNIQUE INDEX IF NOT EXISTS ux_observation_asset
  ON observation(asset_id, extractor, extractor_version) WHERE fragment_id IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS ux_observation_fragment
  ON observation(fragment_id, extractor, extractor_version) WHERE fragment_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS measure (
  id             INTEGER PRIMARY KEY,
  observation_id INTEGER NOT NULL REFERENCES observation(id) ON DELETE CASCADE,
  metric_term_id INTEGER NOT NULL REFERENCES vocab_term(id),
  value          REAL NOT NULL,
  unit           TEXT,
  confidence     REAL NOT NULL DEFAULT 0.5 CHECK (confidence BETWEEN 0 AND 1)
);

CREATE INDEX IF NOT EXISTS idx_measure_metric ON measure(metric_term_id, value);

-- ============================================================ 5. 基元、手法、迁移

-- 手法实现基元：'ui.spacing-scale' --节奏--> 'rhythm'
CREATE TABLE IF NOT EXISTS primitive_link (
  device_term_id    INTEGER NOT NULL REFERENCES vocab_term(id) ON DELETE CASCADE,
  primitive_term_id INTEGER NOT NULL REFERENCES vocab_term(id),
  mechanism         TEXT NOT NULL,           -- 机制一句话：为什么它产生了这个基元
  weight            REAL NOT NULL DEFAULT 1.0,
  PRIMARY KEY (device_term_id, primitive_term_id)
);

-- 某作品用了某手法
CREATE TABLE IF NOT EXISTS device_usage (
  id             INTEGER PRIMARY KEY,
  work_id        INTEGER NOT NULL REFERENCES work(id) ON DELETE CASCADE,
  device_term_id INTEGER NOT NULL REFERENCES vocab_term(id),
  fragment_id    INTEGER REFERENCES fragment(id),
  strength       REAL NOT NULL DEFAULT 0.5 CHECK (strength BETWEEN 0 AND 1),
  note           TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS ux_device_usage_asset
  ON device_usage(work_id, device_term_id) WHERE fragment_id IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS ux_device_usage_fragment
  ON device_usage(work_id, device_term_id, fragment_id) WHERE fragment_id IS NOT NULL;

-- 跨域迁移：建筑柱廊 → UI 导航栏
CREATE TABLE IF NOT EXISTS transfer (
  id                INTEGER PRIMARY KEY,
  from_device_id    INTEGER NOT NULL REFERENCES vocab_term(id),
  to_device_id      INTEGER NOT NULL REFERENCES vocab_term(id),
  mechanism         TEXT NOT NULL,           -- 共享机制
  feasibility       REAL NOT NULL DEFAULT 0.5 CHECK (feasibility BETWEEN 0 AND 1),
  risk              TEXT,                    -- 迁移后可能失效的地方
  status            TEXT NOT NULL DEFAULT 'draft'
                    CHECK (status IN ('draft','tested','adopted','rejected')),
  evidence_count    INTEGER NOT NULL DEFAULT 0,
  created_at        TEXT NOT NULL,
  UNIQUE (from_device_id, to_device_id)
);

-- ============================================================ 6. 原则（原 pattern，跨域）

CREATE TABLE IF NOT EXISTS principle (
  id           INTEGER PRIMARY KEY,
  key          TEXT NOT NULL UNIQUE,         -- 'rhythm.equal-intervals-need-one-break'
  statement    TEXT NOT NULL,                -- 可执行的一句话
  rationale    TEXT,
  context_json TEXT NOT NULL DEFAULT '{}',   -- 成立条件
  boundary     TEXT,                         -- 什么时候失效
  counterexample TEXT,
  status       TEXT NOT NULL DEFAULT 'candidate'
               CHECK (status IN ('candidate','published','deprecated')),
  confidence   REAL NOT NULL DEFAULT 0.4 CHECK (confidence BETWEEN 0 AND 1),
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS principle_scope (
  principle_id INTEGER NOT NULL REFERENCES principle(id) ON DELETE CASCADE,
  domain_id    TEXT NOT NULL REFERENCES domain(id),
  weight       REAL NOT NULL DEFAULT 1.0,
  PRIMARY KEY (principle_id, domain_id)
);

CREATE TABLE IF NOT EXISTS principle_evidence (
  principle_id   INTEGER NOT NULL REFERENCES principle(id) ON DELETE CASCADE,
  work_id        INTEGER NOT NULL REFERENCES work(id) ON DELETE CASCADE,
  role           TEXT NOT NULL DEFAULT 'support'
                 CHECK (role IN ('support','counter','variant')),
  device_term_id INTEGER REFERENCES vocab_term(id),
  note           TEXT,
  weight         REAL NOT NULL DEFAULT 1.0,
  PRIMARY KEY (principle_id, work_id, role)
);

-- ============================================================ 7. 判美

-- 判美只用成对比较 / 典范入选 / 落地结果，不用绝对分数。
CREATE TABLE IF NOT EXISTS judgment (
  id                INTEGER PRIMARY KEY,
  work_id           INTEGER NOT NULL REFERENCES work(id) ON DELETE CASCADE,
  kind              TEXT NOT NULL CHECK (kind IN ('pairwise','canon','outcome','expert_score')),
  opponent_work_id  INTEGER REFERENCES work(id),
  context_json      TEXT NOT NULL DEFAULT '{}',
  population        TEXT,                    -- 'expert:design' | 'general' | 'target-audience'
  outcome           TEXT NOT NULL,           -- a_wins|b_wins|tie|included|excluded|adopted|abandoned
  grade             TEXT NOT NULL DEFAULT 'C' CHECK (grade IN ('A','B','C')),
  note              TEXT,
  created_at        TEXT NOT NULL,
  CHECK (kind <> 'pairwise' OR opponent_work_id IS NOT NULL)
);

CREATE INDEX IF NOT EXISTS idx_judgment_work ON judgment(work_id, kind);

CREATE TABLE IF NOT EXISTS preference_model (
  context_key  TEXT PRIMARY KEY,             -- domain|audience|medium
  model_kind   TEXT NOT NULL,                -- bradley-terry | elo | monotone
  params_json  TEXT NOT NULL,
  fit_json     TEXT NOT NULL DEFAULT '{}',   -- 准确率、ECE、样本量
  fitted_at    TEXT NOT NULL
);

-- ============================================================ 8. 融合与交付

CREATE TABLE IF NOT EXISTS blend (
  id            INTEGER PRIMARY KEY,
  seed_work_id  INTEGER REFERENCES work(id),
  donor_work_id INTEGER REFERENCES work(id),
  generic_json  TEXT NOT NULL,               -- 共享的抽象结构
  emergent_json TEXT NOT NULL,               -- 融合后新出现、两个输入都没有的结构
  candidate_md  TEXT,
  score_json    TEXT NOT NULL DEFAULT '{}',
  status        TEXT NOT NULL DEFAULT 'idea'
                CHECK (status IN ('idea','tested','adopted','dropped')),
  created_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS spec (
  id           INTEGER PRIMARY KEY,
  domain_id    TEXT NOT NULL REFERENCES domain(id),
  name         TEXT NOT NULL,
  profile_json TEXT NOT NULL,
  body_md      TEXT NOT NULL,
  tokens_json  TEXT NOT NULL DEFAULT '{}',
  parent_id    INTEGER REFERENCES spec(id),
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS spec_principle (
  spec_id      INTEGER NOT NULL REFERENCES spec(id) ON DELETE CASCADE,
  principle_id INTEGER NOT NULL REFERENCES principle(id),
  slot         TEXT NOT NULL,
  note         TEXT,
  PRIMARY KEY (spec_id, principle_id, slot)
);

CREATE TABLE IF NOT EXISTS spec_device (
  spec_id        INTEGER NOT NULL REFERENCES spec(id) ON DELETE CASCADE,
  device_term_id INTEGER NOT NULL REFERENCES vocab_term(id),
  slot           TEXT NOT NULL,
  params_json    TEXT NOT NULL DEFAULT '{}',
  PRIMARY KEY (spec_id, device_term_id, slot)
);

-- ============================================================ 9. 质量与学习

CREATE TABLE IF NOT EXISTS validation_result (
  id          INTEGER PRIMARY KEY,
  scope_table TEXT NOT NULL,
  scope_id    INTEGER NOT NULL,
  rule        TEXT NOT NULL,
  severity    TEXT NOT NULL CHECK (severity IN ('error','warn','info')),
  message     TEXT NOT NULL,
  checked_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS review_queue (
  id          INTEGER PRIMARY KEY,
  kind        TEXT NOT NULL,                 -- observation|principle|transfer|term
  ref_id      INTEGER NOT NULL,
  reason      TEXT NOT NULL,
  status      TEXT NOT NULL DEFAULT 'open'
              CHECK (status IN ('open','resolved','rejected')),
  created_at  TEXT NOT NULL,
  resolved_at TEXT
);

CREATE TABLE IF NOT EXISTS feedback (
  id          INTEGER PRIMARY KEY,
  target_kind TEXT NOT NULL CHECK (target_kind IN ('work','principle','transfer','spec','blend')),
  target_id   INTEGER NOT NULL,
  signal      TEXT NOT NULL,                 -- accepted|rejected|shipped|edited|surprising
  weight      REAL NOT NULL DEFAULT 1.0,
  note        TEXT,
  created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS embedding (
  owner_kind TEXT NOT NULL CHECK (owner_kind IN ('work','principle','device','blend')),
  owner_id   INTEGER NOT NULL,
  model      TEXT NOT NULL,
  dim        INTEGER NOT NULL,
  vec        BLOB NOT NULL,
  norm       REAL NOT NULL,
  PRIMARY KEY (owner_kind, owner_id, model)
);

-- ============================================================ 10. 全文检索

CREATE TABLE IF NOT EXISTS search_doc (
  doc_id     INTEGER PRIMARY KEY,
  kind       TEXT NOT NULL CHECK (kind IN ('work','principle','device','spec')),
  ref_id     INTEGER NOT NULL,
  title      TEXT NOT NULL,
  body       TEXT NOT NULL DEFAULT '',
  terms      TEXT NOT NULL DEFAULT '',       -- 引擎分词后写入，中文必须走这一列
  updated_at TEXT NOT NULL,
  UNIQUE (kind, ref_id)
);

CREATE VIRTUAL TABLE IF NOT EXISTS search_fts USING fts5(
  title, body, terms,
  content='search_doc', content_rowid='doc_id',
  tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS search_doc_ai AFTER INSERT ON search_doc BEGIN
  INSERT INTO search_fts(rowid, title, body, terms) VALUES (new.doc_id, new.title, new.body, new.terms);
END;
CREATE TRIGGER IF NOT EXISTS search_doc_ad AFTER DELETE ON search_doc BEGIN
  INSERT INTO search_fts(search_fts, rowid, title, body, terms)
  VALUES ('delete', old.doc_id, old.title, old.body, old.terms);
END;
CREATE TRIGGER IF NOT EXISTS search_doc_au AFTER UPDATE ON search_doc BEGIN
  INSERT INTO search_fts(search_fts, rowid, title, body, terms)
  VALUES ('delete', old.doc_id, old.title, old.body, old.terms);
  INSERT INTO search_fts(rowid, title, body, terms)
  VALUES (new.doc_id, new.title, new.body, new.terms);
END;

-- ============================================================ 11. kind 校验（防标签汤）

CREATE TRIGGER IF NOT EXISTS trg_device_usage_kind BEFORE INSERT ON device_usage
WHEN (SELECT kind FROM vocab_term WHERE id = NEW.device_term_id) <> 'device'
BEGIN SELECT RAISE(ABORT, 'device_usage.device_term_id must reference kind=device'); END;

CREATE TRIGGER IF NOT EXISTS trg_primitive_link_kinds BEFORE INSERT ON primitive_link
WHEN (SELECT kind FROM vocab_term WHERE id = NEW.device_term_id) <> 'device'
  OR (SELECT kind FROM vocab_term WHERE id = NEW.primitive_term_id) <> 'primitive'
BEGIN SELECT RAISE(ABORT, 'primitive_link requires device + primitive terms'); END;

CREATE TRIGGER IF NOT EXISTS trg_work_tag_kind BEFORE INSERT ON work_tag
WHEN (SELECT status FROM vocab_term WHERE id = NEW.vocab_term_id) <> 'active'
BEGIN SELECT RAISE(ABORT, 'work_tag requires an active vocab term'); END;

-- ============================================================ 12. 视图

CREATE VIEW IF NOT EXISTS v_principle_card AS
SELECT p.id, p.key, p.statement, p.status, p.confidence, p.boundary,
       (SELECT count(*) FROM principle_evidence e WHERE e.principle_id = p.id AND e.role='support') AS supports,
       (SELECT count(*) FROM principle_evidence e WHERE e.principle_id = p.id AND e.role='counter') AS counters,
       (SELECT count(DISTINCT w.domain_id) FROM principle_evidence e
          JOIN work w ON w.id = e.work_id WHERE e.principle_id = p.id) AS domains
FROM principle p;

-- 每个领域的基元覆盖缺口：跨域涌现的第一输入
CREATE VIEW IF NOT EXISTS v_domain_gap AS
SELECT d.id AS domain_id, vt.term AS primitive,
       (SELECT count(*) FROM device_usage du
          JOIN work w      ON w.id = du.work_id
          JOIN primitive_link pl ON pl.device_term_id = du.device_term_id
         WHERE w.domain_id = d.id AND pl.primitive_term_id = vt.id) AS usages
FROM domain d
CROSS JOIN vocab_term vt
WHERE vt.kind = 'primitive' AND vt.status = 'active' AND d.pack_id <> 'core';

CREATE VIEW IF NOT EXISTS v_transfer_ready AS
SELECT t.id, a.label AS from_device, b.label AS to_device, t.mechanism,
       t.feasibility, t.evidence_count, dn_a.id AS from_domain, dn_b.id AS to_domain
FROM transfer t
JOIN vocab_term a ON a.id = t.from_device_id
JOIN vocab_term b ON b.id = t.to_device_id
JOIN pack pa ON pa.id = a.pack_id
JOIN pack pb ON pb.id = b.pack_id
JOIN domain dn_a ON dn_a.pack_id = pa.id
JOIN domain dn_b ON dn_b.pack_id = pb.id
WHERE t.status IN ('tested','adopted') AND t.feasibility >= 0.5;

CREATE VIEW IF NOT EXISTS v_stale_work AS
SELECT id, canonical_url, title, domain_id, last_seen_at
FROM work
WHERE status = 'active' AND julianday('now') - julianday(last_seen_at) > 90;
