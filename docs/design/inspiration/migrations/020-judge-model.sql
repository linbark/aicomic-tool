-- 020 · 裁判校准：用人工裁决反推每个裁判的可信度
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS judge_model (
  name       TEXT PRIMARY KEY,
  provider   TEXT NOT NULL DEFAULT '',
  model      TEXT NOT NULL DEFAULT '',
  samples    INTEGER NOT NULL DEFAULT 0,
  agreements INTEGER NOT NULL DEFAULT 0,
  weight     REAL NOT NULL DEFAULT 1.0,
  updated_at TEXT NOT NULL
);

INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '21');
