-- 025 · Independent versioned workshop (schema 26). Legacy alchemy is untouched.
CREATE TABLE atelier_revision (
  kind TEXT NOT NULL CHECK(kind IN ('technique','recipe','formula')),
  asset_key TEXT NOT NULL,
  version INTEGER NOT NULL CHECK(version > 0),
  body_json TEXT NOT NULL CHECK(json_valid(body_json) AND json_type(body_json)='object'),
  hash TEXT NOT NULL CHECK(length(hash)=64),
  created_at TEXT NOT NULL,
  PRIMARY KEY(kind,asset_key,version)
);
CREATE TRIGGER atelier_revision_no_update BEFORE UPDATE ON atelier_revision
BEGIN SELECT RAISE(ABORT,'atelier revision is immutable'); END;
CREATE TRIGGER atelier_revision_no_delete BEFORE DELETE ON atelier_revision
BEGIN SELECT RAISE(ABORT,'atelier revision is append-only'); END;

CREATE TABLE atelier_run (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('running','completed','failed','cancelled')),
  snapshot_json TEXT NOT NULL CHECK(json_valid(snapshot_json)),
  snapshot_hash TEXT NOT NULL CHECK(length(snapshot_hash)=64),
  output_json TEXT CHECK(output_json IS NULL OR json_valid(output_json)),
  output_hash TEXT,
  error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TRIGGER atelier_run_frozen BEFORE UPDATE ON atelier_run
WHEN NEW.snapshot_json IS NOT OLD.snapshot_json OR NEW.snapshot_hash IS NOT OLD.snapshot_hash
  OR NEW.id IS NOT OLD.id OR NEW.name IS NOT OLD.name OR NEW.created_at IS NOT OLD.created_at
BEGIN SELECT RAISE(ABORT,'atelier run snapshot is immutable'); END;
CREATE TRIGGER atelier_run_terminal BEFORE UPDATE ON atelier_run
WHEN OLD.status <> 'running'
BEGIN SELECT RAISE(ABORT,'atelier terminal run is immutable'); END;
CREATE TRIGGER atelier_run_no_delete BEFORE DELETE ON atelier_run
BEGIN SELECT RAISE(ABORT,'atelier run history is append-only'); END;

-- Each actual transition is appended. Never rewrite running into a fake success.
CREATE TABLE atelier_stage (
  id INTEGER PRIMARY KEY,
  run_id INTEGER NOT NULL REFERENCES atelier_run(id),
  node_id TEXT NOT NULL,
  op TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('running','completed','failed','cancelled','blocked')),
  input_json TEXT NOT NULL CHECK(json_valid(input_json)),
  output_json TEXT CHECK(output_json IS NULL OR json_valid(output_json)),
  hash TEXT NOT NULL CHECK(length(hash)=64),
  error TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX atelier_stage_run ON atelier_stage(run_id,id);
CREATE TRIGGER atelier_stage_no_update BEFORE UPDATE ON atelier_stage
BEGIN SELECT RAISE(ABORT,'atelier stage event is immutable'); END;
CREATE TRIGGER atelier_stage_no_delete BEFORE DELETE ON atelier_stage
BEGIN SELECT RAISE(ABORT,'atelier stage journal is append-only'); END;
INSERT OR REPLACE INTO schema_meta(key,value) VALUES('schema_version','26');
