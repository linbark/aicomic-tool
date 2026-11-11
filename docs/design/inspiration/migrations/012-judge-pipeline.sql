-- 012 · 评判流水线：预检 -> 机器盲评 -> 人工干预 -> 归并
-- 目标：评判不是一个分数，而是一条可重放、可插人工的流水线。

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS judge_run (
  id            INTEGER PRIMARY KEY,
  pipeline      TEXT NOT NULL DEFAULT 'default',
  status        TEXT NOT NULL DEFAULT 'created'
                CHECK (status IN ('created','precheck_failed','judging','awaiting_human','done')),
  stage         TEXT NOT NULL DEFAULT 'precheck',
  input_json    TEXT NOT NULL DEFAULT '{}',   -- pills, seed, order
  precheck_json TEXT,
  judge_json    TEXT,                          -- 机器盲评原始输出
  human_json    TEXT,
  final_json    TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS judge_item (
  id            INTEGER PRIMARY KEY,
  run_id        INTEGER NOT NULL REFERENCES judge_run(id) ON DELETE CASCADE,
  pill_id       INTEGER NOT NULL REFERENCES pill(id),
  label         TEXT NOT NULL,                 -- 盲评标签 A/B/C
  display_order INTEGER NOT NULL,
  precheck      REAL,
  machine_json  TEXT,
  machine_total REAL,
  human_score   REAL,
  human_note    TEXT,
  final_total   REAL,
  status        TEXT NOT NULL DEFAULT 'pending'
                CHECK (status IN ('pending','machine_done','human_needed','done','rejected')),
  UNIQUE (run_id, pill_id)
);

CREATE VIEW IF NOT EXISTS v_judge_queue AS
SELECT i.id AS item_id, r.id AS run_id, i.label, i.pill_id, i.machine_total, i.precheck,
       p.score_json, substr(p.output_md, 1, 200) AS excerpt
FROM judge_item i
JOIN judge_run r ON r.id = i.run_id
JOIN pill p ON p.id = i.pill_id
WHERE i.status = 'human_needed' AND r.status <> 'done';

INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '13');
