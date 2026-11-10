-- 002 · 个人层（ToC：每个人都能参与评定，并形成自己的美学内核）
-- 依赖：schema.v2.sql。用法：sqlite3 inspiration.db < migrations/002-personal-layer.sql
--
-- 设计取舍：
--   1. 个人判定默认只在本地（visibility='private'），上传聚合需显式同意；
--   2. 内核是「跨语境仍然成立」的个人成分，风格是「可借用、可丢弃」的表达层；
--   3. 内核每次重拟合生成新版本，旧版本保留（可回溯「我的口味怎么变的」）。

PRAGMA foreign_keys = ON;

-- ---------------------------------------------------------------- 人

CREATE TABLE IF NOT EXISTS person (
  id            TEXT PRIMARY KEY,              -- 设备本地 UUID，不含邮箱/手机号
  display_name  TEXT,
  locale        TEXT,
  privacy_level TEXT NOT NULL DEFAULT 'local'
                CHECK (privacy_level IN ('local','federated')),
  consent_json  TEXT NOT NULL DEFAULT '{}',    -- 同意项与时间戳，逐项可撤回
  reliability   REAL NOT NULL DEFAULT 0.5 CHECK (reliability BETWEEN 0 AND 1),
  created_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS judgment_session (
  id         INTEGER PRIMARY KEY,
  person_id  TEXT NOT NULL REFERENCES person(id),
  kind       TEXT NOT NULL CHECK (kind IN ('onboarding','daily','deep','consistency','style-probe')),
  started_at TEXT NOT NULL,
  ended_at   TEXT,
  stats_json TEXT NOT NULL DEFAULT '{}'
);

ALTER TABLE judgment ADD COLUMN actor_id TEXT REFERENCES person(id);
ALTER TABLE judgment ADD COLUMN session_id INTEGER REFERENCES judgment_session(id);
ALTER TABLE judgment ADD COLUMN visibility TEXT NOT NULL DEFAULT 'private'
  CHECK (visibility IN ('private','federated','public'));
ALTER TABLE judgment ADD COLUMN latency_ms INTEGER;

CREATE INDEX IF NOT EXISTS idx_judgment_actor ON judgment(actor_id, created_at);

-- ---------------------------------------------------------------- 内核

CREATE TABLE IF NOT EXISTS kernel (
  id            INTEGER PRIMARY KEY,
  person_id     TEXT NOT NULL REFERENCES person(id),
  version       INTEGER NOT NULL,
  model_kind    TEXT NOT NULL DEFAULT 'hierarchical-bt',
  weights_json  TEXT NOT NULL,                 -- 38 维个人权重（相对人群先验的偏移）
  prior_json    TEXT NOT NULL,                 -- 人群先验 + 每维不确定度
  fit_json      TEXT NOT NULL,                 -- n / 留出准确率 / rho / kernelness / ECE
  maturity      TEXT NOT NULL DEFAULT 'L0'
                CHECK (maturity IN ('L0','L1','L2','L3')),
  status        TEXT NOT NULL DEFAULT 'active'
                CHECK (status IN ('draft','active','superseded')),
  created_at    TEXT NOT NULL,
  UNIQUE (person_id, version)
);

-- 内核卡：人类可读的主张，五类
CREATE TABLE IF NOT EXISTS kernel_claim (
  id                INTEGER PRIMARY KEY,
  kernel_id         INTEGER NOT NULL REFERENCES kernel(id) ON DELETE CASCADE,
  kind              TEXT NOT NULL CHECK (kind IN ('prefer','avoid','tradeoff','floor','signature')),
  statement         TEXT NOT NULL,
  primitive_term_id INTEGER REFERENCES vocab_term(id),
  effect            REAL,                      -- 相对人群的效应量（σ）
  confidence        REAL CHECK (confidence BETWEEN 0 AND 1),
  evidence_count    INTEGER NOT NULL DEFAULT 0,
  counterexample    TEXT
);

CREATE TABLE IF NOT EXISTS pair_task (
  id               INTEGER PRIMARY KEY,
  person_id        TEXT NOT NULL REFERENCES person(id),
  a_work_id        INTEGER NOT NULL REFERENCES work(id),
  b_work_id        INTEGER NOT NULL REFERENCES work(id),
  purpose          TEXT NOT NULL CHECK (purpose IN ('kernel','style','consistency','coverage')),
  information_gain REAL,
  status           TEXT NOT NULL DEFAULT 'queued'
                   CHECK (status IN ('queued','asked','answered','expired')),
  created_at       TEXT NOT NULL,
  CHECK (a_work_id <> b_work_id)
);

-- ---------------------------------------------------------------- 风格层

CREATE TABLE IF NOT EXISTS style (
  id            INTEGER PRIMARY KEY,
  pack_id       TEXT REFERENCES pack(id),      -- NULL = 跨域风格（如「侘寂」）
  key           TEXT NOT NULL,
  title         TEXT NOT NULL,
  definition    TEXT NOT NULL,
  era           TEXT,
  consensus_json TEXT NOT NULL DEFAULT '{}',   -- 采用者的聚合倾向（只存聚合，不存个人）
  status        TEXT NOT NULL DEFAULT 'active'
                CHECK (status IN ('draft','active','deprecated')),
  UNIQUE (pack_id, key)
);

CREATE TABLE IF NOT EXISTS style_work (
  style_id INTEGER NOT NULL REFERENCES style(id) ON DELETE CASCADE,
  work_id  INTEGER NOT NULL REFERENCES work(id) ON DELETE CASCADE,
  role     TEXT NOT NULL DEFAULT 'exemplar'
           CHECK (role IN ('exemplar','variant','counter')),
  weight   REAL NOT NULL DEFAULT 1.0,
  PRIMARY KEY (style_id, work_id, role)
);

CREATE TABLE IF NOT EXISTS style_adoption (
  person_id  TEXT NOT NULL REFERENCES person(id) ON DELETE CASCADE,
  style_id   INTEGER NOT NULL REFERENCES style(id) ON DELETE CASCADE,
  weight     REAL NOT NULL DEFAULT 0.5,
  status     TEXT NOT NULL DEFAULT 'trial'
             CHECK (status IN ('trial','active','dropped')),
  note       TEXT,
  created_at TEXT NOT NULL,
  PRIMARY KEY (person_id, style_id)
);

-- ---------------------------------------------------------------- 视图

CREATE VIEW IF NOT EXISTS v_kernel_card AS
SELECT k.person_id, k.version, k.maturity, k.fit_json,
       c.kind, c.statement, c.effect, c.confidence, c.evidence_count, c.counterexample
FROM kernel k
LEFT JOIN kernel_claim c ON c.kernel_id = k.id
WHERE k.status = 'active';

CREATE VIEW IF NOT EXISTS v_person_activity AS
SELECT p.id AS person_id,
       (SELECT count(*) FROM judgment j WHERE j.actor_id = p.id) AS judgments,
       (SELECT count(*) FROM judgment_session s WHERE s.person_id = p.id) AS sessions,
       (SELECT max(created_at) FROM judgment j WHERE j.actor_id = p.id) AS last_at
FROM person p;

-- 与人群的分歧：个人判定与同一对作品上其他人的多数判定相反
CREATE VIEW IF NOT EXISTS v_divergence AS
SELECT j.actor_id AS person_id, j.work_id, j.opponent_work_id, j.outcome,
       (SELECT j2.outcome FROM judgment j2
         WHERE j2.actor_id IS NOT NULL AND j2.actor_id <> j.actor_id
           AND j2.visibility IN ('federated','public')
           AND j2.work_id = j.work_id AND j2.opponent_work_id = j.opponent_work_id
         GROUP BY j2.outcome ORDER BY count(*) DESC LIMIT 1) AS majority_outcome
FROM judgment j
WHERE j.actor_id IS NOT NULL AND j.kind = 'pairwise';

CREATE INDEX IF NOT EXISTS idx_kernel_active ON kernel(person_id, status);
CREATE INDEX IF NOT EXISTS idx_style_adoption ON style_adoption(person_id, status);

INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '3');
