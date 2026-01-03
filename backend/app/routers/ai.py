import json
import os
import re
from typing import Optional, Any, Dict, List, Tuple

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field
import httpx


router = APIRouter(
    prefix="/ai",
    tags=["AI (DeepSeek)"],
)


def _data_dir() -> str:
    return os.environ.get("AICOMIC_DATA_DIR") or os.path.join(os.getcwd(), "data")


def _settings_path() -> str:
    # 约定：AICOMIC_DATA_DIR 指向 app_data_dir/data（桌面版）或 ./data（开发版）
    base_dir = os.path.dirname(os.path.abspath(_data_dir()))
    return os.path.join(base_dir, "ai_settings.json")

def _prompts_path() -> str:
    # 与 ai_settings.json 同级，避免被 /files 静态目录直接暴露
    base_dir = os.path.dirname(os.path.abspath(_data_dir()))
    return os.path.join(base_dir, "prompt_templates.json")


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


def _default_prompt_templates() -> Dict[str, Dict[str, Any]]:
    """
    内置模板（可在前端编辑/覆盖）。prompt 支持 {placeholder}。
    """
    return {
        "split_scenes_system": {
            "title": "自动分场（system）",
            "category": "storyboard",
            "prompt": (
                "You are a screenplay assistant. "
                "Split the given episode script into scenes. "
                "Return ONLY a JSON array, no markdown, no code fences. "
                "Each item MUST be an object with keys: title (string), description (string). "
                "Limit to at most {max_scenes} scenes."
            ),
            "variables": ["max_scenes"],
        },
        "split_shots_system": {
            "title": "自动分镜（system）",
            "category": "storyboard",
            "prompt": (
                "You are a storyboard assistant. "
                "Split the given scene script into shots. "
                "Return ONLY a JSON array, no markdown, no code fences. "
                "Each item MUST be an object with keys: title (string, optional) and action_text (string). "
                "Limit to at most {max_shots} shots."
            ),
            "variables": ["max_shots"],
        },
        "outline_optimize_system": {
            "title": "大纲优化（system）",
            "category": "writing",
            "prompt": (
                "You are a professional story editor. "
                "Given a story outline, improve structure, pacing, character motivations, and clarity. "
                "Return the improved outline in Chinese with clear bullet sections: "
                "Logline, Characters, Act1, Act2, Act3, KeyBeats."
            ),
            "variables": [],
        },
        "script_generate_system": {
            "title": "剧本生成（system）",
            "category": "writing",
            "prompt": (
                "You are a screenwriter. "
                "Write a Chinese episode script with scene headings, action, and dialogue. "
                "Keep it concise, visual, and suitable for storyboard generation."
            ),
            "variables": [],
        },
    }


def _read_prompts_raw() -> Dict[str, Any]:
    path = _prompts_path()
    if not os.path.exists(path):
        return {}
    try:
        with open(path, "r", encoding="utf-8") as f:
            return json.load(f) or {}
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to read prompt templates: {e}")


def _write_prompts_raw(data: Dict[str, Any]) -> None:
    path = _prompts_path()
    os.makedirs(os.path.dirname(path), exist_ok=True)
    try:
        with open(path, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to write prompt templates: {e}")


def _normalize_key(key: str) -> str:
    return (key or "").strip()


def _effective_templates() -> Tuple[Dict[str, Dict[str, Any]], Dict[str, Dict[str, Any]]]:
    """
    returns (defaults, effective)
    effective = defaults merged with overrides/custom from storage
    """
    defaults = _default_prompt_templates()
    raw = _read_prompts_raw()
    raw_templates = raw.get("templates") if isinstance(raw, dict) else None
    if not isinstance(raw_templates, dict):
        raw_templates = {}

    effective: Dict[str, Dict[str, Any]] = {}
    keys = set(defaults.keys()) | set(raw_templates.keys())
    for k in keys:
        if k in raw_templates and isinstance(raw_templates.get(k), dict):
            effective[k] = raw_templates[k]
        elif k in defaults:
            effective[k] = defaults[k]
    return defaults, effective


def _template_to_read(key: str, tpl: Dict[str, Any], defaults: Dict[str, Dict[str, Any]], raw_templates: Dict[str, Any]) -> PromptTemplateRead:
    is_builtin = key in defaults
    base = defaults.get(key) if is_builtin else None
    is_modified = False
    if is_builtin and key in raw_templates:
        try:
            is_modified = (
                (tpl.get("prompt") or "") != (base.get("prompt") or "")
                or (tpl.get("title") or "") != (base.get("title") or "")
                or (tpl.get("category") or "") != (base.get("category") or "")
            )
        except Exception:
            is_modified = True
    variables = tpl.get("variables")
    if not isinstance(variables, list):
        variables = base.get("variables") if base else []
    variables = [str(v) for v in variables] if variables else []
    return PromptTemplateRead(
        key=key,
        title=str(tpl.get("title") or key),
        category=str(tpl.get("category") or "misc"),
        prompt=str(tpl.get("prompt") or ""),
        is_builtin=is_builtin,
        is_modified=is_modified,
        variables=variables,
    )


def _render_prompt(template: str, variables: Dict[str, Any]) -> str:
    # 简单安全渲染：缺失变量就返回原模板
    try:
        return (template or "").format(**variables)
    except KeyError:
        return template or ""
    except Exception:
        return template or ""


def _read_settings_raw() -> Dict[str, Any]:
    path = _settings_path()
    if not os.path.exists(path):
        return {}
    try:
        with open(path, "r", encoding="utf-8") as f:
            return json.load(f) or {}
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to read ai settings: {e}")


def _write_settings_raw(data: Dict[str, Any]) -> None:
    path = _settings_path()
    os.makedirs(os.path.dirname(path), exist_ok=True)
    try:
        with open(path, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
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
        timeout_seconds=float(raw.get("timeout_seconds") or 30.0),
    )

def _extract_json_any(text: str) -> Any:
    """
    尝试从 LLM 返回中提取 JSON（容错：去掉 code fence、截取首个 [...] 或 {...}）。
    """
    if text is None:
        raise ValueError("empty response")
    s = text.strip()
    # 去 code fence
    s = re.sub(r"^```(?:json)?\s*", "", s, flags=re.IGNORECASE)
    s = re.sub(r"\s*```$", "", s)
    # 先直接 parse
    try:
        return json.loads(s)
    except Exception:
        pass
    # 尝试提取数组
    l = s.find("[")
    r = s.rfind("]")
    if l != -1 and r != -1 and r > l:
        return json.loads(s[l : r + 1])
    # 尝试提取对象
    l = s.find("{")
    r = s.rfind("}")
    if l != -1 and r != -1 and r > l:
        return json.loads(s[l : r + 1])
    raise ValueError("failed to parse json")

async def _call_deepseek_chat(
    *,
    base_url: str,
    api_key: str,
    model: str,
    messages: List[Dict[str, str]],
    temperature: float,
    max_tokens: int,
    timeout_seconds: float,
) -> str:
    """
    DeepSeek 官方 API（OpenAI 兼容）调用：POST {base_url}/v1/chat/completions
    返回 choices[0].message.content
    """
    if not api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置")
    if not base_url:
        base_url = "https://api.deepseek.com"
    url = base_url.rstrip("/") + "/v1/chat/completions"

    headers = {
        "Authorization": f"Bearer {api_key}",
        "Content-Type": "application/json",
    }
    payload = {
        "model": model or "deepseek-chat",
        "messages": messages,
        "temperature": temperature,
        "max_tokens": max_tokens,
    }

    timeout = httpx.Timeout(timeout_seconds)
    async with httpx.AsyncClient(timeout=timeout) as client:
        try:
            resp = await client.post(url, headers=headers, json=payload)
        except httpx.RequestError as e:
            raise HTTPException(status_code=502, detail=f"AI request failed: {e}")

    if resp.status_code >= 400:
        # 尝试透出错误内容，便于用户排查
        try:
            err = resp.json()
        except Exception:
            err = resp.text
        raise HTTPException(status_code=502, detail={"status": resp.status_code, "error": err})

    try:
        data = resp.json()
        return data["choices"][0]["message"]["content"]
    except Exception as e:
        raise HTTPException(status_code=502, detail=f"AI response parse failed: {e}")


class AiSettingsRead(BaseModel):
    has_api_key: bool
    base_url: str = "https://api.deepseek.com"
    model: str = "deepseek-chat"
    temperature: float = 0.2
    max_tokens: int = 2048
    timeout_seconds: float = 30.0


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
        content = await _call_deepseek_chat(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            messages=[
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "Reply with exactly: OK"},
            ],
            temperature=0.0,
            max_tokens=8,
            timeout_seconds=settings.timeout_seconds,
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

    defaults, effective = _effective_templates()
    # 从模板取 system prompt（允许用户覆盖）
    tpl = effective.get("split_scenes_system") or defaults.get("split_scenes_system") or {}
    system_prompt = _render_prompt(str(tpl.get("prompt") or ""), {"max_scenes": req.max_scenes})
    content = await _call_deepseek_chat(
        base_url=settings.base_url,
        api_key=raw.get("api_key") or "",
        model=settings.model,
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_text},
        ],
        temperature=settings.temperature,
        max_tokens=settings.max_tokens,
        timeout_seconds=settings.timeout_seconds,
    )

    try:
        parsed = _extract_json_any(content)
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

    defaults, effective = _effective_templates()
    tpl = effective.get("split_shots_system") or defaults.get("split_shots_system") or {}
    system_prompt = _render_prompt(str(tpl.get("prompt") or ""), {"max_shots": req.max_shots})
    content = await _call_deepseek_chat(
        base_url=settings.base_url,
        api_key=raw.get("api_key") or "",
        model=settings.model,
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_text},
        ],
        temperature=settings.temperature,
        max_tokens=settings.max_tokens,
        timeout_seconds=settings.timeout_seconds,
    )

    try:
        parsed = _extract_json_any(content)
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
    defaults = _default_prompt_templates()
    raw = _read_prompts_raw()
    raw_templates = raw.get("templates") if isinstance(raw, dict) else None
    if not isinstance(raw_templates, dict):
        raw_templates = {}

    _, effective = _effective_templates()
    out: List[PromptTemplateRead] = []
    for key in sorted(effective.keys()):
        out.append(_template_to_read(key, effective[key], defaults, raw_templates))
    return out


@router.post("/prompts", response_model=PromptTemplateRead)
def create_prompt(payload: PromptTemplateCreate):
    key = _normalize_key(payload.key)
    if not key:
        raise HTTPException(status_code=400, detail="key 不能为空")
    defaults = _default_prompt_templates()
    raw = _read_prompts_raw()
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
    _write_prompts_raw({"templates": raw_templates})
    return _template_to_read(key, raw_templates[key], defaults, raw_templates)


@router.put("/prompts/{key}", response_model=PromptTemplateRead)
def upsert_prompt(key: str, payload: PromptTemplateUpsert):
    key = _normalize_key(key)
    if not key:
        raise HTTPException(status_code=400, detail="key 不能为空")
    defaults = _default_prompt_templates()
    raw = _read_prompts_raw()
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
    _write_prompts_raw({"templates": raw_templates})

    # effective 用于 is_modified 的判断
    effective_tpl = raw_templates[key] if key in raw_templates else defaults.get(key) or {}
    return _template_to_read(key, effective_tpl, defaults, raw_templates)


@router.post("/prompts/{key}/reset", response_model=PromptTemplateRead)
def reset_prompt(key: str):
    key = _normalize_key(key)
    defaults = _default_prompt_templates()
    if key not in defaults:
        raise HTTPException(status_code=404, detail="该 key 不是内置模板，无法重置")

    raw = _read_prompts_raw()
    raw_templates = raw.get("templates") if isinstance(raw, dict) else None
    if not isinstance(raw_templates, dict):
        raw_templates = {}
    if key in raw_templates:
        raw_templates.pop(key, None)
        _write_prompts_raw({"templates": raw_templates})
    return _template_to_read(key, defaults[key], defaults, raw_templates)


@router.delete("/prompts/{key}")
def delete_prompt(key: str):
    key = _normalize_key(key)
    defaults = _default_prompt_templates()
    raw = _read_prompts_raw()
    raw_templates = raw.get("templates") if isinstance(raw, dict) else None
    if not isinstance(raw_templates, dict):
        raw_templates = {}

    if key in defaults:
        # 删除内置模板等价于重置
        if key in raw_templates:
            raw_templates.pop(key, None)
            _write_prompts_raw({"templates": raw_templates})
        return {"message": "Prompt reset"}

    if key not in raw_templates:
        raise HTTPException(status_code=404, detail="Prompt not found")
    raw_templates.pop(key, None)
    _write_prompts_raw({"templates": raw_templates})
    return {"message": "Prompt deleted"}


# ==========================
# 写作类能力：大纲优化 / 剧本生成
# ==========================

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

    defaults, effective = _effective_templates()
    tpl = effective.get("outline_optimize_system") or defaults.get("outline_optimize_system") or {}
    system_prompt = str(tpl.get("prompt") or "")

    content = await _call_deepseek_chat(
        base_url=settings.base_url,
        api_key=raw.get("api_key") or "",
        model=settings.model,
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_text},
        ],
        temperature=settings.temperature,
        max_tokens=settings.max_tokens,
        timeout_seconds=settings.timeout_seconds,
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

    defaults, effective = _effective_templates()
    tpl = effective.get("script_generate_system") or defaults.get("script_generate_system") or {}
    system_prompt = str(tpl.get("prompt") or "")

    content = await _call_deepseek_chat(
        base_url=settings.base_url,
        api_key=raw.get("api_key") or "",
        model=settings.model,
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_text},
        ],
        temperature=settings.temperature,
        max_tokens=settings.max_tokens,
        timeout_seconds=settings.timeout_seconds,
    )
    return ScriptGenerateResponse(text=(content or "").strip())


