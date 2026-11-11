-- 006 · 炼丹模型：药液（extract）、配方（recipe）、丹方（formula）、丹药（pill）
--
-- 与既有表的关系：
--   药材 = work / asset / fragment（已有）
--   火炉 = 引擎本身；火候 = technique.model + params
--   观察 → 度量 → 基元向量（已有）是"化验结果"，
--   药液则是"可入药的成品中间物"：带维度、带质量、可寻址、可配伍。

PRAGMA foreign_keys = ON;

-- 手法：一段可复用的提示词模板 + 输出契约
CREATE TABLE IF NOT EXISTS technique (
  id              INTEGER PRIMARY KEY,
  key             TEXT NOT NULL UNIQUE,              -- 't.palette' | 't.layout' | 't.mood'
  title           TEXT NOT NULL,
  dimension       TEXT NOT NULL,                     -- 该手法炼出的药液维度
  prompt_template TEXT NOT NULL,
  params_json     TEXT NOT NULL DEFAULT '{}',        -- 温度、模型偏好等
  schema_json     TEXT,                              -- 期望输出结构
  version         TEXT NOT NULL DEFAULT '1',
  status          TEXT NOT NULL DEFAULT 'active'
                  CHECK (status IN ('draft','active','deprecated')),
  created_at      TEXT NOT NULL
);

-- 药液：某味药材 + 某个手法 + 某口火候 的产物
CREATE TABLE IF NOT EXISTS extract (
  id           INTEGER PRIMARY KEY,
  work_id      INTEGER REFERENCES work(id) ON DELETE CASCADE,
  technique_id INTEGER NOT NULL REFERENCES technique(id),
  dimension    TEXT NOT NULL,
  content_json TEXT NOT NULL,
  content_md   TEXT,
  quality      REAL NOT NULL DEFAULT 0.5 CHECK (quality BETWEEN 0 AND 1),
  fire_model   TEXT NOT NULL,
  hash         TEXT NOT NULL,                        -- sha256(work|technique|version|model|content)
  created_at   TEXT NOT NULL,
  UNIQUE (work_id, technique_id, dimension, fire_model)
);

CREATE INDEX IF NOT EXISTS idx_extract_dim ON extract(dimension, quality);

-- 背包：把药液收进自己的库
CREATE TABLE IF NOT EXISTS inventory (
  extract_id INTEGER PRIMARY KEY REFERENCES extract(id) ON DELETE CASCADE,
  owner_id   TEXT NOT NULL DEFAULT 'me',
  qty        INTEGER NOT NULL DEFAULT 1,
  note       TEXT,
  added_at   TEXT NOT NULL
);

-- 配方：哪些维度、什么配比、怎么合成
CREATE TABLE IF NOT EXISTS recipe (
  id                 INTEGER PRIMARY KEY,
  key                TEXT NOT NULL UNIQUE,
  name               TEXT NOT NULL,
  goal               TEXT NOT NULL,
  combination_prompt TEXT NOT NULL,                  -- 模板，渲染时填入各 slot 的药液与配比
  constraints_json   TEXT NOT NULL DEFAULT '{}',     -- 相冲规则、禁用项
  version            TEXT NOT NULL DEFAULT '1',
  created_at         TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS recipe_component (
  recipe_id  INTEGER NOT NULL REFERENCES recipe(id) ON DELETE CASCADE,
  slot       TEXT NOT NULL,                          -- base | accent | anti
  dimension  TEXT NOT NULL,
  ratio      REAL NOT NULL CHECK (ratio BETWEEN 0 AND 1),
  required   INTEGER NOT NULL DEFAULT 1,
  fallback   TEXT,
  PRIMARY KEY (recipe_id, slot, dimension)
);

-- 丹方：一次完整炼丹的 DAG（可重放）
CREATE TABLE IF NOT EXISTS formula (
  id         INTEGER PRIMARY KEY,
  key        TEXT NOT NULL UNIQUE,
  name       TEXT NOT NULL,
  steps_json TEXT NOT NULL,                          -- [{id, op, inputs[], technique|recipe}]
  created_at TEXT NOT NULL
);

-- 丹药：最终产物与其成本、评分
CREATE TABLE IF NOT EXISTS pill (
  id          INTEGER PRIMARY KEY,
  formula_id  INTEGER REFERENCES formula(id),
  recipe_id   INTEGER REFERENCES recipe(id),
  output_md   TEXT NOT NULL,
  output_json TEXT NOT NULL DEFAULT '{}',
  score_json  TEXT NOT NULL DEFAULT '{}',
  seed        INTEGER,
  cost_json   TEXT NOT NULL DEFAULT '{}',            -- {model, tokens, ms}
  created_at  TEXT NOT NULL
);

CREATE VIEW IF NOT EXISTS v_bag AS
SELECT i.owner_id, e.id AS extract_id, e.dimension,
       coalesce(v.label, e.dimension) AS dimension_label,
       e.quality, e.fire_model, w.title AS material, w.id AS work_id,
       t.key AS technique, t.version AS technique_version
FROM inventory i
JOIN extract e ON e.id = i.extract_id
LEFT JOIN vocab_term v ON v.term = 'dim.' || e.dimension
LEFT JOIN work w ON w.id = e.work_id
JOIN technique t ON t.id = e.technique_id;

INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '7');
