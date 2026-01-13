from typing import Any, Dict, Optional

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel

from ..services import prompt_registry
from ..services.llm_client import LlmChatSettings
from .ai_shared import _build_memory_context, _chat_client, _mask_settings, _read_settings_raw
from .ai_helpers import log_ui, safe_parse_json

router = APIRouter(tags=["AI (DeepSeek)"])


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
    import logging
    logger = logging.getLogger(__name__)
    
    logger.info(f"[outline_optimize] Entry: project_id={req.project_id}, text length={len(req.text or '')}")
    log_ui(req.project_id, "outline_optimize", f"Entry: text length={len(req.text or '')}", "INFO")
    
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    logger.info(f"[outline_optimize] Settings loaded: has_api_key={settings.has_api_key}, model={settings.model}")
    log_ui(req.project_id, "outline_optimize", f"Settings loaded: has_api_key={settings.has_api_key}, model={settings.model}", "INFO")
    
    if not settings.has_api_key:
        log_ui(req.project_id, "outline_optimize", "AI API Key 未配置", "ERROR")
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    user_text = (req.text or "").strip()
    logger.info(f"[outline_optimize] User text: length={len(user_text)}, preview={user_text[:200]}")
    log_ui(req.project_id, "outline_optimize", f"User text: length={len(user_text)}", "INFO")
    
    if not user_text:
        log_ui(req.project_id, "outline_optimize", "用户输入为空", "ERROR")
        return OutlineOptimizeResponse(text="")

    logger.info(f"[outline_optimize] Getting system prompt template")
    log_ui(req.project_id, "outline_optimize", "Getting system prompt template", "INFO")
    system_prompt = prompt_registry.get_template_prompt("outline_optimize_system")
    logger.info(f"[outline_optimize] System prompt length={len(system_prompt)}")
    log_ui(req.project_id, "outline_optimize", f"System prompt length={len(system_prompt)}", "INFO")

    # 如果提供了 project_id，检索并注入记忆
    if req.project_id:
        logger.info(f"[outline_optimize] Starting memory retrieval for project_id={req.project_id}")
        log_ui(req.project_id, "outline_optimize", f"Starting memory retrieval for project_id={req.project_id}", "INFO")
        try:
            from ..services.memory_indexer import MemoryIndexer
            from ..services.memory_retriever import get_memory_retriever

            logger.info(f"[outline_optimize] Indexing series bible")
            log_ui(req.project_id, "outline_optimize", "Indexing series bible", "INFO")
            # 确保记忆已索引
            indexer = MemoryIndexer()
            indexer.index_series_bible(project_id=req.project_id, version="v1")
            logger.info(f"[outline_optimize] Series bible indexed")
            log_ui(req.project_id, "outline_optimize", "Series bible indexed", "INFO")

            logger.info(f"[outline_optimize] Retrieving memories")
            log_ui(req.project_id, "outline_optimize", "Retrieving memories", "INFO")
            # 检索记忆
            retriever = get_memory_retriever()
            retrieval_results = retriever.retrieve_for_task(
                project_id=req.project_id,
                task_description=f"优化大纲: {user_text[:200]}",
            )
            logger.info(f"[outline_optimize] Memory retrieval completed")
            log_ui(req.project_id, "outline_optimize", "Memory retrieval completed", "INFO")

            # 格式化并注入到 system prompt
            memory_context = _build_memory_context(retrieval_results)
            if memory_context:
                logger.info(f"[outline_optimize] Memory context length={len(memory_context)}")
                log_ui(req.project_id, "outline_optimize", f"Memory context length={len(memory_context)}", "INFO")
                system_prompt = f"{system_prompt}\n\n## 记忆上下文\n\n{memory_context}"
            else:
                logger.info(f"[outline_optimize] No memory context")
                log_ui(req.project_id, "outline_optimize", "No memory context", "INFO")
        except Exception as e:
            logger.error(f"[outline_optimize] Memory retrieval failed: {type(e).__name__}: {e}", exc_info=True)
            log_ui(req.project_id, "outline_optimize", f"Memory retrieval failed: {type(e).__name__}: {e}", "ERROR")

    effective_max_tokens = max(int(settings.max_tokens or 0), 4096)
    logger.info(f"[outline_optimize] Calling LLM: max_tokens={effective_max_tokens}, timeout={settings.timeout_seconds}")
    log_ui(req.project_id, "outline_optimize", f"Calling LLM: max_tokens={effective_max_tokens}, timeout={settings.timeout_seconds}", "INFO")
    
    try:
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
        logger.info(f"[outline_optimize] LLM call completed, response length={len(content or '')}")
        log_ui(req.project_id, "outline_optimize", f"LLM call completed, response length={len(content or '')}", "INFO")
    except Exception as e:
        logger.error(f"[outline_optimize] LLM call failed: {type(e).__name__}: {e}", exc_info=True)
        log_ui(req.project_id, "outline_optimize", f"LLM call failed: {type(e).__name__}: {e}", "ERROR")
        raise
    
    result = OutlineOptimizeResponse(text=(content or "").strip())
    logger.info(f"[outline_optimize] Returning response, text length={len(result.text)}")
    log_ui(req.project_id, "outline_optimize", f"Returning response, text length={len(result.text)}", "INFO")
    return result


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

