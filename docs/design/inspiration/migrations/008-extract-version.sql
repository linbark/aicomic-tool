-- 008 · 药液记录手法版本；修正内置手法里 confidence 的写法
-- 起因：t.layout 的模板里写了 "confidence":0.0，模型照抄 0.0，质量分被算成 0。
-- 同时补上 extract.technique_version：手法改版后，旧药液要能区分是哪一版炼的。

PRAGMA foreign_keys = ON;

ALTER TABLE extract ADD COLUMN technique_version TEXT;

UPDATE technique SET version='2',
  prompt_template='看图，只输出一个 JSON 对象：{"structure":"版式骨架（一句话，说明单列/多列/网格/散点）","anchors":"视觉锚点及位置（含占比）","hierarchy":"层级顺序（从大到小）","weight":"重心位置与上下比例","ratios":"关键比例（如标题宽占画面 60%，估计值标（估计））","confidence":0.85}。confidence 表示你对这次判断的把握，填 0–1 的小数，不要写 0.0。只输出 JSON，不要解释文字。'
WHERE key='t.layout';

UPDATE technique SET version='2',
  prompt_template='看图，只输出一个 JSON 对象：{"families":"字体气质判断（几何无衬线/过渡衬线/等宽…）","scale":"字号层级与倍数（估计值标（估计））","weights":"字重与字距","treatment":"特殊处理（描边/渐变/斜排/无文字）","confidence":0.85}。confidence 表示把握程度，填 0–1 的小数。只写图上能验证的，判断不了就给空字符串。'
WHERE key='t.type';

INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '9');
