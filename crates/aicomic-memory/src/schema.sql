CREATE TABLE IF NOT EXISTS memory_records (
 id TEXT PRIMARY KEY, project_id INTEGER NOT NULL, namespace TEXT NOT NULL, type TEXT NOT NULL,
 entity TEXT, content TEXT NOT NULL, payload_json TEXT, source_ref TEXT, time_index TEXT,
 status TEXT, confidence REAL, source_kind TEXT, evidence_ids_json TEXT, story_order TEXT,
 story_time_json TEXT, hash TEXT, created_at_ms INTEGER, updated_at_ms INTEGER);
CREATE TABLE IF NOT EXISTS episodic_memories (
 id TEXT PRIMARY KEY, project_id INTEGER NOT NULL, event TEXT NOT NULL, state_changes_json TEXT NOT NULL,
 entities_json TEXT NOT NULL, episode_id INTEGER, scene_id INTEGER, beat_index INTEGER, created_at_ms INTEGER,
 FOREIGN KEY(id) REFERENCES memory_records(id));
CREATE TABLE IF NOT EXISTS canonical_evidences (
 evidence_id TEXT PRIMARY KEY, project_id INTEGER NOT NULL, episode_id INTEGER, scene_id INTEGER,
 span_json TEXT, quote TEXT NOT NULL, speaker TEXT, tags_json TEXT, created_at_ms INTEGER);
CREATE TABLE IF NOT EXISTS canonical_entities (
 entity_id TEXT PRIMARY KEY, project_id INTEGER NOT NULL, entity_type TEXT NOT NULL, canonical_name TEXT NOT NULL,
 aliases_json TEXT, status TEXT, confidence REAL, source_kind TEXT, created_from_evidence_id TEXT,
 created_at_ms INTEGER, updated_at_ms INTEGER);
CREATE TABLE IF NOT EXISTS canonical_events (
 event_id TEXT PRIMARY KEY, project_id INTEGER NOT NULL, story_order TEXT NOT NULL, story_time_key TEXT,
 story_time_json TEXT, episode_id INTEGER, scene_id INTEGER, event_type TEXT, summary TEXT NOT NULL,
 participants_json TEXT, evidence_ids_json TEXT, status TEXT, confidence REAL, source_kind TEXT,
 created_at_ms INTEGER, updated_at_ms INTEGER);
CREATE TABLE IF NOT EXISTS canonical_time_constraints (
 constraint_id TEXT PRIMARY KEY, project_id INTEGER NOT NULL, relation TEXT NOT NULL, from_event_id TEXT,
 to_event_id TEXT, anchor_id TEXT, interval_json TEXT, evidence_ids_json TEXT, status TEXT, confidence REAL,
 source_kind TEXT, created_at_ms INTEGER, updated_at_ms INTEGER);
CREATE TABLE IF NOT EXISTS canonical_time_blocks (
 block_id TEXT PRIMARY KEY, project_id INTEGER NOT NULL, name TEXT, parent_block_id TEXT, anchor_id TEXT,
 constraints_json TEXT, event_ids_json TEXT, status TEXT, confidence REAL, source_kind TEXT,
 created_at_ms INTEGER, updated_at_ms INTEGER);
CREATE TABLE IF NOT EXISTS canonical_snapshots (
 snapshot_id TEXT PRIMARY KEY, project_id INTEGER NOT NULL, entity_id TEXT NOT NULL,
 valid_from_story_time_key TEXT, valid_to_story_time_key TEXT, valid_from_story_order TEXT, valid_to_story_order TEXT,
 fields_json TEXT NOT NULL, why TEXT, evidence_ids_json TEXT, status TEXT, confidence REAL, source_kind TEXT,
 created_at_ms INTEGER, updated_at_ms INTEGER);
CREATE TABLE IF NOT EXISTS canonical_state_changes (
 state_change_id TEXT PRIMARY KEY, project_id INTEGER NOT NULL, event_id TEXT NOT NULL, target_entity_id TEXT,
 patch_json TEXT NOT NULL, before_snapshot_id TEXT, after_snapshot_id TEXT, evidence_ids_json TEXT, status TEXT,
 confidence REAL, source_kind TEXT, created_at_ms INTEGER, updated_at_ms INTEGER);
CREATE TABLE IF NOT EXISTS canonical_changesets (
 changeset_id TEXT PRIMARY KEY, project_id INTEGER NOT NULL, episode_id INTEGER, payload_json TEXT NOT NULL,
 review_status TEXT NOT NULL, review_log_json TEXT, created_at_ms INTEGER, updated_at_ms INTEGER);
CREATE TABLE IF NOT EXISTS canonical_conflicts (
 conflict_id TEXT PRIMARY KEY, project_id INTEGER NOT NULL, changeset_id TEXT, conflict_type TEXT NOT NULL,
 entity_id TEXT, old_claim_json TEXT, new_claim_json TEXT, suggested_actions_json TEXT, status TEXT NOT NULL,
 resolved_by TEXT, resolution_note TEXT, created_at_ms INTEGER, updated_at_ms INTEGER);
CREATE TABLE IF NOT EXISTS rust_memory_vectors (
 record_id TEXT NOT NULL, provider_key TEXT NOT NULL, dimension INTEGER NOT NULL,
 content_hash TEXT NOT NULL, vector BLOB NOT NULL, updated_at_ms INTEGER,
 PRIMARY KEY(record_id,provider_key));
CREATE TABLE IF NOT EXISTS rust_memory_run_changesets (
 project_id INTEGER NOT NULL, run_id TEXT NOT NULL, changeset_id TEXT NOT NULL,
 raw_payload_hash TEXT NOT NULL, created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
 PRIMARY KEY(project_id,run_id), UNIQUE(changeset_id));
CREATE TABLE IF NOT EXISTS rust_memory_vector_projects (
 project_id INTEGER NOT NULL, provider_key TEXT NOT NULL, dimension INTEGER NOT NULL,
 updated_at_ms INTEGER NOT NULL, PRIMARY KEY(project_id,provider_key));
CREATE INDEX IF NOT EXISTS idx_project_namespace ON memory_records(project_id,namespace);
CREATE INDEX IF NOT EXISTS idx_project_entity ON memory_records(project_id,entity);
CREATE INDEX IF NOT EXISTS idx_time_index ON memory_records(time_index);
CREATE INDEX IF NOT EXISTS idx_episodic_project ON episodic_memories(project_id);
CREATE INDEX IF NOT EXISTS idx_canonical_evidences_project ON canonical_evidences(project_id);
CREATE INDEX IF NOT EXISTS idx_canonical_entities_project ON canonical_entities(project_id);
CREATE INDEX IF NOT EXISTS idx_canonical_entities_name ON canonical_entities(project_id,canonical_name);
CREATE INDEX IF NOT EXISTS idx_canonical_events_project ON canonical_events(project_id);
CREATE INDEX IF NOT EXISTS idx_canonical_events_story_order ON canonical_events(project_id,story_order);
CREATE INDEX IF NOT EXISTS idx_canonical_events_story_time ON canonical_events(project_id,story_time_key);
CREATE INDEX IF NOT EXISTS idx_canonical_time_constraints_project ON canonical_time_constraints(project_id);
CREATE INDEX IF NOT EXISTS idx_canonical_time_blocks_project ON canonical_time_blocks(project_id);
CREATE INDEX IF NOT EXISTS idx_canonical_snapshots_project ON canonical_snapshots(project_id);
CREATE INDEX IF NOT EXISTS idx_canonical_snapshots_entity ON canonical_snapshots(project_id,entity_id);
CREATE INDEX IF NOT EXISTS idx_canonical_state_changes_project ON canonical_state_changes(project_id);
CREATE INDEX IF NOT EXISTS idx_canonical_state_changes_event ON canonical_state_changes(project_id,event_id);
CREATE INDEX IF NOT EXISTS idx_canonical_changesets_project ON canonical_changesets(project_id);
CREATE INDEX IF NOT EXISTS idx_canonical_changesets_status ON canonical_changesets(project_id,review_status);
CREATE INDEX IF NOT EXISTS idx_canonical_conflicts_project ON canonical_conflicts(project_id);
CREATE INDEX IF NOT EXISTS idx_canonical_conflicts_status ON canonical_conflicts(project_id,status);
