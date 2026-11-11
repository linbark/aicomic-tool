-- 015 · 版式骨架改成枚举，禁止写作品名
PRAGMA foreign_keys = ON;
UPDATE technique SET version='4',
  prompt_template='看图，只输出一个 JSON 对象：{"structure":"单列|两栏|三栏|网格|散点|全屏图 六选一","anchors":["视觉锚点及位置"],"hierarchy":["层级顺序"],"weight":"上重下轻|下重上轻|居中|均衡","ratios":["关键比例，含数值并标（估计）"],"confidence":0.85}。structure 只能从六个枚举里选一个，禁止写作品名、品牌名或自造词。只输出 JSON。'
WHERE key='t.layout';
INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '16');
