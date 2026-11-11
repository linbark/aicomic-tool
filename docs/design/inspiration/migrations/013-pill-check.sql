-- 013 · 丹药实现检查：把"规格是否被落实"落库
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS pill_check (
  id         INTEGER PRIMARY KEY,
  pill_id    INTEGER NOT NULL REFERENCES pill(id) ON DELETE CASCADE,
  artifact   TEXT NOT NULL,          -- 被检查的文件
  checks_json TEXT NOT NULL,
  passed     INTEGER NOT NULL CHECK (passed IN (0,1)),
  checked_at TEXT NOT NULL
);

CREATE VIEW IF NOT EXISTS v_pill_status AS
SELECT p.id AS pill_id, p.recipe_id, json_extract(p.score_json,'$.total') AS precheck,
       (SELECT passed FROM pill_check c WHERE c.pill_id=p.id ORDER BY c.id DESC LIMIT 1) AS last_passed,
       (SELECT count(*) FROM pill_check c WHERE c.pill_id=p.id) AS checks
FROM pill p;

INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '14');
