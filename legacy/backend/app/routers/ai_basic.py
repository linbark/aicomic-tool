from typing import Any, Dict, List, Optional

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

from ..services import prompt_registry
from ..services.json_extract import extract_json_any
from ..services.llm_client import LlmChatSettings
from .ai_shared import AiSettingsRead, _chat_client, _mask_settings, _read_settings_raw, _write_settings_raw


router = APIRouter(tags=["AI (DeepSeek)"])


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
    run_id: Optional[str] = None


@router.post("/test", response_model=AiTestResponse)
async def test_ai():
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        return AiTestResponse(ok=False, detail="API Key 未配置", run_id=None)

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
        run_id = None
        if (content or "").strip() == "OK":
            return AiTestResponse(ok=True, detail="连接成功", run_id=run_id)
        return AiTestResponse(ok=True, detail=f"连接成功（返回：{(content or '').strip()[:50]}）", run_id=run_id)
    except HTTPException as e:
        # 透出后端代理错误
        return AiTestResponse(ok=False, detail=str(e.detail), run_id=None)


class SplitScenesRequest(BaseModel):
    text: str
    max_scenes: int = 50
    run_id: str


class SplitSceneItem(BaseModel):
    title: str
    description: str


class SplitShotsRequest(BaseModel):
    text: str
    max_shots: int = 80
    run_id: str


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
