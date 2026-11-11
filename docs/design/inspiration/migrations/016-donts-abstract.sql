-- 016 · 禁忌必须抽象：不得出现源图里的品牌名/产品名
PRAGMA foreign_keys = ON;
UPDATE technique SET version='3',
  prompt_template='看图，只输出一个 JSON 对象：{"donts":[{"text":"抽象的禁忌（不含任何品牌/产品/人名）","applies_to":"landing|auth|dashboard|portfolio|other"}],"confidence":0.85}。写三条「复现这类页面时最容易做错的地方」：必须把源图的具体内容抽象成通用规则，禁止出现源图里的品牌名、产品名或具体文案。'
WHERE key='t.donts';
INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('schema_version', '17');
