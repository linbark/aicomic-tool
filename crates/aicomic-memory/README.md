# aicomic-memory

原生 Rust 记忆库。`memory_store.db` 是事实来源，SQLite 通过 bundled rusqlite 随应用链接。查看、词面检索、原文证据、人工审核不需要模型、Python、Node 或后台服务。

```rust,ignore
let store = MemoryStore::open_root(data_root)?;
let facts = store.list_records(&MemoryQuery::new(project_id))?;
let settings = EmbeddingSettings::load(data_root)?;
let memory = MemoryService::new(data_root, settings);
// 只有明确点击重建时调用；从现有 SQLite 内容构建。
let count = memory.rebuild(project_id).await?;
let results = memory.semantic_search(project_id, "人物获得的道具", 10).await?;
```

`EmbeddingSettings` 保存到 `embedding_settings.json`，默认 `mode="disabled"`。`http` 直接调用 OpenAI compatible `/embeddings`，`local` 使用显式目录中的 `config.json`、`tokenizer.json`、`model.safetensors`。本地后端通过 Cargo feature `local-bge` 启用，Candle 在 CPU 上执行 XLM-RoBERTa，取 CLS 密集向量并 L2 归一。不会自动下载权重。安全读取 safetensors 会使用较多内存；完整 BGE-M3 模型需自行准备。所有服务方法的模型加载、推理、HTTP 和数据库工作均在 Tokio blocking pool 中运行。

`MemoryService::task_context` 提供原任务关键词分解、分层检索、MMR、confirmed 负向约束、潜在冲突与 prompt 片段。关闭模型时输出明确的 `retrieval_mode="structured"`；语义搜索按钮本身返回未配置错误。每项目/模型首次索引显式重建，此后 Service 在任务检索、语义搜索前只增量嵌入新增或修改的事实，无需每次全库重建。模型身份或维度变化仍需要重建，绝不混用或伪造语义向量。

## 迁移合同

- 兼容旧 `memory_store.db` 表和字符串 ID；老主表只补缺少的 canonical 列，使用显式列读取。
- 证据、实体、快照、时间关系/时间块均同步维护 canonical 表与主记录。中文切片按 Unicode 字符数执行，支持 episode、scene、tags。
- changeset 保存原始 JSON、审核记录、批准/拒绝。批准在事务中执行，错误整单回滚，不会把失败提案标为 approved。
- 每实体 confirmed 最新快照物化为固定 `profile::<entity_id>`；后批的旧章节不会使当前设定退回。
- 冲突创建/解决同时更新审计记录。SeriesBible 与 VisualDNA 切片索引、结构化 beats/shots/QC 的 episodic 写回及章节摘要均已提供。
- AI 抽取先调用 `normalize_extracted_changeset` 约束输入证据、补项目/ID/时序、规范快照字段，再调用 `resolve_changeset_entities` 处理唯一精确名/别名、歧义、重复、强类型词提示与新 alias 建议。`resolve_changeset_entities_with_provider` 追加向量候选供人工选择，绝不凭相似度自动合并。归一不会让模型改写原文证据。
- `ensure_changeset_for_run(project_id,episode_id,run_id,payload)` 在单个事务中归一并保存提案、关联冲突、运行唯一映射。相同运行/输入在重启后复用同一 ID；待审提案可修订，已审核提案的更改会明确报错。`MemoryService` 提供同名异步入口，在配置模型时加入向量候选。复用既有提案不需要重新加载权重。
- 旧 Qdrant 本地目录不读写；Rust 没有把 Qdrant HTTP 客户端当成本地目录引擎。新 `rust_memory_vectors` 是可删除重建的 SQLite 密集向量索引，当前为精确余弦扫描。

## 已核实差异与边界

1. 实际 BGE-M3 大权重未下载或做嵌入质量等价评估；测试使用显式微型 XLM-RoBERTa 权重验证真实 Rust 推理路径、CLS 和归一化。完整模型的 CPU 速度/峰值内存、Windows 运行时需要对应环境验证。
2. EntityResolver 的精确名/别名、歧义、payload 去重、类型提示、新 alias 建议和向量候选已迁移；向量候选需要配置模型并显式重建现有实体索引。同步 Store 的无 provider 入口只做结构化归一，配置模型的异步 Service 入口会同时生成相似候选。
3. 原 Python changeset 的 events/state_changes 会保留在可审核 payload 中，但原 apply_changeset 本来就没有把它们写入 canonical_events/canonical_state_changes，因此没有宣称新增该能力。
4. 原 Fountain 状态抽取、NER、L0 buffer、prompt token 截断及全量 VisualDNA 枚举原本为 TODO/恒空，未伪装为已实现功能。
5. 持久化向量索引实现为精确扫描；不承诺旧 Qdrant HNSW 的大数据规模性能。外部 HTTP 模型同名升级需要用户重建。
6. 桌面 `integration.rs` 已提供自动桥接：prepare_memory 把源文本保存为可重放的证据；apply_memory 连接 architect 的 SeriesBible、writer/分镜的结构化状态、QC、提案审核、已批准素材的实体/VisualDNA 与静态设定。它只读取 core 已入库的素材 ID，不重复创建素材或分集；来源白名单取运行 request.json。模型候选通过异步 Service 提案入口执行，避免 GUI 同步路径加载 Candle。

验证：`cargo test -p aicomic-memory --features local-bge`。所有测试仅用临时目录，不触及用户数据库或下载模型。
