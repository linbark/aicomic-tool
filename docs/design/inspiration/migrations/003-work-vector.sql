-- 003 · 作品的基元向量（φ 物化）
-- 依赖：schema.v2.sql + 002。用法：sqlite3 inspiration.db < migrations/003-work-vector.sql
--
-- 为什么单开一张表：φ 是检索、内核拟合、规格装配每次都要读的东西，
-- 从 measure 现算太慢，也不方便记录"这一维是量出来的、还是猜的"。

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS work_primitive (
  work_id           INTEGER NOT NULL REFERENCES work(id) ON DELETE CASCADE,
  primitive_term_id INTEGER NOT NULL REFERENCES vocab_term(id),
  score             REAL NOT NULL CHECK (score BETWEEN 0 AND 1),
  confidence        REAL NOT NULL CHECK (confidence BETWEEN 0 AND 1),
  source            TEXT NOT NULL CHECK (source IN ('pixel-proxy','vision','manual','mixed')),
  extractor_version TEXT,
  updated_at        TEXT NOT NULL,
  PRIMARY KEY (work_id, primitive_term_id)
);

CREATE INDEX IF NOT EXISTS idx_work_primitive_term ON work_primitive(primitive_term_id, score);

-- 每个领域里每个基元的覆盖率：M1 的进度看板
CREATE VIEW IF NOT EXISTS v_primitive_coverage AS
SELECT d.id AS domain_id, vt.term AS primitive,
       (SELECT count(*) FROM work_primitive wp JOIN work w ON w.id = wp.work_id
         WHERE w.domain_id = d.id AND wp.primitive_term_id = vt.id) AS works,
       (SELECT count(*) FROM work w WHERE w.domain_id = d.id) AS domain_works,
       (SELECT avg(wp.confidence) FROM work_primitive wp JOIN work w ON w.id = wp.work_id
         WHERE w.domain_id = d.id AND wp.primitive_term_id = vt.id) AS avg_confidence
FROM domain d CROSS JOIN vocab_term vt
WHERE vt.kind = 'primitive' AND vt.status = 'active';

INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '4');
