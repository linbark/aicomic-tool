"""
AI 路由聚合入口（/ai）。

说明：
- 该文件仅负责聚合各功能子模块的 APIRouter，避免单文件过大。
- 具体实现拆分在同目录下的 `ai_*.py`。
"""

from fastapi import APIRouter

from . import (
    ai_basic,
    ai_chat,
    ai_context,
    ai_prompts,
    ai_runs_files,
    ai_visual_dna,
    ai_workflows,
    ai_writing,
)

router = APIRouter(prefix="/ai", tags=["AI (DeepSeek)"])

# 基础能力与设置
router.include_router(ai_basic.router)
router.include_router(ai_prompts.router)

# 写作与编排
router.include_router(ai_writing.router)
router.include_router(ai_chat.router)
router.include_router(ai_workflows.router)

# 上下文与产物
router.include_router(ai_context.router)
router.include_router(ai_runs_files.router)
router.include_router(ai_visual_dna.router)

"""
AI 路由聚合入口（/ai）。

说明：
- 该文件仅负责聚合各功能子模块的 APIRouter，避免单文件过大。
- 具体实现拆分在同目录下的 `ai_*.py`。
"""

from fastapi import APIRouter

from . import (
    ai_basic,
    ai_chat,
    ai_context,
    ai_prompts,
    ai_runs_files,
    ai_visual_dna,
    ai_workflows,
    ai_writing,
)


router = APIRouter(prefix="/ai", tags=["AI (DeepSeek)"])

router.include_router(ai_basic.router)
router.include_router(ai_prompts.router)
router.include_router(ai_writing.router)
router.include_router(ai_context.router)
router.include_router(ai_chat.router)
router.include_router(ai_workflows.router)
router.include_router(ai_runs_files.router)
router.include_router(ai_visual_dna.router)

import json
from typing import Optional, Any, Dict, List, Literal

from fastapi import APIRouter, BackgroundTasks, HTTPException, Depends
from pydantic import BaseModel, Field

from ..services.app_paths import ai_settings_path
from ..services.json_extract import extract_json_any
from ..services.llm_client import DeepSeekChatClient, LlmChatSettings
from ..services.context_store import ContextStore, new_run_id
from ..services.prompt_composer import PromptModules, compose_system_prompt_xml
from ..services import prompt_registry
from ..workflows.schemas import BeatSheetItem, QcReport, SeriesBible, ShotSpec, PromptPair
from ..database import get_db, SessionLocal
from .. import models
from sqlalchemy.orm import Session
from ..services.project_lookup import resolve_project, resolve_project_pk
import time
import asyncio


router = APIRouter(
    prefix="/ai",
    tags=["AI (DeepSeek)"],
)

class PromptTemplateRead(BaseModel):
    key: str
    title: str
    category: str
    prompt: str
    is_builtin: bool = False
    is_modified: bool = False
    variables: List[str] = []


class PromptTemplateUpsert(BaseModel):
    title: str
    category: str
    prompt: str


class PromptTemplateCreate(BaseModel):
    key: str
    title: str
    category: str
    prompt: str


def _read_settings_raw() -> Dict[str, Any]:
    path = ai_settings_path()
    try:
        import os as _os
        if not _os.path.exists(path):
            return {}
    except Exception:
        return {}
    try:
        import json as _json
        with open(path, "r", encoding="utf-8") as f:
            return _json.load(f) or {}
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to read ai settings: {e}")


def _write_settings_raw(data: Dict[str, Any]) -> None:
    path = ai_settings_path()
    import os as _os
    import json as _json
    _os.makedirs(_os.path.dirname(path), exist_ok=True)
    try:
        with open(path, "w", encoding="utf-8") as f:
            _json.dump(data, f, ensure_ascii=False, indent=2)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to write ai settings: {e}")

def _mask_settings(raw: Dict[str, Any]) -> "AiSettingsRead":
    api_key = raw.get("api_key") or ""
    return AiSettingsRead(
        has_api_key=bool(api_key),
        base_url=raw.get("base_url") or "https://api.deepseek.com",
        model=raw.get("model") or "deepseek-chat",
        temperature=float(raw.get("temperature") or 0.2),
        max_tokens=int(raw.get("max_tokens") or 8192),
        timeout_seconds=float(raw.get("timeout_seconds") or 120.0),
    )


def _get_llm_settings() -> LlmChatSettings:
    """
    兼容旧代码：返回可直接喂给 `_chat_client.chat()` 的 LlmChatSettings。
    """
    raw = _read_settings_raw()
    masked = _mask_settings(raw)
    if not masked.has_api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置")
    return LlmChatSettings(
        base_url=str(masked.base_url),
        api_key=str(raw.get("api_key") or ""),
        model=str(masked.model),
        temperature=float(masked.temperature),
        max_tokens=int(masked.max_tokens),
        timeout_seconds=float(masked.timeout_seconds),
    )

_chat_client = DeepSeekChatClient()
_context_store = ContextStore()


def _smoke_check_workflows_module() -> None:
    """
    最小冒烟自检：
    - 不调用 LLM
    - 仅验证 workflows 代码路径能构建 system prompt（避免 NameError/导入问题）
    """
    try:
        _ = compose_system_prompt_xml(
            PromptModules(
                role_definition="smoke",
                series_bible={},
                constraints=["only json"],
                instruction=["return {}"],
                output_format="json",
                extra_blocks={"beat_sheet": json.dumps([], ensure_ascii=False, indent=2)},
            )
        )
    except Exception as e:
        # 不阻断服务启动，但打印错误便于排查
        print(f"[AI][SmokeCheck][Warning] workflows module check failed: {e}")


_smoke_check_workflows_module()


async def _repair_json_with_same_agent(
    *,
    llm_settings: LlmChatSettings,
    system_prompt: str,
    bad_output_text: str,
    error_hint: str,
    expected_hint: str,
) -> Any:
    """
    轻量修复回路：把错误输出与错误原因喂回同一个 agent，请它“只输出修复后的 JSON”。
    """
    repair_user = json.dumps(
        {
            "task": "repair_json_output",
            "error": error_hint,
            "expected": expected_hint,
            "bad_output": bad_output_text,
        },
        ensure_ascii=False,
        indent=2,
    )
    repaired = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=llm_settings.base_url,
            api_key=llm_settings.api_key,
            model=llm_settings.model,
            temperature=0.0,
            max_tokens=llm_settings.max_tokens,
            timeout_seconds=llm_settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": repair_user},
        ],
    )
    return extract_json_any(repaired)


class AiSettingsRead(BaseModel):
    has_api_key: bool
    base_url: str = "https://api.deepseek.com"
    model: str = "deepseek-chat"
    temperature: float = 0.2
    max_tokens: int = 2048
    timeout_seconds: float = 120.0


class AiSettingsUpdate(BaseModel):
    # 传 null 表示不改；传空字符串表示清空
    api_key: Optional[str] = Field(default=None)
    base_url: Optional[str] = Field(default=None)
    model: Optional[str] = Field(default=None)
    temperature: Optional[float] = Field(default=None)
    max_tokens: Optional[int] = Field(default=None)
    timeout_seconds: Optional[float] = Field(default=None)


@router.get("/settings", response_model=AiSettingsRead)
def get_settings():
    raw = _read_settings_raw()
    return _mask_settings(raw)


@router.put("/settings", response_model=AiSettingsRead)
def update_settings(payload: AiSettingsUpdate):
    raw = _read_settings_raw()

    if payload.api_key is not None:
        if payload.api_key == "":
            raw["api_key"] = ""
        else:
            raw["api_key"] = payload.api_key.strip()

    if payload.base_url is not None:
        raw["base_url"] = payload.base_url.strip()
    if payload.model is not None:
        raw["model"] = payload.model.strip()
    if payload.temperature is not None:
        raw["temperature"] = payload.temperature
    if payload.max_tokens is not None:
        raw["max_tokens"] = payload.max_tokens
    if payload.timeout_seconds is not None:
        raw["timeout_seconds"] = payload.timeout_seconds

    _write_settings_raw(raw)
    return get_settings()


class AiTestResponse(BaseModel):
    ok: bool
    detail: Optional[str] = None


@router.post("/test", response_model=AiTestResponse)
async def test_ai():
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        return AiTestResponse(ok=False, detail="API Key 未配置")

    try:
        content = await _chat_client.chat(
            settings=LlmChatSettings(
                base_url=settings.base_url,
                api_key=raw.get("api_key") or "",
                model=settings.model,
                temperature=0.0,
                max_tokens=8,
                timeout_seconds=settings.timeout_seconds,
            ),
            messages=[
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "Reply with exactly: OK"},
            ],
        )
        if (content or "").strip() == "OK":
            return AiTestResponse(ok=True, detail="连接成功")
        return AiTestResponse(ok=True, detail=f"连接成功（返回：{(content or '').strip()[:50]}）")
    except HTTPException as e:
        # 透出后端代理错误
        return AiTestResponse(ok=False, detail=str(e.detail))


class SplitScenesRequest(BaseModel):
    text: str
    max_scenes: int = 50


class SplitSceneItem(BaseModel):
    title: str
    description: str


class SplitShotsRequest(BaseModel):
    text: str
    max_shots: int = 80


class SplitShotItem(BaseModel):
    title: Optional[str] = None
    action_text: str


@router.post("/split-scenes", response_model=List[SplitSceneItem])
async def split_scenes(req: SplitScenesRequest):
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    user_text = (req.text or "").strip()
    if not user_text:
        return []

    system_prompt = prompt_registry.get_template_prompt("split_scenes_system", {"max_scenes": req.max_scenes})
    content = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=settings.temperature,
            max_tokens=settings.max_tokens,
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_text},
        ],
    )

    try:
        parsed = extract_json_any(content)
        if not isinstance(parsed, list):
            raise ValueError("not a list")
        out: List[SplitSceneItem] = []
        for item in parsed[: req.max_scenes]:
            if not isinstance(item, dict):
                continue
            title = str(item.get("title") or "").strip()
            desc = str(item.get("description") or "").strip()
            if not desc and not title:
                continue
            if not title:
                title = f"场{len(out)+1}"
            out.append(SplitSceneItem(title=title, description=desc))
        return out
    except Exception as e:
        raise HTTPException(status_code=422, detail=f"AI output parse failed: {e}")


@router.post("/split-shots", response_model=List[SplitShotItem])
async def split_shots(req: SplitShotsRequest):
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    user_text = (req.text or "").strip()
    if not user_text:
        return []

    system_prompt = prompt_registry.get_template_prompt("split_shots_system", {"max_shots": req.max_shots})
    content = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=settings.temperature,
            max_tokens=settings.max_tokens,
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_text},
        ],
    )

    try:
        parsed = extract_json_any(content)
        if not isinstance(parsed, list):
            raise ValueError("not a list")
        out: List[SplitShotItem] = []
        for item in parsed[: req.max_shots]:
            if not isinstance(item, dict):
                continue
            title = item.get("title")
            title = str(title).strip() if title is not None else None
            action = str(item.get("action_text") or "").strip()
            if not action:
                continue
            out.append(SplitShotItem(title=title or None, action_text=action))
        return out
    except Exception as e:
        raise HTTPException(status_code=422, detail=f"AI output parse failed: {e}")


# ==========================
# Prompt 模板管理
# ==========================

@router.get("/prompts", response_model=List[PromptTemplateRead])
def list_prompts():
    return [PromptTemplateRead(**item) for item in prompt_registry.list_templates_read()]


@router.post("/prompts", response_model=PromptTemplateRead)
def create_prompt(payload: PromptTemplateCreate):
    key = prompt_registry.normalize_key(payload.key)
    if not key:
        raise HTTPException(status_code=400, detail="key 不能为空")
    defaults = prompt_registry.default_prompt_templates()
    raw = prompt_registry.read_prompts_raw()
    raw_templates = raw.get("templates") if isinstance(raw, dict) else None
    if not isinstance(raw_templates, dict):
        raw_templates = {}
    if key in defaults or key in raw_templates:
        raise HTTPException(status_code=400, detail="key 已存在")

    raw_templates[key] = {
        "title": payload.title,
        "category": payload.category,
        "prompt": payload.prompt,
        "variables": [],
    }
    prompt_registry.write_prompts_raw({"templates": raw_templates})
    return PromptTemplateRead(**prompt_registry.template_to_read(key=key, tpl=raw_templates[key], defaults=defaults, raw_templates=raw_templates))


@router.put("/prompts/{key}", response_model=PromptTemplateRead)
def upsert_prompt(key: str, payload: PromptTemplateUpsert):
    key = prompt_registry.normalize_key(key)
    if not key:
        raise HTTPException(status_code=400, detail="key 不能为空")
    defaults = prompt_registry.default_prompt_templates()
    raw = prompt_registry.read_prompts_raw()
    raw_templates = raw.get("templates") if isinstance(raw, dict) else None
    if not isinstance(raw_templates, dict):
        raw_templates = {}

    # 对内置 key：写入 overrides；对非内置：等同创建/更新自定义
    base_vars = []
    if key in defaults:
        base_vars = defaults[key].get("variables") or []
    elif key in raw_templates:
        base_vars = raw_templates[key].get("variables") or []

    raw_templates[key] = {
        "title": payload.title,
        "category": payload.category,
        "prompt": payload.prompt,
        "variables": base_vars if isinstance(base_vars, list) else [],
    }
    prompt_registry.write_prompts_raw({"templates": raw_templates})

    # effective 用于 is_modified 的判断
    effective_tpl = raw_templates[key] if key in raw_templates else defaults.get(key) or {}
    return PromptTemplateRead(
        **prompt_registry.template_to_read(key=key, tpl=effective_tpl, defaults=defaults, raw_templates=raw_templates)
    )


@router.post("/prompts/{key}/reset", response_model=PromptTemplateRead)
def reset_prompt(key: str):
    key = prompt_registry.normalize_key(key)
    defaults = prompt_registry.default_prompt_templates()
    if key not in defaults:
        raise HTTPException(status_code=404, detail="该 key 不是内置模板，无法重置")

    raw = prompt_registry.read_prompts_raw()
    raw_templates = raw.get("templates") if isinstance(raw, dict) else None
    if not isinstance(raw_templates, dict):
        raw_templates = {}
    if key in raw_templates:
        raw_templates.pop(key, None)
        prompt_registry.write_prompts_raw({"templates": raw_templates})
    return PromptTemplateRead(**prompt_registry.template_to_read(key=key, tpl=defaults[key], defaults=defaults, raw_templates=raw_templates))


@router.delete("/prompts/{key}")
def delete_prompt(key: str):
    key = prompt_registry.normalize_key(key)
    defaults = prompt_registry.default_prompt_templates()
    raw = prompt_registry.read_prompts_raw()
    raw_templates = raw.get("templates") if isinstance(raw, dict) else None
    if not isinstance(raw_templates, dict):
        raw_templates = {}

    if key in defaults:
        # 删除内置模板等价于重置
        if key in raw_templates:
            raw_templates.pop(key, None)
            prompt_registry.write_prompts_raw({"templates": raw_templates})
        return {"message": "Prompt reset"}

    if key not in raw_templates:
        raise HTTPException(status_code=404, detail="Prompt not found")
    raw_templates.pop(key, None)
    prompt_registry.write_prompts_raw({"templates": raw_templates})
    return {"message": "Prompt deleted"}


# ==========================
# 写作类能力：大纲优化 / 剧本生成
# ==========================

# 记忆系统辅助函数
def _build_memory_context(retrieval_results: Dict[str, Any]) -> str:
    """将检索结果格式化为可注入 prompt 的上下文"""
    from ..services.context_assembly import assemble_memory_context
    sections = assemble_memory_context(retrieval_results)
    return sections.to_markdown()


class OutlineGenerateRequest(BaseModel):
    text: str
    project_id: Optional[int] = None  # 新增：如果提供则启用记忆检索

class OutlineGenerateResponse(BaseModel):
    text: str

@router.post("/outline-generate", response_model=OutlineGenerateResponse)
async def outline_generate(req: OutlineGenerateRequest):
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    user_text = (req.text or "").strip()
    if not user_text:
        return OutlineGenerateResponse(text="")

    system_prompt = prompt_registry.get_template_prompt("outline_generate_system")
    
    # 如果提供了 project_id，检索并注入记忆
    if req.project_id:
        try:
            from ..services.memory_retriever import get_memory_retriever
            from ..services.memory_indexer import MemoryIndexer
            
            # 确保记忆已索引
            indexer = MemoryIndexer()
            indexer.index_series_bible(project_id=req.project_id, version="v1")
            
            # 检索记忆
            retriever = get_memory_retriever()
            retrieval_results = retriever.retrieve_for_task(
                project_id=req.project_id,
                task_description=f"生成大纲: {user_text[:200]}",
            )
            
            # 格式化并注入到 system prompt
            memory_context = _build_memory_context(retrieval_results)
            if memory_context:
                system_prompt = f"{system_prompt}\n\n## 记忆上下文\n\n{memory_context}"
        except Exception as e:
            # 记忆检索失败不阻断主流程，只记录日志
            print(f"[AI][outline_generate] Memory retrieval failed: {e}")
    
    # 大纲类输出容易较长，给一个最低 max_tokens，避免中途截断
    effective_max_tokens = max(int(settings.max_tokens or 0), 4096)
    content = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=settings.temperature,
            max_tokens=effective_max_tokens,
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_text},
        ],
    )
    return OutlineGenerateResponse(text=(content or "").strip())


class OutlineOptimizeRequest(BaseModel):
    text: str
    project_id: Optional[int] = None  # 新增：如果提供则启用记忆检索


class OutlineOptimizeResponse(BaseModel):
    text: str


@router.post("/outline-optimize", response_model=OutlineOptimizeResponse)
async def outline_optimize(req: OutlineOptimizeRequest):
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    user_text = (req.text or "").strip()
    if not user_text:
        return OutlineOptimizeResponse(text="")

    system_prompt = prompt_registry.get_template_prompt("outline_optimize_system")
    
    # 如果提供了 project_id，检索并注入记忆
    if req.project_id:
        try:
            from ..services.memory_retriever import get_memory_retriever
            from ..services.memory_indexer import MemoryIndexer
            
            # 确保记忆已索引
            indexer = MemoryIndexer()
            indexer.index_series_bible(project_id=req.project_id, version="v1")
            
            # 检索记忆
            retriever = get_memory_retriever()
            retrieval_results = retriever.retrieve_for_task(
                project_id=req.project_id,
                task_description=f"优化大纲: {user_text[:200]}",
            )
            
            # 格式化并注入到 system prompt
            memory_context = _build_memory_context(retrieval_results)
            if memory_context:
                system_prompt = f"{system_prompt}\n\n## 记忆上下文\n\n{memory_context}"
        except Exception as e:
            print(f"[AI][outline_optimize] Memory retrieval failed: {e}")
    
    effective_max_tokens = max(int(settings.max_tokens or 0), 4096)
    content = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=settings.temperature,
            max_tokens=effective_max_tokens,
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_text},
        ],
    )
    return OutlineOptimizeResponse(text=(content or "").strip())


class ScriptGenerateRequest(BaseModel):
    text: str
    project_id: Optional[int] = None  # 新增：如果提供则启用记忆检索


class ScriptGenerateResponse(BaseModel):
    text: str


@router.post("/generate-script", response_model=ScriptGenerateResponse)
async def generate_script(req: ScriptGenerateRequest):
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    user_text = (req.text or "").strip()
    if not user_text:
        return ScriptGenerateResponse(text="")

    system_prompt = prompt_registry.get_template_prompt("script_generate_system")
    
    # 如果提供了 project_id，检索并注入记忆
    if req.project_id:
        try:
            from ..services.memory_retriever import get_memory_retriever
            from ..services.memory_indexer import MemoryIndexer
            
            # 确保记忆已索引
            indexer = MemoryIndexer()
            indexer.index_series_bible(project_id=req.project_id, version="v1")
            
            # 检索记忆（剧本生成需要更多上下文）
            retriever = get_memory_retriever()
            retrieval_results = retriever.retrieve_for_task(
                project_id=req.project_id,
                task_description=f"生成剧本: {user_text[:200]}",
                top_k_per_layer={
                    "L1": 15,  # 更多历史剧情
                    "L2_static": 10,  # 更多世界观设定
                    "L2_dynamic": 10,
                },
            )
            
            # 格式化并注入到 system prompt
            memory_context = _build_memory_context(retrieval_results)
            if memory_context:
                system_prompt = f"{system_prompt}\n\n## 记忆上下文\n\n{memory_context}"
        except Exception as e:
            print(f"[AI][generate_script] Memory retrieval failed: {e}")
    
    effective_max_tokens = max(int(settings.max_tokens or 0), 4096)
    content = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=settings.temperature,
            max_tokens=effective_max_tokens,
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_text},
        ],
    )
    return ScriptGenerateResponse(text=(content or "").strip())


class ScriptOptimizeRequest(BaseModel):
    text: str
    project_id: Optional[int] = None  # 新增：如果提供则启用记忆检索


class ScriptOptimizeResponse(BaseModel):
    text: str


@router.post("/script-optimize", response_model=ScriptOptimizeResponse)
async def script_optimize(req: ScriptOptimizeRequest):
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    user_text = (req.text or "").strip()
    if not user_text:
        return ScriptOptimizeResponse(text="")

    system_prompt = prompt_registry.get_template_prompt("script_optimize_system")
    
    # 如果提供了 project_id，检索并注入记忆
    if req.project_id:
        try:
            from ..services.memory_retriever import get_memory_retriever
            from ..services.memory_indexer import MemoryIndexer
            
            # 确保记忆已索引
            indexer = MemoryIndexer()
            indexer.index_series_bible(project_id=req.project_id, version="v1")
            
            # 检索记忆
            retriever = get_memory_retriever()
            retrieval_results = retriever.retrieve_for_task(
                project_id=req.project_id,
                task_description=f"优化剧本: {user_text[:200]}",
            )
            
            # 格式化并注入到 system prompt
            memory_context = _build_memory_context(retrieval_results)
            if memory_context:
                system_prompt = f"{system_prompt}\n\n## 记忆上下文\n\n{memory_context}"
        except Exception as e:
            print(f"[AI][script_optimize] Memory retrieval failed: {e}")
    
    effective_max_tokens = max(int(settings.max_tokens or 0), 4096)
    content = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=settings.temperature,
            max_tokens=effective_max_tokens,
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_text},
        ],
    )
    return ScriptOptimizeResponse(text=(content or "").strip())


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
    if not re.match(r'^v\d+$', payload.version):
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
    if not re.match(r'^v\d+$', payload.version):
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
    if not re.match(r'^v\d+$', payload.version):
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


# ==========================
# Chat Orchestrator（意图识别 + 多步编排）
# ==========================

class ChatActRequest(BaseModel):
    project_id: str
    episode_id: Optional[int] = None
    # 当前 UI Tab（用于偏置意图识别）
    current_action_key: Optional[str] = None  # outline_generate/outline_optimize/generate_script/script_optimize/...
    message: str
    ui_context: Optional[Dict[str, Any]] = None
    debug: bool = False


class ChatPlanStep(BaseModel):
    action_key: str
    # 如果留空，后端会根据上下文自动填充
    input_text: Optional[str] = None
    why: Optional[str] = None


class ChatPlan(BaseModel):
    intent_summary: str
    steps: List[ChatPlanStep]
    final_action_key: str
    # 可选：意图不明时让 planner 直接要求澄清
    needs_clarification: Optional[bool] = None
    clarifying_question: Optional[str] = None
    clarifying_options: Optional[List[str]] = None


class ChatStepTrace(BaseModel):
    step_index: int
    action_key: str
    input_text: str
    output_text_preview: str
    ms: int


class ChatMemoryTraceItem(BaseModel):
    key: str
    # 仅 debug 模式回传
    text: Optional[str] = None


class ChatActResponse(BaseModel):
    assistant_message: str
    created_run: Optional[Dict[str, Any]] = None
    # 非 debug：也可返回轻量卡片（用于前端 inline 审阅/确认）
    cards: Optional[List[Dict[str, Any]]] = None
    # debug only
    plan: Optional[Dict[str, Any]] = None
    steps_trace: Optional[List[Dict[str, Any]]] = None
    planner_prompt: Optional[str] = None
    memory_trace: Optional[List[Dict[str, Any]]] = None
    planner_raw: Optional[str] = None


def _now_ms() -> int:
    return int(time.time() * 1000)


def _trim_preview(text: str, limit: int = 300) -> str:
    t = (text or "").strip()
    return t[:limit] + ("..." if len(t) > limit else "")


class ChatActAsyncResponse(BaseModel):
    run_id: str
    status: str = "queued"  # queued|running|done|error


async def _chat_act_core(
    *,
    req: "ChatActRequest",
    db: Session,
    emit_stages: bool = False,
    run_id: Optional[str] = None,
) -> "ChatActResponse":
    """
    chat_act 核心逻辑：
    - emit_stages=True 时，把 plan/step start/end/final 写入 runs-files stages，供前端轮询展示“执行步骤”
    """
    project_id_pk = resolve_project_pk(db, req.project_id)
    if emit_stages and run_id:
        _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "running", "at_ms": _now_ms()})

    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    message = (req.message or "").strip()
    if not message:
        raise HTTPException(status_code=400, detail="message 不能为空")

    # -------- Intent clarification (rule-first) --------
    def _looks_ambiguous(msg: str) -> bool:
        m = (msg or "").strip()
        if len(m) <= 6:
            return True
        verbs = ["生成", "提取", "优化", "改", "润色", "分镜", "拆", "入库", "保存", "整理", "总结", "加快", "减少"]
        if not any(v in m for v in verbs):
            return True
        return False

    if _looks_ambiguous(message):
        resp = ChatActResponse(
            assistant_message="我需要你明确一下目标：你希望我对本集做什么？（可多选）",
            cards=[
                {
                    "type": "clarify_intent",
                    "title": "请选择你的意图",
                    "options": [
                        {"label": "提取大纲/节拍表", "value": "outline"},
                        {"label": "生成/续写剧本", "value": "script"},
                        {"label": "优化节奏（减少对白/加快推进）", "value": "pace"},
                        {"label": "拆分分镜（把某段场景拆成镜头）", "value": "storyboard"},
                        {"label": "把设定/角色变化入库并提交我确认", "value": "memory_review"},
                    ],
                    "hint": "你也可以直接说：例如“先提取大纲，再按节奏要求优化，然后生成剧本，最后提交入库变更给我确认”。",
                }
            ],
        )
        if emit_stages and run_id:
            _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.final", data=resp.model_dump())
            _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "done", "at_ms": _now_ms()})
            _context_store.snapshot_run(project_id=project_id_pk, run_id=run_id, request=req.model_dump(), response=resp.model_dump(), meta={"workflow": "chat_act"})
        return resp

    # -------- Memory (for planner / debug) --------
    memory_trace: List[Dict[str, Any]] = []
    memory_context_text = ""
    if req.project_id:
        try:
            from ..services.memory_retriever import get_memory_retriever
            from ..services.memory_indexer import MemoryIndexer

            indexer = MemoryIndexer()
            indexer.index_series_bible(project_id=project_id_pk, version="v1")
            retriever = get_memory_retriever()
            retrieval_results = retriever.retrieve_for_task(
                project_id=project_id_pk,
                task_description=f"用户意图: {message[:200]}",
            )
            memory_context_text = _build_memory_context(retrieval_results)
            if req.debug:
                for k in ["L2_static", "L1", "L2_dynamic", "negative_constraints"]:
                    try:
                        memory_trace.append({"key": k, "text": (retriever.format_for_prompt(retrieval_results) or {}).get(k)})
                    except Exception:
                        memory_trace.append({"key": k, "text": None})
        except Exception as e:
            print(f"[AI][chat_act] Memory retrieval failed: {e}")

    # -------- Planner Prompt --------
    ui_ctx = req.ui_context or {}
    current_action_key = (req.current_action_key or "").strip()
    master_script_preview = _trim_preview(str(ui_ctx.get("master_script") or ""), 1200)
    selected_input_preview = _trim_preview(str(ui_ctx.get("current_input") or ""), 800)

    planner_system = f"""你是一个“写作编排器（planner）”，负责把用户的自然语言意图拆解为可执行的动作序列。

你必须输出严格的 JSON（不要输出其它文字）。

当用户意图不明确/信息不足时：你应该输出“澄清请求”，不要输出 steps：
{{
  "needs_clarification": true,
  "clarifying_question": "一句话追问（中文）",
  "clarifying_options": ["选项1","选项2","选项3"]
}}

当你能规划执行时：输出“执行计划”，结构如下：
{{
  "intent_summary": "一句话总结用户想要什么（中文）",
  "steps": [
    {{
      "action_key": "outline_generate|outline_optimize|generate_script|script_optimize|workflow_script|workflow_storyboard|memory_extract_changeset",
      "input_text": "可选；如留空，由系统根据上下文填充",
      "why": "可选；这一步的目的"
    }}
  ],
  "final_action_key": "同上，表示最终要写入版本记录的 action"
}}

可用动作说明：
- outline_generate：从输入材料生成本集大纲（JSON）
- outline_optimize：在已有大纲基础上按意图优化大纲（JSON）
- generate_script：从大纲/材料生成剧本（文本）
- script_optimize：在已有剧本基础上按意图优化剧本（文本）
- workflow_script：运行“workflow script”（series_bible + beat_sheet + script_fountain + qc_report）
- workflow_storyboard：运行“workflow storyboard”（把场景文本拆成镜头列表）
- memory_extract_changeset：从当前产物/文本中抽取 changeset.v0 并创建 ChangeSet（后端返回审阅卡片）

硬约束：
1) 仅允许使用上述 action_key
2) steps 最多 4 步，尽量少步完成
3) 如果用户意图涉及多阶段（例如：先生成大纲再生成剧本），请拆成多步
4) final_action_key 必须等于 steps 最后一步的 action_key
5) 如果上下文不足（例如没有可优化的文本），第一步应选择生成类动作
"""

    planner_user = f"""用户输入：
{message}

当前 UI tab（偏置参考，可为空）：
{current_action_key or "(none)"}

当前 Master Script（预览，可能为空）：
{master_script_preview or "(empty)"}

当前工作台输入（预览，可能为空）：
{selected_input_preview or "(empty)"}
"""
    if memory_context_text:
        planner_user += f"\n\n记忆上下文（必须遵守，可能为空）：\n{memory_context_text}"

    planner_raw = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=min(float(settings.temperature or 0.2), 0.3),
            max_tokens=max(int(settings.max_tokens or 0), 2048),
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": planner_system},
            {"role": "user", "content": planner_user},
        ],
    )
    plan_parsed = extract_json_any(planner_raw or "")
    if not isinstance(plan_parsed, dict):
        raise HTTPException(status_code=502, detail=f"Planner 输出无法解析为 JSON: {(planner_raw or '')[:500]}")

    if bool(plan_parsed.get("needs_clarification")):
        q = str(plan_parsed.get("clarifying_question") or "").strip() or "我需要你补充一下目标/范围：你希望我具体做什么？"
        opts = plan_parsed.get("clarifying_options")
        options = []
        if isinstance(opts, list):
            options = [{"label": str(x), "value": str(x)} for x in opts[:8] if str(x).strip()]
        resp = ChatActResponse(
            assistant_message=q,
            cards=[
                {
                    "type": "clarify_intent",
                    "title": "需要澄清",
                    "options": options or None,
                    "hint": "你也可以直接用一句话说明：例如“先提取大纲，再优化节奏，然后生成剧本，最后提交入库变更给我确认”。",
                }
            ],
            plan=plan_parsed if req.debug else None,
            planner_prompt=(planner_system + "\n\n---\n\n" + planner_user) if req.debug else None,
            memory_trace=memory_trace if req.debug else None,
            planner_raw=(planner_raw or "") if req.debug else None,
        )
        if emit_stages and run_id:
            _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.plan", data={"plan": plan_parsed})
            _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.final", data=resp.model_dump())
            _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "done", "at_ms": _now_ms()})
            _context_store.snapshot_run(project_id=project_id_pk, run_id=run_id, request=req.model_dump(), response=resp.model_dump(), meta={"workflow": "chat_act"})
        return resp

    steps = plan_parsed.get("steps") or []
    if not isinstance(steps, list) or len(steps) == 0:
        raise HTTPException(status_code=502, detail=f"Planner steps 为空: {plan_parsed}")
    if len(steps) > 4:
        steps = steps[:4]
    allowed = {
        "outline_generate",
        "outline_optimize",
        "generate_script",
        "script_optimize",
        "workflow_script",
        "workflow_storyboard",
        "memory_extract_changeset",
    }
    for s in steps:
        if not isinstance(s, dict) or s.get("action_key") not in allowed:
            raise HTTPException(status_code=502, detail=f"Planner steps 包含非法 action_key: {s}")
    final_action_key = steps[-1].get("action_key")
    plan_parsed["final_action_key"] = final_action_key

    if emit_stages and run_id:
        _context_store.snapshot_stage(
            project_id=project_id_pk,
            run_id=run_id,
            stage_name="chat.plan",
            data={"plan": {"intent_summary": plan_parsed.get("intent_summary"), "steps": steps, "final_action_key": final_action_key}},
        )

    artifacts: Dict[str, Any] = {}
    steps_trace: List[Dict[str, Any]] = []
    cards: List[Dict[str, Any]] = []

    def _step_timeout_seconds(action_key: str) -> float:
        base = float(getattr(settings, "timeout_seconds", 60.0) or 60.0)
        base = max(base, 60.0)
        ak = str(action_key or "")
        if ak in ("workflow_script",):
            return max(base * 4.0, 240.0)
        if ak in ("workflow_storyboard", "memory_extract_changeset"):
            return max(base * 2.0, 180.0)
        # 普通单步（大纲/剧本）给一点 buffer，避免网络抖动造成“永远等不到”
        return max(base + 30.0, 90.0)

    for idx, s in enumerate(steps):
        ak = str(s.get("action_key"))
        in_text = (s.get("input_text") or "").strip()
        why = (s.get("why") or "").strip()

        if not in_text:
            if ak in ("outline_optimize",) and isinstance(artifacts.get("outline"), str) and artifacts.get("outline").strip():
                in_text = artifacts["outline"]
            elif ak in ("script_optimize",) and isinstance(artifacts.get("script"), str) and artifacts.get("script").strip():
                in_text = artifacts["script"]
            elif ak in ("generate_script",) and isinstance(artifacts.get("outline"), str) and artifacts.get("outline").strip():
                in_text = artifacts["outline"]
            else:
                in_text = selected_input_preview or master_script_preview or message

        if ak in ("outline_optimize", "script_optimize"):
            in_text = f"{in_text}\n\n[用户意图]\n{message}"

        if emit_stages and run_id:
            _context_store.snapshot_stage(
                project_id=project_id_pk,
                run_id=run_id,
                stage_name=f"chat.step.{idx}.start",
                data={"step_index": idx, "action_key": ak, "why": why or None, "input_preview": _trim_preview(in_text, 400), "at_ms": _now_ms()},
            )
            # 额外写入当前 step 指针（用于前端兜底显示进度）
            _context_store.snapshot_stage(
                project_id=project_id_pk,
                run_id=run_id,
                stage_name="chat.status",
                data={"status": "running", "at_ms": _now_ms(), "current_step_index": idx, "current_action_key": ak},
            )

        t0 = _now_ms()
        out_text = ""

        async def _do_step() -> str:
            nonlocal out_text
            if ak == "outline_generate":
                resp = await outline_generate(OutlineGenerateRequest(text=in_text, project_id=project_id_pk))
                out_text = (resp.text or "").strip()
                artifacts["outline"] = out_text
                return out_text
            if ak == "outline_optimize":
                resp = await outline_optimize(OutlineOptimizeRequest(text=in_text, project_id=project_id_pk))
                out_text = (resp.text or "").strip()
                artifacts["outline"] = out_text
                return out_text
            if ak == "generate_script":
                resp = await generate_script(ScriptGenerateRequest(text=in_text, project_id=project_id_pk))
                out_text = (resp.text or "").strip()
                artifacts["script"] = out_text
                return out_text
            if ak == "script_optimize":
                resp = await script_optimize(ScriptOptimizeRequest(text=in_text, project_id=project_id_pk))
                out_text = (resp.text or "").strip()
                artifacts["script"] = out_text
                return out_text
            if ak == "workflow_script":
                wf_resp = await workflow_script(
                    WorkflowScriptRequest(
                        project_id=req.project_id,
                        input_text=in_text,
                        options=WorkflowScriptOptions(),
                    )
                )
                artifacts["series_bible"] = wf_resp.series_bible
                artifacts["beat_sheet"] = wf_resp.beat_sheet
                artifacts["script_fountain"] = wf_resp.script_fountain
                artifacts["qc_report"] = wf_resp.qc_report
                out_text = (wf_resp.script_fountain or "").strip()
                return out_text
            if ak == "workflow_storyboard":
                wf_resp = await workflow_storyboard(
                    WorkflowStoryboardRequest(
                        project_id=req.project_id,
                        scene_text=in_text,
                        options=WorkflowStoryboardOptions(),
                    )
                )
                artifacts["shots"] = wf_resp.shots
                out_text = json.dumps(wf_resp.shots, ensure_ascii=False, indent=2)
                return out_text
            if ak == "memory_extract_changeset":
                from ..services.evidence_ingestor import chunk_text_to_evidences
                from ..services.changeset_extractor import extract_changeset_v0_with_llm_with_trace
                from ..services.entity_resolver import resolve_changeset_entities_with_trace
                from ..services.memory_store import get_memory_store

                store = get_memory_store()
                base_text = ""
                if artifacts.get("script_fountain"):
                    base_text += f"### script_fountain\n{artifacts.get('script_fountain')}\n\n"
                if artifacts.get("beat_sheet"):
                    try:
                        base_text += "### beat_sheet\n" + json.dumps(artifacts.get("beat_sheet"), ensure_ascii=False, indent=2) + "\n\n"
                    except Exception:
                        pass
                if artifacts.get("series_bible"):
                    try:
                        base_text += "### series_bible\n" + json.dumps(artifacts.get("series_bible"), ensure_ascii=False, indent=2) + "\n\n"
                    except Exception:
                        pass
                if not base_text:
                    base_text = master_script_preview or message

                evidences = chunk_text_to_evidences(
                    project_id=project_id_pk,
                    episode_id=int(req.episode_id) if req.episode_id else None,
                    text=base_text,
                    max_quote_chars=600,
                    tags=["chat_act"],
                )
                evidence_ids: List[str] = []
                for ev in evidences:
                    try:
                        evidence_ids.append(store.upsert_evidence(ev))
                    except Exception:
                        continue

                payload, extractor_trace = await extract_changeset_v0_with_llm_with_trace(
                    llm_settings=LlmChatSettings(
                        base_url=settings.base_url,
                        api_key=raw.get("api_key") or "",
                        model=settings.model,
                        temperature=min(float(settings.temperature or 0.2), 0.3),
                        max_tokens=max(int(settings.max_tokens or 0), 4096),
                        timeout_seconds=settings.timeout_seconds,
                    ),
                    project_id=project_id_pk,
                    episode_id=int(req.episode_id) if req.episode_id else None,
                    story_order_base=f"CH{str(req.episode_id or 1).zfill(2)}",
                    evidences=[e.model_dump() for e in evidences],
                )
                payload, resolver_trace = resolve_changeset_entities_with_trace(store=store, project_id=project_id_pk, payload=payload)
                changeset_id = store.create_changeset(project_id=project_id_pk, payload=payload, episode_id=int(req.episode_id) if req.episode_id else None)
                try:
                    store.append_changeset_review_entry(changeset_id=changeset_id, entry={"at_ms": _now_ms(), "action": "extractor_trace", "data": extractor_trace})
                except Exception:
                    pass
                try:
                    store.append_changeset_review_entry(changeset_id=changeset_id, entry={"at_ms": _now_ms(), "action": "resolver_trace", "data": resolver_trace})
                except Exception:
                    pass

                def _count_by_type(items: Any) -> Dict[str, int]:
                    out: Dict[str, int] = {}
                    if not isinstance(items, list):
                        return out
                    for it in items:
                        if not isinstance(it, dict):
                            continue
                        t = str(it.get("entity_type") or it.get("type") or "unknown")
                        out[t] = out.get(t, 0) + 1
                    return out

                ent_counts = _count_by_type(payload.get("entities") or [])
                conflicts_n = len(payload.get("conflicts") or []) if isinstance(payload.get("conflicts"), list) else 0
                summary_lines = [
                    f"- 实体：{sum(ent_counts.values())}（" + ", ".join([f"{k}:{v}" for k, v in ent_counts.items()]) + "）" if ent_counts else "- 实体：0",
                    f"- 角色切片 snapshots：{len(payload.get('snapshots') or [])}",
                    f"- 事件 events：{len(payload.get('events') or [])}",
                    f"- 状态变更 state_changes：{len(payload.get('state_changes') or [])}",
                    f"- 冲突 conflicts：{conflicts_n}",
                ]
                cards.append(
                    {
                        "type": "review_changeset",
                        "changeset_id": changeset_id,
                        "title": "需要确认：更新设定/事件（ChangeSet）",
                        "summary": "\n".join(summary_lines),
                        "actions": [
                            {"action": "approve_changeset", "label": "确认提交", "changeset_id": changeset_id},
                            {"action": "reject_changeset", "label": "驳回", "changeset_id": changeset_id},
                        ],
                    }
                )
                out_text = f"已生成待审阅变更单：{changeset_id}（evidence={len(evidence_ids)}）"
                return out_text
            raise HTTPException(status_code=400, detail=f"Unsupported action_key: {ak}")

        try:
            out_text = await asyncio.wait_for(_do_step(), timeout=_step_timeout_seconds(ak))
        except Exception as e:
            if emit_stages and run_id:
                err_text = f"{type(e).__name__}: {e}"
                _context_store.snapshot_stage(
                    project_id=project_id_pk,
                    run_id=run_id,
                    stage_name=f"chat.step.{idx}.error",
                    data={"step_index": idx, "action_key": ak, "error": err_text, "at_ms": _now_ms()},
                )
                _context_store.snapshot_stage(
                    project_id=project_id_pk,
                    run_id=run_id,
                    stage_name="chat.error",
                    data={"error": err_text, "step_index": idx, "action_key": ak, "at_ms": _now_ms()},
                )
                _context_store.snapshot_stage(
                    project_id=project_id_pk,
                    run_id=run_id,
                    stage_name="chat.status",
                    data={"status": "error", "at_ms": _now_ms(), "current_step_index": idx, "current_action_key": ak},
                )
            raise

        dt = _now_ms() - t0
        steps_trace.append(
            {
                "step_index": idx,
                "action_key": ak,
                "input_text": in_text if req.debug else _trim_preview(in_text, 300),
                "output_text_preview": _trim_preview(out_text, 500),
                "ms": int(dt),
            }
        )

        if emit_stages and run_id:
            _context_store.snapshot_stage(
                project_id=project_id_pk,
                run_id=run_id,
                stage_name=f"chat.step.{idx}.end",
                data={"step_index": idx, "action_key": ak, "ms": int(dt), "output_preview": _trim_preview(out_text, 800), "at_ms": _now_ms()},
            )

    created_run = None
    persistable = {"outline_generate", "generate_script", "script_optimize", "workflow_script", "workflow_storyboard"}
    if req.episode_id and final_action_key in persistable:
        if final_action_key in ("generate_script", "script_optimize"):
            final_output = str(artifacts.get("script") or "")
        elif final_action_key == "workflow_script":
            final_output = str(artifacts.get("script_fountain") or "")
        elif final_action_key == "workflow_storyboard":
            try:
                final_output = json.dumps(artifacts.get("shots") or [], ensure_ascii=False, indent=2)
            except Exception:
                final_output = str(artifacts.get("shots") or "")
        else:
            final_output = str(artifacts.get("outline") or "")
        persist_action_key = final_action_key
        if persist_action_key == "outline_optimize":
            persist_action_key = "outline_generate"
        db_obj = models.AiActionRun(
            project_id=project_id_pk,
            target_type="episode",
            target_id=int(req.episode_id),
            action_key=str(persist_action_key),
            input_text=message,
            output_text=final_output,
            meta_data={
                "source": "chat",
                "intent": plan_parsed.get("intent_summary"),
                "planner_final_action_key": final_action_key,
                "plan": plan_parsed if req.debug else None,
            },
        )
        db.add(db_obj)
        db.commit()
        db.refresh(db_obj)
        created_run = {"id": db_obj.id, "action_key": db_obj.action_key, "created_at": str(db_obj.created_at)}

    assistant_msg = f"已完成：{plan_parsed.get('intent_summary') or '执行完成'}（最终动作：{final_action_key}）"
    resp = ChatActResponse(
        assistant_message=assistant_msg,
        created_run=created_run,
        cards=cards or None,
        plan=plan_parsed if req.debug else None,
        steps_trace=steps_trace if req.debug else None,
        planner_prompt=(planner_system + "\n\n---\n\n" + planner_user) if req.debug else None,
        memory_trace=memory_trace if req.debug else None,
        planner_raw=(planner_raw or "") if req.debug else None,
    )

    if emit_stages and run_id:
        _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.final", data=resp.model_dump())
        _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "done", "at_ms": _now_ms()})
        _context_store.snapshot_run(project_id=project_id_pk, run_id=run_id, request=req.model_dump(), response=resp.model_dump(), meta={"workflow": "chat_act"})
    return resp

@router.post("/chat/act", response_model=ChatActResponse)
async def chat_act(req: ChatActRequest, db: Session = Depends(get_db)):
    """
    对话驱动编排：
    1) 通过 LLM 生成 JSON 计划（steps）
    2) 顺序执行 steps（复用现有原子能力：大纲/剧本生成与优化）
    3) 仅将最终结果写入 AiActionRun（source=chat），并返回 debug trace（可选）
    """
    return await _chat_act_core(req=req, db=db, emit_stages=False, run_id=None)


@router.post("/chat/act_async", response_model=ChatActAsyncResponse)
def chat_act_async(req: ChatActRequest, bg: BackgroundTasks):
    """
    异步版：返回 run_id；执行过程写入 runs-files stages，供前端轮询展示执行步骤。
    """
    run_id = new_run_id()
    _db = SessionLocal()
    try:
        project_id_pk = resolve_project_pk(_db, req.project_id)
    finally:
        try:
            _db.close()
        except Exception:
            pass
    _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "queued", "at_ms": _now_ms()})
    payload = req.model_dump()

    def _runner():
        db = SessionLocal()
        try:
            import anyio

            async def _go():
                try:
                    await _chat_act_core(req=ChatActRequest(**payload), db=db, emit_stages=True, run_id=run_id)
                except Exception as e:
                    _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.error", data={"error": str(e), "at_ms": _now_ms()})
                    _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "error", "at_ms": _now_ms()})

            anyio.run(_go)
        finally:
            try:
                db.close()
            except Exception:
                pass

    # IMPORTANT: 不使用 BackgroundTasks（其执行在同一 worker 线程，可能阻塞事件循环，导致轮询接口也卡死）。
    # 这里用 daemon thread 真正后台执行，确保 /ai/runs-files 等轻量查询不会被长耗时 LLM 调用阻塞。
    import threading
    threading.Thread(target=_runner, daemon=True).start()
    return ChatActAsyncResponse(run_id=run_id, status="queued")


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
    # workflow 输出通常更长，给一个最低 max_tokens，避免中途截断
    effective_max_tokens = max(int(settings.max_tokens or 0), 4096)

    # 读取已有 series_bible（可为空）
    existing_bible = _context_store.get_series_bible(project_id=project_id, version="v1")

    # 检索并注入记忆（增强一致性）
    memory_context = ""
    try:
        from ..services.memory_retriever import get_memory_retriever
        from ..services.memory_indexer import MemoryIndexer
        
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
    extra_blocks = {}
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
            max_tokens=effective_max_tokens,
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
                max_tokens=effective_max_tokens,
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
        beat_sheet_items = [BeatSheetItem.model_validate(x).model_dump() for x in (beat_sheet_raw if isinstance(beat_sheet_raw, list) else [])]
    except Exception as e:
        # repair once
        repaired = await _repair_json_with_same_agent(
            llm_settings=LlmChatSettings(
                base_url=settings.base_url,
                api_key=raw.get("api_key") or "",
                model=settings.model,
                temperature=settings.temperature,
                max_tokens=effective_max_tokens,
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
            beat_sheet_items = [BeatSheetItem.model_validate(x).model_dump() for x in (beat_sheet_raw if isinstance(beat_sheet_raw, list) else [])]
        except Exception as e2:
            raise HTTPException(status_code=422, detail=f"Architect output schema invalid: {e2}")
    beat_sheet = beat_sheet_items
    _context_store.snapshot_stage(project_id=project_id, run_id=run_id, stage_name="architect.parsed", data={"series_bible": series_bible, "beat_sheet": beat_sheet})

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
            max_tokens=effective_max_tokens,
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
                max_tokens=effective_max_tokens,
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
        from ..services.state_extractor import StateChangeExtractor
        from ..services.memory_store import get_memory_store
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
            summary_text = f"章节摘要：包含 {len(beat_sheet)} 个节拍。主要节拍：{', '.join([b.get('title', '') or b.get('description', '')[:30] for b in beat_sheet[:5]])}"
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
                max_tokens=effective_max_tokens,
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
                        max_tokens=effective_max_tokens,
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
        _context_store.snapshot_stage(project_id=project_id, run_id=run_id, stage_name=f"qc.parsed.{_i+1}", data={"qc_report": qc_report, "script_fountain": script_fountain})
        
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
                max_tokens=effective_max_tokens,
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
        from ..services.memory_retriever import get_memory_retriever
        from ..services.memory_indexer import MemoryIndexer
        
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
    extra_blocks = {
        "locked_visual_dna": json.dumps(visual_dna_list, ensure_ascii=False, indent=2) if visual_dna_list else "",
    }
    if memory_context:
        extra_blocks["memory_context"] = memory_context
    
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
            extra_blocks=extra_blocks,
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
            prompt_pairs = []
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


# ==========================
# Run 快照审计 API
# ==========================

@router.get("/runs-files")
def list_runs_files(project_id: str, db: Session = Depends(get_db)):
    """
    列出项目的所有 run 快照（仅返回 meta 信息）。
    """
    pid = resolve_project_pk(db, project_id)
    runs = _context_store.list_runs(project_id=pid)
    return {"project_id": str(project_id), "runs": runs}


@router.get("/runs-files/{run_id}")
def get_run_file(project_id: str, run_id: str, db: Session = Depends(get_db)):
    """
    读取指定 run 的完整信息（request + response + meta）。
    """
    pid = resolve_project_pk(db, project_id)
    run_data = _context_store.read_run(project_id=pid, run_id=run_id)
    if not run_data:
        raise HTTPException(status_code=404, detail="Run not found")
    return {"project_id": str(project_id), "run_id": run_id, **run_data}


@router.get("/runs-files/{run_id}/stages")
def list_run_stages(project_id: str, run_id: str, db: Session = Depends(get_db)):
    """
    列出该 run 的所有 stage 名称。
    """
    pid = resolve_project_pk(db, project_id)
    stages = _context_store.list_stages(project_id=pid, run_id=run_id)
    return {"project_id": str(project_id), "run_id": run_id, "stages": stages}


@router.get("/runs-files/{run_id}/stages/{stage_name}")
def get_run_stage(project_id: str, run_id: str, stage_name: str, db: Session = Depends(get_db)):
    """
    读取指定 stage 的内容。
    """
    pid = resolve_project_pk(db, project_id)
    stage_data = _context_store.read_stage(project_id=pid, run_id=run_id, stage_name=stage_name)
    if stage_data is None:
        raise HTTPException(status_code=404, detail="Stage not found")
    return {"project_id": str(project_id), "run_id": run_id, "stage_name": stage_name, "data": stage_data}


# ==========================
# Visual DNA 摄取（图片→JSON）
# ==========================

class VisualDnaIngestRequest(BaseModel):
    project_id: str
    item_id: int
    asset_file_path: str
    version: str = "v1"


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

    run_id = new_run_id()

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
    user_prompt = f"请分析以下图片文件并提取 Visual DNA：\n文件路径：{file_path}\n\n如果无法直接查看图片，请基于文件路径和可能的文件名信息进行合理推断。"

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


