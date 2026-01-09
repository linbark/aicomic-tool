import json
from typing import Any, Dict

from fastapi import HTTPException
from pydantic import BaseModel

from ..services.app_paths import ai_settings_path
from ..services.context_store import ContextStore
from ..services.json_extract import extract_json_any
from ..services.llm_client import DeepSeekChatClient, LlmChatSettings
from ..services.prompt_composer import PromptModules, compose_system_prompt_xml


class AiSettingsRead(BaseModel):
    has_api_key: bool
    base_url: str = "https://api.deepseek.com"
    model: str = "deepseek-chat"
    temperature: float = 0.2
    max_tokens: int = 2048
    timeout_seconds: float = 120.0


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
    import json as _json
    import os as _os

    _os.makedirs(_os.path.dirname(path), exist_ok=True)
    try:
        with open(path, "w", encoding="utf-8") as f:
            _json.dump(data, f, ensure_ascii=False, indent=2)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to write ai settings: {e}")


def _mask_settings(raw: Dict[str, Any]) -> AiSettingsRead:
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


def _build_memory_context(retrieval_results: Dict[str, Any]) -> str:
    """将检索结果格式化为可注入 prompt 的上下文"""
    from ..services.context_assembly import assemble_memory_context

    sections = assemble_memory_context(retrieval_results)
    return sections.to_markdown()

