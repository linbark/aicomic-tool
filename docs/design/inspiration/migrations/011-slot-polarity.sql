-- 011 · 配方槽位加极性：药引（反例）必须能被渲染成"禁止"，而不是混在正例里
PRAGMA foreign_keys = ON;

ALTER TABLE recipe_component ADD COLUMN polarity TEXT NOT NULL DEFAULT 'positive'
  CHECK (polarity IN ('positive','negative'));

UPDATE recipe_component SET polarity='negative' WHERE slot='anti';

INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '12');
