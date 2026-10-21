> 设计研究，非当前实现状态。文中方案和模型效果描述用于设计讨论，不构成效果保证；实际能力与限制见 [Rust 迁移说明](../rust-migration.md)。

利用 LLM 反向解析图片为 JSON 并在后续生成中复用的方法，目标是将**非结构化的视觉信息（Unstructured Data）转化为结构化的配置参数（Structured Configuration）**。

这与AI漫剧创作Prompt指导原则中提到的**“世界观提取”**和**“核心标签锁定”**理念完全吻合，且从工程实现角度来看，JSON 格式比纯文本描述更具鲁棒性。

结合文档内容，这种方法之所以有效，是因为它构建了一个**“数字化视觉 DNA 库”**。以下是如何将其系统化落地的分析：

### 1. 为什么 JSON 能解决一致性问题？

根据文档，AI 模型在处理长文本时容易遗忘早期设定 。而 JSON 的优势在于：

* **减少描述歧义：** JSON 键值对（如 `"clothing_material": "leather", "color_hex": "#000000"`）比“帅气的衣服”等描述更明确，但仍需校验字段和模型输出。
* 
**明确上下文引用：** 结构化上下文（如 XML/JSON）便于组织和校验信息，但不能保证模型始终正确引用。


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
      "face": "...",
      "body_type": "...",
      "hair_style": "...",
      "distinguishing_marks": "..."
    },
    "attire": {
      "base_layer": "...",
      "accessories": "..."
    }
  },
  "technical_specs": {
    "lighting_style": "...",
    "camera_angle": "...",
    "composition": "...",
    "color_palette": ["#Hex1", "#Hex2"]
  },
  "stable_diffusion_tags": "..."
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

使用 JSON 作为中间层，便于将描述转为可版本控制、可校验的配置；格式有效并不代表内容准确或生成结果一致。
