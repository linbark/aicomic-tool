import json
from typing import Optional, Any, Dict, List, Literal

from fastapi import APIRouter, HTTPException, Depends
from pydantic import BaseModel, Field

from ..services.app_paths import ai_settings_path
from ..services.json_extract import extract_json_any
from ..services.llm_client import DeepSeekChatClient, LlmChatSettings
from ..services.context_store import ContextStore, new_run_id
from ..services.prompt_composer import PromptModules, compose_system_prompt_xml
from ..services import prompt_registry
from ..workflows.schemas import BeatSheetItem, QcReport, SeriesBible, ShotSpec, PromptPair
from ..database import get_db
from .. import models
from sqlalchemy.orm import Session


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
        max_tokens=int(raw.get("max_tokens") or 2048),
        timeout_seconds=float(raw.get("timeout_seconds") or 120.0),
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

class OutlineGenerateRequest(BaseModel):
    text: str

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
    return OutlineGenerateResponse(text=(content or "").strip())


class OutlineOptimizeRequest(BaseModel):
    text: str


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
    return OutlineOptimizeResponse(text=(content or "").strip())


class ScriptGenerateRequest(BaseModel):
    text: str


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
    return ScriptGenerateResponse(text=(content or "").strip())


class ScriptOptimizeRequest(BaseModel):
    text: str


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
    return ScriptOptimizeResponse(text=(content or "").strip())


# ==========================
# Context 管理（file-first）
# ==========================

class ContextWriteRequest(BaseModel):
    data: Dict[str, Any]
    version: str = "v1"


class ContextReadResponse(BaseModel):
    project_id: int
    kind: Literal["series_bible", "visual_dna"]
    version: str
    exists: bool
    data: Optional[Dict[str, Any]] = None


class ContextWriteResponse(BaseModel):
    project_id: int
    kind: Literal["series_bible", "visual_dna"]
    version: str
    path: str
    updated_at_ms: int


@router.get("/context/series-bible", response_model=ContextReadResponse)
def get_series_bible(project_id: int, version: str = "v1"):
    data = _context_store.get_series_bible(project_id=project_id, version=version)
    return ContextReadResponse(
        project_id=project_id,
        kind="series_bible",
        version=version,
        exists=bool(data),
        data=data,
    )


@router.put("/context/series-bible", response_model=ContextWriteResponse)
def put_series_bible(project_id: int, payload: ContextWriteRequest):
    meta = _context_store.put_series_bible(project_id=project_id, data=payload.data, version=payload.version)
    return ContextWriteResponse(
        project_id=project_id,
        kind="series_bible",
        version=meta.version,
        path=meta.path,
        updated_at_ms=meta.updated_at_ms,
    )


@router.get("/context/visual-dna", response_model=ContextReadResponse)
def get_visual_dna(project_id: int, item_id: int, version: str = "v1"):
    data = _context_store.get_visual_dna(project_id=project_id, item_id=item_id, version=version)
    return ContextReadResponse(
        project_id=project_id,
        kind="visual_dna",
        version=version,
        exists=bool(data),
        data=data,
    )


@router.put("/context/visual-dna", response_model=ContextWriteResponse)
def put_visual_dna(project_id: int, item_id: int, payload: ContextWriteRequest):
    meta = _context_store.put_visual_dna(project_id=project_id, item_id=item_id, data=payload.data, version=payload.version)
    return ContextWriteResponse(
        project_id=project_id,
        kind="visual_dna",
        version=meta.version,
        path=meta.path,
        updated_at_ms=meta.updated_at_ms,
    )


# ==========================
# Workflows（后端统一编排）
# ==========================

class WorkflowScriptOptions(BaseModel):
    qc_loops: int = 1
    max_scenes: int = 50
    derived_split_scenes: bool = False


class WorkflowScriptRequest(BaseModel):
    project_id: int
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
async def workflow_script(req: WorkflowScriptRequest):
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    user_text = (req.input_text or "").strip()
    if not user_text:
        raise HTTPException(status_code=400, detail="input_text 不能为空")

    run_id = new_run_id()
    project_id = int(req.project_id)

    # 读取已有 series_bible（可为空）
    existing_bible = _context_store.get_series_bible(project_id=project_id, version="v1")

    # 1) 架构师：产出 series_bible + beat_sheet
    architect_role = prompt_registry.get_template_prompt("architect_system")
    architect_system = compose_system_prompt_xml(
        PromptModules(
            role_definition=architect_role,
            series_bible=existing_bible,
            constraints=[
                "视觉优先：忽略内心独白，只提取可被镜头呈现的信息。",
                "输出必须是 JSON object，且仅包含 keys: series_bible(object), beat_sheet(array)。",
            ],
            instruction=[
                "阅读 user 输入（可能是大纲/章节/需求）。",
                "生成 series_bible（世界观规则、角色、视觉DNA引用、术语表、禁忌）。",
                "生成 beat_sheet（节拍类型/情感电荷/视觉重点/预估格数）。",
                "仅输出 JSON，不要任何额外文本。",
            ],
            output_format="json",
        )
    )
    architect_content = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=settings.temperature,
            max_tokens=settings.max_tokens,
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
                max_tokens=settings.max_tokens,
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
                max_tokens=settings.max_tokens,
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
            max_tokens=settings.max_tokens,
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
                max_tokens=settings.max_tokens,
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
                max_tokens=settings.max_tokens,
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
                        max_tokens=settings.max_tokens,
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
                max_tokens=settings.max_tokens,
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


class WorkflowStoryboardRequest(BaseModel):
    project_id: int
    scene_text: str
    options: WorkflowStoryboardOptions = Field(default_factory=WorkflowStoryboardOptions)


class WorkflowStoryboardResponse(BaseModel):
    run_id: str
    shots: List[Dict[str, Any]]


@router.post("/workflows/storyboard", response_model=WorkflowStoryboardResponse)
async def workflow_storyboard(req: WorkflowStoryboardRequest):
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    scene_text = (req.scene_text or "").strip()
    if not scene_text:
        raise HTTPException(status_code=400, detail="scene_text 不能为空")

    run_id = new_run_id()
    project_id = int(req.project_id)
    series_bible = _context_store.get_series_bible(project_id=project_id, version="v1") or {}

    visual_dna_list: List[Dict[str, Any]] = []
    for item_id in req.options.asset_item_ids[:50]:
        dna = _context_store.get_visual_dna(project_id=project_id, item_id=int(item_id), version="v1")
        if isinstance(dna, dict) and dna:
            visual_dna_list.append({"item_id": int(item_id), "visual_dna": dna})

    # Step1: storyboard 拆分 ShotSpec（不含 SD prompt）
    storyboard_role = prompt_registry.get_template_prompt("storyboard_system")
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
                "动作原子化：每个镜头只包含一个清晰可视动作。",
                "仅输出 JSON，不要任何额外文本。",
            ],
            output_format="json",
            extra_blocks={
                "locked_visual_dna": json.dumps(visual_dna_list, ensure_ascii=False, indent=2) if visual_dna_list else "",
            },
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

    # Step2: 翻译为 SD prompt
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
    project_id: int
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
    project_id = int(payload.project_id)
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
    project_id: int
    scene_id: int
    run_id: str
    overwrite_shots: bool = True


@router.post("/workflows/storyboard/apply")
def apply_workflow_storyboard(payload: ApplyStoryboardWorkflowRequest, db: Session = Depends(get_db)):
    """
    从 run 快照中读取 workflow_storyboard 的 shots，并写回 DB：
    - Scene.shots: action_text/dialogue/prompt/negative_prompt/title/sequence_number
    """
    project_id = int(payload.project_id)
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


