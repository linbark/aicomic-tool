from typing import Any, Dict, Literal, Optional

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel
from sqlalchemy.orm import Session

from ..services.json_extract import extract_json_any
from ..database import get_db
from ..services.project_lookup import resolve_project_pk
from .ai_shared import _chat_client, _context_store, _get_llm_settings


router = APIRouter(tags=["AI (DeepSeek)"])


# ==========================
# Context 管理（file-first）
# ==========================


class ContextWriteRequest(BaseModel):
    data: Dict[str, Any]
    version: str = "v1"


class ContextReadResponse(BaseModel):
    project_id: str
    kind: Literal["series_bible", "visual_dna"]
    version: str
    exists: bool
    data: Optional[Dict[str, Any]] = None


class ContextWriteResponse(BaseModel):
    project_id: str
    kind: Literal["series_bible", "visual_dna"]
    version: str
    path: str
    updated_at_ms: int


@router.get("/context/series-bible", response_model=ContextReadResponse)
def get_series_bible(project_id: str, version: str = "v1", db: Session = Depends(get_db)):
    pid = resolve_project_pk(db, project_id)
    data = _context_store.get_series_bible(project_id=pid, version=version)
    return ContextReadResponse(
        project_id=str(project_id),
        kind="series_bible",
        version=version,
        exists=bool(data),
        data=data,
    )


@router.put("/context/series-bible", response_model=ContextWriteResponse)
def put_series_bible(project_id: str, payload: ContextWriteRequest, db: Session = Depends(get_db)):
    # 校验 version 格式（允许 v1, v2, ...）
    import re

    if not re.match(r"^v\d+$", payload.version):
        raise HTTPException(status_code=400, detail=f"version 格式无效，应为 v1, v2, ... 格式，当前: {payload.version}")
    # 校验 data 必须是 dict
    if not isinstance(payload.data, dict):
        raise HTTPException(status_code=400, detail="data 必须是 JSON object")
    pid = resolve_project_pk(db, project_id)
    meta = _context_store.put_series_bible(project_id=pid, data=payload.data, version=payload.version)
    return ContextWriteResponse(
        project_id=str(project_id),
        kind="series_bible",
        version=meta.version,
        path=meta.path,
        updated_at_ms=meta.updated_at_ms,
    )


@router.get("/context/visual-dna", response_model=ContextReadResponse)
def get_visual_dna(project_id: str, item_id: int, version: str = "v1", db: Session = Depends(get_db)):
    pid = resolve_project_pk(db, project_id)
    data = _context_store.get_visual_dna(project_id=pid, item_id=item_id, version=version)
    return ContextReadResponse(
        project_id=str(project_id),
        kind="visual_dna",
        version=version,
        exists=bool(data),
        data=data,
    )


@router.put("/context/visual-dna", response_model=ContextWriteResponse)
def put_visual_dna(project_id: str, item_id: int, payload: ContextWriteRequest, db: Session = Depends(get_db)):
    # 校验 version 格式（允许 v1, v2, ...）
    import re

    if not re.match(r"^v\d+$", payload.version):
        raise HTTPException(status_code=400, detail=f"version 格式无效，应为 v1, v2, ... 格式，当前: {payload.version}")
    # 校验 data 必须是 dict
    if not isinstance(payload.data, dict):
        raise HTTPException(status_code=400, detail="data 必须是 JSON object")
    pid = resolve_project_pk(db, project_id)
    meta = _context_store.put_visual_dna(project_id=pid, item_id=item_id, data=payload.data, version=payload.version)
    return ContextWriteResponse(
        project_id=str(project_id),
        kind="visual_dna",
        version=meta.version,
        path=meta.path,
        updated_at_ms=meta.updated_at_ms,
    )


# ==========================
# Project Outline（项目级大纲）
# ==========================


class ProjectOutlineReadResponse(BaseModel):
    project_id: str
    version: str
    exists: bool
    data: Optional[Dict[str, Any]] = None


class ProjectOutlineWriteRequest(BaseModel):
    data: Dict[str, Any]
    version: str = "v1"


class ProjectOutlineWriteResponse(BaseModel):
    project_id: str
    version: str
    path: str
    updated_at_ms: int


@router.get("/context/project-outline", response_model=ProjectOutlineReadResponse)
def get_project_outline(project_id: str, version: str = "v1", db: Session = Depends(get_db)):
    """获取项目级大纲"""
    pid = resolve_project_pk(db, project_id)
    data = _context_store.get_project_outline(project_id=pid, version=version)
    return ProjectOutlineReadResponse(
        project_id=str(project_id),
        version=version,
        exists=bool(data),
        data=data,
    )


@router.put("/context/project-outline", response_model=ProjectOutlineWriteResponse)
def put_project_outline(project_id: str, payload: ProjectOutlineWriteRequest, db: Session = Depends(get_db)):
    """保存项目级大纲"""
    import re

    if not re.match(r"^v\d+$", payload.version):
        raise HTTPException(status_code=400, detail=f"version 格式无效，应为 v1, v2, ... 格式，当前: {payload.version}")
    if not isinstance(payload.data, dict):
        raise HTTPException(status_code=400, detail="data 必须是 JSON object")
    pid = resolve_project_pk(db, project_id)
    meta = _context_store.put_project_outline(project_id=pid, data=payload.data, version=payload.version)
    return ProjectOutlineWriteResponse(
        project_id=str(project_id),
        version=meta.version,
        path=meta.path,
        updated_at_ms=meta.updated_at_ms,
    )


class ProjectOutlineGenerateRequest(BaseModel):
    project_id: str
    input_text: str  # 故事灵感/概要
    num_episodes: int = 12  # 预计集数


class ProjectOutlineGenerateResponse(BaseModel):
    project_outline: Dict[str, Any]


@router.post("/project-outline-generate", response_model=ProjectOutlineGenerateResponse)
async def project_outline_generate(req: ProjectOutlineGenerateRequest, db: Session = Depends(get_db)):
    """
    生成项目级大纲（整体故事概要 + 分集大纲）
    """
    settings = _get_llm_settings()

    system_prompt = """你是一位专业的漫剧编剧和故事架构师。
你的任务是根据用户提供的故事灵感，生成一份完整的项目级大纲。

输出必须是严格的 JSON 对象，包含以下字段：
{
  "title": "作品标题",
  "logline": "一句话故事概要（50字以内）",
  "genre": "类型（如：都市情感、古风悬疑、科幻冒险等）",
  "target_audience": "目标受众",
  "total_episodes": 预计总集数(整数),
  "synopsis": "整体故事概要（200-500字）",
  "main_characters": [
    {"name": "角色名", "role": "主角/配角/反派", "description": "角色简介"}
  ],
  "story_arc": "整体故事弧线描述",
  "episode_outlines": [
    {"episode": 1, "title": "分集标题", "synopsis": "本集概要（50-100字）", "key_events": ["关键事件1", "关键事件2"]}
  ],
  "themes": ["主题1", "主题2"],
  "visual_style": "视觉风格描述"
}

注意：
1. episode_outlines 数组的长度应与 total_episodes 一致
2. 确保故事有清晰的开端、发展、高潮、结局
3. 各集之间要有连贯性和递进关系
4. 不要输出 JSON 以外的任何内容"""

    user_prompt = f"""故事灵感/概要：
{req.input_text}

预计集数：{req.num_episodes} 集

请生成完整的项目级大纲。"""

    content = await _chat_client.chat(
        settings=settings,
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt},
        ],
    )

    # 解析 JSON
    parsed = extract_json_any(content, expected_hint="JSON object with project outline")
    if parsed is None or not isinstance(parsed, dict):
        raise HTTPException(status_code=502, detail=f"AI 返回无法解析为 JSON: {content[:500]}")

    # 保存到 context
    pid = resolve_project_pk(db, req.project_id)
    _context_store.put_project_outline(project_id=pid, data=parsed, version="v1")

    return ProjectOutlineGenerateResponse(project_outline=parsed)


class ProjectOutlineOptimizeRequest(BaseModel):
    project_id: str
    current_outline: str  # 当前大纲 JSON 字符串
    optimization_instructions: str = ""  # 可选的优化指令


class ProjectOutlineOptimizeResponse(BaseModel):
    project_outline: Dict[str, Any]
    changes_summary: str


@router.post("/project-outline-optimize", response_model=ProjectOutlineOptimizeResponse)
async def project_outline_optimize(req: ProjectOutlineOptimizeRequest, db: Session = Depends(get_db)):
    """
    优化项目级大纲
    """
    settings = _get_llm_settings()

    system_prompt = """你是一位资深的漫剧故事编辑和优化专家。
你的任务是优化用户提供的项目级大纲，使其更加精炼、连贯、引人入胜。

输出必须是严格的 JSON 对象，包含以下字段：
{
  "project_outline": {完整的优化后大纲，格式与原大纲相同},
  "changes_summary": "优化说明（列出主要修改点，100字以内）"
}

优化原则：
1. 保持原有的核心故事和角色设定
2. 增强情节的戏剧张力和情感深度
3. 确保各集之间的节奏和递进合理
4. 优化人物弧线的发展
5. 如有用户指令，优先按用户要求优化

不要输出 JSON 以外的任何内容。"""

    user_prompt = f"""当前项目大纲：
{req.current_outline}

优化指令：
{req.optimization_instructions or "请根据专业判断进行全面优化"}

请输出优化后的大纲。"""

    content = await _chat_client.chat(
        settings=settings,
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt},
        ],
    )

    # 解析 JSON
    parsed = extract_json_any(content, expected_hint="JSON object with project_outline and changes_summary")
    if parsed is None or not isinstance(parsed, dict):
        raise HTTPException(status_code=502, detail=f"AI 返回无法解析为 JSON: {content[:500]}")

    project_outline = parsed.get("project_outline") or parsed
    changes_summary = parsed.get("changes_summary", "优化完成")

    # 保存到 context
    pid = resolve_project_pk(db, req.project_id)
    _context_store.put_project_outline(project_id=pid, data=project_outline, version="v1")

    return ProjectOutlineOptimizeResponse(
        project_outline=project_outline,
        changes_summary=changes_summary,
    )

