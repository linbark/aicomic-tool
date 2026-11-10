-- 004 · 修正 observation / device_usage 的唯一性
-- 背景：表级 UNIQUE(asset_id, fragment_id, extractor, extractor_version) 在 SQLite 里
--       遇到 fragment_id IS NULL 时**不生效**——NULL 互不相等，约束形同虚设。
--       实测：同一 asset 同一抽取器可重复插入（见设计稿 §3.5）。
-- 做法：保留 id 最小的那行，其余删除，再用部分唯一索引兜住。

PRAGMA foreign_keys = ON;

DELETE FROM observation
WHERE id NOT IN (SELECT min(id) FROM observation GROUP BY asset_id, coalesce(fragment_id, -1), extractor, extractor_version);

DELETE FROM device_usage
WHERE id NOT IN (SELECT min(id) FROM device_usage GROUP BY work_id, device_term_id, coalesce(fragment_id, -1));

CREATE UNIQUE INDEX IF NOT EXISTS ux_observation_asset
  ON observation(asset_id, extractor, extractor_version) WHERE fragment_id IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS ux_observation_fragment
  ON observation(fragment_id, extractor, extractor_version) WHERE fragment_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS ux_device_usage_asset
  ON device_usage(work_id, device_term_id) WHERE fragment_id IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS ux_device_usage_fragment
  ON device_usage(work_id, device_term_id, fragment_id) WHERE fragment_id IS NOT NULL;

INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '5');
