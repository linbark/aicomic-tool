from __future__ import annotations

from dataclasses import dataclass
import asyncio
import json
import random
import re
from typing import Dict, List, Optional

import httpx
from fastapi import HTTPException
import logging


logger = logging.getLogger(__name__)
_MAX_TOKENS_UPPER_BOUND = 8192


@dataclass(frozen=True)
class LlmChatSettings:
    base_url: str
    api_key: str
    model: str
    temperature: float
    max_tokens: Optional[int]  # None 表示使用默认较大 max_tokens
    timeout_seconds: float

    def __repr__(self) -> str:
        masked = "***" if (self.api_key or "").strip() else ""
        return (
            "LlmChatSettings("
            f"base_url={self.base_url!r}, "
            f"api_key={masked!r}, "
            f"model={self.model!r}, "
            f"temperature={self.temperature!r}, "
            f"max_tokens={self.max_tokens!r}, "
            f"timeout_seconds={self.timeout_seconds!r}"
            ")"
        )


class DeepSeekChatClient:
    """
    DeepSeek 官方 API（OpenAI 兼容）：POST {base_url}/v1/chat/completions
    返回 choices[0].message.content
    """

    async def chat(self, *, settings: LlmChatSettings, messages: List[Dict[str, str]]) -> str:
        if not settings.api_key:
            raise HTTPException(status_code=400, detail="AI API Key 未配置")

        base_url = settings.base_url or "https://api.deepseek.com"
        url = base_url.rstrip("/") + "/v1/chat/completions"

        headers = {
            "Authorization": f"Bearer {settings.api_key}",
            "Content-Type": "application/json",
            "Accept": "application/json",
        }
        payload = {
            "model": settings.model or "deepseek-chat",
            "messages": messages,
            "temperature": settings.temperature,
        }
        payload["max_tokens"] = int(_normalize_max_tokens(settings.max_tokens))

        timeout = httpx.Timeout(
            connect=min(10.0, settings.timeout_seconds),
            read=settings.timeout_seconds,
            write=min(30.0, settings.timeout_seconds),
            pool=min(10.0, settings.timeout_seconds),
        )

        def _is_likely_truncated_json(text: str) -> bool:
            stripped = (text or "").strip()
            if not stripped:
                return False
            if stripped[0] not in {"{", "["}:
                return False
            if stripped[0] == "{" and not stripped.endswith("}"):
                return True
            if stripped[0] == "[" and not stripped.endswith("]"):
                return True
            return False

        def _extract_content_and_meta(response_json: Dict) -> tuple[str, Optional[str], Optional[Dict]]:
            choices = response_json.get("choices") or []
            if not choices or not isinstance(choices, list):
                raise ValueError("missing choices")

            choice0 = choices[0] or {}
            if not isinstance(choice0, dict):
                raise ValueError("invalid choices[0]")

            finish_reason = choice0.get("finish_reason")
            message = choice0.get("message") or {}
            if not isinstance(message, dict):
                raise ValueError("invalid message")

            content = message.get("content")
            if content is None:
                content = ""
            if not isinstance(content, str):
                content = str(content)

            usage = response_json.get("usage")
            if usage is not None and not isinstance(usage, dict):
                usage = {"raw": str(usage)}

            return content, (str(finish_reason) if finish_reason is not None else None), usage

        async def _sleep_backoff(attempt: int) -> None:
            # 0.35s, 0.7s, 1.4s... + 抖动，避免同时重试“撞车”
            base = 0.35 * (2**attempt)
            jitter = random.uniform(0.0, 0.25)
            await asyncio.sleep(base + jitter)

        def _is_retryable_httpx_error(e: Exception) -> bool:
            # 网络抖动/断流/服务端提前断开常见于这些异常
            if isinstance(e, httpx.TimeoutException):
                return True
            if isinstance(e, httpx.RemoteProtocolError):
                return True
            if isinstance(e, httpx.ReadError):
                return True
            if isinstance(e, httpx.ConnectError):
                return True
            if isinstance(e, httpx.NetworkError):
                return True
            return False

        last_exc: Optional[Exception] = None
        max_attempts = 3

        for attempt in range(max_attempts):
            async with httpx.AsyncClient(timeout=timeout) as client:
                try:
                    resp = await client.post(url, headers=headers, json=payload)
                    break
                except Exception as e:
                    last_exc = e
                    error_name = type(e).__name__
                    error_msg = str(e)
                    logger.warning(
                        "[AI] Request failed (attempt %s/%s): %s: %s",
                        attempt + 1,
                        max_attempts,
                        error_name,
                        error_msg,
                    )

                    # 非可重试错误：直接抛出
                    if not _is_retryable_httpx_error(e) or attempt == max_attempts - 1:
                        # SSL 错误特殊提示
                        if "SSL" in error_name or "ssl" in error_msg.lower():
                            detail = (
                                f"SSL 连接失败 ({error_name})。可能原因：1) 正在使用代理（Clash等），请关闭或添加直连规则；"
                                f"2) Base URL 配置错误。原始错误: {error_msg}"
                            )
                        elif error_name == "RemoteProtocolError" or "incomplete chunked read" in error_msg.lower():
                            detail = (
                                f"AI request failed: {error_name} - {error_msg}。常见原因：网络/代理导致连接被上游提前断开。"
                                f"建议：1) 临时关闭代理或为 {base_url} 配置直连；2) 适当增大 timeout；3) 稍后重试。"
                            )
                        else:
                            detail = f"AI request failed: {error_name} - {error_msg}"

                        raise HTTPException(status_code=502, detail=detail)

            await _sleep_backoff(attempt)

        if resp.status_code >= 400:
            # 尝试透出错误内容，便于用户排查
            try:
                err = resp.json()
            except Exception:
                err = resp.text
            raise HTTPException(status_code=502, detail={"status": resp.status_code, "error": err})

        try:
            data = resp.json()
            content, finish_reason, usage = _extract_content_and_meta(data)

            is_truncated = finish_reason in {"length", "max_tokens"} or _is_likely_truncated_json(content)
            if is_truncated:
                raise HTTPException(
                    status_code=502,
                    detail={
                        "error": "LLM 输出被截断",
                        "finish_reason": finish_reason,
                        "usage": usage,
                        "hint": "这通常是模型/服务端的输出 token 上限导致（不等同于 httpx timeout）。建议：在 AI 设置里显式把 max_tokens 调大，或减少一次输入的 evidences/输出字段。",
                    },
                )

            return content
        except Exception as e:
            # 尽可能给出可读信息（避免把超长响应塞进错误里）
            preview = ""
            try:
                preview = (resp.text or "")[:500]
            except Exception:
                preview = ""
            raise HTTPException(status_code=502, detail=f"AI response parse failed: {e}. preview={preview!r}")

    async def reason_qa_json(self, *, settings: LlmChatSettings, question: str) -> Dict[str, str]:
        base_url = settings.base_url or "https://api.deepseek.com"
        url = base_url.rstrip("/") + "/v1/chat/completions"

        headers = {
            "Authorization": f"Bearer {settings.api_key}",
            "Content-Type": "application/json",
            "Accept": "application/json",
        }

        system_prompt = (
            "你处于推理模式（reason）。只输出一个 JSON 对象，且仅包含字段：question, answer。"
            "不要输出 Markdown，不要输出代码块，不要输出额外解释。"
        )

        messages = [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": str(question or "").strip()},
        ]

        payload = {
            "model": "deepseek-reasoner",
            "messages": messages,
            "response_format": {"type": "json_object"},
            "max_tokens": int(_normalize_max_tokens(settings.max_tokens)),
        }

        timeout = httpx.Timeout(
            connect=min(10.0, settings.timeout_seconds),
            read=settings.timeout_seconds,
            write=min(30.0, settings.timeout_seconds),
            pool=min(10.0, settings.timeout_seconds),
        )

        try:
            async with httpx.AsyncClient(timeout=timeout) as client:
                resp = await client.post(url, headers=headers, json=payload)
        except Exception as e:
            logger.exception("[AI][reason_qa_json] request failed: %s", type(e).__name__)
            raise HTTPException(status_code=502, detail=f"AI request failed: {type(e).__name__} - {e}")

        if resp.status_code >= 400:
            try:
                err = resp.json()
            except Exception:
                err = resp.text
            logger.error("[AI][reason_qa_json] bad status=%s err=%s", resp.status_code, str(err)[:1000])
            raise HTTPException(status_code=502, detail={"status": resp.status_code, "error": err})

        try:
            data = resp.json()
            choice0 = (data.get("choices") or [{}])[0] or {}
            message = choice0.get("message") or {}
            content = message.get("content") or ""
            parsed = parse_question_answer_json(question=str(question or "").strip(), content=str(content))
            if not (parsed.get("answer") or "").strip():
                raise ValueError("empty answer")
            return parsed
        except HTTPException:
            raise
        except Exception as e:
            preview = ""
            try:
                preview = (resp.text or "")[:800]
            except Exception:
                preview = ""
            logger.exception("[AI][reason_qa_json] parse failed: %s", type(e).__name__)
            raise HTTPException(status_code=502, detail=f"AI response parse failed: {type(e).__name__}: {e}. preview={preview!r}")


def _normalize_max_tokens(v: Optional[int]) -> int:
    try:
        if v is None:
            return _MAX_TOKENS_UPPER_BOUND
        n = int(v)
        if n <= 0:
            return _MAX_TOKENS_UPPER_BOUND
        return min(n, _MAX_TOKENS_UPPER_BOUND)
    except Exception:
        return _MAX_TOKENS_UPPER_BOUND


def parse_question_answer_json(*, question: str, content: str) -> Dict[str, str]:
    q = str(question or "").strip()
    text = str(content or "").strip()

    obj: Optional[Dict] = None
    try:
        parsed = json.loads(text)
        if isinstance(parsed, dict):
            obj = parsed
    except Exception:
        obj = None

    if obj is None:
        start = text.find("{")
        end = text.rfind("}")
        if start != -1 and end != -1 and end > start:
            candidate = text[start : end + 1]
            candidate = re.sub(r",\s*}", "}", candidate)
            candidate = re.sub(r",\s*]", "]", candidate)
            parsed = json.loads(candidate)
            if isinstance(parsed, dict):
                obj = parsed

    if obj is None:
        raise ValueError("not a json object")

    ans = obj.get("answer")
    if ans is None:
        raise ValueError("missing answer")
    answer = str(ans).strip()
    if not answer:
        raise ValueError("empty answer")

    out_q = str(obj.get("question") or q).strip()
    if not out_q:
        out_q = q

    return {"question": out_q, "answer": answer}
