-- 021 · 不可变药液版本（schema_version 22）。
-- db::migrate 必须在事务前关闭 foreign_keys，事务中执行本脚本并做
-- foreign_key_check，提交后恢复 foreign_keys。不可在 FK ON 下 DROP extract，
-- 否则 inventory 的 ON DELETE CASCADE 会误删用户背包。
-- 不重命名旧表：避免 SQLite 将 inventory 外键自动改写到旧表名。
DROP VIEW v_bag;
CREATE TABLE extract_v22 (
  id                   INTEGER PRIMARY KEY,
  work_id              INTEGER REFERENCES work(id) ON DELETE CASCADE,
  technique_id         INTEGER NOT NULL REFERENCES technique(id),
  dimension            TEXT NOT NULL,
  content_json         TEXT NOT NULL,
  content_md           TEXT,
  quality              REAL NOT NULL DEFAULT 0.5 CHECK (quality BETWEEN 0 AND 1),
  fire_model           TEXT NOT NULL,
  hash                 TEXT NOT NULL,
  created_at           TEXT NOT NULL,
  technique_version    TEXT,
  input_fingerprint    TEXT,
  input_json           TEXT,
  revision             INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0)
);
INSERT INTO extract_v22(id,work_id,technique_id,dimension,content_json,content_md,
                        quality,fire_model,hash,created_at,technique_version)
SELECT id,work_id,technique_id,dimension,content_json,content_md,
       quality,fire_model,hash,created_at,technique_version FROM extract;
DROP TABLE extract;
ALTER TABLE extract_v22 RENAME TO extract;
CREATE INDEX idx_extract_dim ON extract(dimension,quality);
CREATE INDEX idx_extract_cache ON extract(input_fingerprint,id DESC);
-- 可变指针与不可变内容分离；复用旧缓存也能重新成为当前 head。
-- 不为旧药液猜测输入身份或自动提升 technique_version。
CREATE TABLE extract_head (
  lineage_key TEXT PRIMARY KEY,
  extract_id INTEGER NOT NULL UNIQUE REFERENCES extract(id)
);
CREATE VIEW v_extract_head AS
SELECT e.* FROM extract_head h JOIN extract e ON e.id=h.extract_id
JOIN technique t ON t.id=e.technique_id
WHERE t.status='active' AND e.technique_version=t.version AND e.dimension=t.dimension
  AND json_valid(e.input_json)
  AND json_extract(e.input_json,'$.technique.prompt')=t.prompt_template
  AND json_extract(e.input_json,'$.technique.params_json')=t.params_json
  AND coalesce(json_extract(e.input_json,'$.technique.schema_json'),'')=coalesce(t.schema_json,'')
  AND (e.work_id IS NULL OR json_extract(e.input_json,'$.domain')=
       (SELECT domain_id FROM work WHERE id=e.work_id));
CREATE VIEW v_bag AS
SELECT i.owner_id, e.id AS extract_id, e.dimension,
       coalesce(v.label,e.dimension) AS dimension_label,
       e.quality, e.fire_model, w.title AS material, w.id AS work_id,
       t.key AS technique, e.technique_version
FROM inventory i JOIN extract e ON e.id=i.extract_id
LEFT JOIN vocab_term v ON v.term='dim.' || e.dimension
LEFT JOIN work w ON w.id=e.work_id
JOIN technique t ON t.id=e.technique_id;
-- 历史药液（包括迁移保留的旧行）只能被引用，禁止改写或级联删除。
-- inventory/head 指针可以调整，但无法将内容或手法版本冒充为另一版。
CREATE TRIGGER extract_immutable_update BEFORE UPDATE ON extract
BEGIN SELECT RAISE(ABORT,'immutable extract: create a new revision instead'); END;
CREATE TRIGGER extract_immutable_delete BEFORE DELETE ON extract
BEGIN SELECT RAISE(ABORT,'immutable extract: historical versions cannot be deleted'); END;
INSERT OR REPLACE INTO schema_meta(key,value) VALUES('schema_version','22');
