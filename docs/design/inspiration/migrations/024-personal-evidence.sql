-- 024 · Explicit local personal preferences from verified final-pixel evidence (schema 25).
-- Old identities, judgments and active kernels are retained verbatim and remain legacy.
ALTER TABLE judgment ADD COLUMN source TEXT NOT NULL DEFAULT 'legacy'
  CHECK (source IN ('human','ai','demo','legacy'));
ALTER TABLE judgment ADD COLUMN source_evaluation_id INTEGER REFERENCES artifact_evaluation(id);
ALTER TABLE judgment ADD COLUMN intent TEXT NOT NULL DEFAULT 'unspecified';
ALTER TABLE judgment ADD COLUMN pair_key TEXT;
ALTER TABLE judgment ADD COLUMN scope TEXT;
CREATE UNIQUE INDEX judgment_evaluation_person ON judgment(source_evaluation_id,actor_id)
  WHERE source_evaluation_id IS NOT NULL;
CREATE INDEX judgment_personal_latest ON judgment(actor_id,scope,pair_key,id);

ALTER TABLE kernel ADD COLUMN training_source TEXT NOT NULL DEFAULT 'legacy'
  CHECK (training_source IN ('human','demo','legacy'));
ALTER TABLE kernel ADD COLUMN scope TEXT;
ALTER TABLE kernel ADD COLUMN feature_contract TEXT;
ALTER TABLE work ADD COLUMN visibility TEXT NOT NULL DEFAULT 'public'
  CHECK (visibility IN ('public','private'));

-- Per-artifact generated work; neither the original material work nor its live phi
-- can substitute for the final screenshot. Hashes bind bytes, not owner identity.
CREATE TABLE artifact_work (
  artifact_id INTEGER PRIMARY KEY REFERENCES artifact(id),
  work_id INTEGER NOT NULL UNIQUE REFERENCES work(id),
  feature_contract TEXT NOT NULL,
  features_json TEXT NOT NULL CHECK (json_valid(features_json) AND json_type(features_json) = 'object'),
  features_sha256 TEXT NOT NULL,
  page_sha256 TEXT NOT NULL,
  shot_sha256 TEXT NOT NULL,
  scope TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TRIGGER artifact_work_no_update BEFORE UPDATE ON artifact_work
BEGIN SELECT RAISE(ABORT,'artifact feature snapshot is immutable'); END;
CREATE TRIGGER artifact_work_no_delete BEFORE DELETE ON artifact_work
BEGIN SELECT RAISE(ABORT,'artifact feature history is append-only'); END;
-- Human bridge provenance is append-only, while legacy/import behavior is unchanged.
CREATE TRIGGER personal_judgment_no_update BEFORE UPDATE ON judgment
WHEN OLD.source_evaluation_id IS NOT NULL
BEGIN SELECT RAISE(ABORT,'personal evidence judgment is immutable'); END;
CREATE TRIGGER personal_judgment_no_delete BEFORE DELETE ON judgment
WHEN OLD.source_evaluation_id IS NOT NULL
BEGIN SELECT RAISE(ABORT,'personal evidence history is append-only'); END;
INSERT OR REPLACE INTO schema_meta(key,value) VALUES('schema_version','25');
