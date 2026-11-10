-- 005 · 基元分数按来源分开存
-- 背景：像素代理与视觉模型对同一基元的判断会冲突（实测 detail.density ρ=0.81，
--       但 balance.symmetry ρ=0.29、contrast.value ρ=0.38）。置信度平均会把两个
--       都不准的来源混成一个更不准的值，而且无法复核。
-- 做法：evidence 表按来源存原始分，work_primitive 只物化"置信度最高的来源"，
--       两路都 ≥0.5 且相差 >0.3 时进 review_queue。

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS work_primitive_evidence (
  work_id           INTEGER NOT NULL REFERENCES work(id) ON DELETE CASCADE,
  primitive_term_id INTEGER NOT NULL REFERENCES vocab_term(id),
  source            TEXT NOT NULL CHECK (source IN ('pixel-proxy','vision','manual')),
  score             REAL NOT NULL CHECK (score BETWEEN 0 AND 1),
  confidence        REAL NOT NULL CHECK (confidence BETWEEN 0 AND 1),
  extractor_version TEXT,
  updated_at        TEXT NOT NULL,
  PRIMARY KEY (work_id, primitive_term_id, source)
);

CREATE INDEX IF NOT EXISTS idx_evidence_term ON work_primitive_evidence(primitive_term_id, source);

-- 回填：现有 pixel-proxy 行是可信的；vision 行也是；mixed 行是旧的平均逻辑，不猜来源，跳过。
INSERT OR IGNORE INTO work_primitive_evidence(work_id, primitive_term_id, source, score, confidence, extractor_version, updated_at)
SELECT work_id, primitive_term_id, source, score, confidence, extractor_version, updated_at
FROM work_primitive WHERE source IN ('pixel-proxy','vision');

INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '6');
