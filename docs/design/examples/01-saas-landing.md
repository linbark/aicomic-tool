# 炼化卡 · Mobbin

> Designeer · inspiration · work #7 ｜ 1200×675 ｜ gemini-3.1-flash-lite  
> 来源：<https://designeer.xyz/inspiration/mobbin>（目录条目，作品权利归 Mobbin）

## 一句话
极简主义 SaaS 落地页，通过高留白与克制的色彩引导视觉焦点。

## 视觉配方

### 构图
- **骨架**：中心对称布局，顶部导航栏悬浮，主体内容垂直居中。
- **锚点**：顶部 Logo 与导航栏，中心主标题，底部合作伙伴 Logo 阵列。
- **层级**：一级标题（最大） > 副标题 > CTA 按钮 > 辅助信息。
- **重心**：重心位于画面垂直中心，视觉重量通过主标题的字重平衡。

### 比例与间距
- **比例**：留白率 0.75，主体内容区域宽度约占总宽度的 40%–50%（估计）。
- **间距**：段落间距采用 24px–48px（估计）的倍数，保持呼吸感。

### 色彩（像素实测）
| HEX | 占比 | 角色 |
| --- | --- | --- |
| `#FFFFFF` | 75% | 底/主色 |
| `#F1F1F1` | 11% | 辅助 |
| `#FDFDFD` | 11% | 辅助 |
| `#AAB5A5` | 2% | 辅助 |

- 留白率 0.75｜边缘密度 0.08｜明度对比 3.4:1｜对称 0.60｜对齐 0.81

### 文字
- **字体气质**：无衬线字体，几何感强，字形简洁。
- **字号层级**：标题字号约为正文的 3–4 倍（估计）。
- **字重字距**：标题使用粗体（Bold），正文使用常规体（Regular）。
- **特殊处理**：行间距设置在 1.2–1.5 倍（估计），确保阅读舒适度。

### 图形与材质
- **图形**：圆角矩形按钮，圆形 Logo 图标。
- **圆角/线宽**：按钮圆角 16–24px（估计），保持柔和感。
- **纹理**：纯色背景，无杂色纹理，强调平面感。

### 光
- **方向**：均匀的漫反射光，无明显阴影。
- **对比**：明度对比 3.4:1，通过色彩明度差异区分层级。
- **材质**：哑光材质，无光泽反射。

### 节奏
- **疏密**：边缘密度 0.08，中心区域疏密有致。
- **重复与破格**：底部 Logo 阵列重复出现，形成视觉节奏。

### 情绪坐标
```text
静 ← calm     █████████· → 动
冷 ← warm     █········· → 暖
繁 ← simple   ██████████ → 简
```

## 一句话配方（可直接粘贴）
> 极简对称布局，高留白，#FFFFFF 背景，#AAB5A5 绿色点缀，专业 SaaS 风格。

## 复现提示词

### 中文（直接可用）
> 极简主义 SaaS 落地页设计，1200x675 尺寸。背景为纯净的 #FFFFFF，局部使用 #F1F1F1 和 #FDFDFD 区分区块。中心放置一个醒目的绿色 #AAB5A5 图标。标题使用大号黑色无衬线字体，居中对齐。下方配有简洁的副标题和两个对比鲜明的 CTA 按钮。底部排列一组灰色的合作伙伴 Logo。整体构图对称，留白率 0.75，视觉重心稳固，光影均匀，无多余装饰，强调专业感与清晰的阅读体验。

### English
> Minimalist SaaS landing page design, 1200x675 resolution. Background is pure #FFFFFF, with subtle #F1F1F1 and #FDFDFD sections for layout structure. A prominent green #AAB5A5 icon is centered. Large bold black sans-serif headline, center-aligned. Below it, a concise subheadline and two high-contrast CTA buttons. A row of grey partner logos at the bottom. Symmetrical composition, 0.75 whitespace ratio, stable visual weight, uniform flat lighting, no unnecessary decorations, emphasizing professional clarity and readability.

### 给代码生成器的 tokens
```json
{
  "colors": [
    "#FFFFFF",
    "#F1F1F1",
    "#FDFDFD",
    "#AAB5A5"
  ],
  "radius": "16–24px（估计）",
  "spacing": "24px–48px（估计）",
  "type_scale": "标题与正文 3:1（估计）"
}
```

## 不要做什么
- 不要使用复杂的背景纹理或渐变，必须保持背景纯净。
- 不要让文字行间距小于 1.2 倍，否则会破坏留白感。
- 不要在非按钮区域使用圆角，确保图形语言统一。

## 可迁移原则
- 当信息层级复杂时，做大留白处理，因为留白能有效隔离视觉噪音。
- 当需要引导用户行为时，做高对比度色彩按钮，因为对比度是视觉注意力的第一驱动力。
- 当布局追求专业感时，做严格的对称对齐，因为对齐是建立秩序感的最低成本方式。

## 变体方向
- 保留极简布局，将绿色点缀改为蓝色系以体现科技感。
- 保留对称结构，将中心图标替换为产品截图以增强直观性。
- 保留留白比例，将顶部导航栏改为侧边栏以改变视觉流向。
