# 炼化卡样例

三张 `ie distill` 的真实输出（未改写，只补了来源与许可行）。用来看清「炼化」到底交付什么：
**不是评分，是一段能直接喂给模型或代码的配方。**

| 文件 | 原始作品 | 领域 | 看点 |
| --- | --- | --- | --- |
| [01-saas-landing.md](01-saas-landing.md) | [Designeer · Mobbin](https://designeer.xyz/inspiration/mobbin) | 产品界面 | 白底极简落地页：实测色板、8pt 级间距、可直接用的中英提示词 |
| [02-icon-grid.md](02-icon-grid.md) | [Designeer · finds](https://designeer.xyz/finds/0xurvish-2111837715549610398) | 视觉设计 | 散点图标阵列：景深与留白的关系，无文字场景怎么拆 |
| [03-oil-painting-colonnade.md](03-oil-painting-colonnade.md) | [Wikimedia Commons](https://commons.wikimedia.org/wiki/File:1872_Vereshchagin_Praesentation_der_Trophaeen_anagoria.JPG)（公有领域） | 绘画 | 跨域样例：柱廊的透视与节奏如何被拆成可迁移原则 |

## 怎么生成的

```sh
GEMINI_API_KEY=... ./inspiration/target/release/ie distill --work <id> --model gemini-3.1-flash-lite
```

2026-11-10 生成，模型 `gemini-3.1-flash-lite`（当时可用的最快档）。卡片结构见
[`ie distill` 说明](../../../inspiration/README.md#炼化把一张图变成可复现的文字配方)。

## 读的时候注意

1. **【实测】与【（估计）】必须分开看**：色板 HEX、占比、留白率、对比度、边缘密度、对称/对齐度来自像素测量，
   精确到小数；而字号倍数、圆角、间距模数只能估计，卡片里都标了「（估计）」或写成范围。
   模型仍可能编造——遇到没有标「（估计）」又不在实测列表里的数字，当作不可信。
2. **模型档位影响拆解深度**：flash-lite 对字体与材质的判断偏粗，换更强的模型会细一些，但成本与额度更高。
3. **tokens 段是给 UI 代码用的**：非界面作品（如那幅油画）会出现 `radius: 0px`、`spacing: 透视递减` 这种错位，
   说明领域模板还没按媒介切换。
4. 作品图片本身不入库、不入仓：这些卡片只包含文字配方与实测数值。

## 用途

- 直接复用：把「复现提示词」或「一句话配方」粘进图像模型 / 代码生成器；
- 交叉复用：把「可迁移原则」拿去另一个领域（例：柱廊的等距节奏 → 列表行距）；
- 回归测试：换模型或改 Prompt 后重跑同三张，对比差异（目前的对照基线就是这三份）。
