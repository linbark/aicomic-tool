# **计算剧作法：AI漫剧创作全流程系统提示词工程深度研究报告**

## **1\. 绪论：生成式叙事的工程化范式转移**

在人工智能与创意产业深度融合的当下，我们正在见证从“辅助创作”向“代理创作”的范式转移。对于AI漫剧（AI Motion Comics/Manju）这一新兴媒介形式而言，其制作流程不再是孤立的文本生成或图像生成，而是一个精密耦合的工程系统。在这个系统中，“系统提示词”（System Prompt）扮演着核心认知架构的角色。它不仅是简单的指令集合，更是定义AI模型行为边界、逻辑推理路径、审美标准以及输出格式的“源代码”。

本报告旨在针对AI漫剧创作的全生命周期——从大纲提炼、剧本生成、逻辑优化到分镜脚本转化——构建一套详尽的系统提示词设计理论与实践指南。基于谷歌（Google）提示工程白皮书、Anthropic的上下文工程策略以及影视工业的标准化流程，我们将深入剖析如何通过结构化、模块化和思维链（Chain-of-Thought, CoT）技术，将模糊的创意意图转化为精确的可执行指令。这不仅要求创作者具备文学素养，更要求其掌握一种全新的语言——“提示词工程语”，一种介于自然语言与机器逻辑之间的中间层语言。

漫剧与传统静态漫画的区别在于其对“流动性”和“分层”的特殊需求。漫剧需要将画面拆解为可动的图层，需要音频与视觉的对位，需要更强的镜头感。因此，用于漫剧的系统提示词必须具备更高的维度，不仅要描述“是什么”（What），还要定义“如何动”（How）以及“层级关系”（Hierarchy）。本报告将以15,000字的篇幅，对这一领域的系统提示词设计进行穷尽式的研究与解构。

## **2\. 系统提示词的认知架构与工程原理**

在深入具体应用场景之前，必须建立一套稳固的系统提示词设计方法论。根据Google Cloud Vertex AI及OpenAI的最佳实践，一个鲁棒的系统提示词应当被视为一个独立的软件模块，具备清晰的解剖结构1。

### **2.1 角色（Persona）：确立认知锚点与领域边界**

角色定义是系统提示词的灵魂。在大型语言模型（LLM）的推理过程中，Role Prompting（角色提示）起到了“预加载上下文”的关键作用。当我们设定AI为“资深好莱坞剧本医生”或“奥斯卡级摄影指导”时，实际上是在高维向量空间中锁定了模型检索知识的范围，使其注意力机制（Attention Mechanism）聚焦于特定领域的术语、逻辑和审美标准2。

对于漫剧创作，单一的“作家”角色往往失之于宽泛。我们需要构建更细分的“代理矩阵”：

* **叙事架构师（Narrative Architect）：** 专注于结构、节拍与宏观逻辑，对细节不敏感，但对起承转合有极高的控制力。  
* **场景描绘者（Scene Painter）：** 专注于视觉细节、光影氛围与环境描写，忽略剧情逻辑，只负责画面的“高保真”。  
* **对话优化师（Dialogue Polisher）：** 专注于潜台词、口语化与性格化语言，负责将书面语转化为生动的角色对白。

更重要的是，角色定义必须包含“负向约束”或“知识边界”。例如，设定“你是一个19世纪的维多利亚时代小说家”，必须同时明确“你不知道任何关于电力、内燃机或现代心理学术语的概念”。这种边界设定对于防止幻觉（Hallucination）和维持时代风格的一致性至关重要1。

### **2.2 上下文（Context）：构建静态真理来源**

在长篇漫剧创作中，上下文的一致性是最大的挑战。AI模型本质上是无状态的（Stateless），哪怕拥有长窗口（Context Window），也容易在多轮对话后遗忘早期的设定。因此，系统提示词必须包含一个结构化的“静态上下文”模块，我们称之为“世界观设定集”（Series Bible）1。

这部分内容应采用XML标签或JSON格式进行封装，以便模型能够精准解析。例如：

XML

\<series\_bible\>  
  \<protagonist\>  
    \<name\>K\</name\>  
    \<visual\_dna\>银色短发，赛博义眼（左眼），黑色战术风衣，颈部有条形码纹身\</visual\_dna\>  
    \<psychology\>虚无主义者，厌恶肢体接触，说话简短\</psychology\>  
  \</protagonist\>  
  \<world\_rules\>  
    \<physics\>低重力环境，跳跃高度是地球的3倍\</physics\>  
    \<technology\>全息投影普及，纸质书是违禁品\</technology\>  
  \</world\_rules\>  
\</series\_bible\>

这种结构化的上下文注入，相当于为模型挂载了一个外部数据库，使其在生成每一句对白、每一个分镜时，都能回溯校验，确保K不会突然拿出一本纸质书阅读，或者表现出热情洋溢的性格1。

### **2.3 指令（Instruction）：原子化与思维链**

指令部分是驱动模型行为的引擎。Google白皮书强调，指令必须具有“原子性”（Atomicity），即不要试图在一个长句中包含复杂的逻辑，而应将其拆解为顺序执行的步骤4。

对于复杂的创意任务，如“将小说章节改编为剧本”，必须强制模型使用思维链（Chain-of-Thought, CoT）策略。我们不能直接要求结果，而要定义过程：

1. **阅读与分析：** 提取核心冲突与情感基调。  
2. **节拍划分：** 将连续的文本拆解为独立的叙事节拍（Beats）。  
3. **视觉转化：** 思考如何用画面表达这些节拍。  
4. **剧本生成：** 按照Fountain格式输出最终文本。

通过在系统提示词中嵌入“请一步步思考”（Let's think step by step）或强制模型先输出\<scratchpad\>（草稿区），我们可以显著降低逻辑错误率，使AI的推理过程显性化，便于人类进行干预和修正1。

### **2.4 输出格式（Output Format）：工程化交付标准**

为了使AI生成的剧本能够无缝接入后续的生产管线（如直接导入Stable Diffusion生成图像，或导入TTS生成语音），输出格式必须是严格标准化的。自然语言的输出对于自动化工作流是灾难性的。

我们推荐使用Markdown表格、JSON对象或严格的Fountain脚本格式。例如，要求分镜脚本必须输出为CSV格式，包含“场号”、“景别”、“提示词”、“音效”等列，这样可以直接通过Python脚本解析并批量发送给绘图API1。

## **3\. 架构师代理：大纲提炼与世界观构建**

漫剧创作的第一步并非直接写剧本，而是从源文本（小说、构思）中提炼出结构化的骨架。这一阶段的系统提示词设计重点在于“信息压缩”与“视觉转译”。

### **3.1 世界观提取（World Building Extraction）**

当面对一部几十万字的小说时，我们需要AI从中提取出用于指导后续所有创作的“圣经”。

**系统提示词指导原则：**

1. **视觉优先原则：** 指令必须明确要求AI关注“可被视觉化”的信息。小说中关于历史背景的抽象描述（如“这是一个经历了千年战乱的帝国”）对画师意义不大，AI需要将其转化为视觉符号（“建筑风格为残破的哥特式尖塔，街道上随处可见生锈的战争残骸，市民穿着灰色的补丁衣服”）7。  
2. **核心标签提取（Core Tag Extraction）：** 对于角色，不能只提取性格，必须提取“视觉DNA”。系统提示词应要求输出适用于绘图模型的关键词串（Prompt Strings），如\`\`。这些标签将成为后续保持角色一致性的锚点1。  
3. **冲突与调性映射：** 要求AI分析故事的核心冲突（如“自然 vs 科技”），并定义相应的视觉隐喻（如“绿色的植物总是出现在画面左侧，冷蓝色的机械占据右侧”）。

**Prompt 示例片段：**

“你是一位资深的概念艺术家和世界观架构师。你的任务是阅读输入的小说文本，并构建一份可视化的\<series\_bible\>。请忽略角色的内心独白，专注于提取物理特征、环境细节和光影氛围。对于每个主要角色，生成一段固定的、不可变的视觉描述标签（Visual DNA），用于后续的AI绘画。”

### **3.2 叙事节拍与大纲提炼（Beat Sheet Generation）**

小说是流动的液体，而漫剧是固定的容器（画格）。大纲提炼的过程就是将液体通过“节拍器”固化为晶体的过程。

**系统提示词指导原则：**

1. **双阶段精炼（Dual-Stage Refinement）：** 强制模型先生成宏观的“故事弧光”（Story Arc），再细化为具体的“场面节拍”（Scene Beats）。禁止一次性生成详细脚本1。  
2. **翻页悬念设计（Page Turn Engineering）：** 漫剧（尤其是页漫）极其讲究“翻页体验”。系统提示词应指示AI识别故事中的惊奇、揭秘或高潮时刻，并将其强制安排在偶数页的最后一格（右下角），以此驱动读者进行翻页动作9。  
3. **动作原子化（Action Atomization）：** 小说中一句“他杀出重围”在漫剧里可能需要一整页的动作分镜。提示词需引导AI识别这种“高密度动作文本”，并将其“像慢动作回放一样”拆解为关键帧序列10。

**表格：大纲提炼的结构化输出要求**

| 维度 | 要求 | 目的 |
| :---- | :---- | :---- |
| **节拍类型** | 必须标注（如：激励事件、危机点、高潮） | 确保叙事结构符合经典剧作法（如《救猫咪》） |
| **情感电荷** | 标注每场戏开始与结束的情感极性（+/-） | 确保每场戏都有价值转折，避免流水账 1 |
| **视觉重点** | 提炼该节拍的核心视觉元素 | 指导后续分镜设计 |
| **预估格数** | 根据信息密度预估所需画格数量 | 控制节奏与篇幅 11 |

## **4\. 编剧代理：剧本生成与标准化**

一旦大纲确立，下一步是生成标准化的剧本。对于AI漫剧，我们推荐使用 **Fountain** 格式，因为它是一种纯文本标记语言，既易于LLM生成，又易于被专业软件识别。

### **4.1 格式即法律：Fountain语法强制**

系统提示词必须像定义编程语言语法一样定义剧本格式，任何格式错误都应被视为Bug。

**系统提示词指导原则：**

1. **严格的语法约束：** 明确规定场景标题（Scene Headings）必须全大写并以INT.或EXT.开头；角色名必须全大写居中；对话必须紧跟角色名。任何多余的Markdown符号（如加粗、斜体）除非用于强调特定的视觉重点，否则一律禁止1。  
2. **垂直流动感（Vertical Flow）：** 漫剧剧本的阅读体验应该是垂直流动的。提示词应限制动作描写段落（Action Blocks）的长度，规定“动作描写不得超过4行”，迫使AI将复杂的动作拆解为多个段落，这直接对应了漫剧的分镜节奏1。  
3. **禁止心理描写（Anti-Psychologizing）：** 这是AI写剧本最容易犯的错误。系统提示词必须包含强烈的负向约束：“严禁描写‘他感到悲伤’或‘他想起了童年’。必须将心理活动转化为可视化的物理动作（如‘他低头看着手中的旧怀表，拇指反复摩挲表盖’）。”这就是著名的“展示，不要讲述”（Show, Don't Tell）原则的工程化实现1。

**Prompt 示例片段：**

“**格式约束（STRICT FOUNTAIN SYNTAX）：**

1. 场景标题前必须有两个空行。  
2. 角色名必须全大写。  
3. 严禁使用心理动词（觉得、认为、感到）。  
4. 每一段动作描写代表一个独立的视觉镜头，长度不得超过3行。”

### **4.2 对话优化与“气泡物理学”**

漫剧的载体特性决定了文字量受到画面的严格限制。

**系统提示词指导原则：**

1. **呼吸原则（The Breath Rule）：** 指示AI，“一个气泡的文字量不应超过人一口气能说完的长度”。通常限制在25个单词（英文）或30个汉字以内。如果内容过多，必须指示AI将其拆分为多个气泡或连珠泡14。  
2. **潜台词注入（Subtext Injection）：** AI生成的对话往往过于直白（On-the-nose）。系统提示词应包含“潜台词策略”，要求角色“永远不要直接说出他们的真实意图，而是通过撒谎、回避、反讽或顾左右而言他来表达”。例如，将“我爱你”重写为“你走的时候，记得把伞带上”1。  
3. **信息密度控制：** 强制AI在每一页剧本后进行自我审查：“检查本页的对话总量是否遮挡了超过30%的画面面积？如果是，请精简对话或增加分镜页数。”17

## **5\. 视觉转译代理：分镜脚本与提示词工程**

这是连接文本与图像的关键桥梁。分镜师代理（Storyboard Agent）不负责编故事，只负责“翻译”——将文学语言翻译为摄影语言和AI绘图提示词。

### **5.1 视觉参数化与受控词汇表**

为了保证生成的图像具有电影感和专业度，系统提示词必须内置一个专业的摄影术语库，并强制AI从中选择参数。

**系统提示词指导原则：**

1. **参数拆解协议：** 要求AI将每一句剧本动作拆解为： \+ \[Action\] \+ \+ \[Camera Angle\] \+ \[Lighting\] \+。  
2. **受控词汇表（Controlled Vocabulary）：**  
   * **景别：** 必须使用 Extreme Long Shot (ELS) 交代环境，Medium Shot (MS) 表现关系，Close Up (CU) 表现情绪，Insert Shot 强调道具细节1。  
   * **角度：** 强制使用 Low Angle 赋予角色权威感，High Angle 表现脆弱，Dutch Angle 表现心理失衡或混乱1。  
   * **光影：** 引入 Chiaroscuro（明暗对比法）、Rim Lighting（轮廓光，用于分离主体）、Volumetric Lighting（体积光，增加氛围）等术语1。

### **5.2 多模态适配与平台语法**

不同的绘图模型（Midjourney, Stable Diffusion, Flux）有完全不同的“方言”。系统提示词需具备适配能力。

**Midjourney 适配指南：**

* **语法风格：** 偏好自然语言描述，强调形容词堆叠。  
* **权重分隔：** 强制使用 :: 分隔符来区分主体、环境和风格（如 Cyberpunk City :: K standing in rain :: Neon lights）。  
* **参数后缀：** 自动添加 \--ar \[比例\] \--v 6.0 \--stylize \[数值\]。提示词应根据画格的物理形状（横构图、竖构图）自动调整 \--ar 参数（如 16:9 或 9:16）1。

**Stable Diffusion / Flux 适配指南：**

* **语法风格：** 偏好标签（Tags）形式，逗号分隔。  
* **权重强调：** 使用括号语法 (keyword:1.2) 或 ((keyword)) 来强调核心元素。  
* **负向提示词（Negative Prompts）：** 系统提示词必须自动生成对应的负向提示词，如 bad anatomy, text, watermark, blurry, low quality，这对于开源模型至关重要1。

**Prompt 示例片段（Midjourney 模式）：**

“将以下剧本行翻译为Midjourney v6提示词。  
剧本：‘K抬头看着高耸的全息广告，雨水顺着他的鼻尖滴落。’  
翻译：Low angle close-up shot :: K looking up in awe :: Towering holographic geisha advertisement in background, neon pink and blue lighting :: Heavy rain, water droplets on face, cinematic lighting, shallow depth of field \--ar 2:3 \--v 6.0 \--stylize 250”

## **6\. 一致性监理代理：连续性与质量控制**

AI漫剧最大的痛点是“角色崩坏”和“风格漂移”。这需要一个专门的“连续性监理”机制嵌入到系统提示词中。

### **6.1 核心标签锁定（Core Tag Locking）**

这是解决角色一致性的终极方案。

**系统提示词指导原则：**

1. **不可变性原则：** 系统提示词应在对话开始时就锁定每个角色的“Core Tag String”。例如：\`\`。  
2. **强制插入：** 无论剧情如何变化，指令必须要求AI在生成任何包含该角色的分镜提示词时，**逐字逐句**地插入这段Core Tag String，严禁修改、同义词替换或重新排序。这是因为AI绘图模型对词序非常敏感，微小的变动都可能导致人脸变化1。  
3. **Seed与Reference管理：** 对于支持参考图的模型（如MJ的--cref或SD的ControlNet），系统提示词应维护一个URL列表，并在生成提示词时自动附带 \--cref 参数23。

### **6.2 逻辑自检与循环优化（Refinement Loops）**

AI生成的初稿往往存在逻辑漏洞或视觉无法实现的问题。我们需要在系统提示词中植入“自我反思”的机制。

**系统提示词指导原则：**

* **元认知提示（Metacognitive Prompting）：** 要求模型在输出最终结果前，先进行自我批判。  
  * “检查：这个分镜中的动作是否符合物理定律？”  
  * “检查：角色A的情绪反应是否连贯接续上一格？”  
  * “检查：是否存在‘穿帮’（如古代场景出现了手机）？”  
* **修正循环：** 如果发现问题，模型应自动进行修正，而不是直接输出错误结果。例如：“自我修正：原剧本中K在雨中点烟，考虑到暴雨环境，修改为K在屋檐下避雨点烟。”1。

## **7\. 高级工作流：多智能体编排（Multi-Agent Orchestration）**

在实际的工程实践中，我们通常不会用一个Prompt完成所有工作，而是构建一个Agentic Workflow（代理工作流）。

### **7.1 总导演代理（The Orchestrator / Showrunner）**

这是一个元代理（Meta-Agent），它的系统提示词不涉及具体创作，而是负责调度。

**系统提示词结构：**

1. **接收用户意图：** 这是一个关于赛博朋克复仇的故事。  
2. **调用架构师：** 生成世界观和人物小传。  
3. **调用编剧：** 基于世界观写出第一场戏的Fountain剧本。  
4. **调用监理：** 检查剧本是否符合世界观（如是否有违禁的纸质书）。  
5. **调用分镜师：** 将审核通过的剧本转化为Midjourney提示词列表。  
6. **最终输出：** 打包所有资产。

这种分工机制模仿了真实的影视工业流水线，确保了每个环节的专业深度，规避了单一模型上下文过载导致的逻辑崩坏1。

## **8\. 结论与未来展望**

AI漫剧的系统提示词设计，本质上是一门**计算剧作法（Computational Dramaturgy）**。它要求创作者同时具备编剧的感性与工程师的理性。

通过本报告构建的体系：

* **结构化**保证了输出的可用性（Fountain/JSON）；  
* **思维链**提升了叙事的逻辑性（节拍/冲突分析）；  
* **受控词汇**确保了视觉的专业性（镜头语言）；  
* **核心标签锁定**解决了生成式AI的一致性难题。

二阶与三阶洞察：  
随着这套工程方法的普及，我们可能会看到\*\*“提示词原生”（Prompt-Native）流派\*\*的诞生。这类漫剧不再试图模仿传统手绘漫画的细腻笔触，而是利用AI“幻觉”的特性，创造出梦境逻辑叙事或超高频快节奏的视觉流。同时，提示词工程师将进化为新时代的“作者导演”（Auteur），他们不直接画线稿，而是通过编写精密的代码（提示词）来构建宇宙的物理法则与视觉美学。这不仅是生产力的提升，更是艺术创作本体论的一次深刻重构。  
引用索引：  
.4

#### **引用的著作**

1. 剧本创作Prompt指导原则研究  
2. Prompt Engineering for Generative AI | Machine Learning \- Google for Developers, 访问时间为 一月 3, 2026， [https://developers.google.com/machine-learning/resources/prompt-eng](https://developers.google.com/machine-learning/resources/prompt-eng)  
3. Effective context engineering for AI agents \- Anthropic, 访问时间为 一月 3, 2026， [https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)  
4. Prompt design strategies | Gemini API | Google AI for Developers, 访问时间为 一月 3, 2026， [https://ai.google.dev/gemini-api/docs/prompting-strategies](https://ai.google.dev/gemini-api/docs/prompting-strategies)  
5. Simplify Information Extraction: A Reusable Prompt Template for GPT Models, 访问时间为 一月 3, 2026， [https://towardsdatascience.com/simplify-information-extraction-a-reusable-prompt-template-for-gpt-models-d6d5f1bd25a0/](https://towardsdatascience.com/simplify-information-extraction-a-reusable-prompt-template-for-gpt-models-d6d5f1bd25a0/)  
6. From Panels to Prose: Generating Literary Narratives from Comics \- CVF Open Access, 访问时间为 一月 3, 2026， [https://openaccess.thecvf.com/content/ICCV2025/papers/Sachdeva\_From\_Panels\_to\_Prose\_Generating\_Literary\_Narratives\_from\_Comics\_ICCV\_2025\_paper.pdf](https://openaccess.thecvf.com/content/ICCV2025/papers/Sachdeva_From_Panels_to_Prose_Generating_Literary_Narratives_from_Comics_ICCV_2025_paper.pdf)  
7. The Essential Guide to Worldbuilding \[from Book Editors\] \- PaperTrue, 访问时间为 一月 3, 2026， [https://www.papertrue.com/blog/worldbuilding/](https://www.papertrue.com/blog/worldbuilding/)  
8. How to Keep Characters Consistent Across AI Scenes: Working Prompt Patterns (2025), 访问时间为 一月 3, 2026， [https://skywork.ai/blog/how-to-consistent-characters-ai-scenes-prompt-patterns-2025/](https://skywork.ai/blog/how-to-consistent-characters-ai-scenes-prompt-patterns-2025/)  
9. Mastering Page Turns: Elevate Your Comic Book Storytelling \- Metal Ninja Studios, 访问时间为 一月 3, 2026， [https://metalninjastudios.com/blogs/the-forge-of-ren/mastering-page-turns-comic-production](https://metalninjastudios.com/blogs/the-forge-of-ren/mastering-page-turns-comic-production)  
10. Breaking Scenes Down \- Evan Waterman, 访问时间为 一月 3, 2026， [https://evanjwaterman.com/guide/writing/breaking-scenes-down/](https://evanjwaterman.com/guide/writing/breaking-scenes-down/)  
11. How do you decide how to pace your webcomic? : r/WebtoonCanvas \- Reddit, 访问时间为 一月 3, 2026， [https://www.reddit.com/r/WebtoonCanvas/comments/1eziznk/how\_do\_you\_decide\_how\_to\_pace\_your\_webcomic/](https://www.reddit.com/r/WebtoonCanvas/comments/1eziznk/how_do_you_decide_how_to_pace_your_webcomic/)  
12. How To Format A Comic Book Script | by Kelly Bender \- Medium, 访问时间为 一月 3, 2026， [https://medium.com/@KellyBender17/how-to-format-a-comic-book-script-09dd8c019e7a](https://medium.com/@KellyBender17/how-to-format-a-comic-book-script-09dd8c019e7a)  
13. 4 Quick Tips to Adapt Your Screenplay into a Graphic Novel Script | Tim Stout, 访问时间为 一月 3, 2026， [https://timstout.wordpress.com/2014/06/16/4-quick-tips-to-adapt-your-screenplay-into-a-graphic-novel-script/](https://timstout.wordpress.com/2014/06/16/4-quick-tips-to-adapt-your-screenplay-into-a-graphic-novel-script/)  
14. How do you guys split dialog? : r/ComicBookCollabs \- Reddit, 访问时间为 一月 3, 2026， [https://www.reddit.com/r/ComicBookCollabs/comments/1fos3rv/how\_do\_you\_guys\_split\_dialog/](https://www.reddit.com/r/ComicBookCollabs/comments/1fos3rv/how_do_you_guys_split_dialog/)  
15. The Art of Conversation Part 2, representing dialogue in comics. \- Richard Mooney, 访问时间为 一月 3, 2026， [https://richardmooneyvi.wordpress.com/2020/10/21/the-art-of-conversation-part-2-representing-dialogue-in-comics/](https://richardmooneyvi.wordpress.com/2020/10/21/the-art-of-conversation-part-2-representing-dialogue-in-comics/)  
16. How I Use AI to Write Dialogue That Doesn't Sound Like a Chatbot Having Feelings, 访问时间为 一月 3, 2026， [https://medium.com/@ajtracysk/how-i-use-ai-to-write-dialogue-that-doesnt-sound-like-a-chatbot-having-feelings-5fe9df77a353](https://medium.com/@ajtracysk/how-i-use-ai-to-write-dialogue-that-doesnt-sound-like-a-chatbot-having-feelings-5fe9df77a353)  
17. How to Plan Panel Layouts for Manga Storyboarding \- Story Boards AI, 访问时间为 一月 3, 2026， [https://www.story-boards.ai/content-hub/blog/how-to-plan-panel-layouts-for-manga-storyboarding](https://www.story-boards.ai/content-hub/blog/how-to-plan-panel-layouts-for-manga-storyboarding)  
18. Pro Artist's Guide to Comic & Manga Layouts, Paneling, Flow | Art Rocket, 访问时间为 一月 3, 2026， [https://www.clipstudio.net/how-to-draw/archives/160963](https://www.clipstudio.net/how-to-draw/archives/160963)  
19. Ultimate Guide to Creating Consistent Characters with AI, 访问时间为 一月 3, 2026， [https://consistentcharacter.ai/blog/ultimate-guide-to-creating-consistent-characters/](https://consistentcharacter.ai/blog/ultimate-guide-to-creating-consistent-characters/)  
20. Prompting Techniques for Stable Diffusion \- MachineLearningMastery.com, 访问时间为 一月 3, 2026， [https://machinelearningmastery.com/prompting-techniques-stable-diffusion/](https://machinelearningmastery.com/prompting-techniques-stable-diffusion/)  
21. Aspect Ratio \- Midjourney, 访问时间为 一月 3, 2026， [https://docs.midjourney.com/hc/en-us/articles/31894244298125-Aspect-Ratio](https://docs.midjourney.com/hc/en-us/articles/31894244298125-Aspect-Ratio)  
22. Civitai's Prompt-Crafting Guide: Part 1 \- Basics, 访问时间为 一月 3, 2026， [https://education.civitai.com/civitais-prompt-crafting-guide-part-1-basics/](https://education.civitai.com/civitais-prompt-crafting-guide-part-1-basics/)  
23. Character Reference \- Midjourney, 访问时间为 一月 3, 2026， [https://docs.midjourney.com/hc/en-us/articles/32162917505293-Character-Reference](https://docs.midjourney.com/hc/en-us/articles/32162917505293-Character-Reference)  
24. Sora 2 Prompting Guide | OpenAI Cookbook, 访问时间为 一月 3, 2026， [https://cookbook.openai.com/examples/sora/sora2\_prompting\_guide](https://cookbook.openai.com/examples/sora/sora2_prompting_guide)  
25. Parameter List \- Midjourney, 访问时间为 一月 3, 2026， [https://docs.midjourney.com/hc/en-us/articles/32859204029709-Parameter-List](https://docs.midjourney.com/hc/en-us/articles/32859204029709-Parameter-List)