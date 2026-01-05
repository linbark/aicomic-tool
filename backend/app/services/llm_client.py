from __future__ import annotations

from dataclasses import dataclass
from typing import Dict, List

import httpx
from fastapi import HTTPException


@dataclass(frozen=True)
class LlmChatSettings:
    base_url: str
    api_key: str
    model: str
    temperature: float
    max_tokens: int
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
        }
        payload = {
            "model": settings.model or "deepseek-chat",
            "messages": messages,
            "temperature": settings.temperature,
            "max_tokens": settings.max_tokens,
        }

        timeout = httpx.Timeout(settings.timeout_seconds)
        async with httpx.AsyncClient(timeout=timeout) as client:
            try:
                resp = await client.post(url, headers=headers, json=payload)
            except httpx.RequestError as e:
                # 打印到终端以便排查
                print(f"[AI][Error] Request failed: {type(e).__name__}: {e}")
                # 返回详细错误给前端
                raise HTTPException(status_code=502, detail=f"AI request failed: {type(e).__name__} - {e}")

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


