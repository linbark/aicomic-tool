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

视觉与像素代理的一致性（同一基元，n=20）：`negative-space.active` Spearman **0.62**，`contrast.value` 仅 **0.13**。后者说明像素对比度代理不可靠——两路证据**不应盲目平均**。当前 `work_primitive` 的合并只是物化折中，原始分数都留在 `observation.payload_json` 里，之后可以按来源重算或做复核队列。

全量 851 张的估算：约 **2.45M token**。

### 额度实测（免费层）

这个 key 是 **Free Tier**，API 不提供"剩余额度"查询（`/v1beta/quotas` 404，响应头也没有配额字段）。能读到的只有触顶后的 429：

```text
quotaId: GenerateRequestsPerMinutePerProjectPerModel-FreeTier
quotaValue: "5"     # 每模型每分钟 5 次
retryDelay: 16–27s
```

因此 `ie vision` 默认 **串行 + 5 次/分钟节流**；遇到 429 按 `retryDelay` 退避重试，遇到「每日」配额才停。不同模型的每分钟额度互相独立（flash-lite 与 3.5-flash 可各自跑）。

按 5 次/分钟算：825 张剩余约 **2.75 小时**；每日上限未知，跑满会以 429 停下并打印原始错误。

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
