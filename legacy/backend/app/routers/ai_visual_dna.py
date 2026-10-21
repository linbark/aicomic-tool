import json
from typing import Any, Dict, Optional

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel
from sqlalchemy.orm import Session

from ..database import get_db
from ..services import prompt_registry
from ..services.llm_client import LlmChatSettings
from ..services.project_lookup import resolve_project_pk
from ..services.prompt_composer import PromptModules, compose_system_prompt_xml
from .ai_shared import _chat_client, _context_store, _mask_settings, _read_settings_raw, _repair_json_with_same_agent


router = APIRouter(tags=["AI (DeepSeek)"])


class VisualDnaIngestRequest(BaseModel):
    project_id: str
    item_id: int
    asset_file_path: str
    version: str = "v1"
    run_id: str


class VisualDnaIngestResponse(BaseModel):
    run_id: str
    visual_dna: Dict[str, Any]
    qc_report: Optional[Dict[str, Any]] = None


@router.post("/visual-dna/ingest", response_model=VisualDnaIngestResponse)
async def ingest_visual_dna(req: VisualDnaIngestRequest, db: Session = Depends(get_db)):
    """
    从图片文件路径摄取 Visual DNA JSON。
    注意：当前 LLM 接口为纯 chat，不支持 vision。此 API 先读取文件元数据/路径信息，
    然后通过文本描述让 LLM 生成 Visual DNA JSON（后续可扩展为真正的 vision provider）。
    """
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    project_id = resolve_project_pk(db, req.project_id)
    item_id = int(req.item_id)
    file_path = (req.asset_file_path or "").strip()
    if not file_path:
        raise HTTPException(status_code=400, detail="asset_file_path 不能为空")

    # 校验文件路径在 /files 目录下（安全约束）
    from ..services.app_paths import data_dir
    import os

    data_dir_path = data_dir()
    full_path = os.path.join(data_dir_path, file_path.lstrip("/"))
    if not os.path.exists(full_path) or not full_path.startswith(os.path.abspath(data_dir_path)):
        raise HTTPException(status_code=400, detail="文件路径无效或不在允许的目录下")

    run_id = (req.run_id or "").strip()
    if not run_id:
        raise HTTPException(status_code=400, detail="run_id 不能为空")

    # 读取 series_bible（用于上下文）
    series_bible = _context_store.get_series_bible(project_id=project_id, version="v1") or {}

    # 构建 ingest prompt
    ingest_role = prompt_registry.get_template_prompt("visual_dna_ingest_system")
    ingest_system = compose_system_prompt_xml(
        PromptModules(
            role_definition=ingest_role,
            series_bible=series_bible,
            constraints=[
                "输出必须是严格的 JSON object，符合指定的 schema。",
                "visual_dna 字段必须包含角色的核心视觉特征（不可变）。",
                "technical_specs 字段包含光影/角度等可变参数。",
            ],
            instruction=[
                f"分析文件路径：{file_path}",
                "提取角色的 Visual DNA（面部特征、体型、发型、服装等）。",
                "提取技术参数（光影、角度、构图、色彩）。",
                "生成 stable_diffusion_tags（逗号分隔的标签串）。",
                "仅输出 JSON，不要任何额外文本。",
            ],
            output_format="json",
        )
    )

    # 注意：当前 LLM 不支持 vision，这里用文件路径作为文本描述
    # 后续可扩展为真正的 vision API（如 GPT-4V、Claude Vision）
    user_prompt = (
        f"请分析以下图片文件并提取 Visual DNA：\n文件路径：{file_path}\n\n"
        "如果无法直接查看图片，请基于文件路径和可能的文件名信息进行合理推断。"
    )

    ingest_content = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=settings.temperature,
            max_tokens=settings.max_tokens,
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": ingest_system},
            {"role": "user", "content": user_prompt},
        ],
    )

    _context_store.snapshot_stage(project_id=project_id, run_id=run_id, stage_name="ingest.raw", data={"text": ingest_content})

    # 解析 JSON
    from ..services.json_extract import extract_json_any

    parsed = extract_json_any(ingest_content)
    if not isinstance(parsed, dict):
        repaired = await _repair_json_with_same_agent(
            llm_settings=LlmChatSettings(
                base_url=settings.base_url,
                api_key=raw.get("api_key") or "",
                model=settings.model,
                temperature=0.0,
                max_tokens=settings.max_tokens,
                timeout_seconds=settings.timeout_seconds,
            ),
            system_prompt=ingest_system,
            bad_output_text=str(ingest_content),
            error_hint="Root type is not JSON object",
            expected_hint="JSON object with keys: character_core, technical_specs, stable_diffusion_tags",
        )
        parsed = repaired
        if not isinstance(parsed, dict):
            raise HTTPException(status_code=422, detail="Visual DNA output must be a JSON object")

    visual_dna = parsed
    _context_store.snapshot_stage(project_id=project_id, run_id=run_id, stage_name="ingest.parsed", data={"visual_dna": visual_dna})

    # 写入 context store
    _context_store.put_visual_dna(project_id=project_id, item_id=item_id, data=visual_dna, version=req.version)

    # 落盘 run 快照
    _context_store.snapshot_run(
        project_id=project_id,
        run_id=run_id,
        request=req.model_dump(),
        response={"visual_dna": visual_dna},
        meta={"workflow": "visual_dna_ingest", "item_id": item_id},
    )

    return VisualDnaIngestResponse(
        run_id=run_id,
        visual_dna=visual_dna,
        qc_report=None,  # 可选：后续可加入 QC 检查
    )
