# 领域设计文档

本目录保存当前领域设计研究。设计文档描述为什么这样设计、工作流与界面如何组织，**不等于已交付功能**；当前能力与兼容边界以 [Rust 迁移说明](../archive/rust-migration.md) 为准。

| 文档 | 内容 | 状态 |
| --- | --- | --- |
| [漫剧开源工作流研究](漫剧开源工作流研究.md) | 开源漫剧工具、小说转剧本、分镜与运镜、一致性、视频、配音、监控与批量生产的外部证据 | 调研完成；决策已提取 |
| [星晴工作流与界面设计](星晴工作流与界面设计.md) | 制作阶段与闸门、信息架构、页面规格、视觉系统、实现范围与验收 | v1 已实现；后续里程碑标注为规划 |
| [灵感引擎设计](灵感引擎设计.md) | 领域无关的灵感引擎：审美基元与领域包、跨域迁移与融合、判美模型、DSH 接入 | 设计稿 v1.1；M1 像素层与 P0 演练已实测 |
| [个人美学内核设计](个人美学内核设计.md) | ToC 层：个人内核 vs 风格、层级效用模型、主动学习与内核卡、隐私与联邦 | 设计稿 v0.1；schema 迁移已就绪 |
| [炼丹模型](炼丹模型.md) | 药材/药液/配方/丹方的数据流：维度化药液、结构化配比、相冲检测、丹方可重放 | 设计稿 v0.1；迁移 006 已就绪 |
| [Designeer 数据源接入](Designeer数据源接入.md) | 首个数据源的端点实测、字段映射、归一化、刷新与降级 | 适配器规范 v0.1；目录层已实跑验证 |

灵感引擎的样例与数据资产：

- [examples/](examples/README.md)：三张真实炼化卡（界面 / 视觉设计 / 绘画），读完能看清「配方」长什么样。

- [inspiration/schema.v2.sql](inspiration/schema.v2.sql)：领域无关核心库（33 张表 + 4 视图 + 6 触发器），`sqlite3 inspiration.db < schema.v2.sql` 可直接建库，已实测。
- [inspiration/migrations/002-personal-layer.sql](inspiration/migrations/002-personal-layer.sql)：个人层迁移（人、会话、内核、内核卡、风格、出题队列），在 v2 上执行后 `schema_version=3`。
- [inspiration/packs/core.pack.json](inspiration/packs/core.pack.json)：38 个跨域审美基元 + 10 个可测度量。
- [inspiration/packs/ui.pack.json](inspiration/packs/ui.pack.json)、[inspiration/packs/architecture.pack.json](inspiration/packs/architecture.pack.json)：两个领域包（12 + 10 个手法）。
- [inspiration/migrations/006-alchemy.sql](inspiration/migrations/006-alchemy.sql)：药液/背包/配方/丹方/丹药七张表 + `v_bag`。
- [inspiration/recipes/](inspiration/recipes/)：示例配方与丹方（JSON）。
- [../inspiration/README.md](../../inspiration/README.md)：M1/P0 的 Rust 研究原型（`ie extract / simulate / fit`），含实测结果与已知限制。

阅读顺序：先看设计文档的 §0 一页结论；需要外部依据再回到研究文档的 §1 与 §12；实现与验收细节看设计文档 §8。
