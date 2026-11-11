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

`ie vision` 默认走 Gemini，需要环境变量 `GEMINI_API_KEY`；只在进程内读取，不写库、不落盘、不打印。

### 换模型：OpenAI 兼容端点

`vision` / `distill` 都支持 `--provider openai`，指向任何 OpenAI 兼容服务（Ollama、vLLM、DashScope compatible-mode、OpenRouter）：

```sh
# 本地 Qwen-VL（Ollama）
ollama pull qwen2.5vl:7b
./inspiration/target/release/ie distill --image ref.jpg \
  --provider openai --base-url http://127.0.0.1:11434/v1 --model qwen2.5vl:7b

# 云端（DashScope 兼容模式）
IE_API_KEY=sk-... ./inspiration/target/release/ie vision --provider openai \
  --base-url https://dashscope.aliyuncs.com/compatible-mode/v1 --model qwen-vl-max --limit 20
```

- OpenAI 路线不强制密钥（本地不需要）；需要时用 `IE_API_KEY` 传 `Authorization: Bearer`。
- 请求体带 `response_format: json_object`，解析时会去掉 ```` ```json ```` 代码围栏。
- 这条路径已用本地 mock 服务验证：`distill` 能出完整卡片，`vision` 能写 observation/device_usage/evidence（2 张 2.1s，150 token/张）。
- **本地 Qwen3-VL 注意**：Ollama 上的 `qwen3-vl:8b` 是**思考版**，长提示词下会把 token 全花在 reasoning 上、`content` 为空（实测 108s/张、0 成功）。要用 `qwen3-vl:8b-instruct`。
  实测 instruct 版：单张视觉抽取约 **35–53 s**（M4/16GB），炼化卡约 **72 s**，JSON 基本合规，但有三类问题——都已处理：
  1. 偶尔把 devices 写成字符串数组 → 两种形态都收，支持中文 label 反查 term；
  2. **空白海报会诱发幻觉**：41 张纯白海报里，模型会给"纯白背景、无任何视觉元素"的图编出 35 个基元和 6 个手法。
     现在 vision 直接跳过 `validation_result` 里标了 `asset.blank` 的素材（实测清理了 7 条幻觉标注）；
  3. 提示词改成「示例 + 硬范围」后（手法 1–5、基元 2–6），输出稳定在 3–5 / 3–5，不再刷满词表；
     解析层另加截断（6/8）作为兜底。
- JSON 解析会剥离 `<think>…</think>`、代码围栏和前后废话，再退化为首尾花括号切片，兼容小模型的常见格式问题。

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
- **个人数据默认不导出**：内核、判定、会话需要 `--with-personal` 显式打开。导出会保留这些行，但**当前版本不导入个人数据**（导入时会明确提示），避免把别人的口味静默合进你的库。
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

带图片的 NDJSON **不把 base64 塞进 asset 行**，而是紧跟着输出多条 blob 行：

```json
{"_type":"blob","sha":"2c86…","index":0,"total":1,"data":"<base64 192KB 分块>"}
```

- 每行 ≤ 256 KB，导入端边读边写临时文件，收齐最后一块才 `rename` 到 `blobs/xx/<sha>`；
- 实测 163 张 → 167 条 blob 行（4 张需要 2 块），最大行 262 KB，导入后 163 个文件、无临时残留；
- `--dry-run` 不写任何文件（回滚前也不落地）；
- 导入结束会自检：`local_path` 指向的文件不存在就置空并提示，不留悬空路径（实测元数据包导入后 851 条路径被清空但元数据保留）。
- **完整性校验**：分块拼完后算 SHA-256，与文件名不符就删掉临时文件并中止导入。
  实测把某块 base64 改一个字符，导入直接报 `blob 校验失败`，磁盘上不留任何文件、事务回滚。

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

## 炼丹 D0：炼药与背包

```sh
# 炼药：像素类手法不需要密钥
./inspiration/target/release/ie refine --work 7 --technique t.palette

# 视觉类手法：本地 Qwen3-VL 或 Gemini
./inspiration/target/release/ie refine --work 7 --technique t.layout,t.type \
  --provider openai --base-url http://127.0.0.1:11434/v1 --model qwen3-vl:8b-instruct

# 看背包
./inspiration/target/release/ie bag [--dimension palette]
```

实测（work 7，Mobbin 海报）：

| 手法 | 维度 | 火候 | 质量 | 耗时 |
| --- | --- | --- | --- | --- |
| `t.palette` | palette | pixel-proxy | 0.85 | 毫秒级（纯计算） |
| `t.layout` | layout | qwen3-vl:8b-instruct | 0.85 | 16 s |
| `t.type` | type | qwen3-vl:8b-instruct | 0.90 | 13 s |

药液按 `(work, technique, dimension, fire_model)` 唯一，重复炼会更新而不新建；`hash` 记录内容指纹，便于缓存与复现。

### 三个小模型上的坑（都在迁移 007–009 里修了）

1. 模板里写 `"confidence":0.0`，模型会**照抄 0.0**，质量分变 0 → 示例改成正数并说明含义；
2. OpenAI 兼容端点加 `response_format: json_object` 会让 Ollama 上的 Qwen 陷入复读并截断 → OpenAI 路线不再发这个字段；
3. 模型会把 `ratios` 写成 `{"a","b"}`（对象里裸字符串）→ 把该字段改成**字符串数组**。

## 炼丹 D1：配方成丹与相冲检测

```sh
ie brew --recipe docs/design/inspiration/recipes/minimal-landing.recipe.json
ie brew --recipe ... --force          # 带着冲突记录强制成丹
ie brew --recipe ... --out data/inspiration/pills
```

流程：读配方 → 入库（按 key upsert）→ 逐槽挑质量最高的药液 → 相冲检测 → 渲染合成提示词 → 存 `pill`。

实测（work 7 的四瓶药液）：

```text
$ ie brew --recipe minimal-landing.recipe.json
Error: 相冲，拒绝炼丹：
- 主色板明度对比 3.35:1 低于配方要求 4.5:1

$ ie brew ... --force
# 丹药 · 极简品牌落地页
> 以 50% 的色彩语言定调（主色 #FFFFFF 75%、#F1F1F1 11%、#FDFDFD 11%、#AAB5A5 2%），
  以 30% 的版式骨架搭结构（单页），15% 的字阶作为强调…
## 相冲检查
- [error] contrast.min：主色板明度对比 3.35:1 低于配方要求 4.5:1
丹药 #2 已入库
```

检查项：`fire.mixed`（一炉混火候，默认拒绝）、`contrast.min`（对比不足）、`quality.low`（药液质量 <0.5 仅告警）。

## 炼丹 D2：丹方

```sh
ie formula --in docs/design/inspiration/recipes/brand-page.formula.json \
  --provider openai --base-url http://127.0.0.1:11434/v1 --model qwen3-vl:8b-instruct
```

丹方按步骤执行，`refine` 步会**同火候复用**已有药液（不重复调用模型），`brew` 步用丹方钉死的输入而不是"背包里挑最好的"。

实测这一炉：

| 步 | 做什么 | 结果 |
| --- | --- | --- |
| s1 | refine(work 7, t.layout) | 复用 #2 |
| s2 | refine(work 632, t.palette) | 复用 #1（palette 是像素手法，不耗模型） |
| s3 | refine(work 679, t.type) | 该图无文字 → 跳过，brew 时回落到背包 |
| s4 | brew(recipe.minimal-landing, [s1,s2,s3]) | 无冲突，丹药 #6 |
| s5 | render | 已由 brew 输出 markdown |

第一次跑时 s2 钉的是 work 7（白底海报，对比 3.35:1），brew 直接拒绝："主色板明度对比 3.35:1 低于配方要求 4.5:1"——**丹方把材料写死，材料不行就当场暴露**，这正是丹方该有的价值。

## 炼丹 D3：丹药预检与配方变异

```sh
ie brew --recipe ...            # 成丹后自动评分
ie pills [--limit N]            # 丹药列表（总分/质量/完整/冲突）
ie mutate --recipe ... --runs 4 --delta 0.15 --seed 7   # 配比扰动实验
```

预检分（`pill.score_json`）由五项加权：

| 分量 | 权重 | 含义 |
| --- | --- | --- |
| quality | 0.25 | 药液质量按配比加权 |
| completeness | 0.30 | 必需槽位是否齐全 |
| diversity | 0.15 | 不同药材的占比（防一家独大） |
| ratio_ok | 0.15 | 配比之和是否归一 |
| prompt_ok | 0.15 | 模板占位符全部渲染、长度合理 |
| 冲突 | 扣分 | error −0.5、warn −0.1 |

**这个分不衡量美**，只回答"这颗丹药合不合配方、材料够不够用"。

### 变异的实测结论（重要）

`ie mutate --runs 4 --delta 0.15` 生成 4 颗变异丹，配比确实变了（palette 0.49–0.50、layout 0.29–0.30），但**预检分全是 0.89**——因为预检分只看合规与材料质量，不看"0.5 还是 0.49 更好"。

结论：**变异能做，裁判没有**。要区分变异优劣，必须外接人评或模型盲评；预检分只能当准入门槛，不能当目标函数。

扩大背包后再验一次（15 件作品、56 瓶药液）：普通配方成丹 #12 预检 **0.937**，四个变异（palette 0.45–0.50）全部 **0.94**——结论不变。

## 评判流水线（含人工干预）

```sh
# 预检 -> 机器盲评 -> 按规则升级人工
ie judge run --pills 12,13,14 --provider openai --base-url http://127.0.0.1:11434/v1 \
  --model qwen3-vl:8b-instruct --seed 5

ie judge list                  # 所有 run 的状态
ie judge review --run 3        # 待人工条目
ie judge record --item 7 --score 82 --note "层级更清楚"
```

四个阶段，每步都落库、可中断重放：

| 阶段 | 做什么 | 失败/升级条件 |
| --- | --- | --- |
| precheck | 复用 `pill.score_json` | 有 error 的丹药直接跳过 |
| machine | 打乱标签 A/B/C，只给规格文字，四维打分（层级/自洽/克制/可执行） | 分差 <3、得分 <40、固定抽第一颗 → 转人工 |
| human | `judge review` 列出、`judge record` 回填 | 人工分回填后重算最终分 |
| final | `0.45·机器 + 0.25·预检 + 0.30·人工` | 无人工时 `0.65·机器 + 0.35·预检` |

实测（本地 qwen3-vl 8B 当裁判，三颗变异丹，同一 seed 重跑）：

- **可传递时**：C 赢 B、C 赢 A、A 赢 B → BT 给出 C=100、A=35.1、B=10.8，C 免人工；
- **成环时**：B 赢 A、C 赢 B、A 赢 C → 无孔多塞胜者，BT 三个都收敛到相同强度，
  流水线直接标「判不出，全部转人工」，而不是编一个假顺序。

这正是换成成对比较的理由：**绝对打分会把成环偏好压成一个看似有序的分数**，成对比较 + BT 让"裁判自己都不一致"这件事暴露出来。

### 人工干预：一屏审阅 + 批量回填

```sh
ie judge screen --run 7                          # 一次看一屏
ie judge fill --run 7 --scores "A=60,B=88,C=72" --note "B 的字阶最稳"
```

`screen` 会把「成对理由 + 每个候选的标签/丹药/机器分/预检/规格摘要」压在一屏里；`fill` 一次写入整屏分数并重算。实测：

```text
已回填 3 条，run #7 重算：
标签   丹药     机器分   人工   最终分
A      12       10.8     60    46.0
B      13      100.0     88    95.0
C      14       35.1     72    61.0
run 状态：done
```

注意 A：机器给 10.8，人给 60——**人机分歧本身就是数据**，后面可以用它校准裁判。

### 盲评泄漏的坑（已修）

第一次成对比较时，规格里混进了作品名（`以 30% 的版式骨架搭结构（COOLEST）`）——`t.layout` 的药液把作品标题写进了 `structure` 字段。盲评前现在会把所有候选的药材名替换成「（某作品）」，避免泄漏来源。

## 端到端：丹药 → 产物 → 验收

```sh
ie brief  --pill 13 --out data/inspiration/briefs/pill-13   # 出简报 + 验收清单 + tokens.json
ie verify --pill 13 --html path/to/index.html                # 静态验收，结果落 pill_check
```

引擎不写 HTML：它交付「可执行规格 + 验收清单」，产物由编码 agent 写，写完回来验收——这样规格才是真正的接口。

`verify` 目前查四项：

| 检查 | 判据 |
| --- | --- |
| palette | 药液色板在产物里出现的比例 ≥60% |
| no_shadow | 禁忌里有投影时，产物不得出现 `box-shadow` |
| contrast | `body` 的 background/color（支持 CSS 变量）≥4.5:1 |
| type_scale | 最大/最小字号 ≥2× |

完整样例（含目标、产物、截图、8 条规格缺口清单）见 [examples/pill13](../../docs/design/examples/pill13/README.md)。

## 规格数值化：把形容词变成可执行参数

端到端样例暴露的 8 条缺口里，前 4 条都是「药液没给数值」，现已补：

| 手法 | 版本 | 新增输出 |
| --- | --- | --- |
| `t.palette` | 计算增强 | `roles: {bg,text,muted,border,accent,text_contrast}`，由 ink/paper 与对比度推断，不调模型 |
| `t.type` | v3 | `base_px` + `steps[{name,size_px,weight,tracking}]` |
| `t.spacing` | 新增 | `base_px`、`steps[]`、`page_margin_px`、`section_gap_px` |
| `t.donts` | v3 | 每条带 `applies_to` 语境标签 |
| `t.layout` | v4 | `structure` 限定为六个枚举，禁止写作品名 |

丹药 #23 的合成规格现在是：

```text
以 42% 的色彩语言定调（底 #FDFCFD、字 #2B2B2B、强调 #90A1A2、线 #DBE0E1，正文对比 13.83:1）；
以 22% 的骨架搭结构（单列）；间距用 13% 的模数（8px，档位 4、8、12、16、24、32、48、64，
页边 40px，段距 88px）；13% 的字阶作为强调（display 72px/600、body 18px/400）。
```

### 顺手修的 CLI bug

参数解析把**任何 `--flag` 都当成要带值**，所以 `--refresh --provider openai` 会把 `--provider` 吃掉、
`openai` 变成位置参数，provider 悄悄退回默认的 gemini。已改成：`--flag` 后面若紧跟另一个 `--xxx` 或没有值，
就按布尔开关处理。

### 间距改为像素测量（不再让模型猜）

原先 `t.spacing` 让模型看图给数值，实测三件作品**全部吐出 8/40/88**——那是模型的 Web 默认值，不是测量。
现在改为从图上量：按行/列能量找内容带与空白带，页边取首尾连续空白列，模数在常见候选（4/6/8/10/12/16/20）
里挑与所有空隙最贴合的，档位是模数的取整倍数。

实测四件作品（1200px 宽折算）：

| 作品 | 模数 | 页边 | 段距 | 档位 |
| --- | --- | --- | --- | --- |
| Mobbin | 4 | 209 | 44 | 8、16、28、40、44、52、80、140 |
| gunnar | 6 | 150 | 31 | 24、30、36、66 |
| coolest | 8 | 240 | 64 | 8、16、24、32 |
| 油画 | 8 | 8 | 16 | 8、16、24、32 |

**这是测量，但仍然粗糙**：模数拟合对复杂版式不稳（4 和 6 明显偏小），页边偏大（把内容前的整块留白都算进去了）。
好处是数值**随作品变化**了，而不是所有图都 8/40/88。confidence 固定 0.7，低于模型抽取，表示"量到了但不确定"。

### 语境标签不只给禁忌

间距也加了 `domain` 标签，brew 时按配方的 `domain`（如 `ui`）过滤——否则油画的页边距会被拼进落地页。
这条经验是通用的：**任何药液都需要语境标签**，否则跨材料合成一定出错。

### 还没修的：禁忌仍然"就图论图"

实测 t.donts 抽出来的是「登陆页使用模糊背景国旗图标」——源图的具体内容，不是可迁移的规则。
单图抽取天然做不到抽象；正确做法是**多图归纳**：拿同类页面的 3–5 张，让模型找共同禁忌。
`applies_to` 语境标签只是第一步，内容抽象还差一次归纳。

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
