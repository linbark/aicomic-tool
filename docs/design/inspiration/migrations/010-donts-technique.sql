-- 010 · 内置手法「设禁」（donts）——配方里的药引
PRAGMA foreign_keys = ON;

INSERT OR IGNORE INTO technique(key,title,dimension,prompt_template,params_json,schema_json,version,status,created_at) VALUES
('t.donts','设禁','donts',
 '看图，只输出一个 JSON 对象：{"donts":["字符串","字符串","字符串"],"confidence":0.85}。donts 写三条「复现这张图时最容易做错的地方」，每条具体到可检查（例如：把留白填满、加投影、用两种强调色、把标题缩小）。不要泛泛而谈，不要解释文字。',
 '{}',
 '{"donts":["string"],"confidence":"number"}',
 '1','active', datetime('now'));

INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '11');
