-- 007 · 内置手法（D0）
-- 三味最常用的手法：调色（像素实测）、定骨（版式）、立字（字号字重）。
-- 版本化：改提示词就新增 version，旧药液的 technique_version 保留。

PRAGMA foreign_keys = ON;

INSERT OR IGNORE INTO technique(key,title,dimension,prompt_template,params_json,schema_json,version,status,created_at) VALUES
('t.palette','调色','palette','',
 '{"kind":"pixel"}',
 '{"colors":"[{hex,share}]","stats":"{whitespace,contrast,edge_density}"}',
 '1','active', datetime('now')),

('t.layout','定骨','layout',
 '看图，只输出一个 JSON 对象：{"structure":"版式骨架（一句话，说明单列/多列/网格/散点）","anchors":"视觉锚点及位置（含占比）","hierarchy":"层级顺序（从大到小列出）","weight":"重心位置与上下比例","ratios":"关键比例（如标题宽占画面 60%，上下留白比 3:1，估计值标（估计））","confidence":0.0}。不要任何解释文字，不要 markdown。',
 '{}',
 '{"structure":"string","anchors":"string","hierarchy":"string","weight":"string","ratios":"string","confidence":"number"}',
 '1','active', datetime('now')),

('t.type','立字','type',
 '看图，只输出一个 JSON 对象：{"families":"字体气质判断（几何无衬线/过渡衬线/等宽…）","scale":"字号层级与倍数（估计值标（估计））","weights":"字重与字距","treatment":"特殊处理（描边/渐变/斜排/无文字）","confidence":0.0}。只写图上能验证的，判断不了就给空字符串。不要解释文字。',
 '{}',
 '{"families":"string","scale":"string","weights":"string","treatment":"string","confidence":"number"}',
 '1','active', datetime('now'));

INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '8');
