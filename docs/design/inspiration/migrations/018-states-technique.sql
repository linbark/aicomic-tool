-- 018 · 组件状态维度（端到端缺口 3）
PRAGMA foreign_keys = ON;
INSERT OR IGNORE INTO technique(key,title,dimension,prompt_template,params_json,schema_json,version,status,created_at) VALUES
('t.states','定态','states',
 '看图，只输出一个 JSON 对象：{"states":[{"component":"button|input|nav|card","hover":"悬停时的变化","focus":"聚焦时的变化","disabled":"禁用态的样子"}],"confidence":0.85}。只写能从图上推断的交互反馈，判断不了就少写，不要编造具体像素值。',
 '{}',
 '{"states":[{"component":"string","hover":"string","focus":"string","disabled":"string"}],"confidence":"number"}',
 '1','active', datetime('now'));
INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '19');
