from __future__ import annotations

from dataclasses import dataclass
import asyncio
import random
from typing import Dict, List, Optional

import httpx
from fastapi import HTTPException


@dataclass(frozen=True)
class LlmChatSettings:
    base_url: str
    api_key: str
    model: str
    temperature: float
    max_tokens: Optional[int]  # None 表示不限制输出长度
    timeout_seconds: float


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
        # 只有当 max_tokens 不为 None 时才添加到 payload 中
        # None 表示不限制输出长度，让 API 自己决定
        if settings.max_tokens is not None:
            payload["max_tokens"] = settings.max_tokens

        timeout = httpx.Timeout(
            connect=min(10.0, settings.timeout_seconds),
            read=settings.timeout_seconds,
            write=min(30.0, settings.timeout_seconds),
            pool=min(10.0, settings.timeout_seconds),
        )

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
                    print(f"[AI][Error] Request failed (attempt {attempt+1}/{max_attempts}): {error_name}: {error_msg}")

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
            return data["choices"][0]["message"]["content"]
        except Exception as e:
            # 尽可能给出可读信息（避免把超长响应塞进错误里）
            preview = ""
            try:
                preview = (resp.text or "")[:500]
            except Exception:
                preview = ""
            raise HTTPException(status_code=502, detail=f"AI response parse failed: {e}. preview={preview!r}")


