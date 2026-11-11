-- 017 · 间距改为像素测量（原提示词版对每张图都吐 Web 默认值 8/40/88，等于没测）
PRAGMA foreign_keys = ON;
UPDATE technique SET version='2',
  prompt_template='',
  params_json='{"kind":"pixel"}',
  schema_json='{"base_px":"number","steps":["number"],"page_margin_px":"number","section_gap_px":"number","method":"whitespace-bands"}'
WHERE key='t.spacing';
INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '18');
