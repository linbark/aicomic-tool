import json
import os
from typing import Any, Dict, List, Tuple

from fastapi import HTTPException

from .app_paths import prompt_templates_path


def default_prompt_templates() -> Dict[str, Dict[str, Any]]:
    """
    内置模板（可在前端编辑/覆盖）。prompt 支持 {placeholder}。
    在保留旧 key 的基础上，为 workflows 新增 key（后端统一编排）。
    """
    return {
        # -------------------------
        # 旧：原子能力（保持兼容）
        # -------------------------
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
        "outline_generate_system": {
            "title": "大纲生成（system）",
            "category": "writing",
            "prompt": (
                "You are a creative screenwriter. "
                "Given a story concept or logline, generate a structured story outline. "
                "Include: Logline, Characters, Act 1, Act 2, Act 3. "
                "Write in Chinese."
            ),
            "variables": [],
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
        "script_optimize_system": {
            "title": "剧本优化（system）",
            "category": "writing",
            "prompt": (
                "You are a script doctor. "
                "Polish the given script for better flow, dialogue, and formatting. "
                "Keep the standard script format (Scene Headings, Action, Dialogue). "
                "Improve character voices and visual descriptions. "
                "Write in Chinese."
            ),
            "variables": [],
        },
        # -------------------------
        # 新：workflow 角色模板
        # -------------------------
        "architect_system": {
            "title": "架构师代理（system）",
            "category": "workflow",
            "prompt": (
                "你是一位资深概念艺术家与世界观架构师（Narrative Architect）。"
                "你会把输入故事压缩成可复用的 series_bible，并输出 beat_sheet。"
                "你不知道角色内心想法，只能看到可被视觉化的事实与动作。"
                "只输出 JSON（不要 markdown/代码块）。"
            ),
            "variables": [],
        },
        "writer_system": {
            "title": "编剧代理（system）",
            "category": "workflow",
            "prompt": (
                "你是一位严格遵循“展示而非讲述”的漫剧编剧。"
                "严禁心理描写（觉得/认为/感到/想起等），必须用可见动作替代。"
                "你将基于 series_bible 与 beat_sheet 输出符合 Fountain 语法的剧本。"
                "只输出 JSON（不要 markdown/代码块）。"
            ),
            "variables": [],
        },
        "qc_system": {
            "title": "一致性监理/质检代理（system）",
            "category": "workflow",
            "prompt": (
                "你是一位连续性监理与剧本质检（QC）代理。"
                "检查：世界观/角色一致性、时代穿帮、物理可实现性、心理动词违规、对话字数是否过长。"
                "必要时给出修订版（revised_script_fountain）。"
                "只输出 JSON（不要 markdown/代码块）。"
            ),
            "variables": [],
        },
        "storyboard_system": {
            "title": "分镜师代理（system）",
            "category": "workflow",
            "prompt": (
                "你是一位分镜师代理（Storyboard Agent）。"
                "把场景文本拆解为 ShotSpec 列表，并用受控词汇表选择镜头参数。"
                "只输出 JSON（不要 markdown/代码块）。"
            ),
            "variables": [],
        },
        "prompt_translate_system": {
            "title": "提示词翻译代理（system）",
            "category": "workflow",
            "prompt": (
                "你是一位视觉提示词工程师。"
                "将 ShotSpec 翻译为 Stable Diffusion/Flux tags 风格的 prompt 与 negative_prompt。"
                "只输出 JSON（不要 markdown/代码块）。"
            ),
            "variables": [],
        },
    }


def read_prompts_raw() -> Dict[str, Any]:
    path = prompt_templates_path()
    if not os.path.exists(path):
        return {}
    try:
        with open(path, "r", encoding="utf-8") as f:
            return json.load(f) or {}
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to read prompt templates: {e}")


def write_prompts_raw(data: Dict[str, Any]) -> None:
    path = prompt_templates_path()
    os.makedirs(os.path.dirname(path), exist_ok=True)
    try:
        with open(path, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to write prompt templates: {e}")


def normalize_key(key: str) -> str:
    return (key or "").strip()


def effective_templates() -> Tuple[Dict[str, Dict[str, Any]], Dict[str, Dict[str, Any]], Dict[str, Any]]:
    """
    returns (defaults, effective, raw_templates)
    effective = defaults merged with overrides/custom from storage
    """
    defaults = default_prompt_templates()
    raw = read_prompts_raw()
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
    return defaults, effective, raw_templates


def template_to_read(
    *, key: str, tpl: Dict[str, Any], defaults: Dict[str, Dict[str, Any]], raw_templates: Dict[str, Any]
) -> Dict[str, Any]:
    """
    返回一个面向 API 的 dict（由 router 决定具体 response_model）。
    """
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
    return {
        "key": key,
        "title": str(tpl.get("title") or key),
        "category": str(tpl.get("category") or "misc"),
        "prompt": str(tpl.get("prompt") or ""),
        "is_builtin": bool(is_builtin),
        "is_modified": bool(is_modified),
        "variables": variables,
    }


def render_prompt(template: str, variables: Dict[str, Any]) -> str:
    # 简单安全渲染：缺失变量就返回原模板
    try:
        return (template or "").format(**variables)
    except KeyError:
        return template or ""
    except Exception:
        return template or ""


def get_template(key: str) -> Dict[str, Any]:
    defaults, effective, _raw = effective_templates()
    return effective.get(key) or defaults.get(key) or {}


def required_variables(key: str) -> List[str]:
    tpl = get_template(key)
    vars_ = tpl.get("variables")
    if not isinstance(vars_, list):
        return []
    return [str(v) for v in vars_ if str(v)]


def render_template_with_validation(key: str, variables: Dict[str, Any] | None = None) -> tuple[str, List[str]]:
    """
    渲染模板，并返回缺失变量列表（便于 workflow 做严格校验/报错）。
    """
    tpl = get_template(key)
    template = str(tpl.get("prompt") or "")
    vars_ = variables or {}
    missing: List[str] = []
    for v in required_variables(key):
        if v not in vars_:
            missing.append(v)
    if missing:
        # 不抛错，返回原模板，交由上层决定是否允许降级
        return template, missing
    return render_prompt(template, vars_), []


def get_template_prompt(key: str, variables: Dict[str, Any] | None = None) -> str:
    prompt, _missing = render_template_with_validation(key, variables)
    return prompt


def list_templates_read() -> List[Dict[str, Any]]:
    defaults, effective, raw_templates = effective_templates()
    out: List[Dict[str, Any]] = []
    for key in sorted(effective.keys()):
        out.append(template_to_read(key=key, tpl=effective[key], defaults=defaults, raw_templates=raw_templates))
    return out


