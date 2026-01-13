import json
from typing import Any, Dict, List, Optional

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel, Field
from sqlalchemy.orm import Session

from .. import models
from ..database import get_db
from ..services import prompt_registry
from ..services.context_store import new_run_id
from ..services.json_extract import extract_json_any
from ..services.llm_client import LlmChatSettings
from ..services.project_lookup import resolve_project_pk
from ..services.prompt_composer import PromptModules, compose_system_prompt_xml
from ..workflows.schemas import BeatSheetItem, PromptPair, QcReport, SeriesBible, ShotSpec
from .ai_shared import (
    _build_memory_context,
    _chat_client,
    _context_store,
    _mask_settings,
    _read_settings_raw,
    _repair_json_with_same_agent,
)


router = APIRouter(tags=["AI (DeepSeek)"])


# ==========================
# Workflows（后端统一编排）
# ==========================


class WorkflowScriptOptions(BaseModel):
    qc_loops: int = 1
    max_scenes: int = 50
    derived_split_scenes: bool = False


class WorkflowScriptRequest(BaseModel):
    project_id: str
    input_text: str
    options: WorkflowScriptOptions = Field(default_factory=WorkflowScriptOptions)


class WorkflowScriptResponse(BaseModel):
    run_id: str
    series_bible: Dict[str, Any]
    beat_sheet: List[Dict[str, Any]]
    script_fountain: str
    qc_report: Dict[str, Any]
    derived: Optional[Dict[str, Any]] = None


@router.post("/workflows/script", response_model=WorkflowScriptResponse)
async def workflow_script(req: WorkflowScriptRequest, db: Session = Depends(get_db)):
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    user_text = (req.input_text or "").strip()
    if not user_text:
        raise HTTPException(status_code=400, detail="input_text 不能为空")

    run_id = new_run_id()
    project_id = resolve_project_pk(db, req.project_id)
    # 不限制 max_tokens，让 API 自己决定输出长度

    # 读取已有 series_bible（可为空）
    existing_bible = _context_store.get_series_bible(project_id=project_id, version="v1")

    # 检索并注入记忆（增强一致性）
    memory_context = ""
    try:
        from ..services.memory_indexer import MemoryIndexer
        from ..services.memory_retriever import get_memory_retriever

        # 确保记忆已索引
        indexer = MemoryIndexer()
        indexer.index_series_bible(project_id=project_id, version="v1")

        # 检索记忆
        retriever = get_memory_retriever()
        retrieval_results = retriever.retrieve_for_task(
            project_id=project_id,
            task_description=f"架构师：生成世界观和节拍表 - {user_text[:200]}",
            top_k_per_layer={
                "L1": 10,
                "L2_static": 10,
                "L2_dynamic": 5,
            },
        )

        # 格式化记忆
        memory_context = _build_memory_context(retrieval_results)
    except Exception as e:
        print(f"[AI][workflow_script][architect] Memory retrieval failed: {e}")

    # 1) 架构师：产出 series_bible + beat_sheet
    architect_role = prompt_registry.get_template_prompt("architect_system")

    # 构建约束（包含检索到的负向约束）
    architect_constraints = [
        "视觉优先：忽略内心独白，只提取可被镜头呈现的信息。",
        "输出必须是 JSON object，且仅包含 keys: series_bible(object), beat_sheet(array)。",
    ]

    # 构建 extra_blocks（包含记忆上下文）
    extra_blocks: Dict[str, Any] = {}
    if memory_context:
        extra_blocks["memory_context"] = memory_context

    architect_system = compose_system_prompt_xml(
        PromptModules(
            role_definition=architect_role,
            series_bible=existing_bible,
            constraints=architect_constraints,
            instruction=[
                "阅读 user 输入（可能是大纲/章节/需求）。",
                "参考 memory_context 中已有的设定和约束。",
                "生成 series_bible（世界观规则、角色、视觉DNA引用、术语表、禁忌）。",
                "生成 beat_sheet（节拍类型/情感电荷/视觉重点/预估格数）。",
                "仅输出 JSON，不要任何额外文本。",
            ],
            output_format="json",
            extra_blocks=extra_blocks,
        )
    )
    architect_content = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=settings.temperature,
            max_tokens=settings.max_tokens,  # None 表示不限制
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": architect_system},
            {"role": "user", "content": user_text},
        ],
    )
    _context_store.snapshot_stage(project_id=project_id, run_id=run_id, stage_name="architect.raw", data={"text": architect_content})
    architect_parsed = extract_json_any(architect_content)
    if not isinstance(architect_parsed, dict):
        # repair once
        repaired = await _repair_json_with_same_agent(
            llm_settings=LlmChatSettings(
                base_url=settings.base_url,
                api_key=raw.get("api_key") or "",
                model=settings.model,
                temperature=settings.temperature,
                max_tokens=settings.max_tokens,  # None 表示不限制
                timeout_seconds=settings.timeout_seconds,
            ),
            system_prompt=architect_system,
            bad_output_text=str(architect_content),
            error_hint="Root type is not JSON object",
            expected_hint="JSON object with keys: series_bible(object), beat_sheet(array)",
        )
        architect_parsed = repaired
        if not isinstance(architect_parsed, dict):
            raise HTTPException(status_code=422, detail="Architect output must be a JSON object")
    series_bible_raw = architect_parsed.get("series_bible") or {}
    beat_sheet_raw = architect_parsed.get("beat_sheet") or []
    try:
        series_bible = SeriesBible.model_validate(series_bible_raw).model_dump()
        beat_sheet_items = [
            BeatSheetItem.model_validate(x).model_dump() for x in (beat_sheet_raw if isinstance(beat_sheet_raw, list) else [])
        ]
    except Exception as e:
        # repair once
        repaired = await _repair_json_with_same_agent(
            llm_settings=LlmChatSettings(
                base_url=settings.base_url,
                api_key=raw.get("api_key") or "",
                model=settings.model,
                temperature=settings.temperature,
                max_tokens=settings.max_tokens,  # None 表示不限制
                timeout_seconds=settings.timeout_seconds,
            ),
            system_prompt=architect_system,
            bad_output_text=json.dumps(architect_parsed, ensure_ascii=False),
            error_hint=str(e),
            expected_hint="series_bible must be object; beat_sheet must be array of objects",
        )
        if not isinstance(repaired, dict):
            raise HTTPException(status_code=422, detail=f"Architect output schema invalid: {e}")
        series_bible_raw = repaired.get("series_bible") or {}
        beat_sheet_raw = repaired.get("beat_sheet") or []
        try:
            series_bible = SeriesBible.model_validate(series_bible_raw).model_dump()
            beat_sheet_items = [
                BeatSheetItem.model_validate(x).model_dump() for x in (beat_sheet_raw if isinstance(beat_sheet_raw, list) else [])
            ]
        except Exception as e2:
            raise HTTPException(status_code=422, detail=f"Architect output schema invalid: {e2}")
    beat_sheet = beat_sheet_items
    _context_store.snapshot_stage(
        project_id=project_id, run_id=run_id, stage_name="architect.parsed", data={"series_bible": series_bible, "beat_sheet": beat_sheet}
    )

    # 写回 context（file-first）
    _context_store.put_series_bible(project_id=project_id, data=series_bible, version="v1")

    # 重新索引记忆（SeriesBible 更新后）
    try:
        from ..services.memory_indexer import MemoryIndexer

        indexer = MemoryIndexer()
        indexer.index_series_bible(project_id=project_id, version="v1", source_ref=f"{run_id}.architect")
    except Exception as e:
        print(f"[AI][workflow_script] Memory indexing failed: {e}")

    # 2) 编剧：产出 Fountain（放在 JSON 字段里传输）
    writer_role = prompt_registry.get_template_prompt("writer_system")
    writer_system = compose_system_prompt_xml(
        PromptModules(
            role_definition=writer_role,
            series_bible=series_bible,
            constraints=[
                "严格 Fountain：场景标题 INT./EXT. + 全大写；角色名全大写；动作描写每段不超过 3-4 行。",
                "禁止心理动词（觉得/认为/感到/想起等），必须转为可见动作。",
                "对话单个气泡不超过 30 个汉字，超限必须拆分。",
                "输出必须是 JSON object，且仅包含 keys: script_fountain(string)。",
            ],
            instruction=[
                "基于 beat_sheet 展开为分场的 Fountain 剧本。",
                "保持节奏：动作原子化（复杂动作拆成镜头段落）。",
                "仅输出 JSON，不要任何额外文本。",
            ],
            output_format="json",
            extra_blocks={"beat_sheet": json.dumps(beat_sheet, ensure_ascii=False, indent=2)},
        )
    )
    writer_content = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=settings.temperature,
            max_tokens=settings.max_tokens,  # None 表示不限制
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": writer_system},
            {"role": "user", "content": "请生成剧本。"},
        ],
    )
    _context_store.snapshot_stage(project_id=project_id, run_id=run_id, stage_name="writer.raw", data={"text": writer_content})
    writer_parsed = extract_json_any(writer_content)
    if not isinstance(writer_parsed, dict):
        repaired = await _repair_json_with_same_agent(
            llm_settings=LlmChatSettings(
                base_url=settings.base_url,
                api_key=raw.get("api_key") or "",
                model=settings.model,
                temperature=settings.temperature,
                max_tokens=settings.max_tokens,  # None 表示不限制
                timeout_seconds=settings.timeout_seconds,
            ),
            system_prompt=writer_system,
            bad_output_text=str(writer_content),
            error_hint="Root type is not JSON object",
            expected_hint="JSON object with key: script_fountain(string)",
        )
        writer_parsed = repaired
        if not isinstance(writer_parsed, dict):
            raise HTTPException(status_code=422, detail="Writer output must be a JSON object")
    script_fountain = str(writer_parsed.get("script_fountain") or "").strip()
    _context_store.snapshot_stage(project_id=project_id, run_id=run_id, stage_name="writer.parsed", data={"script_fountain": script_fountain})

    # Writer 完成：提取并写入 dynamic_plot（章节摘要、人物关系）
    try:
        from ..services.memory_store import get_memory_store
        from ..services.state_extractor import StateChangeExtractor
        from ..workflows.memory_schemas import MemoryNamespace, MemoryRecord, MemoryType

        extractor = StateChangeExtractor()
        memory_store = get_memory_store()

        # 从 beat_sheet 和 script_fountain 提取状态变更
        structured_data = {
            "beat_sheet": beat_sheet,
            "script_fountain": script_fountain,
        }
        extractor.extract_from_structured_output(
            project_id=project_id,
            structured_data=structured_data,
            source_ref=f"{run_id}.writer",
        )

        # 写入 dynamic_plot（章节摘要）
        # 简化：将 beat_sheet 摘要写入 dynamic_plot
        if beat_sheet:
            summary_text = (
                f"章节摘要：包含 {len(beat_sheet)} 个节拍。主要节拍：{', '.join([b.get('title', '') or b.get('description', '')[:30] for b in beat_sheet[:5]])}"
            )
            dynamic_record = MemoryRecord(
                project_id=project_id,
                namespace=MemoryNamespace.DYNAMIC_PLOT,
                type=MemoryType.EVENT,
                entity=None,
                content=summary_text,
                payload_json={
                    "beat_count": len(beat_sheet),
                    "beat_sheet": beat_sheet[:10],  # 只存前10个节拍
                },
                source_ref=f"{run_id}.writer",
            )
            memory_store.write(dynamic_record)
    except Exception as e:
        print(f"[AI][workflow_script][writer] Memory write failed: {e}")

    # 3) QC：循环自检/修订
    qc_role = prompt_registry.get_template_prompt("qc_system")
    qc_report: Dict[str, Any] = {"issues": []}
    loops = max(0, min(int(req.options.qc_loops or 0), 5))
    for _i in range(loops):
        qc_system = compose_system_prompt_xml(
            PromptModules(
                role_definition=qc_role,
                series_bible=series_bible,
                constraints=[
                    "检查并列出 issues（数组）：type, message, location(optional)。",
                    "如需修订，返回 revised_script_fountain（string）。",
                    "输出必须是 JSON object，仅包含 keys: issues(array), revised_script_fountain(optional)。",
                ],
                instruction=[
                    "阅读当前 script_fountain。",
                    "做一致性与格式检查，并给出修订（如有必要）。",
                    "仅输出 JSON，不要任何额外文本。",
                ],
                output_format="json",
            )
        )
        qc_content = await _chat_client.chat(
            settings=LlmChatSettings(
                base_url=settings.base_url,
                api_key=raw.get("api_key") or "",
                model=settings.model,
                temperature=settings.temperature,
                max_tokens=settings.max_tokens,  # None 表示不限制
                timeout_seconds=settings.timeout_seconds,
            ),
            messages=[
                {"role": "system", "content": qc_system},
                {"role": "user", "content": json.dumps({"script_fountain": script_fountain}, ensure_ascii=False)},
            ],
        )
        _context_store.snapshot_stage(project_id=project_id, run_id=run_id, stage_name=f"qc.raw.{_i+1}", data={"text": qc_content})
        parsed = extract_json_any(qc_content)
        if isinstance(parsed, dict):
            try:
                qc_obj = QcReport.model_validate(parsed)
                qc_report = qc_obj.model_dump()
                revised = qc_obj.revised_script_fountain
                if isinstance(revised, str) and revised.strip():
                    script_fountain = revised.strip()
            except Exception as e:
                repaired = await _repair_json_with_same_agent(
                    llm_settings=LlmChatSettings(
                        base_url=settings.base_url,
                        api_key=raw.get("api_key") or "",
                        model=settings.model,
                        temperature=settings.temperature,
                        max_tokens=settings.max_tokens,  # None 表示不限制
                        timeout_seconds=settings.timeout_seconds,
                    ),
                    system_prompt=qc_system,
                    bad_output_text=json.dumps(parsed, ensure_ascii=False),
                    error_hint=str(e),
                    expected_hint="JSON object with keys: issues(array of {type,message,location?}), revised_script_fountain(optional string)",
                )
                try:
                    qc_obj = QcReport.model_validate(repaired)
                    qc_report = qc_obj.model_dump()
                    revised = qc_obj.revised_script_fountain
                    if isinstance(revised, str) and revised.strip():
                        script_fountain = revised.strip()
                except Exception as e2:
                    raise HTTPException(status_code=422, detail=f"QC output schema invalid: {e2}")
        _context_store.snapshot_stage(
            project_id=project_id,
            run_id=run_id,
            stage_name=f"qc.parsed.{_i+1}",
            data={"qc_report": qc_report, "script_fountain": script_fountain},
        )

        # 每轮 QC：写入修订原因+变化摘要到 episodic 记忆
        if qc_report.get("issues"):
            try:
                from ..services.state_extractor import StateChangeExtractor

                extractor = StateChangeExtractor()
                extractor.extract_from_qc_report(
                    project_id=project_id,
                    qc_report=qc_report,
                    source_ref=f"{run_id}.qc.{_i+1}",
                )
            except Exception as e:
                print(f"[AI][workflow_script][qc.{_i+1}] Memory write failed: {e}")

    derived: Optional[Dict[str, Any]] = None
    if bool(req.options.derived_split_scenes):
        # 复用 split-scenes 模板与 JSON 解析
        split_system = prompt_registry.get_template_prompt("split_scenes_system", {"max_scenes": req.options.max_scenes})
        split_content = await _chat_client.chat(
            settings=LlmChatSettings(
                base_url=settings.base_url,
                api_key=raw.get("api_key") or "",
                model=settings.model,
                temperature=settings.temperature,
                max_tokens=settings.max_tokens,  # None 表示不限制
                timeout_seconds=settings.timeout_seconds,
            ),
            messages=[
                {"role": "system", "content": split_system},
                {"role": "user", "content": script_fountain},
            ],
        )
        _context_store.snapshot_stage(project_id=project_id, run_id=run_id, stage_name="split_scenes.raw", data={"text": split_content})
        parsed = extract_json_any(split_content)
        if isinstance(parsed, list):
            derived = {"scenes": parsed[: req.options.max_scenes]}
            _context_store.snapshot_stage(project_id=project_id, run_id=run_id, stage_name="split_scenes.parsed", data=derived)

    response: Dict[str, Any] = {
        "run_id": run_id,
        "series_bible": series_bible,
        "beat_sheet": beat_sheet,
        "script_fountain": script_fountain,
        "qc_report": qc_report,
        "derived": derived,
    }

    _context_store.snapshot_run(
        project_id=project_id,
        run_id=run_id,
        request=req.model_dump(),
        response=response,
        meta={"workflow": "script"},
    )
    return WorkflowScriptResponse(**response)


class WorkflowStoryboardOptions(BaseModel):
    max_shots: int = 80
    asset_item_ids: List[int] = Field(default_factory=list)
    prompt_style: str = "sd_tags"  # "sd_tags" | "mj_v6"
    aspect_ratio: Optional[str] = None  # 如 "16:9", "9:16", "2:3"


class WorkflowStoryboardRequest(BaseModel):
    project_id: str
    scene_text: str
    options: WorkflowStoryboardOptions = Field(default_factory=WorkflowStoryboardOptions)


class WorkflowStoryboardResponse(BaseModel):
    run_id: str
    shots: List[Dict[str, Any]]


@router.post("/workflows/storyboard", response_model=WorkflowStoryboardResponse)
async def workflow_storyboard(req: WorkflowStoryboardRequest, db: Session = Depends(get_db)):
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    scene_text = (req.scene_text or "").strip()
    if not scene_text:
        raise HTTPException(status_code=400, detail="scene_text 不能为空")

    run_id = new_run_id()
    project_id = resolve_project_pk(db, req.project_id)
    series_bible = _context_store.get_series_bible(project_id=project_id, version="v1") or {}

    visual_dna_list: List[Dict[str, Any]] = []
    for item_id in req.options.asset_item_ids[:50]:
        dna = _context_store.get_visual_dna(project_id=project_id, item_id=int(item_id), version="v1")
        if isinstance(dna, dict) and dna:
            visual_dna_list.append({"item_id": int(item_id), "visual_dna": dna})

    # 检索并注入记忆（增强视觉一致性）
    memory_context = ""
    try:
        from ..services.memory_indexer import MemoryIndexer
        from ..services.memory_retriever import get_memory_retriever

        # 确保记忆已索引（包括 VisualDNA）
        indexer = MemoryIndexer()
        indexer.index_series_bible(project_id=project_id, version="v1")
        for item_id in req.options.asset_item_ids[:50]:
            indexer.index_visual_dna(project_id=project_id, item_id=int(item_id), version="v1")

        # 检索记忆（重点检索角色设计和视觉规则）
        retriever = get_memory_retriever()
        retrieval_results = retriever.retrieve_for_task(
            project_id=project_id,
            task_description=f"分镜师：拆分场景为镜头列表 - {scene_text[:200]}",
            top_k_per_layer={
                "L1": 5,  # 较少历史剧情
                "L2_static": 15,  # 更多角色设计和世界观
                "L2_dynamic": 5,
            },
        )

        # 格式化记忆
        memory_context = _build_memory_context(retrieval_results)
    except Exception as e:
        print(f"[AI][workflow_storyboard] Memory retrieval failed: {e}")

    # Step1: storyboard 拆分 ShotSpec（不含 SD prompt）
    storyboard_role = prompt_registry.get_template_prompt("storyboard_system")

    # 构建 extra_blocks（包含 visual_dna 和记忆上下文）
    extra_blocks2: Dict[str, Any] = {
        "locked_visual_dna": json.dumps(visual_dna_list, ensure_ascii=False, indent=2) if visual_dna_list else "",
    }
    if memory_context:
        extra_blocks2["memory_context"] = memory_context

    storyboard_system = compose_system_prompt_xml(
        PromptModules(
            role_definition=storyboard_role,
            series_bible=series_bible,
            constraints=[
                f"最多输出 {int(req.options.max_shots)} 个镜头。",
                "受控词汇表：shot_size=ELS|LS|MS|CU|ECU|INSERT；camera_angle=EYE|LOW|HIGH|DUTCH；lighting_style=SOFT|HARD|CHIAROSCURO|RIM|VOLUMETRIC。",
                "输出必须是 JSON array，每项是 object，包含 keys: title(optional), action_text, dialogue(optional), shot_size, camera_angle, lighting_style。",
            ],
            instruction=[
                "把 scene_text 拆成镜头列表。",
                "参考 locked_visual_dna 和 memory_context 中的角色设计，确保视觉一致性。",
                "动作原子化：每个镜头只包含一个清晰可视动作。",
                "仅输出 JSON，不要任何额外文本。",
            ],
            output_format="json",
            extra_blocks=extra_blocks2,
        )
    )
    step1_content = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=settings.temperature,
            max_tokens=settings.max_tokens,
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": storyboard_system},
            {"role": "user", "content": scene_text},
        ],
    )
    _context_store.snapshot_stage(project_id=project_id, run_id=run_id, stage_name="storyboard.raw", data={"text": step1_content})
    step1_parsed = extract_json_any(step1_content)
    if not isinstance(step1_parsed, list):
        repaired = await _repair_json_with_same_agent(
            llm_settings=LlmChatSettings(
                base_url=settings.base_url,
                api_key=raw.get("api_key") or "",
                model=settings.model,
                temperature=settings.temperature,
                max_tokens=settings.max_tokens,
                timeout_seconds=settings.timeout_seconds,
            ),
            system_prompt=storyboard_system,
            bad_output_text=str(step1_content),
            error_hint="Root type is not JSON array",
            expected_hint="JSON array of ShotSpec objects",
        )
        step1_parsed = repaired
        if not isinstance(step1_parsed, list):
            raise HTTPException(status_code=422, detail="Storyboard output must be a JSON array")
    raw_shots = step1_parsed[: int(req.options.max_shots)]
    try:
        shot_specs = [ShotSpec.model_validate(x).model_dump() for x in raw_shots]
    except Exception as e:
        repaired = await _repair_json_with_same_agent(
            llm_settings=LlmChatSettings(
                base_url=settings.base_url,
                api_key=raw.get("api_key") or "",
                model=settings.model,
                temperature=settings.temperature,
                max_tokens=settings.max_tokens,
                timeout_seconds=settings.timeout_seconds,
            ),
            system_prompt=storyboard_system,
            bad_output_text=json.dumps(raw_shots, ensure_ascii=False),
            error_hint=str(e),
            expected_hint="Each item must include: action_text(string), shot_size one of ELS|LS|MS|CU|ECU|INSERT, camera_angle one of EYE|LOW|HIGH|DUTCH, lighting_style one of SOFT|HARD|CHIAROSCURO|RIM|VOLUMETRIC",
        )
        if not isinstance(repaired, list):
            raise HTTPException(status_code=422, detail=f"Storyboard output schema invalid: {e}")
        raw_shots = repaired[: int(req.options.max_shots)]
        try:
            shot_specs = [ShotSpec.model_validate(x).model_dump() for x in raw_shots]
        except Exception as e2:
            raise HTTPException(status_code=422, detail=f"Storyboard output schema invalid: {e2}")
    shot_list = shot_specs
    _context_store.snapshot_stage(project_id=project_id, run_id=run_id, stage_name="storyboard.parsed", data={"shots": shot_list})

    # Storyboard 完成：写入 episodic（镜头层状态变更：新增道具、伤势、人物入场/退场）
    try:
        from ..services.state_extractor import StateChangeExtractor

        extractor = StateChangeExtractor()
        extractor.extract_from_structured_output(
            project_id=project_id,
            structured_data={"shots": shot_list},
            source_ref=f"{run_id}.storyboard",
        )
    except Exception as e:
        print(f"[AI][workflow_storyboard][storyboard] Memory write failed: {e}")

    # Step2: 翻译为 prompt（根据 prompt_style 选择模板）
    prompt_style = (req.options.prompt_style or "sd_tags").strip()
    aspect_ratio = req.options.aspect_ratio

    if prompt_style == "mj_v6":
        # Midjourney v6 模式
        translate_role = prompt_registry.get_template_prompt("prompt_translate_mj_system")
        translate_system = compose_system_prompt_xml(
            PromptModules(
                role_definition=translate_role,
                series_bible=series_bible,
                constraints=[
                    "输出必须是 JSON array，与输入 shots 等长。",
                    "每项必须包含 key: prompt(string)。",
                    "prompt 使用 Midjourney v6 语法（:: 分隔符，自然语言描述）。",
                    f"根据 aspect_ratio={aspect_ratio or 'auto'} 添加 --ar 参数。",
                ],
                instruction=[
                    "读取 shots（含镜头参数与动作）。",
                    "为每个镜头生成 Midjourney v6 风格的 prompt，并尽量复用 locked_visual_dna 的核心要素。",
                    "在 prompt 末尾添加 --v 6.0 --stylize 250 和 --ar 参数。",
                    "仅输出 JSON，不要任何额外文本。",
                ],
                output_format="json",
                extra_blocks={
                    "shots": json.dumps(shot_list, ensure_ascii=False, indent=2),
                    "locked_visual_dna": json.dumps(visual_dna_list, ensure_ascii=False, indent=2) if visual_dna_list else "",
                    "aspect_ratio": aspect_ratio or "auto",
                },
            )
        )
    else:
        # SD/Flux tags 模式（默认）
        translate_role = prompt_registry.get_template_prompt("prompt_translate_system")
        translate_system = compose_system_prompt_xml(
            PromptModules(
                role_definition=translate_role,
                series_bible=series_bible,
                constraints=[
                    "输出必须是 JSON array，与输入 shots 等长。",
                    "每项必须包含 keys: prompt, negative_prompt。",
                    "prompt 使用逗号分隔 tags；negative_prompt 也使用 tags。",
                ],
                instruction=[
                    "读取 shots（含镜头参数与动作）。",
                    "为每个镜头生成 prompt/negative_prompt，并尽量复用 locked_visual_dna 的核心要素。",
                    "仅输出 JSON，不要任何额外文本。",
                ],
                output_format="json",
                extra_blocks={
                    "shots": json.dumps(shot_list, ensure_ascii=False, indent=2),
                    "locked_visual_dna": json.dumps(visual_dna_list, ensure_ascii=False, indent=2) if visual_dna_list else "",
                },
            )
        )
    step2_content = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=settings.temperature,
            max_tokens=settings.max_tokens,
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": translate_system},
            {"role": "user", "content": "请输出 prompt 列表。"},
        ],
    )
    _context_store.snapshot_stage(project_id=project_id, run_id=run_id, stage_name="prompt_translate.raw", data={"text": step2_content})
    step2_parsed = extract_json_any(step2_content)
    if not isinstance(step2_parsed, list) or len(step2_parsed) != len(shot_list):
        repaired = await _repair_json_with_same_agent(
            llm_settings=LlmChatSettings(
                base_url=settings.base_url,
                api_key=raw.get("api_key") or "",
                model=settings.model,
                temperature=settings.temperature,
                max_tokens=settings.max_tokens,
                timeout_seconds=settings.timeout_seconds,
            ),
            system_prompt=translate_system,
            bad_output_text=str(step2_content),
            error_hint="Output must be JSON array and length must equal shots length",
            expected_hint=f"JSON array length == {len(shot_list)}, each item has prompt and negative_prompt",
        )
        step2_parsed = repaired
    if not isinstance(step2_parsed, list) or len(step2_parsed) != len(shot_list):
        raise HTTPException(status_code=422, detail="Prompt translate output length mismatch")

    # 根据 prompt_style 处理不同的输出格式
    if prompt_style == "mj_v6":
        # MJ 模式：只有 prompt 字段
        try:
            prompt_pairs: List[Dict[str, str]] = []
            for x in step2_parsed:
                if not isinstance(x, dict):
                    raise ValueError("Each item must be a dict")
                prompt_str = str(x.get("prompt") or "").strip()
                if not prompt_str:
                    raise ValueError("prompt is required")
                prompt_pairs.append({"prompt": prompt_str, "negative_prompt": ""})  # MJ 不需要 negative_prompt
        except Exception as e:
            repaired = await _repair_json_with_same_agent(
                llm_settings=LlmChatSettings(
                    base_url=settings.base_url,
                    api_key=raw.get("api_key") or "",
                    model=settings.model,
                    temperature=settings.temperature,
                    max_tokens=settings.max_tokens,
                    timeout_seconds=settings.timeout_seconds,
                ),
                system_prompt=translate_system,
                bad_output_text=json.dumps(step2_parsed, ensure_ascii=False),
                error_hint=str(e),
                expected_hint=f"JSON array length == {len(shot_list)}, each item has non-empty prompt",
            )
            if not isinstance(repaired, list) or len(repaired) != len(shot_list):
                raise HTTPException(status_code=422, detail=f"Prompt translate output schema invalid: {e}")
            prompt_pairs = []
            for x in repaired:
                prompt_str = str(x.get("prompt") or "").strip()
                prompt_pairs.append({"prompt": prompt_str, "negative_prompt": ""})
    else:
        # SD/Flux 模式：需要 prompt 和 negative_prompt
        try:
            prompt_pairs = [PromptPair.model_validate(x).model_dump() for x in step2_parsed]
        except Exception as e:
            repaired = await _repair_json_with_same_agent(
                llm_settings=LlmChatSettings(
                    base_url=settings.base_url,
                    api_key=raw.get("api_key") or "",
                    model=settings.model,
                    temperature=settings.temperature,
                    max_tokens=settings.max_tokens,
                    timeout_seconds=settings.timeout_seconds,
                ),
                system_prompt=translate_system,
                bad_output_text=json.dumps(step2_parsed, ensure_ascii=False),
                error_hint=str(e),
                expected_hint=f"JSON array length == {len(shot_list)}, each item has non-empty prompt and non-empty negative_prompt",
            )
            if not isinstance(repaired, list) or len(repaired) != len(shot_list):
                raise HTTPException(status_code=422, detail=f"Prompt translate output schema invalid: {e}")
            prompt_pairs = [PromptPair.model_validate(x).model_dump() for x in repaired]

    merged: List[Dict[str, Any]] = []
    for i, sh in enumerate(shot_list):
        item: Dict[str, Any] = dict(sh)
        add = prompt_pairs[i]
        item["prompt"] = add.get("prompt")
        item["negative_prompt"] = add.get("negative_prompt")
        merged.append(item)
    _context_store.snapshot_stage(project_id=project_id, run_id=run_id, stage_name="prompt_translate.parsed", data={"shots": merged})

    # PromptTranslate 完成：写入 production 记忆（prompt 参数、成功样例）
    try:
        from ..services.memory_store import get_memory_store
        from ..workflows.memory_schemas import MemoryNamespace, MemoryRecord, MemoryType

        memory_store = get_memory_store()

        # 写入成功的 prompt 样例（只存前几个作为参考）
        for i, shot in enumerate(merged[:5]):  # 只存前5个作为样例
            if shot.get("prompt"):
                production_record = MemoryRecord(
                    project_id=project_id,
                    namespace=MemoryNamespace.PRODUCTION,
                    type=MemoryType.PROMPT_TEMPLATE,
                    entity=None,
                    content=f"Prompt 样例 {i+1}: {shot.get('prompt', '')[:200]}",
                    payload_json={
                        "prompt": shot.get("prompt"),
                        "negative_prompt": shot.get("negative_prompt"),
                        "prompt_style": req.options.prompt_style,
                        "aspect_ratio": req.options.aspect_ratio,
                        "shot_size": shot.get("shot_size"),
                        "camera_angle": shot.get("camera_angle"),
                        "lighting_style": shot.get("lighting_style"),
                    },
                    source_ref=f"{run_id}.prompt_translate",
                )
                memory_store.write(production_record)
    except Exception as e:
        print(f"[AI][workflow_storyboard][prompt_translate] Memory write failed: {e}")

    response: Dict[str, Any] = {"run_id": run_id, "shots": merged}
    _context_store.snapshot_run(
        project_id=project_id,
        run_id=run_id,
        request=req.model_dump(),
        response=response,
        meta={"workflow": "storyboard"},
    )
    return WorkflowStoryboardResponse(**response)


# ==========================
# Apply-to-DB（生成→落库）
# ==========================


class ApplyScriptWorkflowRequest(BaseModel):
    project_id: str
    episode_id: int
    run_id: str
    overwrite_scenes: bool = False


@router.post("/workflows/script/apply")
def apply_workflow_script(payload: ApplyScriptWorkflowRequest, db: Session = Depends(get_db)):
    """
    从 run 快照中读取 workflow_script 的产物，并写回 DB：
    - Episode.description = script_fountain
    - 可选：derived.scenes -> Episode.scenes（按 sequence_number 重建）
    """
    project_id = resolve_project_pk(db, payload.project_id)
    ep_id = int(payload.episode_id)
    run_id = (payload.run_id or "").strip()
    if not run_id:
        raise HTTPException(status_code=400, detail="run_id 不能为空")

    snap = _context_store.read_run_response(project_id=project_id, run_id=run_id)
    if not isinstance(snap, dict):
        raise HTTPException(status_code=404, detail="run snapshot not found")

    script_fountain = str(snap.get("script_fountain") or "").strip()
    if not script_fountain:
        raise HTTPException(status_code=422, detail="snapshot missing script_fountain")

    ep = db.query(models.Episode).filter(models.Episode.id == ep_id).first()
    if not ep:
        raise HTTPException(status_code=404, detail="Episode not found")
    if int(ep.project_id) != project_id:
        raise HTTPException(status_code=400, detail="episode_id 不属于该 project_id")

    ep.description = script_fountain

    derived = snap.get("derived") or {}
    scenes = derived.get("scenes") if isinstance(derived, dict) else None
    if payload.overwrite_scenes and isinstance(scenes, list):
        # 清空旧 scenes（级联删除 shots）
        existing = db.query(models.Scene).filter(models.Scene.episode_id == ep.id).all()
        for sc in existing:
            db.delete(sc)
        db.flush()

        # 重建 scenes（只写 title/description；shot 由 storyboard workflow 负责）
        for idx, item in enumerate(scenes, start=1):
            if not isinstance(item, dict):
                continue
            title = str(item.get("title") or f"场{idx}")
            desc = str(item.get("description") or "").strip()
            db.add(models.Scene(episode_id=ep.id, sequence_number=idx, title=title, description=desc))

    db.commit()
    return {"ok": True, "episode_id": ep.id, "run_id": run_id}


class ApplyStoryboardWorkflowRequest(BaseModel):
    project_id: str
    scene_id: int
    run_id: str
    overwrite_shots: bool = True


@router.post("/workflows/storyboard/apply")
def apply_workflow_storyboard(payload: ApplyStoryboardWorkflowRequest, db: Session = Depends(get_db)):
    """
    从 run 快照中读取 workflow_storyboard 的 shots，并写回 DB：
    - Scene.shots: action_text/dialogue/prompt/negative_prompt/title/sequence_number
    """
    project_id = resolve_project_pk(db, payload.project_id)
    scene_id = int(payload.scene_id)
    run_id = (payload.run_id or "").strip()
    if not run_id:
        raise HTTPException(status_code=400, detail="run_id 不能为空")

    snap = _context_store.read_run_response(project_id=project_id, run_id=run_id)
    if not isinstance(snap, dict):
        raise HTTPException(status_code=404, detail="run snapshot not found")

    shots = snap.get("shots")
    if not isinstance(shots, list) or not shots:
        raise HTTPException(status_code=422, detail="snapshot missing shots")

    sc = db.query(models.Scene).filter(models.Scene.id == scene_id).first()
    if not sc:
        raise HTTPException(status_code=404, detail="Scene not found")
    # 校验 project_id
    ep = db.query(models.Episode).filter(models.Episode.id == sc.episode_id).first()
    if not ep or int(ep.project_id) != project_id:
        raise HTTPException(status_code=400, detail="scene_id 不属于该 project_id")

    if payload.overwrite_shots:
        existing = db.query(models.Shot).filter(models.Shot.scene_id == sc.id).all()
        for sh in existing:
            db.delete(sh)
        db.flush()

    for idx, item in enumerate(shots, start=1):
        if not isinstance(item, dict):
            continue
        title = str(item.get("title") or f"镜头 {idx}")
        action_text = str(item.get("action_text") or "").strip()
        if not action_text:
            continue
        dialogue = item.get("dialogue")
        dialogue = str(dialogue).strip() if dialogue is not None else None
        prompt = item.get("prompt")
        prompt = str(prompt).strip() if prompt is not None else None
        negative_prompt = item.get("negative_prompt")
        negative_prompt = str(negative_prompt).strip() if negative_prompt is not None else None

        db.add(
            models.Shot(
                scene_id=sc.id,
                sequence_number=idx,
                title=title,
                action_text=action_text,
                dialogue=dialogue,
                prompt=prompt or "",
                negative_prompt=negative_prompt,
                status="draft",
            )
        )

    db.commit()
    return {"ok": True, "scene_id": sc.id, "run_id": run_id}

