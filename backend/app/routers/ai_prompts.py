from typing import List

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel

from ..services import prompt_registry


router = APIRouter(tags=["AI (DeepSeek)"])


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
    return PromptTemplateRead(
        **prompt_registry.template_to_read(key=key, tpl=raw_templates[key], defaults=defaults, raw_templates=raw_templates)
    )


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
    return PromptTemplateRead(**prompt_registry.template_to_read(key=key, tpl=effective_tpl, defaults=defaults, raw_templates=raw_templates))


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
    return PromptTemplateRead(
        **prompt_registry.template_to_read(key=key, tpl=defaults[key], defaults=defaults, raw_templates=raw_templates)
    )


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

