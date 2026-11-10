-- UI 灵感引擎 · 本地数据库 schema v1（SQLite）
-- 设计说明见 ../../UI灵感引擎设计.md；本节可直接 `sqlite3 inspiration.db < schema.v1.sql`。
-- 约定：
--   1. 一切可追溯：任何结论（pattern/spec）都能回溯到 capture → asset → work → source。
--   2. 原文与衍生分离：work.subtitle_quoted 是数据源原文，必须带署名；引擎自己的摘要放 work.summary_derived。
--   3. 幂等：抓取与抽取以内容哈希去重，重跑不产生重复证据。

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS schema_meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
INSERT OR REPLACE INTO schema_meta(key, value) VALUES
  ('schema_version', '1'),
  ('created_by', 'inspiration-engine');

-- ---------------------------------------------------------------- 数据源与抓取

CREATE TABLE IF NOT EXISTS source (
  id            TEXT PRIMARY KEY,           -- 'designeer' | 'capture-inbox' | 'project-repo'
  kind          TEXT NOT NULL CHECK (kind IN ('catalogue','capture','repo','manual')),
  title         TEXT NOT NULL,
  homepage      TEXT,
  licence_note  TEXT,                       -- 署名/再分发摘要，人工核对后写入
  policy_json   TEXT NOT NULL DEFAULT '{}', -- robots、限速、允许端点、禁抓路径
  created_at    TEXT NOT NULL
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
  body_path      TEXT,                      -- 内容寻址 blob：blobs/<sha 前两位>/<sha>
  fetched_at     TEXT NOT NULL,
  UNIQUE (source_id, url, content_sha256)
);

-- ---------------------------------------------------------------- 目录条目

CREATE TABLE IF NOT EXISTS work (
  id             INTEGER PRIMARY KEY,
  source_id      TEXT NOT NULL REFERENCES source(id),
  collection     TEXT NOT NULL,             -- inspiration|resources|tools|portfolios|finds
  slug           TEXT NOT NULL,
  canonical_url  TEXT NOT NULL UNIQUE,      -- 目录页规范地址（不含 .md）
  title          TEXT NOT NULL,
  subtitle_quoted TEXT,                     -- 源站原文，引用必须署名
  summary_derived TEXT,                     -- 引擎生成的一句话，不得冒充源站观点
  author_name    TEXT,
  author_url     TEXT,
  origin_url     TEXT,                      -- 被收录对象自身网址
  platform       TEXT,                      -- web|ios|android|macos|figma|meta
  kind           TEXT,                      -- gallery|component-library|tool|portfolio|find
  licence        TEXT,
  rights_holder  TEXT,
  status         TEXT NOT NULL DEFAULT 'active'
                 CHECK (status IN ('active','gone','blocked','draft','retired')),
  payload_json   TEXT NOT NULL DEFAULT '{}',-- 未归一化的源字段，保留原样
  first_seen_at  TEXT NOT NULL,
  last_seen_at   TEXT NOT NULL,
  UNIQUE (source_id, collection, slug)
);

CREATE INDEX IF NOT EXISTS idx_work_collection ON work(collection, status);
CREATE INDEX IF NOT EXISTS idx_work_origin ON work(origin_url);

CREATE TABLE IF NOT EXISTS work_tag (
  work_id INTEGER NOT NULL REFERENCES work(id) ON DELETE CASCADE,
  facet   TEXT NOT NULL,                    -- platform|page|style|density|motion|color|era|topic
  value   TEXT NOT NULL,
  weight  REAL NOT NULL DEFAULT 1.0,
  origin  TEXT NOT NULL DEFAULT 'source'
          CHECK (origin IN ('source','extracted','manual')),
  PRIMARY KEY (work_id, facet, value)
);

CREATE TABLE IF NOT EXISTS asset (
  id          INTEGER PRIMARY KEY,
  work_id     INTEGER REFERENCES work(id) ON DELETE CASCADE,
  capture_id  INTEGER REFERENCES capture(id),
  role        TEXT NOT NULL,                -- poster|screenshot|logo|cover
  url         TEXT,
  local_path  TEXT,
  sha256      TEXT,
  width       INTEGER,
  height      INTEGER,
  bytes       INTEGER,
  fetched_at  TEXT,
  UNIQUE (work_id, role, sha256)
);

-- ---------------------------------------------------------------- 观察与结论

CREATE TABLE IF NOT EXISTS observation (
  id              INTEGER PRIMARY KEY,
  asset_id        INTEGER NOT NULL REFERENCES asset(id) ON DELETE CASCADE,
  extractor       TEXT NOT NULL,            -- vision-llm|css-probe|manual
  extractor_version TEXT NOT NULL,          -- 模型或规则版本，便于重跑比较
  schema_version  INTEGER NOT NULL DEFAULT 1,
  payload_json    TEXT NOT NULL,            -- 结构化观察：布局/字阶/色板/间距/圆角/动效
  confidence      REAL NOT NULL DEFAULT 0.5 CHECK (confidence BETWEEN 0 AND 1),
  created_at      TEXT NOT NULL,
  UNIQUE (asset_id, extractor, extractor_version, schema_version)
);

CREATE TABLE IF NOT EXISTS pattern (
  id             INTEGER PRIMARY KEY,
  key            TEXT NOT NULL UNIQUE,      -- 'auth.single-column-inline-labels'
  name           TEXT NOT NULL,
  category       TEXT NOT NULL,             -- layout|typography|color|motion|component|content
  statement      TEXT NOT NULL,             -- 可执行的设计主张（一句话）
  rationale      TEXT,
  constraints_json TEXT NOT NULL DEFAULT '{}',
  antipattern    TEXT,                      -- 什么时候不该用
  status         TEXT NOT NULL DEFAULT 'candidate'
                 CHECK (status IN ('candidate','published','deprecated')),
  confidence     REAL NOT NULL DEFAULT 0.4,
  evidence_count INTEGER NOT NULL DEFAULT 0,
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_pattern_cat ON pattern(category, status);

CREATE TABLE IF NOT EXISTS pattern_evidence (
  pattern_id     INTEGER NOT NULL REFERENCES pattern(id) ON DELETE CASCADE,
  work_id        INTEGER NOT NULL REFERENCES work(id) ON DELETE CASCADE,
  observation_id INTEGER REFERENCES observation(id),
  role           TEXT NOT NULL DEFAULT 'support'
                 CHECK (role IN ('support','counter','variant')),
  note           TEXT,
  weight         REAL NOT NULL DEFAULT 1.0,
  PRIMARY KEY (pattern_id, work_id, role)
);

CREATE TABLE IF NOT EXISTS token (
  id         INTEGER PRIMARY KEY,
  pattern_id INTEGER REFERENCES pattern(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL,                 -- color|space|radius|type|duration|easing|shadow
  name       TEXT NOT NULL,
  value      TEXT NOT NULL,
  unit       TEXT,
  note       TEXT
);

CREATE TABLE IF NOT EXISTS context_facet (
  pattern_id INTEGER NOT NULL REFERENCES pattern(id) ON DELETE CASCADE,
  facet      TEXT NOT NULL,
  value      TEXT NOT NULL,
  weight     REAL NOT NULL DEFAULT 1.0,
  PRIMARY KEY (pattern_id, facet, value)
);

-- ---------------------------------------------------------------- 交付物

CREATE TABLE IF NOT EXISTS spec (
  id           INTEGER PRIMARY KEY,
  name         TEXT NOT NULL,
  profile_json TEXT NOT NULL,               -- 产品简报：受众/平台/密度/品牌
  body_md      TEXT NOT NULL,               -- DESIGN.md 正文
  tokens_json  TEXT NOT NULL DEFAULT '{}',
  parent_id    INTEGER REFERENCES spec(id),
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS spec_pattern (
  spec_id    INTEGER NOT NULL REFERENCES spec(id) ON DELETE CASCADE,
  pattern_id INTEGER NOT NULL REFERENCES pattern(id),
  slot       TEXT NOT NULL,                 -- hero|nav|form|empty-state...
  PRIMARY KEY (spec_id, pattern_id, slot)
);

CREATE TABLE IF NOT EXISTS feedback (
  id          INTEGER PRIMARY KEY,
  target_kind TEXT NOT NULL CHECK (target_kind IN ('work','pattern','spec')),
  target_id   INTEGER NOT NULL,
  signal      TEXT NOT NULL,                -- accepted|rejected|shipped|edited
  weight      REAL NOT NULL DEFAULT 1.0,
  note        TEXT,
  created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS embedding (
  owner_kind TEXT NOT NULL CHECK (owner_kind IN ('work','pattern')),
  owner_id   INTEGER NOT NULL,
  model      TEXT NOT NULL,
  dim        INTEGER NOT NULL,
  vec        BLOB NOT NULL,                 -- float32 little-endian，查询时暴力余弦
  norm       REAL NOT NULL,
  PRIMARY KEY (owner_kind, owner_id, model)
);

-- ---------------------------------------------------------------- 全文检索
-- 单一索引覆盖 work / pattern / spec：Agent 的「全库检索」只需要一次 MATCH。
-- unicode61 不切分中文，故 terms 列由引擎分词后写入（jieba-rs，空格分隔），
-- 查询词同样先分词再拼 MATCH；body 保留原文用于英文与短语匹配。

CREATE TABLE IF NOT EXISTS search_doc (
  doc_id     INTEGER PRIMARY KEY,
  kind       TEXT NOT NULL CHECK (kind IN ('work','pattern','spec')),
  ref_id     INTEGER NOT NULL,
  title      TEXT NOT NULL,
  body       TEXT NOT NULL DEFAULT '',
  terms      TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL,
  UNIQUE (kind, ref_id)
);

CREATE VIRTUAL TABLE IF NOT EXISTS search_fts USING fts5(
  title, body, terms,
  content='search_doc', content_rowid='doc_id',
  tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS search_doc_ai AFTER INSERT ON search_doc BEGIN
  INSERT INTO search_fts(rowid, title, body, terms)
  VALUES (new.doc_id, new.title, new.body, new.terms);
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

-- ---------------------------------------------------------------- 常用视图

CREATE VIEW IF NOT EXISTS v_pattern_card AS
SELECT p.id, p.key, p.name, p.category, p.statement, p.status, p.confidence,
       p.evidence_count,
       (SELECT count(*) FROM pattern_evidence e WHERE e.pattern_id = p.id AND e.role = 'support')   AS supports,
       (SELECT count(*) FROM pattern_evidence e WHERE e.pattern_id = p.id AND e.role = 'counter')   AS counters
FROM pattern p;

CREATE VIEW IF NOT EXISTS v_stale_work AS
SELECT id, canonical_url, title, collection, last_seen_at
FROM work
WHERE status = 'active' AND julianday('now') - julianday(last_seen_at) > 30;
