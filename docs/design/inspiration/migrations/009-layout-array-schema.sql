-- 009 · 把 t.layout 的输出改成纯数组结构
-- 起因：小模型会把 ratios 写成 {"a","b"} 这种非法 JSON（对象里裸字符串）。
-- 数组对模型更友好，语义不变。

PRAGMA foreign_keys = ON;

UPDATE technique SET version='3',
  prompt_template='看图，只输出一个 JSON 对象，字段类型必须严格照下面这个形状：\
{"structure":"字符串","anchors":["字符串","字符串"],"hierarchy":["字符串"],"weight":"字符串","ratios":["字符串"],"confidence":0.85}\
。anchors、hierarchy、ratios 都是**字符串数组**，不要写成对象；ratios 每条写清数值并标（估计）；confidence 是 0–1 的小数。只输出 JSON，不要解释。'
WHERE key='t.layout';

INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '10');
