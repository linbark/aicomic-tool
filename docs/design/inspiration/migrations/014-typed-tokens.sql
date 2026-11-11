-- 014 · 把「字阶 / 间距 / 禁忌」从形容词变成数值与语境
-- 起因：端到端样例里，规格只给「标题:正文=1.5:1」和「按 33% 骨架搭结构（COOLEST）」，
--       生成器无法执行；禁忌也不带页面语境，跨材料取药时完全错配。

PRAGMA foreign_keys = ON;

UPDATE technique SET version='3',
  prompt_template='看图，只输出一个 JSON 对象：{"families":"字体气质（几何无衬线/过渡衬线/等宽…）","base_px":16,"steps":[{"name":"display","size_px":72,"weight":600,"tracking":"-0.03em"},{"name":"body","size_px":18,"weight":400,"tracking":"0"}],"treatment":"特殊处理或空字符串","confidence":0.85}。size_px 按 1200px 宽画布估计，最多 5 级；判断不了就把 steps 留空。只输出 JSON。'
WHERE key='t.type';

UPDATE technique SET version='2',
  prompt_template='看图，只输出一个 JSON 对象：{"donts":[{"text":"具体到可检查的禁忌","applies_to":"landing|auth|dashboard|portfolio|other"}],"confidence":0.85}。写三条「复现这张图时最容易做错的地方」，每条必须标注它适用于哪类页面；不要泛泛而谈。'
WHERE key='t.donts';

INSERT OR IGNORE INTO technique(key,title,dimension,prompt_template,params_json,schema_json,version,status,created_at) VALUES
('t.spacing','定距','spacing',
 '看图，只输出一个 JSON 对象：{"base_px":8,"steps":[4,8,12,16,24,32,48,64],"page_margin_px":40,"section_gap_px":88,"confidence":0.85}。steps 是可观察到的间距档位（按 1200px 宽画布估计），只写看得出的；判断不了就留空数组。',
 '{}',
 '{"base_px":"number","steps":["number"],"page_margin_px":"number","section_gap_px":"number","confidence":"number"}',
 '1','active', datetime('now'));

INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '15');
