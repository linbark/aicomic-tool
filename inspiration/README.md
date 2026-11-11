# ie · 灵感引擎研究原型

本目录是 [灵感引擎设计](../docs/design/灵感引擎设计.md) 的 M1（提取）+ P0（个人层闭环）落地验证，**不是正式产品代码**：独立 workspace，不参与根 `Cargo.toml` 的构建与发布。

```text
inspiration/
├── crates/ie/            # CLI：extract / simulate / fit / stats
└── Cargo.toml
```

## 构建

```sh
cd inspiration && cargo build --release --offline
```

依赖只用本地已缓存的 crate（image / rusqlite / rayon / serde_json），不联网。

## 数据前提

本地库在 `data/inspiration/inspiration.db`（gitignore，不提交）。建库顺序：

```sh
sqlite3 data/inspiration/inspiration.db < docs/design/inspiration/schema.v2.sql
sqlite3 data/inspiration/inspiration.db < docs/design/inspiration/migrations/002-personal-layer.sql
sqlite3 data/inspiration/inspiration.db < docs/design/inspiration/migrations/003-work-vector.sql
sqlite3 data/inspiration/inspiration.db < docs/design/inspiration/migrations/004-observation-unique.sql
```

## 命令

```sh
ie extract  --db data/inspiration/inspiration.db --root data/inspiration [--limit N]
ie vision   --db ... --root ... --model gemini-3.1-flash-lite [--limit N] [--thinking 0]
ie stats    --db data/inspiration/inspiration.db
ie simulate --db <db> --person demo --n 200 [--seed 7] [--mode random|active]
ie fit      --db <db> --person demo
```

`ie vision` 需要环境变量 `GEMINI_API_KEY`，只在进程内读取，不写库、不落盘、不打印。

`simulate` 是演练：用已知的个人内核生成合成判定，`fit` 再把它估回来，用来验证"判定 → 拟合 → 内核卡"的代码路径。**它不是真实用户数据。**

## 实测结果（2026-11-10，本机）

| 项目 | 结果 |
| --- | --- |
| 素材 | 851 张（827 张 UI 目录海报 + 24 张建筑照片），解码成功率 100% |
| 抽取速度 | 851 张 / 约 2.8 s（rayon 多线程，单张约 3 ms） |
| 度量 | 6 类 × 851 = 5106 条 `measure` |
| 基元向量 | 810 件作品 × 6 基元 = 4860 条 `work_primitive`（UI 786 + 建筑 24） |
| 空白海报 | 41 张（4.8%），边缘密度 <0.01，已标 `asset.blank` 并跳过向量 |
| 覆盖基元 | 6/38（比例体系、对称、主动留白、明度对比、单色克制、细节密度） |
| P0 演练（n=200） | 留出准确率 0.775–0.875，与真值 ρ=0.91–0.98，产出 L3 内核卡 |

度量分布（UI 域，n=827）：

| 度量 | 最小 | 均值 | 最大 |
| --- | --- | --- | --- |
| 边缘密度 | 0.000 | 0.124 | 0.674 |
| 对称指数 | 0.000 | 0.601 | 1.000 |
| 留白率（低细节瓦片） | 0.000 | 0.711 | 1.000 |
| 色板紧凑度 | 0.000 | 0.829 | 1.000 |
| 对比度 | 1.000 | 7.114 | 21.000 |
| 对位残差 | 0.000 | 0.415 | 1.000 |

## 视觉抽取（Gemini，2026-11-10 试跑）

| 项目 | 实测（20 张试跑） |
| --- | --- |
| 可用模型 | `gemini-3.1-flash-lite`（约 2–18 s/张）、`gemini-3.5-flash`（约 5–26 s/张） |
| 不可用 | `gemini-2.5-flash` 提示"不再对新用户开放"；`gemini-3.8-flash` 返回 503（高需求） |
| 单张成本 | 约 2.9k token（文本约 1.35k + 图像固定约 1.09k + 输出约 0.37k） |
| 成功率 | 20/20 |
| 产出 | 手法 30+ 条、基元 50+ 条，全部落在词表内（词表外 term 会被丢弃） |
| 主要手法 | `ui.type-scale` 14、`ui.grid-columns` 9、`ui.corner-system` 9、`ui.hero-focal` 8 |
| 主要基元 | `negative-space.active` 20、`contrast.value` 18、`scale.hierarchy` 14、`rhythm.equal` 11 |

视觉与像素代理的一致性（同一基元，n=46，三模型混合）：

| 基元 | Spearman | 解读 |
| --- | --- | --- |
| `detail.density` | **0.81** | 两路都靠谱，可互相印证 |
| `negative-space.active` | **0.73** | 同上 |
| `contrast.value` | 0.38 | 像素分位数代理不可靠 |
| `balance.symmetry` | 0.29 | 像素镜像对白底敏感 |
| `restraint.single-hue` | 0.25 | 调色板紧凑度过粗 |

结论：**两路证据不能平均**。已改为按来源存证据（`work_primitive_evidence`，迁移 005）：
`work_primitive` 只物化置信度最高的那一路，两路都 ≥0.5 且相差 >0.3 时写 `review_queue` 待复核。
像素代理的置信度不是拍脑袋，而是按上面的一致性标定的：`detail.density` 0.80、`negative-space.active` 0.75、`proportion.system` 0.40、`contrast.value` 0.40、`balance.symmetry` 0.35、`restraint.single-hue` 0.35。视觉模型用自报置信度（通常 0.6–0.9），所以不可靠的代理会被自动替换，而两路都可靠时的分歧才会进复核队列。

注意：当前库里的 46 条视觉标注来自三个模型、像素侧还是旧的 0.45 固定置信度，属于过渡数据；计费后的单模型全量 pass 会重建全部证据，届时口径才统一。

46 张视觉标注已覆盖 **28 个基元**（像素层只有 6 个），平均每张 5.9 个基元。

全量 851 张的估算：约 **2.45M token**。

### 额度实测（免费层）

这个 key 是 **Free Tier**，API 不提供"剩余额度"查询（`/v1beta/quotas` 404，响应头也没有配额字段）。能读到的只有触顶后的 429：

```text
quotaId: GenerateRequestsPerMinutePerProjectPerModel-FreeTier
quotaValue: "5"     # 每模型每分钟 5 次
retryDelay: 16–27s
```

因此 `ie vision` 默认 **串行 + 5 次/分钟节流**；遇到 429 按 `retryDelay` 退避重试，遇到「每日」配额才停。

跑满当天额度后拿到第二条，才是真正的天花板：

```text
quotaId: GenerateRequestsPerDayPerProjectPerModel-FreeTier
quotaValue: "20"      # 每模型每天 20 次
retryDelay: 76003s    # 约 21 小时
```

结论：

| 事实 | 数值 |
| --- | --- |
| 每分钟 | 5 次 / 模型 |
| 每天 | **20 次 / 模型**（各模型独立计） |
| 已消耗 | flash-lite 约 20、3.6-flash（3.5 别名）20 |
| 当天可用 | 只剩 `gemini-3-flash-preview`（约 19 次）；其余模型 503/404 |

按免费层算，851 张需要 **3–6 周**（每天 20–40 次）。全量语义标注在免费层不可行，只能做**分层抽样**（finds 优先、高分海报优先），或者开计费后一次跑完（约 2.45M token）。

## 导入导出（倒腾数据）

```sh
# 全量元数据（不含图片）
ie export --db data/inspiration/inspiration.db --out corpus.json

# 同一份数据导出为 NDJSON（首行 header，之后一行一条，按依赖顺序）
ie export --db ... --out corpus.ndjson            # 后缀自动识别
ie export --db ... --out corpus.txt --format ndjson

# 只导 finds，并把图片一起打进去
ie export --db ... --out finds.json --collection finds --with-assets

# 导进另一个库（可重复执行，不会产生副本）
ie import --db /tmp/other.db --in finds.json --root /tmp/other --dry-run
ie import --db /tmp/other.db --in finds.json --root /tmp/other
```

要点：

- **用自然键而不是自增 id**：work = `source + collection + slug`，asset = `work + role + sha256`，
  observation = `asset + extractor + version`。所以能导进任意库，不用对齐 id。
- **幂等**：同一份包导入两次，各表行数不变（实测 865 作品 / 851 素材 / 897 观察 / 5106 度量 / 4921 证据）。
- **图片可选**：默认只带 sha 与路径；`--with-assets` 把 blob 以 base64 塞进包（163 张 finds ≈ 8.6 MB），
  导入端按 sha 落到 `<root>/blobs/xx/<sha>`，天然去重。
- **个人数据默认不导出**：内核、判定、会话需要 `--with-personal` 显式打开。
- **`--dry-run`** 会完整跑一遍再回滚，只报数量。
- **两种格式**：`.json`（一个 bundle 对象，便于肉眼查看）与 `.ndjson`（一行一条，导入端流式消费，
  内存只占一行）。导入按首行 `{"_type":"header"}` 自动识别，不需要额外参数。

实测（2026-11-11）：

| 指标 | JSON bundle | NDJSON |
| --- | --- | --- |
| 体积 | 4.2 MB | 4.4 MB |
| 行数 | — | 12866 |
| 导入后作品/素材/观察/度量 | 865 / 851 / 897 / 5106 | 865 / 851 / 897 / 5106 |
| 重复导入 | 不翻倍 | 不翻倍 |
| `--with-assets`（8 张柱廊） | 可用 | 可用，导入后 8 个 blob 落盘 |

一个已知差异：源库里 `work_primitive` 有 5026 行，导入后是 4921 行——多出的 105 行是早期"两路平均"逻辑留下的
`source='mixed'` 记录，迁移 005 时没有回填证据。等单模型全量 pass 重跑后两边就一致了。

## 炼化：把一张图变成可复现的文字配方

标注（基元分数）只是内部索引，不能直接复用。`ie distill` 产出的是**配方**：

```sh
GEMINI_API_KEY=... ./inspiration/target/release/ie distill --image ~/Pictures/ref.jpg
# 或从库里取：--work 7
```

输出一张 Markdown「炼化卡」，包含：

| 区块 | 内容 | 来源 |
| --- | --- | --- |
| 一句话 | 它为什么好看 | 模型 |
| 构图 / 比例 / 文字 / 图形 / 光 / 节奏 | 分维度拆解，每节带可检查的判断 | 模型 + 实测 |
| 色彩 | HEX + 面积占比 + 角色 | **像素实测** |
| 一句话配方 | 可直接粘进任意提示词开头的风格串 | 模型 |
| 复现提示词 | 中文 / English，自包含，可直接投喂图像模型 | 模型 |
| 给代码生成器的 tokens | colors / radius / spacing / type_scale | 模型 |
| 不要做什么 | 3 条最容易做错的地方 | 模型 |
| 可迁移原则 | 3 条「当…时，做…，因为…」 | 模型 |
| 变体方向 | 3 条保留内核、改一处结构的衍生 | 模型 |

硬规则：只有【实测】里的数字可以是确定值，其余必须写成范围并标「（估计）」，防止模型编造精确到个位的数值。

样例（已入仓，可直接读）：

- [01-saas-landing.md](../docs/design/examples/01-saas-landing.md)：白底极简 SaaS 落地页
- [02-icon-grid.md](../docs/design/examples/02-icon-grid.md)：散点圆形图标阵列
- [03-oil-painting-colonnade.md](../docs/design/examples/03-oil-painting-colonnade.md)：19 世纪油画（跨域样例）

设计说明见 [灵感引擎设计](../docs/design/灵感引擎设计.md) §6.3 与 [个人美学内核设计](../docs/design/个人美学内核设计.md)。

## 内核卡（演练输出）

```text
内核 v1：n=200 训练准确率=0.806 留出准确率=0.800 成熟度=L3
与演练真值的相关系数 rho=0.960
  [avoid]  你在「对称」上更不在意（-1.41σ，基于 200 次判定）
  [prefer] 你在「主动留白」上更看重（+1.23σ，基于 200 次判定）
  [prefer] 你在「单色克制」上更看重（+0.79σ，基于 200 次判定）
  [prefer] 你在「细节密度」上更看重（+0.76σ，基于 200 次判定）
  [signature] L3：当前只覆盖像素可测的 6 个基元，语义手法待视觉抽取
```

## 已知限制

1. **只覆盖 6 个基元**。其余 32 个（节奏、停顿、并置、引用等）需要视觉模型或人工标注，当前没有可用的授权模型，故意不猜。
2. **空白海报**占 4.8%，说明上游目录有占位图；`asset.blank` 只是标记，上游质量仍需人工抽检。
3. **代理相关性**：6 个基元里有 3 个直接来自几何量（对称/留白/对比），彼此并不独立；内核卡的主张要避免把相关当成独立证据。
4. **只有 UI 域有量**。建筑 24 件只有图，还没有抽取（同一套 extract 可以直接跑）。
5. 判美与跨域迁移（M2）尚未开始。

## 一个数据工程教训

SQLite 的 `UNIQUE(asset_id, fragment_id, extractor, version)` 在 `fragment_id IS NULL` 时**不生效**（NULL 互不相等），实际可以重复插入。已改用两个部分唯一索引兜住，见 `migrations/004-observation-unique.sql`。同类问题也适用于 `device_usage`。
