-- 022 · 评分来源与可信校准。未知旧分只标 legacy，不猜测审核人或时间，不删除分数。
ALTER TABLE judge_item ADD COLUMN human_source TEXT NOT NULL DEFAULT 'legacy'
  CHECK (human_source IN ('human','ai','demo','legacy'));
ALTER TABLE judge_item ADD COLUMN human_reviewer TEXT;
ALTER TABLE judge_item ADD COLUMN human_recorded_at TEXT;
ALTER TABLE judge_item ADD COLUMN human_dimension TEXT;

-- 追加式评分历史；legacy 的原始审核时间未知，必须保持 NULL。
CREATE TABLE judge_review (
  id INTEGER PRIMARY KEY,
  item_id INTEGER NOT NULL REFERENCES judge_item(id),
  score REAL NOT NULL,
  source TEXT NOT NULL CHECK (source IN ('human','ai','demo','legacy')),
  reviewer TEXT,
  recorded_at TEXT,
  dimension TEXT,
  note TEXT,
  CHECK (source <> 'human' OR (reviewer IS NOT NULL AND length(trim(reviewer)) > 0)),
  CHECK (source = 'legacy' OR (score >= 0 AND score <= 100))
);
INSERT INTO judge_review(item_id,score,source,note)
  SELECT id,human_score,'legacy',human_note FROM judge_item WHERE human_score IS NOT NULL;

CREATE TRIGGER judge_review_after_insert AFTER INSERT ON judge_item
WHEN NEW.human_score IS NOT NULL
BEGIN
  INSERT INTO judge_review(item_id,score,source,reviewer,recorded_at,dimension,note)
    VALUES(NEW.id,NEW.human_score,NEW.human_source,NEW.human_reviewer,
           NEW.human_recorded_at,NEW.human_dimension,NEW.human_note);
END;
CREATE TRIGGER judge_review_after_update
AFTER UPDATE OF human_score,human_source,human_reviewer,human_recorded_at,human_dimension,human_note
ON judge_item WHEN NEW.human_score IS NOT NULL
BEGIN
  INSERT INTO judge_review(item_id,score,source,reviewer,recorded_at,dimension,note)
    VALUES(NEW.id,NEW.human_score,NEW.human_source,NEW.human_reviewer,
           NEW.human_recorded_at,NEW.human_dimension,NEW.human_note);
END;

-- 旧权重仍可审计，但绝不可由 panel 当作可信校准自动加载。
ALTER TABLE judge_model ADD COLUMN calibration_source TEXT NOT NULL DEFAULT 'legacy'
  CHECK (calibration_source IN ('legacy','human'));
ALTER TABLE judge_model ADD COLUMN calibration_trusted INTEGER NOT NULL DEFAULT 0
  CHECK (calibration_trusted IN (0,1));
CREATE TABLE judge_model_history (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  samples INTEGER NOT NULL,
  agreements INTEGER NOT NULL,
  weight REAL NOT NULL,
  updated_at TEXT NOT NULL,
  calibration_source TEXT NOT NULL,
  calibration_trusted INTEGER NOT NULL,
  archived_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
INSERT INTO judge_model_history(name,provider,model,samples,agreements,weight,updated_at,
                                calibration_source,calibration_trusted)
  SELECT name,provider,model,samples,agreements,weight,updated_at,
         calibration_source,calibration_trusted FROM judge_model;
CREATE TRIGGER judge_model_archive_before_update BEFORE UPDATE ON judge_model
BEGIN
  INSERT INTO judge_model_history(name,provider,model,samples,agreements,weight,updated_at,
                                  calibration_source,calibration_trusted)
    VALUES(OLD.name,OLD.provider,OLD.model,OLD.samples,OLD.agreements,OLD.weight,OLD.updated_at,
           OLD.calibration_source,OLD.calibration_trusted);
END;

CREATE INDEX judge_item_calibration_source ON judge_item(run_id,human_source,human_reviewer);
INSERT OR REPLACE INTO schema_meta(key,value) VALUES('schema_version','23');
