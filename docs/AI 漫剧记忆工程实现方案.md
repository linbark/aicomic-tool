# **计算剧作法：AI漫剧创作全流程中的记忆工程架构深度研究报告**

## **1\. 绪论：生成式叙事的工程化范式转移与记忆危机**

在人工智能与创意产业深度融合的当下，我们正在见证从“辅助创作”向“代理创作”的范式转移。AI漫剧（AI Motion Comics/Manju）作为一种新兴的媒介形式，其制作流程不再是孤立的文本生成或图像生成，而是一个精密耦合的工程系统。在这个系统中，大型语言模型（LLM）不仅是文本的生成器，更是整个叙事世界的“认知引擎”。然而，随着漫剧创作向长篇幅、复杂剧情和连续性视觉表现发展，现有的生成式模型面临着一个根本性的认知瓶颈——**无状态性（Statelessness）** 1。

尽管GPT-4、Gemini 1.5 Pro等前沿模型拥有令人惊叹的长上下文窗口（Context Window），它们在本质上仍然是无状态的推理机。每一次API调用对于模型而言都是一次全新的“初生”，除非我们在提示词（Prompt）中显式地注入历史信息，否则模型无法保留对前序剧情、角色设定或视觉风格的记忆 3。在一部包含数十个章节、上百个场景、数千个分镜的漫剧制作中，这种健忘症会导致灾难性的后果：主角的性格在第5章突然改变，核心道具在第10章凭空消失，或者画面的美术风格从赛博朋克漂移至水彩画风。这种现象被称为“上下文腐烂”（Context Rot）或“叙事幻觉” 1。

为了解决这一问题，单纯依赖模型的原生上下文窗口是不可持续的，无论是在经济成本上还是在注意力机制的精度上。我们需要引入\*\*记忆工程（Memory Engineering）\*\*的概念，构建一套混合认知架构。本报告将深入剖析两种核心记忆机制——**缓冲记忆（Buffer Memory）与向量存储（Vector Store）**——在AI漫剧创作中的理论基础与工程实现。我们将论证，只有通过精细编排这两种记忆系统，结合结构化的“世界观设定集”（Series Bible）和多智能体协作网络，才能实现专业级的、具有长期连贯性的AI漫剧创作 1。本报告旨在为该领域的从业者提供一份详尽的技术指南与方法论框架。

## ---

**2\. 机器记忆的二元论：理论框架与技术解构**

在构建AI代理（AI Agent）的认知架构时，记忆并非单一的数据存储，而是一个分层的检索与处理系统。理解**缓冲记忆**与**向量存储**在技术机制、检索逻辑及认知功能上的本质差异，是设计漫剧工作流的前提。这不仅仅是数据库的选择问题，更是对人类“工作记忆”与“长期记忆”的仿生学重构。

### **2.1 缓冲记忆（Buffer Memory）：线性流动的短期意识**

技术定义与运行机制  
缓冲记忆，在LangChain等框架中通常体现为ConversationBufferMemory或其变体，代表了AI代理的“意识流”或“工作记忆” 4。从数据结构的角度看，它是一个线性增长的列表（List），按时间顺序存储了用户输入（User Input）与AI输出（AI Response）的原始文本对。

* **先进先出（FIFO）与滑动窗口：** 由于LLM的上下文窗口是有限的（尽管在不断扩大，但仍受限于推理成本与注意力分散），缓冲记忆通常采用滑动窗口机制（Sliding Window）。当对话轮数超过设定的阈值$K$时，最旧的交互记录将被移除（Eviction），以便为新的交互腾出空间 5。  
* **全量注入：** 在每一次生成请求中，缓冲窗口内的所有内容都会被作为Prompt的一部分，完整地注入到模型的Context中。这意味着模型能够“看到”最近发生的对话细节，从而保持语气、指代关系和逻辑流的连贯性 3。

在漫剧场景中的认知角色  
在漫剧创作的具体场景中，缓冲记忆负责维护场景内的连贯性（Intra-scene Continuity）。

* **对话流（Dialogue Flow）：** 当主角K在分镜A中问“那是谁？”，在分镜B中配角回答“是敌人”。这种紧密的问答逻辑依赖于缓冲记忆。如果缓冲记忆缺失，配角的回答可能会变得毫无上下文，比如突然开始自我介绍。  
* **情绪惯性（Emotional Inertia）：** 如果K在上一句台词中表现出愤怒，缓冲记忆确保他在下一句台词中不会毫无理由地变得平静。它维持了叙事的“热度”和即时状态 7。

局限性分析：金鱼效应  
缓冲记忆的致命弱点在于其容量限制。对于长篇漫剧而言，这被称为“金鱼效应”（Goldfish Effect）。一旦某个关键情节（例如：第一章中K把一把枪藏在了靴子里）滑出了缓冲窗口，对于模型而言，这个事实就不复存在了。在第十章的战斗场景中，即便K面临生死关头，模型也不会让他拔出那把枪，因为在它的“工作记忆”中，那把枪从未出现过 8。

### **2.2 向量存储（Vector Store）：语义关联的长期档案**

技术定义与运行机制  
向量存储（Vector Database）是AI代理的“长期记忆”或“语义记忆”。它不直接存储文本字符串，而是存储文本的嵌入（Embeddings）——即通过Embedding Model（如OpenAI text-embedding-3或HuggingFace BGE）转化而成的高维浮点数向量（Vector） 10。

* **语义检索（Semantic Retrieval）：** 当系统需要查询信息时，它不会进行关键词匹配（Keyword Matching），而是计算查询向量与库中存储向量之间的距离（通常使用余弦相似度 Cosine Similarity）。这意味着，即使用户查询“K的防御武器”，系统也能检索到存储内容为“K在左靴藏了一把微型激光手枪”的记录，因为两者在语义空间中高度接近 13。  
* **无限容量与非线性访问：** 理论上，向量存储的容量仅受限于物理存储空间。更重要的是，它允许**非线性访问**。无论信息是在第1章还是第100章写入的，只要它与当前的上下文相关，就能被瞬间召回。

在漫剧场景中的认知角色  
向量存储在漫剧创作中负责维护跨场景的连贯性（Inter-scene Consistency）和世界观的一致性（World Consistency）。

* **事实锚定：** 无论剧情发展到哪里，K的瞳孔颜色、所属帮派的历史、世界的物理法则（如“低重力环境”）都存储在向量库中。当需要生成K的外貌描述时，系统从向量库中检索这些静态事实，而不是依赖不可靠的短期记忆 1。  
* **伏笔回收：** 向量存储允许系统在适当的时候“回忆”起久远的细节，从而实现复杂的叙事结构。

局限性分析：语义幻觉与时序丢失  
向量存储并非完美。其主要缺陷在于：

1. **时序信息的丢失：** 向量空间是扁平的。系统可能检索到“K杀死了反派”和“反派威胁了K”两个片段，但无法仅凭向量距离判断哪个先发生。这在处理角色成长弧光时尤为棘手 15。  
2. **语义幻觉（Retrieval Hallucination）：** 如果查询不够精确，系统可能检索到语义相似但不相关的片段。例如，查询“K的爱人”可能错误地检索到“K杀死的那个长得像他爱人的敌人” 16。

### **2.3 比较分析与工程权衡**

为了更直观地理解两者的差异，我们通过下表进行深度对比：

| 维度 | 缓冲记忆 (Buffer Memory) | 向量存储 (Vector Store) |
| :---- | :---- | :---- |
| **核心机制** | 序列化文本存储，滑动窗口截断 | 高维向量嵌入，近似最近邻搜索 (ANN) |
| **检索逻辑** | 线性读取，全量注入当前上下文 | 基于语义相似度 (Similarity) 的按需召回 |
| **时间敏感性** | 极高 (Recency Bias)，保留时序 | 较低，默认无时序 (需元数据辅助) |
| **容量限制** | 受限于 LLM Token Window (如 8k-128k) | 理论无限 (取决于磁盘/云存储) |
| **计算成本** | 随对话长度增加呈线性/二次方增长 | 检索成本低，但需预先计算 Embedding |
| **漫剧应用** | 维持当前场景对话流、动作连贯性 | 存储角色设定、世界观规则、历史剧情 |
| **典型缺陷** | 遗忘早期关键信息 (Context Rot) | 检索精度波动，缺乏叙事因果链 |

**二阶洞察：** 在漫剧工程中，我们不能做“二选一”的选择题。真正的挑战在于**编排（Orchestration）**。一个成熟的漫剧AI Agent必须同时拥有这两种记忆：用Buffer处理“当下”，用Vector锚定“永恒”。如果没有Buffer，对话将变得机械且缺乏语境；如果没有Vector，故事将分崩离析，变成一堆毫无关联的碎片 9。

## ---

**3\. 漫剧场景下的Buffer Memory实现：构建“工作台”**

在漫剧的具体生产流程中，Buffer Memory不仅仅是聊天记录。它需要被设计为编剧和分镜师的“工作台”，在这个工作台上，最近的叙事动作、未完成的逻辑链条和当前的情感基调被暂时存放。

### **3.1 窗口策略与摘要增强**

对于漫剧创作，简单的ConversationBufferWindowMemory（仅保留最后N条）往往是不够的。漫剧的一个场景（Scene）可能包含数十个来回的交互（修改台词、调整分镜、细化动作）。如果窗口设置得太小（如k=5），编剧Agent可能会忘记场景开始时设定的“天气是暴风雨”，导致结尾时角色突然在阳光下行走。

工程指导原则：摘要缓冲混合策略 (Summary-Buffer Hybrid)  
我们建议采用ConversationSummaryBufferMemory策略 6。

* **实现逻辑：** 系统在内存中维护两个区域。一个是“热数据区”，存储最近的$N$条完整交互（例如最近10个分镜的详细描述）；另一个是“冷数据区”，存储$N$条之前的交互的**摘要（Summary）**。  
* **漫剧应用：** 当创作第20个分镜时，Buffer向LLM提供的上下文是：“\[摘要：K潜入了基地，解决了一名守卫，目前正躲在通风管道里\] \+ \[最近5个分镜的详细Prompt：K的动作、表情、光影参数\]”。  
* **优势：** 这种混合策略既保证了对当前微观动作的精确控制（通过最近的记录），又保留了宏观的场景状态（通过摘要），且不会撑爆Token限制。

### **3.2 链式思维（CoT）的暂存与清洗**

在生成复杂的剧本时，我们往往要求AI进行“链式思维”（Chain-of-Thought），即先思考再输出。例如：“思考：K此时应该感到恐惧还是愤怒？基于他的性格...” 1。

工程指导原则：暂存区管理 (Scratchpad Management)  
这些中间的推理过程属于Buffer Memory的范畴，但不应被永久存储。

* **实现：** 在Buffer中应当划分“临时思维链”区域。当模型完成最终输出（如确定的台词）后，应当有一个**清洗机制（Sanitization Protocol）**，将繁琐的推理过程从Buffer中移除，只保留最终的叙事结果。  
* **目的：** 防止Buffer被大量的推理噪声填满，确保上下文窗口的信噪比（Signal-to-Noise Ratio）。如果在Buffer中保留过多的推理过程，模型在后续生成中可能会产生“过度反思”的倾向，导致输出变得犹豫不决或充满解释性文字，破坏剧本的直接性。

### **3.3 状态传递与多智能体协作**

在LangGraph等多智能体架构中，Buffer Memory实际上是作为\*\*图状态（Graph State）\*\*在不同节点间传递的 19。

**漫剧应用示例：**

1. **编剧Agent**生成了一段文本描述（存入Buffer）。  
2. **分镜Agent**读取Buffer中的文本，将其转化为图像Prompt（存入Buffer）。  
3. **校验Agent**读取Buffer中的Prompt，检查是否违背了设定（读取Vector Store进行比对），并将修改意见写回Buffer。

在这种流转中，Buffer不仅是记忆，更是**通信协议**。设计Buffer结构时，必须采用结构化数据（如JSON对象），明确区分script\_text、visual\_prompt、feedback\_log等字段，而不仅仅是堆砌纯文本字符串。

## ---

**4\. 漫剧场景下的Vector Store实现：构建“世界观圣经”**

如果说Buffer是流动的工作台，那么Vector Store就是漫剧的“圣经”（Series Bible）——一个绝对真理的静态与动态结合的存储库。在AI漫剧创作中，Vector Store的实现远比通用的RAG（检索增强生成）要复杂，它需要处理高度结构化的数据和多模态信息。

### **4.1 “世界观设定集”的结构化向量化**

传统的RAG通常是对非结构化文本（如PDF文档）进行切片（Chunking）。但在漫剧创作中，如果简单地将一本小说切成500字的片段存入向量库，效果会非常差。因为角色的描述可能散落在书的各个角落，单一的切片无法概括角色的全貌。

工程指导原则：语义切片与元数据增强 (Semantic Chunking & Metadata Enrichment)  
我们需要构建一个结构化的世界观设定集（Series Bible），并对其进行特殊的向量化处理 1。  
推荐的Schema设计：  
不要存储大段文本，而是存储带标签的原子化知识单元。

| 字段 (Field) | 类型 | 示例值 | 用途 |
| :---- | :---- | :---- | :---- |
| id | UUID | char\_k\_001 | 唯一标识 |
| content | Text | "K有着银色短发，左眼是红色的赛博义眼，身穿黑色战术风衣。" | 用于生成Embedding，进行语义检索 |
| metadata.type | String | character\_design | **过滤器：** 确保画图时只检索视觉设定，不检索剧情 |
| metadata.entity | String | K | **实体锚点：** 锁定特定角色 |
| metadata.visual\_dna | String | silver hair, red cybernetic eye, tactical trench coat, techwear | **核心标签锁定：** 直接注入绘图Prompt 1 |
| metadata.negative | String | smile, warm colors, sunshine, beard | **负向约束：** 注入Negative Prompt 1 |
| metadata.ref\_img | URL | s3://assets/k\_ref.png | **视觉RAG：** 用于ControlNet或Reference输入 21 |

**实现细节：**

* **语义分块：** 使用Markdown标题或特定的分隔符将设定集切分为“角色”、“场景”、“道具”、“规则”等独立块。不要让“K的外貌”和“K的背景故事”混在一个Chunk里 22。  
* **元数据注入：** 在写入向量库时，必须将visual\_dna（视觉DNA）作为独立的元数据字段存储。这串标签是经过人工精调的Prompt，它是**不可变**的。当系统检索到关于K的信息时，它不仅仅拿到一段描述文本，而是直接拿到了这串可以直接发送给Midjourney的代码 1。

### **4.2 视觉DNA的锁定与多模态检索**

在漫剧创作中，角色一致性是最大的痛点。Vector Store必须支持**多模态锚定**。

工程指导原则：引用图管理 (Reference Image Management)  
向量库不应只存文本。对于每一个主要角色和关键场景，元数据中必须包含标准参考图（Character Sheet）的URL。

* **检索流程：**  
  1. 用户指令：“生成K在雨中行走的画面”。  
  2. Vector检索：查询entity:K且type:character\_design。  
  3. 返回结果：包含文本描述（Prompt）和参考图URL（ref\_img）。  
  4. Prompt组装：系统自动将参考图URL作为--cref（Character Reference）参数或ControlNet输入，传递给图像生成模型。  
* 这种“视觉RAG”机制从根本上解决了角色脸部崩坏的问题，确保了第1话和第50话的主角长得一样 21。

### **4.3 负向约束与知识边界的存储**

定义“角色不是什么”往往比“角色是什么”更重要。例如，一个维多利亚时代的故事绝对不能出现手机。

**工程指导原则：负向知识库 (Negative Constraints Store)**

* **存储：** 建立专门的world\_rules集合，存储如“本世界没有电力”、“K从不笑”等规则。  
* **检索与应用：** 在生成每一个Prompt之前，系统应进行一次“约束检索”。如果场景涉及“通讯”，系统检索到“本世界没有电力”，则Consistency Supervisor（一致性监理）会强制LLM修改“打电话”的情节为“写信”或“派信鸽”。  
* **负向提示词：** 将检索到的metadata.negative直接填入绘图工具的Negative Prompt区域（如mobile phone, modern car, electricity），从像素层面封堵逻辑漏洞 1。

## ---

**5\. 记忆工程的高级指导原则：构建混合架构**

基于上述分析，我们提出在AI漫剧领域实施记忆工程的**七大指导原则**。这些原则构成了“计算剧作法”的核心。

### **原则一：不可变视觉核心原则 (The Principle of Immutable Visuals)**

核心思想： 视觉一致性是非概率性的。角色的长相不能交给LLM去“发挥”。  
实施指南： 在Vector Store中存储固定的Core Tag String（如\<K\_Visual\_Core\>）。系统提示词必须强制规定：在生成该角色的任何分镜时，必须逐字逐句地插入这段Tag，严禁同义词替换或语序调整。因为AI绘图模型对词序极度敏感，微小的文本变动会导致人脸特征的漂移 1。

### **原则二：分层检索原则 (The Principle of Hierarchical Retrieval)**

核心思想： 不同的叙事任务需要不同层级的记忆。  
实施指南： 建立三级检索架构 26：

1. **L1 场景级（Buffer）：** 最近10句对话。用于处理当前的交互反应。  
2. **L2 章节级（Vector \- Summary）：** 当前章节的故事大纲和已发生事件的摘要。用于保持章节内的逻辑流。  
3. L3 世界级（Vector \- Static）： 不可变的世界观设定、物理法则、角色档案。用于底层的逻辑约束和视觉锚定。  
   在生成时，系统应同时从这三层获取信息，并通过Prompt Template将它们组合（如：System Prompt \+ L3 Rules \+ L2 Summary \+ L1 Recent History）。

### **原则三：情节压缩与写入原则 (The Principle of Episodic Compression)**

核心思想： 不要把所有流水账都存入长期记忆，只存储关键状态的改变。  
实施指南： 在每个场景（Scene）结束时，触发一个总结代理（Summarizer Agent）。它不应该只概括剧情，而应该提取状态变更（State Changes）。

* *错误示范：* “K和敌人打了很久，最后赢了。”  
* 正确示范： “事件：K击败帮派头目。状态变更：K获得关键道具【加密芯片】；K左臂受伤。”  
  将这种结构化的状态变更存入Vector Store，以便后续章节能够准确检索到“K有芯片”和“K受了伤”这两个事实 5。

### **原则四：状态隔离原则 (The Principle of State Segregation)**

核心思想： 将“世界是什么样”（静态）与“发生了什么”（动态）物理隔离。  
实施指南： 在Vector Store中使用不同的Namespaces（命名空间）或Collections。

* static\_bible：只读，存储设定。  
* dynamic\_plot：读写，存储剧情发展。  
  这种隔离防止了剧情的临时发展污染了底层的世界观设定，同时也方便创作者随时手动修正剧情走向而不破坏基础设定 29。

### **原则五：混合检索与最大边际相关性 (Hybrid Retrieval & MMR)**

核心思想： 避免检索到重复冗余的信息，追求信息的多样性。  
实施指南： 单纯的向量相似度搜索往往会返回几条极其相似的记录（例如5条都关于K的外貌）。在漫剧创作中，我们可能同时需要：K的外貌、当前的心理状态、所在环境的特征。

* **应用MMR（Maximal Marginal Relevance）：** 在检索时启用MMR算法，它不仅考虑结果与查询的相似度，还考虑结果之间的差异性。这确保检索出的上下文覆盖了外貌、剧情、环境等多个维度，为LLM提供全面的创作素材 31。

### **原则六：逻辑自检与循环优化 (Refinement Loops)**

核心思想： 生成即错误，记忆即校验。  
实施指南： 引入元认知提示（Metacognitive Prompting）和一致性监理代理。在内容输出给用户之前，监理代理会拿着Vector Store中的“真理”去核对生成内容。

* Check： “分镜中出现了阳光，但数据库显示本世界无阳光。” \-\> Action： 自动修正为“阴暗的天空”。  
  记忆不仅仅是用来生成的素材，更是用来拒绝错误生成的防火墙 1。

### **原则七：显式状态传递 (Explicit State Passing)**

核心思想： 在多智能体工作流中，不要假设记忆会自动同步。  
实施指南： 在LangGraph等架构中，必须显式定义一个全局的AgentState对象，包含messages（Buffer）和retrieved\_docs（Vector Context）。这个状态包需要在编剧、分镜、绘图等不同Agent之间显式传递。分镜Agent不需要重新检索K的身高，它应该直接从上游传递过来的State中读取由编剧Agent检索并锁定的数据。这减少了检索开销，保证了链路的一致性 19。

## ---

**6\. 结论：通向计算剧作法的未来**

AI漫剧的创作不再是简单的“提示词抽卡”，而是一场精密的数据工程。通过将**Buffer Memory**的短期连贯性与**Vector Store**的长期语义锚定相结合，我们构建了一个具有认知韧性的创作系统。

在这个系统中，**Series Bible**不再是躺在文件夹里的PDF，它被液化、切片、向量化，变成了AI创作过程中的实时约束流；**角色**不再是概率生成的幻影，而是由不可变的**Visual DNA**和**语义记忆**构成的持久实体。

这种从“无状态生成”到“有状态工程”的跨越，正是\*\*计算剧作法（Computational Dramaturgy）\*\*的核心所在。它将人类创作者从繁琐的一致性维护中解放出来，专注于更高维度的叙事构架与审美决策，真正实现了人机协同的艺术创造。

---

**关键引用索引：**

* **无状态性与上下文危机：** 1  
* **世界观设定集与Visual DNA：** 1  
* **Buffer Memory机制与策略：** 4  
* **Vector Store机制与RAG：** 10  
* **多智能体编排与LangGraph：** 1  
* **高级检索策略（MMR/Hybrid）：** 14  
* **记忆工程架构与隐私/安全：** 35

#### **引用的著作**

1. AI 漫剧创作 Prompt 指导原则  
2. How AI Agents Evolved and What's Next | by Kushal Banda \- Towards AI, 访问时间为 一月 7, 2026， [https://pub.towardsai.net/evolution-of-ai-agents-39a14b54dccc](https://pub.towardsai.net/evolution-of-ai-agents-39a14b54dccc)  
3. The “Illusion of Memory”: A Developer's Guide to Stateless AI | by Daman Pal Singh Khanna, 访问时间为 一月 7, 2026， [https://medium.com/@deepeesingh/the-illusion-of-memory-a-developers-guide-to-stateless-ai-82c3b125dd1b](https://medium.com/@deepeesingh/the-illusion-of-memory-a-developers-guide-to-stateless-ai-82c3b125dd1b)  
4. Different Types of Memory | Towards AI, 访问时间为 一月 7, 2026， [https://towardsai.net/p/l/different-types-of-memory](https://towardsai.net/p/l/different-types-of-memory)  
5. Agent Memory: How to Build Agents that Learn and Remember \- Letta, 访问时间为 一月 7, 2026， [https://www.letta.com/blog/agent-memory](https://www.letta.com/blog/agent-memory)  
6. Memory in LangChain \- GeeksforGeeks, 访问时间为 一月 7, 2026， [https://www.geeksforgeeks.org/artificial-intelligence/memory-in-langchain-1/](https://www.geeksforgeeks.org/artificial-intelligence/memory-in-langchain-1/)  
7. Types of LangChain Memory and How to Use Them \- ProjectPro, 访问时间为 一月 7, 2026， [https://www.projectpro.io/article/langchain-memory/1161](https://www.projectpro.io/article/langchain-memory/1161)  
8. Vector Store Memory in LangChain \- GeeksforGeeks, 访问时间为 一月 7, 2026， [https://www.geeksforgeeks.org/artificial-intelligence/vector-store-memory-in-langchain/](https://www.geeksforgeeks.org/artificial-intelligence/vector-store-memory-in-langchain/)  
9. From Goldfish to Elephant: How Vector Database Memory Makes AI Agents Actually Smart | by Tarush Singh | Medium, 访问时间为 一月 7, 2026， [https://medium.com/@singh.tarus/from-goldfish-to-elephant-how-vector-database-memory-makes-ai-agents-actually-smart-edf572061c30](https://medium.com/@singh.tarus/from-goldfish-to-elephant-how-vector-database-memory-makes-ai-agents-actually-smart-edf572061c30)  
10. How AI Agents Remember Things: The Role of Vector Stores in LLM Memory, 访问时间为 一月 7, 2026， [https://www.freecodecamp.org/news/how-ai-agents-remember-things-vector-stores-in-llm-memory/](https://www.freecodecamp.org/news/how-ai-agents-remember-things-vector-stores-in-llm-memory/)  
11. How AI Agents Remember Things: The Role of Vector Stores in LLM Memory \- TuringTalks, 访问时间为 一月 7, 2026， [https://www.turingtalks.ai/p/how-ai-agents-remember-things-the-role-of-vector-stores-in-llm-memory](https://www.turingtalks.ai/p/how-ai-agents-remember-things-the-role-of-vector-stores-in-llm-memory)  
12. What is a Vector Database? \- Qdrant, 访问时间为 一月 7, 2026， [https://qdrant.tech/articles/what-is-a-vector-database/](https://qdrant.tech/articles/what-is-a-vector-database/)  
13. Vector Databases: Tutorial, Best Practices & Examples \- Nexla, 访问时间为 一月 7, 2026， [https://nexla.com/ai-infrastructure/vector-databases/](https://nexla.com/ai-infrastructure/vector-databases/)  
14. Logical Consistency is Vital: Neural-Symbolic Information Retrieval for Negative-Constraint Queries \- arXiv, 访问时间为 一月 7, 2026， [https://arxiv.org/html/2505.22299v1](https://arxiv.org/html/2505.22299v1)  
15. Understanding Long Videos via LLM-Powered Entity Relation Graphs \- arXiv, 访问时间为 一月 7, 2026， [https://arxiv.org/html/2501.15953v1](https://arxiv.org/html/2501.15953v1)  
16. Retrieval-Augmented Generation: A Comprehensive Survey of Architectures, Enhancements, and Robustness Frontiers \- arXiv, 访问时间为 一月 7, 2026， [https://arxiv.org/html/2506.00054v1](https://arxiv.org/html/2506.00054v1)  
17. Beyond Vector Databases: Architectures for True Long-Term AI Memory, 访问时间为 一月 7, 2026， [https://vardhmanandroid2015.medium.com/beyond-vector-databases-architectures-for-true-long-term-ai-memory-0d4629d1a006](https://vardhmanandroid2015.medium.com/beyond-vector-databases-architectures-for-true-long-term-ai-memory-0d4629d1a006)  
18. Langchain- Memory Types in Simple Words | by Rahul \- Medium, 访问时间为 一月 7, 2026， [https://medium.com/@rahulpant.me/langchain-memory-types-in-simple-words-9fc142003567](https://medium.com/@rahulpant.me/langchain-memory-types-in-simple-words-9fc142003567)  
19. Part 1: How LangGraph Manages State for Multi-Agent Workflows (Best Practices) \- Medium, 访问时间为 一月 7, 2026， [https://medium.com/@bharatraj1918/langgraph-state-management-part-1-how-langgraph-manages-state-for-multi-agent-workflows-da64d352c43b](https://medium.com/@bharatraj1918/langgraph-state-management-part-1-how-langgraph-manages-state-for-multi-agent-workflows-da64d352c43b)  
20. Scalable Character Insights from Novels Using Vector Search and LLMs \- DEV Community, 访问时间为 一月 7, 2026， [https://dev.to/exson\_joseph/scalable-character-insights-from-novels-using-vector-search-and-llms-1937](https://dev.to/exson_joseph/scalable-character-insights-from-novels-using-vector-search-and-llms-1937)  
21. Maintaining Character Consistency in AI Art: Pro Tips | Anifusion, 访问时间为 一月 7, 2026， [https://anifusion.ai/articles/character-consistency-tips](https://anifusion.ai/articles/character-consistency-tips)  
22. Semantic Chunking for RAG: Better Context, Better Results \- Multimodal, 访问时间为 一月 7, 2026， [https://www.multimodal.dev/post/semantic-chunking-for-rag](https://www.multimodal.dev/post/semantic-chunking-for-rag)  
23. The Ultimate Guide to Chunking Strategies for RAG Applications with Databricks \- Medium, 访问时间为 一月 7, 2026， [https://medium.com/@debusinha2009/the-ultimate-guide-to-chunking-strategies-for-rag-applications-with-databricks-e495be6c0788](https://medium.com/@debusinha2009/the-ultimate-guide-to-chunking-strategies-for-rag-applications-with-databricks-e495be6c0788)  
24. Implement Long-Term Memory in AI Characters with Convai, 访问时间为 一月 7, 2026， [https://convai.com/blog/long-term-memeory](https://convai.com/blog/long-term-memeory)  
25. How to handle negative search queries in a vector similarity search query? \- Stack Overflow, 访问时间为 一月 7, 2026， [https://stackoverflow.com/questions/79640246/how-to-handle-negative-search-queries-in-a-vector-similarity-search-query](https://stackoverflow.com/questions/79640246/how-to-handle-negative-search-queries-in-a-vector-similarity-search-query)  
26. HiAgent: Hierarchical Working Memory Management for Solving Long-Horizon Agent Tasks with Large Language Model \- ACL Anthology, 访问时间为 一月 7, 2026， [https://aclanthology.org/2025.acl-long.1575.pdf](https://aclanthology.org/2025.acl-long.1575.pdf)  
27. How to Design Efficient Memory Architectures for Agentic AI Systems \- Towards AI, 访问时间为 一月 7, 2026， [https://pub.towardsai.net/how-to-design-efficient-memory-architectures-for-agentic-ai-systems-81ed456bb74f](https://pub.towardsai.net/how-to-design-efficient-memory-architectures-for-agentic-ai-systems-81ed456bb74f)  
28. How To Create A Series Bible (How To Plan & Write A Series, \#4) \- Heart Breathings, 访问时间为 一月 7, 2026， [https://heartbreathings.com/how-to-create-a-series-bible-how-to-plan-write-a-series-4/](https://heartbreathings.com/how-to-create-a-series-bible-how-to-plan-write-a-series-4/)  
29. Long-term memory \- Docs by LangChain, 访问时间为 一月 7, 2026， [https://docs.langchain.com/oss/python/langchain/long-term-memory](https://docs.langchain.com/oss/python/langchain/long-term-memory)  
30. Powering Long-Term Memory for Agents With LangGraph and MongoDB, 访问时间为 一月 7, 2026， [https://www.mongodb.com/company/blog/product-release-announcements/powering-long-term-memory-for-agents-langgraph](https://www.mongodb.com/company/blog/product-release-announcements/powering-long-term-memory-for-agents-langgraph)  
31. MemoryVectorStore \- Docs by LangChain, 访问时间为 一月 7, 2026， [https://docs.langchain.com/oss/javascript/integrations/vectorstores/memory](https://docs.langchain.com/oss/javascript/integrations/vectorstores/memory)  
32. Improving RAG accuracy: 10 techniques that actually work \- Redis, 访问时间为 一月 7, 2026， [https://redis.io/blog/10-techniques-to-improve-rag-accuracy/](https://redis.io/blog/10-techniques-to-improve-rag-accuracy/)  
33. How and when to build multi-agent systems \- LangChain Blog, 访问时间为 一月 7, 2026， [https://blog.langchain.com/how-and-when-to-build-multi-agent-systems/](https://blog.langchain.com/how-and-when-to-build-multi-agent-systems/)  
34. Building Multi AI Agent Workflows With LangChain In 2025 \- Intuz, 访问时间为 一月 7, 2026， [https://www.intuz.com/blog/building-multi-ai-agent-workflows-with-langchain](https://www.intuz.com/blog/building-multi-ai-agent-workflows-with-langchain)  
35. Unveiling Privacy Risks in LLM Agent Memory \- ACL Anthology, 访问时间为 一月 7, 2026， [https://aclanthology.org/2025.acl-long.1227.pdf](https://aclanthology.org/2025.acl-long.1227.pdf)  
36. Smarter Memories, Stronger Agents: How Selective Recall Boosts LLM Performance, 访问时间为 一月 7, 2026， [https://d3.harvard.edu/smarter-memories-stronger-agents-how-selective-recall-boosts-llm-performance/](https://d3.harvard.edu/smarter-memories-stronger-agents-how-selective-recall-boosts-llm-performance/)