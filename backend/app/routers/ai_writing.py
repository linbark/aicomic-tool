import time
from typing import Any, Dict, Optional

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel

from ..services import prompt_registry
from ..services.llm_client import LlmChatSettings
from .ai_shared import _build_memory_context, _chat_client, _mask_settings, _read_settings_raw
from .ai_helpers import log_ui, safe_parse_json

router = APIRouter(tags=["AI (DeepSeek)"])

def _now_ms() -> int:
    return int(time.time() * 1000)

def _log_writing_event(
    project_id: Optional[int],
    run_id: str,
    stage: str,
    summary: str,
    level: str = "INFO",
    data: Optional[Dict[str, Any]] = None,
):
    if not project_id:
        return
    payload: Dict[str, Any] = {
        "stage": str(stage),
        "summary": str(summary),
        "run_id": str(run_id),
        "project_id": int(project_id),
        "data": data or {},
        "at_ms": _now_ms(),
    }
    log_ui(payload, level)


class OutlineGenerateRequest(BaseModel):
    text: str
    project_id: Optional[int] = None  # 新增：如果提供则启用记忆检索
    run_id: str


class OutlineGenerateResponse(BaseModel):
    text: str
    run_id: Optional[str] = None


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
            from ..services.memory_indexer import MemoryIndexer
            from ..services.memory_retriever import get_memory_retriever

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

    # 不限制 max_tokens，让 API 自己决定输出长度
    run_id = (req.run_id or "").strip()
    if not run_id:
        raise HTTPException(status_code=400, detail="run_id 不能为空")
    content = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=settings.temperature,
            max_tokens=settings.max_tokens,  # None 表示不限制
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_text},
        ],
    )
    return OutlineGenerateResponse(text=(content or "").strip(), run_id=run_id)


class OutlineOptimizeRequest(BaseModel):
    text: str
    project_id: Optional[int] = None  # 新增：如果提供则启用记忆检索
    run_id: str


class OutlineOptimizeResponse(BaseModel):
    text: str


@router.post("/outline-optimize", response_model=OutlineOptimizeResponse)
async def outline_optimize(req: OutlineOptimizeRequest):
    import logging
    logger = logging.getLogger(__name__)
    run_id = (req.run_id or "").strip()
    if not run_id:
        raise HTTPException(status_code=400, detail="run_id 不能为空")
    
    logger.info(f"[outline_optimize] Entry: project_id={req.project_id}, text length={len(req.text or '')}")
    _log_writing_event(req.project_id, run_id, "writing.outline_optimize.entry", "进入接口", "INFO", data={"text_len": len(req.text or "")})
    
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    logger.info(f"[outline_optimize] Settings loaded: has_api_key={settings.has_api_key}, model={settings.model}")
    _log_writing_event(
        req.project_id,
        run_id,
        "writing.outline_optimize.settings",
        "读取 AI 配置",
        "INFO",
        data={"has_api_key": bool(settings.has_api_key), "model": str(settings.model)},
    )
    
    if not settings.has_api_key:
        _log_writing_event(req.project_id, run_id, "writing.outline_optimize.settings", "AI API Key 未配置", "ERROR")
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    user_text = (req.text or "").strip()
    logger.info(f"[outline_optimize] User text: length={len(user_text)}, preview={user_text[:200]}")
    _log_writing_event(req.project_id, run_id, "writing.outline_optimize.validate", "读取输入", "INFO", data={"text_len": len(user_text)})
    
    if not user_text:
        _log_writing_event(req.project_id, run_id, "writing.outline_optimize.validate", "用户输入为空", "ERROR")
        return OutlineOptimizeResponse(text="")

    logger.info(f"[outline_optimize] Getting system prompt template")
    _log_writing_event(req.project_id, run_id, "writing.outline_optimize.prompt", "加载 system prompt", "INFO")
    system_prompt = prompt_registry.get_template_prompt("outline_optimize_system")
    logger.info(f"[outline_optimize] System prompt length={len(system_prompt)}")
    _log_writing_event(req.project_id, run_id, "writing.outline_optimize.prompt", "system prompt 已加载", "INFO", data={"prompt_len": len(system_prompt)})

    # 如果提供了 project_id，检索并注入记忆
    if req.project_id:
        logger.info(f"[outline_optimize] Starting memory retrieval for project_id={req.project_id}")
        _log_writing_event(req.project_id, run_id, "writing.outline_optimize.memory.start", "开始检索记忆", "INFO")
        try:
            from ..services.memory_indexer import MemoryIndexer
            from ..services.memory_retriever import get_memory_retriever

            logger.info(f"[outline_optimize] Indexing series bible")
            _log_writing_event(req.project_id, run_id, "writing.outline_optimize.memory.index", "索引 series bible", "INFO")
            # 确保记忆已索引
            indexer = MemoryIndexer()
            indexer.index_series_bible(project_id=req.project_id, version="v1")
            logger.info(f"[outline_optimize] Series bible indexed")
            _log_writing_event(req.project_id, run_id, "writing.outline_optimize.memory.index", "索引完成", "INFO")

            logger.info(f"[outline_optimize] Retrieving memories")
            _log_writing_event(req.project_id, run_id, "writing.outline_optimize.memory.retrieve", "检索记忆中", "INFO")
            # 检索记忆
            retriever = get_memory_retriever()
            retrieval_results = retriever.retrieve_for_task(
                project_id=req.project_id,
                task_description=f"优化大纲: {user_text[:200]}",
            )
            logger.info(f"[outline_optimize] Memory retrieval completed")
            _log_writing_event(req.project_id, run_id, "writing.outline_optimize.memory.retrieve", "检索完成", "INFO")

            # 格式化并注入到 system prompt
            memory_context = _build_memory_context(retrieval_results)
            if memory_context:
                logger.info(f"[outline_optimize] Memory context length={len(memory_context)}")
                _log_writing_event(
                    req.project_id,
                    run_id,
                    "writing.outline_optimize.memory.context",
                    "注入记忆上下文",
                    "INFO",
                    data={"memory_context_len": len(memory_context)},
                )
                system_prompt = f"{system_prompt}\n\n## 记忆上下文\n\n{memory_context}"
            else:
                logger.info(f"[outline_optimize] No memory context")
                _log_writing_event(req.project_id, run_id, "writing.outline_optimize.memory.context", "无可用记忆上下文", "INFO")
        except Exception as e:
            logger.error(f"[outline_optimize] Memory retrieval failed: {type(e).__name__}: {e}", exc_info=True)
            _log_writing_event(
                req.project_id,
                run_id,
                "writing.outline_optimize.memory.error",
                "记忆检索失败",
                "ERROR",
                data={"error": f"{type(e).__name__}: {e}"},
            )

    logger.info(f"[outline_optimize] Calling LLM: max_tokens={settings.max_tokens} (None=unlimited), timeout={settings.timeout_seconds}")
    _log_writing_event(
        req.project_id,
        run_id,
        "writing.outline_optimize.llm.call",
        "调用 LLM",
        "INFO",
        data={"timeout_seconds": settings.timeout_seconds, "max_tokens": settings.max_tokens},
    )
    
    try:
        content = await _chat_client.chat(
            settings=LlmChatSettings(
                base_url=settings.base_url,
                api_key=raw.get("api_key") or "",
                model=settings.model,
                temperature=settings.temperature,
                max_tokens=settings.max_tokens,  # None 表示不限制
                timeout_seconds=settings.timeout_seconds,
            ),
            messages=[
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_text},
            ],
        )
        logger.info(f"[outline_optimize] LLM call completed, response length={len(content or '')}")
        _log_writing_event(
            req.project_id,
            run_id,
            "writing.outline_optimize.llm.done",
            "LLM 返回",
            "INFO",
            data={"response_len": len(content or "")},
        )
    except Exception as e:
        logger.error(f"[outline_optimize] LLM call failed: {type(e).__name__}: {e}", exc_info=True)
        _log_writing_event(
            req.project_id,
            run_id,
            "writing.outline_optimize.llm.error",
            "LLM 调用失败",
            "ERROR",
            data={"error": f"{type(e).__name__}: {e}"},
        )
        raise
    
    result = OutlineOptimizeResponse(text=(content or "").strip())
    logger.info(f"[outline_optimize] Returning response, text length={len(result.text)}")
    _log_writing_event(req.project_id, run_id, "writing.outline_optimize.exit", "返回结果", "INFO", data={"result_len": len(result.text)})
    return result


class ScriptGenerateRequest(BaseModel):
    text: str
    project_id: Optional[int] = None  # 新增：如果提供则启用记忆检索
    run_id: str


class ScriptGenerateResponse(BaseModel):
    text: str
    run_id: Optional[str] = None


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
            from ..services.memory_indexer import MemoryIndexer
            from ..services.memory_retriever import get_memory_retriever

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

    run_id = (req.run_id or "").strip()
    if not run_id:
        raise HTTPException(status_code=400, detail="run_id 不能为空")
    content = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=settings.temperature,
            max_tokens=settings.max_tokens,  # None 表示不限制
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_text},
        ],
    )
    return ScriptGenerateResponse(text=(content or "").strip(), run_id=run_id)


class ScriptOptimizeRequest(BaseModel):
    text: str
    project_id: Optional[int] = None  # 新增：如果提供则启用记忆检索
    run_id: str


class ScriptOptimizeResponse(BaseModel):
    text: str
    run_id: Optional[str] = None


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
            from ..services.memory_indexer import MemoryIndexer
            from ..services.memory_retriever import get_memory_retriever

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

    run_id = (req.run_id or "").strip()
    if not run_id:
        raise HTTPException(status_code=400, detail="run_id 不能为空")
    content = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=settings.temperature,
            max_tokens=settings.max_tokens,  # None 表示不限制
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_text},
        ],
    )
    return ScriptOptimizeResponse(text=(content or "").strip(), run_id=run_id)
