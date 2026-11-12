-- 023 · Controlled, immutable two-arm artifact experiments. Text scores stay separate.
-- Snapshots are intentionally self-contained: later edits to pill/recipe/extract do not
-- rewrite history. Public manifests never contain the private arm/pill mapping.
CREATE TABLE experiment (
  id INTEGER PRIMARY KEY,
  content_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  conditions_json TEXT NOT NULL,
  conditions_hash TEXT NOT NULL,
  pill_a_id INTEGER NOT NULL REFERENCES pill(id),
  pill_b_id INTEGER NOT NULL REFERENCES pill(id),
  pill_a_snapshot_json TEXT NOT NULL,
  pill_b_snapshot_json TEXT NOT NULL,
  pill_a_snapshot_hash TEXT NOT NULL,
  pill_b_snapshot_hash TEXT NOT NULL,
  recipe_a_hash TEXT NOT NULL,
  recipe_b_hash TEXT NOT NULL,
  blind_mapping_json TEXT NOT NULL,
  manifest_json TEXT,
  manifest_hash TEXT,
  public_exports_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(public_exports_json) AND json_type(public_exports_json) = 'array'),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  CHECK (pill_a_id <> pill_b_id),
  CHECK (pill_a_snapshot_hash <> pill_b_snapshot_hash),
  CHECK ((manifest_json IS NULL) = (manifest_hash IS NULL))
);
CREATE TRIGGER experiment_frozen BEFORE UPDATE ON experiment
WHEN NEW.id IS NOT OLD.id OR NEW.content_json IS NOT OLD.content_json
  OR NEW.content_hash IS NOT OLD.content_hash
  OR NEW.conditions_json IS NOT OLD.conditions_json OR NEW.conditions_hash IS NOT OLD.conditions_hash
  OR NEW.pill_a_id IS NOT OLD.pill_a_id OR NEW.pill_b_id IS NOT OLD.pill_b_id
  OR NEW.pill_a_snapshot_json IS NOT OLD.pill_a_snapshot_json
  OR NEW.pill_b_snapshot_json IS NOT OLD.pill_b_snapshot_json
  OR NEW.pill_a_snapshot_hash IS NOT OLD.pill_a_snapshot_hash
  OR NEW.pill_b_snapshot_hash IS NOT OLD.pill_b_snapshot_hash
  OR NEW.recipe_a_hash IS NOT OLD.recipe_a_hash OR NEW.recipe_b_hash IS NOT OLD.recipe_b_hash
  OR NEW.blind_mapping_json IS NOT OLD.blind_mapping_json OR NEW.created_at IS NOT OLD.created_at
  OR (OLD.manifest_hash IS NOT NULL AND
      (NEW.manifest_hash IS NOT OLD.manifest_hash OR NEW.manifest_json IS NOT OLD.manifest_json))
BEGIN SELECT RAISE(ABORT,'experiment snapshot/manifest is immutable'); END;
CREATE TRIGGER experiment_exports_append BEFORE UPDATE OF public_exports_json ON experiment
WHEN EXISTS (SELECT 1 FROM json_each(OLD.public_exports_json) old_export
  LEFT JOIN json_each(NEW.public_exports_json) new_export ON old_export.key = new_export.key
  WHERE old_export.value IS NOT new_export.value)
BEGIN SELECT RAISE(ABORT,'public export evidence paths are append-only'); END;
CREATE TRIGGER experiment_no_delete BEFORE DELETE ON experiment
BEGIN SELECT RAISE(ABORT,'experiment history is append-only'); END;

CREATE TABLE artifact (
  id INTEGER PRIMARY KEY,
  experiment_id INTEGER NOT NULL REFERENCES experiment(id),
  arm TEXT NOT NULL CHECK (arm IN ('a','b')),
  arm_version INTEGER NOT NULL DEFAULT 1 CHECK (arm_version = 1),
  original_path TEXT NOT NULL,
  archived_html_path TEXT NOT NULL,
  screenshot_path TEXT NOT NULL,
  page_sha256 TEXT NOT NULL,
  screenshot_sha256 TEXT NOT NULL,
  capture_receipt_json TEXT NOT NULL,
  capture_receipt_path TEXT NOT NULL,
  capture_receipt_sha256 TEXT NOT NULL,
  capture_conditions_hash TEXT NOT NULL,
  content_status TEXT NOT NULL CHECK (content_status IN ('compliant','noncompliant')),
  content_report_json TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  UNIQUE (experiment_id,arm)
);
CREATE TRIGGER artifact_no_update BEFORE UPDATE ON artifact
BEGIN SELECT RAISE(ABORT,'artifact is immutable; create a new experiment'); END;
CREATE TRIGGER artifact_no_delete BEFORE DELETE ON artifact
BEGIN SELECT RAISE(ABORT,'artifact history is append-only'); END;

CREATE TABLE artifact_evaluation (
  id INTEGER PRIMARY KEY,
  experiment_id INTEGER NOT NULL REFERENCES experiment(id),
  manifest_hash TEXT NOT NULL,
  source TEXT NOT NULL DEFAULT 'demo' CHECK (source IN ('human','demo','ai')),
  reviewer TEXT,
  dimension TEXT NOT NULL CHECK (dimension = 'visual'),
  winner TEXT NOT NULL CHECK (winner IN ('A','B','tie','abstain')),
  note TEXT,
  recorded_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  CHECK (source <> 'human' OR (reviewer IS NOT NULL AND length(trim(reviewer)) > 0))
);
CREATE INDEX artifact_evaluation_latest ON artifact_evaluation(experiment_id,source,reviewer,dimension,id);
CREATE TRIGGER artifact_evaluation_no_update BEFORE UPDATE ON artifact_evaluation
BEGIN SELECT RAISE(ABORT,'artifact evaluations are append-only'); END;
CREATE TRIGGER artifact_evaluation_no_delete BEFORE DELETE ON artifact_evaluation
BEGIN SELECT RAISE(ABORT,'artifact evaluations are append-only'); END;
INSERT OR REPLACE INTO schema_meta(key,value) VALUES('schema_version','24');
