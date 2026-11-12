-- 019 · 动效维度：三帧抽样，看变化而不是看静态
PRAGMA foreign_keys = ON;
INSERT OR IGNORE INTO technique(key,title,dimension,prompt_template,params_json,schema_json,version,status,created_at) VALUES
('t.motion','观动','motion',
 '你会看到同一界面的三帧（按时间顺序）。只输出一个 JSON 对象：{"motion":[{"element":"发生变化的元素","change":"从什么变成什么","duration_ms":0,"easing":"缓动或none","confidence":0.0}],"confidence":0.0}。只写三帧之间能确认的变化；看不出来就留空数组，不要编造毫秒数。',
 '{"frames":3}',
 '{"motion":[{"element":"string","change":"string","duration_ms":"number","easing":"string","confidence":"number"}],"confidence":"number"}',
 '1','active', datetime('now'));
INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '20');
