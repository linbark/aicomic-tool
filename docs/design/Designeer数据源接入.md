# Designeer 数据源接入规范

> 适配器 v0.1 ｜ 实测日期 2026-11-10 ｜ 所属引擎：[灵感引擎设计](灵感引擎设计.md) v1.0 ｜ 上游：<https://designeer.xyz> ｜ 上游运营者：Dhruv Jaradi
>
> 本文的端点状态、数量与字段全部来自当天的真实请求（`curl` + 抓取工具各跑一遍，结论一致），§6 对账表是当天按本规范实跑一次的结果（元数据全量 + 海报抽样）。数量随时间变化，接入实现必须以运行时对账为准，不能硬编码。

## 0. 结论

1. Designeer 是**手工策展目录站**：4 个集合（Inspiration / Resources / Tools / Portfolios）+ 1 个视觉流（Finds），条目是被收录对象的一句话介绍与海报，**不是界面截图语料库**。
2. 可自动获取的只有**目录骨架 + 海报**：`sitemap.xml` 854 条 URL、`llms.txt` 50 条带描述的条目、CDN 上 827 张 1200×675 海报。详情页、`.md` 版本、`/api/catalog` 全部被 Cloudflare 人机校验拦在 403。
3. 它足够撑起引擎的**目录层**（谁值得看、属于哪类、看什么图），不足以撑起视觉层与决策层——那两层必须靠海报抽取与用户样本补齐。
4. 站点在 `ai.txt` 里明确允许 AI 摘要、引用、索引与训练，但**要求署名、要求链接规范地址、禁止伪造引言与评价、禁止把策展文案说成自己的**。适配器把这几条写成硬约束，而不是提示词里的建议。

## 1. 端点实测矩阵

| 端点 | 状态 | 可得内容 | 处置 |
| --- | --- | --- | --- |
| `/robots.txt` | 200 | 抓取策略、允许的 API 路径 | 读取并遵守 |
| `/ai.txt` | 200 | 许可、署名、禁止项 | 写入 `source.licence_note` 与 `policy_json` |
| `/llms.txt` | 200 | 站点自述、集合计数、50 条条目（标题 + 集合 + 一句描述） | 目录层富化主来源 |
| `/sitemap.xml` | 200 | 854 个 URL + 827 个 `image:loc` + `lastmod` | 发现面主来源 |
| `/llms-full.txt` | 500 | 无（服务端错误） | 每周重试，恢复即是最大收益 |
| `/`（首页 HTML） | 403 | Cloudflare 人机校验 | 不绕过 |
| `/inspiration.md`、`/index.md`、`/licence.md` | 403 | 同上 | 不绕过 |
| `/inspiration/mobbin.md` | 403 | 同上 | 走 B 级（浏览器辅助） |
| `/api/catalog?collection=inspiration&limit=3` | 403 | 同上 | 暂不可用 |
| `/api/md/_catalog` | 403 | 同上 | 暂不可用 |
| `cdn.designeer.xyz/items/<uuid>/poster/1200.webp` | 200 | `image/webp`，1200×675，实测 16–52 KB | 下载并内容寻址 |
| `/feed.xml`、`/rss.xml`、`/sitemap-index.xml` | 404 | 无 | 不存在 |

补充观察：404 走的是应用自己的 404 页（约 35 KB HTML），**没有**人机校验；只有已注册路由被拦。说明拦截发生在路由层，而非全站 WAF。这不改变结论——我们仍然不尝试绕过，只使用站点声明的机器接口与人工辅助。

## 2. 站点结构（2026-11-10 实测）

| 集合 | sitemap URL 数 | 扣除集合索引页后的条目 | 带海报 | `llms.txt` 声明 |
| --- | --- | --- | --- | --- |
| inspiration | 139 | 138 | 136 | 137 |
| resources | 207 | 206 | 204 | 205 |
| tools | 162 | 161 | 161 | 158 |
| portfolios | 174 | 173 | 163 | 170 |
| finds | 163 | 163 | 163 | 157 |
| 静态页（info/submit/sponsor/privacy/terms/licence/refunds/cookies/home） | 8 | — | — | — |

- 目录条目合计 **678**，finds **163**，合计 **841** 个可建 `work`。
- `llms.txt` 自述"670 published listings + 157 Finds"，与 sitemap 相差 +8 / +6。两个数都不是错，只是口径与更新时间不同：**sitemap 是发现面（discovered），llms.txt 是声明面（declared）**。入库以发现面为准，声明面写进 `fetch_run.stats_json` 做对账。
- `portfolios` 缺海报最多（173 条里 10 条无图），这类条目只能作纯链接参考。
- `lastmod` 分布：finds 有 17 条是 2026-11 更新，其余集合多为 2026-10 建档 → finds 日更、目录周更的刷新节奏是合理的。

### 2.1 Finds 的 slug 自带出处

finds 的 slug 形如 `<x_handle>-<status_id>`：

```text
0xurvish-2111837715549610398   →  https://x.com/0xurvish/status/2111837715549610398
adityasur11-2111313755112137098 →  https://x.com/adityasur11/status/2111313755112137098
```

可以无损还原原始 X 帖与作者主页，满足 `ai.txt` 的"credit the creator + link the original"。正则：`^(?P<handle>[a-z0-9_]+)-(?P<status>\d{15,20})$`。

### 2.2 slug 会跨集合重复

实测 `dev-chauhan`、`diar-muradi` 在两个集合各出现一次。唯一键必须是 `(source_id, collection, slug)`；`canonical_url` 另行唯一。

## 3. 许可与署名（来自 `ai.txt`）

允许：摘要公开页面、短引用并署名、作为来源引用、索引、用于训练。

禁止（原文要点）：

- 伪造引言，或把不存在的话归给 Designeer 及其条目；
- 编造 Designeer 的评分、评价或背书；
- **把 Designeer 的描述或策展成果说成模型自己的产出**；
- 用私有用户数据（收藏、文件夹、投稿、账号）训练。

署名要求：写出 Designeer 名称 + 链接规范 HTML 地址（去掉 `.md`）；按页面写法给创作者署名并链接原始出处；作品权利归创作者。

落到实现：

| 约束 | 实现 |
| --- | --- |
| 署名 | 每条输出带 `citation.source='designeer'` + 规范 URL；`spec` 附录列出来源 |
| 不冒充 | `work.subtitle_quoted` 原样存储 + `subtitle_credit`；引擎自己的话只写 `summary_derived` |
| 不编造评价 | 不存在 `rating` 字段；排序分不对外展示为"Designeer 推荐" |
| 版权 | 海报只做本地缓存；不打包分发；对外输出给链接与缩略引用 |

> `licence` 页本身被 403 拦住，本文的许可结论只引用 `ai.txt` 原文，不臆测未读到的条款。实现落地前如需更强依据，由人打开 `https://designeer.xyz/licence` 核对一次。

## 4. 字段映射

### 4.1 sitemap → `work` / `asset`

| 上游 | 本地 | 规则 |
| --- | --- | --- |
| `<loc>` | `work.canonical_url` | 去尾部 `/`，去 `.md` |
| path 第 1 段 | `work.collection` | inspiration/resources/tools/portfolios/finds |
| path 第 2 段 | `work.slug` | 集合索引页（slug == collection）跳过 |
| `image:loc` | `asset.url` | `role='poster'`，下载后写 `sha256/width/height/bytes` 到 `blobs/` |
| `lastmod` | `payload_json.sitemap_lastmod` | **不**写 `work.updated_at`（那是内容更新时间，抓取时间不等于内容时间）；用于刷新调度 |
| 抓取时刻 | `work.first_seen_at` / `last_seen_at` | 首次/最近一次见到 |
| — | `work.status` | 有标题描述 → `active`；只有 slug → `draft`；403 → `blocked`；404 → `gone` |

### 4.2 llms.txt 条目 → `work`

行格式：`- [Title (Collection)](https://designeer.xyz/<collection>/<slug>.md) - Description`

| 上游 | 本地 |
| --- | --- |
| `Title` | `work.title`，`payload_json.title_origin='llms'` |
| `(Collection)` | 校验 `work.collection` 一致，不一致记 warning |
| `.md` 链接 | 去后缀后匹配 `work.canonical_url` |
| `Description` | `work.subtitle_quoted`（原文，引用） |

### 4.3 finds → `work` 的出处字段

| 上游 | 本地 |
| --- | --- |
| slug `handle-status` | `author_name=handle`、`author_url=https://x.com/<handle>` |
| status id | `origin_url=https://x.com/<handle>/status/<id>`、`payload_json.x_status_id` |
| — | `kind='find'`、`platform='meta'`（社交媒体作品，不是可交互产品） |

## 5. 归一化与富化规则

1. **标题来源三分**：`llms`（可信）/ `slug`（占位，如 `ui-camera` → `UI Camera`，必须标记）/ `manual`。`title_origin='slug'` 的条目只能出现在 `ie search` 的 evidence 结果里，**不得进入 `spec`**——规格引用一条连标题都没核实的条目，等于给幻觉盖章。
2. **描述率是硬指标**：当前只有 50/670（7.5%）有官方描述。M0 不追求覆盖率，转而用海报 vision 抽取补 `summary_derived`，并在报告里分开统计"官方描述率"与"衍生摘要率"。
3. **海报去重**：同一 `sha256` 出现在多个条目（转载图）时只存一份 blob，多条 `asset` 行共享。
4. **集合即弱标签**：没有逐条 tag，先把 collection 写进 `work_tag(facet='topic', origin='source')`，等 vision 抽取补 `page/style/density/motion/color`。
5. **不猜**：任何上游没有的字段留空，不推断作者、不推断平台、不推断许可。

## 6. 导入流程

```text
ie sync designeer --mode auto
  1. GET robots.txt / ai.txt / llms.txt / sitemap.xml        # 1 req/s，带条件请求
  2. 解析 sitemap → 节点表；剔除 8 个静态页
  3. 对每个 listing：upsert work(status='draft', title_origin='slug')
  4. 解析 llms.txt 的 50 条 → 覆盖 title/subtitle，status='active'
  5. 海报批量下载：条件 GET → sha256 → blobs/，写 asset
  6. 生成对账报告：声明 vs 发现、缺图、slug 冲突、描述率
  7. 提交事务并写 fetch_run.stats_json
```

幂等性来自三个唯一键：

```sql
UNIQUE (source_id, url, content_sha256)      -- capture
UNIQUE (source_id, collection, slug)         -- work
UNIQUE (work_id, role, sha256)               -- asset
```

同一份 sitemap 重跑两次，`work` 与 `asset` 行数不变；只有 `fetch_run` 与 `last_seen_at` 前进。

对账报告（2026-11-10 已按本文规则实跑一次，产出在 `data/inspiration/`（gitignore），口径为元数据全量 + 12 张海报抽样）：

| 指标 | 实测 |
| --- | --- |
| work 总数 | 841（678 listings + 163 finds） |
| 有官方标题/描述 | 50（`status='active'`，其余 791 条为 `draft`） |
| 有海报 URL 的条目 | 827（98.3%） |
| slug 跨集合冲突 | 2（`dev-chauhan`、`diar-muradi`） |
| 海报抽样下载 | 12/12 成功，1200×675，合计 359 KB，内容寻址去重 |
| 上游声明 vs 发现 | 670/157 对 678/163，差 +8 / +6，记 warning 不报错 |
| FTS 抽查 | `MATCH 'mobbin'`、`MATCH 'flows'` 均命中，中文需分词后才可检索（见 [灵感引擎设计](灵感引擎设计.md) §5 的检索设计） |

这次实跑只下载了 12 张海报做管线验证，全量 827 张按实测单张 16–52 KB 估算约 25–40 MB，属于 M0 的收尾动作。

## 7. 刷新与降级

| 对象 | 频率 | 方式 |
| --- | --- | --- |
| sitemap | 每日 | 条件 GET，变化才重解析 |
| finds | 每日 | `lastmod` 变更触发 |
| listings | 每周 | 同上 |
| 海报 | 一次 | 有 sha 就跳过，除非条目 `lastmod` 变了 |
| `llms-full.txt` | 每周一次探测 | 目前 500；恢复后单请求可拿全站正文，优先级最高 |

降级路径：

- **A 失效 → B 浏览器辅助**：用户用浏览器打开条目页，`Cmd+S` 存 HTML（或复制 Markdown）丢进 `inbox/`，`ie sync designeer --from-inbox` 解析标题、描述、tag、原站链接。这是当前唯一能拿到详情层的合规路径。
- **B 失效 → C 手工**：`ie add --url ... --title ... --tag platform=web`。
- **永不做**：用无头浏览器或指纹伪装绕过人机校验。站点已开放 `llms.txt`/`sitemap`/`api/catalog` 三类机器接口，绕过它的技术控制既无必要也不体面。

## 8. 归一化样例

```json
[
  {
    "kind": "work",
    "source_id": "designeer",
    "collection": "inspiration",
    "slug": "mobbin",
    "canonical_url": "https://designeer.xyz/inspiration/mobbin",
    "title": "Mobbin",
    "title_origin": "llms",
    "subtitle_quoted": "Real mobile and web product flows.",
    "origin_url": null,
    "platform": null,
    "kind_detail": "gallery",
    "status": "active",
    "payload_json": {"sitemap_lastmod": "2026-10-10T11:51:10.865Z"},
    "tags": [{"facet": "topic", "value": "inspiration", "origin": "source"}],
    "assets": [{
      "role": "poster",
      "url": "https://cdn.designeer.xyz/items/60820d1c-fcee-4629-a94f-03c5ebdc9ba0/poster/1200.webp",
      "width": 1200, "height": 675, "media_type": "image/webp"
    }]
  },
  {
    "kind": "work",
    "source_id": "designeer",
    "collection": "finds",
    "slug": "0xurvish-2111837715549610398",
    "canonical_url": "https://designeer.xyz/finds/0xurvish-2111837715549610398",
    "title": "0xurvish",
    "title_origin": "slug",
    "author_name": "0xurvish",
    "author_url": "https://x.com/0xurvish",
    "origin_url": "https://x.com/0xurvish/status/2111837715549610398",
    "kind_detail": "find",
    "status": "draft",
    "payload_json": {"x_status_id": "2111837715549610398"}
  }
]
```

## 9. 对引擎的影响（必须写进排期）

- M0 只能用"集合 + slug + 海报"建库，检索质量上限不高；真正让它可用的是 M1 的 vision 抽取与人工 pattern 标注。
- Designeer 的分布偏向**英文 web 设计目录**（SaaS 落地页、作品集、组件库），移动端与 B 端后台样本稀疏。`spec` 输出必须带 `bias` 标注，否则会把这一小撮口味当成全行业共识。
- finds 是唯一的"真实作品"信号（来自 X 的设计师产出），价值高于目录条目，但只有海报、没有描述，最适合做 vision 抽取的首批样本。
