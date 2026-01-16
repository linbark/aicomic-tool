## 问题定位
- 当前 Step4（episode_asset_ingest）在写入 interrupt 时引用了未定义变量 `changeset_id`，因此直接抛 `NameError: name 'changeset_id' is not defined`，流程无法进入“等待确认入库”。
- 该分支目前只生成了 `asset_ingest_plan/ingest_preview`（面向主库角色/资产），但没有创建/赋值 `changeset_id`（面向真相库/审阅台）。

## 现有库与表结构（来自代码定义）
- **主库（database.db，SQLAlchemy models）**：
  - projects(id, uuid, name, description, created_at)
  - characters(id, project_id, name, description, base_prompt, negative_prompt, category, avatar_asset_id)
  - episodes(id, project_id, title, order, description, action_text, prompt, script_locked, script_locked_at, last_exec_run_id, exec_status, exec_artifacts)
  - scenes(id, episode_id, sequence_number, title, description, action_text, dialogue, prompt)
  - shots(id, scene_id, sequence_number, title, action_text, dialogue, prompt, negative_prompt, selected_asset_id, status, video_path)
  - assets(id, character_id, shot_id, file_path, file_type, meta_data, created_at, is_favorite)
  - events(id, project_id, name, color, start_time_sort_key, description, graph_data)
  - event_nodes(id, event_id, target_type, target_id, description)
  - ai_action_runs(id, project_id, target_type, target_id, action_key, input_text, output_text, meta_data, created_at)
- **记忆/真相库（memory_store.db，SQLite CREATE TABLE）**：
  - memory_records（主表+向量索引用的 payload）
  - episodic_memories
  - canonical_evidences（证据库）
  - canonical_entities（实体主干）
  - canonical_snapshots（实体切片/画像）
  - canonical_events / canonical_state_changes
  - canonical_time_constraints / canonical_time_blocks
  - canonical_changesets（审阅台变更单，主键 changeset_id）
  - canonical_conflicts

## 字段一致性目标（对齐“之前 LLM 的 changeset.v0 输出骨架”）
- Step4 生成的变更单 payload 统一满足这些顶层字段（全部存在，类型一致）：
  - schema_version（固定 "changeset.v0"）
  - project_id（int）、episode_id（int 可选）、story_order_base（str）
  - evidences（list）、entities（list）、snapshots（list）、events（list）、state_changes（list）
  - time_constraints（list）、time_blocks（list）、conflicts（list）
  - materialize（dict，至少包含 write_static_bible）

## 实施方案（不引入 LLM，按既定 JSON 结构组装）
1) **Step4 先生成两份“入库计划”**（分别覆盖多库）：
   - **角色/资产库计划（主库）**：保留现有 `asset_ingest_plan`（items/series_style）与 `ingest_preview`（create_count/update_count 等）。
   - **真相库计划（memory_store）**：基于 `assets_visual_dna` 构造一个 changeset.v0 payload：
     - entities：把 characters/locations/props 映射为 canonical_entities（entity_type=character/location/prop，canonical_name=name，aliases=[] 等）。
     - snapshots：仅对 character 生成 snapshot，把 visual_dna + stable_diffusion_tags 放入 `fields.visual` 命名空间。
     - evidences：用现有 `chunk_text_to_evidences()` 对 `script_text` 做 deterministic 切片，把 evidence 写入 canonical_evidences，并在 snapshots.evidence_ids 引用（满足 evidence-first）。
     - id 稳定化：entity_id/snapshot_id 采用“project_id + type + name”的稳定哈希，避免重复运行产生重复实体。
   - 用 `MemoryStore.create_changeset(project_id, payload, episode_id)` 创建 `changeset_id`，写入 artifacts（并写入 interrupt）。

2) **修复 NameError**
   - 通过上一步确保 Step4 分支内一定定义 `changeset_id`；并让 interrupt/confirm 都从 artifacts 或 interrupt 中读取同一个 `changeset_id`。

3) **确认入库时写入多库**
   - `confirm_ingest && decision==confirmed`：
     - 先 `_apply_asset_ingest_plan(...)` 写主库（characters + visual_dna + series_style）。
     - 再 `MemoryStore.apply_changeset(changeset_id, reviewer="human", note="episode_execute")` 写真相库（canonical_* 表 + materialize 到旧分层记忆）。

4) **兼容性与字段校验**
   - 在 Step4 生成 changeset payload 时做显式校验：确保上述顶层字段齐全、类型正确；对缺失字段填默认值（空数组/空对象）。
   - 保持前端不改：仍使用 `ingest_preview` 做展示；新增的 `changeset_id` 只用于后端确认链路与审计。

## 验证计划
- 新增/更新后端测试：覆盖 Step4 生成 interrupt 时不再抛 `NameError`，并断言 artifacts 中包含 `changeset_id` 且 payload 顶层字段齐全。
- 冒烟验证：跑一次执行到 Step4 → 出现“确认入库”按钮 → 点击后主库 characters 有新增/更新、memory_store canonical_entities/snapshots 有写入。