利用 LLM 反向解析图片为 JSON 并在后续生成中复用的方法，实际上是将**非结构化的视觉信息（Unstructured Data）转化为结构化的配置参数（Structured Configuration）**。

这与AI漫剧创作Prompt指导原则中提到的**“世界观提取”**和**“核心标签锁定”**理念完全吻合，且从工程实现角度来看，JSON 格式比纯文本描述更具鲁棒性。

结合文档内容，这种方法之所以有效，是因为它构建了一个**“数字化视觉 DNA 库”**。以下是如何将其系统化落地的分析：

### 1. 为什么 JSON 能解决一致性问题？

根据文档，AI 模型在处理长文本时容易遗忘早期设定 。而 JSON 的优势在于：

* **消除语义歧义：** 自然语言（如“帅气的衣服”）存在解释空间，而 JSON 键值对（如 `"clothing_material": "leather", "color_hex": "#000000"`）是确定的参数。
* 
**强制注意力聚焦：** 文档提到，结构化的上下文注入（如 XML/JSON）相当于为模型挂载了一个外部数据库，使其能精准回溯校验 。


* 
**解耦视觉元素：** JSON 可以将“人物本身”（Visual DNA）与“环境/光影”（Context）彻底分离，符合文档中提到的“参数拆解协议” 。



### 2. 优化后的 JSON Schema (基于文档原则)

您提到的提示词 `Analyse this image in exhaustive JSON detail` 是一个很好的启动器（Trigger）。为了更好地适配漫剧工作流，我们可以基于文档中的**受控词汇表** 和 **Visual DNA**  概念，定义一个标准的 Schema。

**建议的 System Prompt 用于反推图片：**

> "作为一个资深视觉技术总监，请分析这张图片，并输出为严格的 JSON 格式。忽略无关背景，重点提取以下用于 AI 绘图模型（如 Midjourney/SD）复现的参数："

```json
{
  "character_core": {
    "visual_dna": {
      [cite_start]"face": "...", // [cite: 19] 对应文档中的面部特征提取
      "body_type": "...",
      "hair_style": "...",
      [cite_start]"distinguishing_marks": "..." // [cite: 19] 如：颈部条形码纹身
    },
    "attire": {
      "base_layer": "...",
      "accessories": "..."
    }
  },
  [cite_start]"technical_specs": { // [cite: 72] 对应文档的视觉参数化
    [cite_start]"lighting_style": "...", // e.g., Chiaroscuro, Rim Lighting [cite: 79]
    [cite_start]"camera_angle": "...", // e.g., Low Angle, Dutch Angle [cite: 78]
    "composition": "...",
    "color_palette": ["#Hex1", "#Hex2"]
  },
  [cite_start]"stable_diffusion_tags": "..." // [cite: 38] 提取为逗号分隔的 Tag 串
}

```

### 3. 工程化落地工作流 (Pipeline)

结合文档提到的**“一致性监理代理”** ，您可以构建如下自动流：

1. **资产数字化 (Asset Ingestion)：**
* 输入：角色立绘（如您的《天狼寨》角色“刘岐”）。
* 处理：LLM 运行 `Analyse... JSON` 提示词。
* 输出：生成 `LiuQi_Profile.json`。


2. **核心标签锁定 (Locking)：**
* 在后续脚本生成分镜提示词时，System Prompt 强制调用该 JSON 中的 `"character_core"` 字段。
* 
**Prompt 逻辑：** “在生成提示词时，必须包含以下来自 JSON 的描述：`{LiuQi_Profile.character_core}`。严禁修改。” 




3. **动态场景融合：**
* 保持 JSON 中的人物描述不变，仅动态替换 `"technical_specs"` 中的光影和角度参数（根据剧本的情绪需求调整） 。





### 4. 总结

使用 JSON 描述作为中间层，本质上是将**“Prompt Engineering”升级为“Configuration Engineering”**。它将模糊的自然语言变成了可版本控制、可校验的代码，这正是您作为架构师最擅长的领域。

---

## 实现状态

✅ **已实现**（详见 `docs/architecture/workflow-implementation.md` Phase 3）：

- **Visual DNA 摄取 API**: `POST /ai/visual-dna/ingest`
  - 支持从图片文件路径自动提取 Visual DNA JSON
  - 文件路径安全校验（必须在 `/files` 目录下）
  - 自动写入 Context Store
  - 生成 run 快照用于审计

- **前端集成**: 在 `ContextPage` 的 Visual DNA 编辑器中，可选择资产条目的图片文件进行自动摄取

- **当前限制**: LLM 接口为纯 chat，不支持真正的图片识别。当前实现通过文件路径和文件名进行推断，效果有限。后续可接入 GPT-4V、Claude Vision 等真正的 vision API。

## 相关文档

- [Workflow 功能实现文档](./architecture/workflow-implementation.md) - 完整的实现细节
- [AI Workflows 架构设计](./architecture/ai-workflows.md) - 整体架构