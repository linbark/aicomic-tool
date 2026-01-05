"""
OpenAI Vision Provider：真实调用 OpenAI API（支持 Vision 模型）
"""
import base64
import json
from typing import TypeVar, Type, Any, Optional
from pydantic import BaseModel, ValidationError

from .base import LLMProvider

T = TypeVar("T", bound=BaseModel)


class OpenAIVisionProvider(LLMProvider):
    """
    OpenAI Vision Provider：调用 OpenAI API（支持 GPT-4 Vision 等模型）
    配置参数（通过 config_data 传入）：
    - api_key: API Key（必需）
    - model: 模型名称（默认 gpt-4o-mini）
    - base_url: API Base URL（可选，默认官方）
    """

    def __init__(self, config_data: dict):
        """
        :param config_data: 配置字典，包含 api_key, model, base_url 等
        """
        self.api_key = config_data.get("api_key")
        self.model = config_data.get("model", "gpt-4o-mini")
        self.base_url = config_data.get("base_url", "https://api.openai.com/v1")
        self._client = None

    @property
    def provider_name(self) -> str:
        return "openai_vision"

    def _get_client(self):
        """延迟加载 OpenAI 客户端（避免导入失败时整个模块无法加载）"""
        if self._client is None:
            try:
                from openai import OpenAI
                self._client = OpenAI(api_key=self.api_key, base_url=self.base_url)
            except ImportError:
                raise ImportError(
                    "openai package not installed. Install with: pip install openai"
                )
        return self._client

    def _is_available(self) -> bool:
        """检查 Provider 是否可用（有 Key 且可导入）"""
        if not self.api_key:
            return False
        try:
            self._get_client()
            return True
        except (ImportError, Exception):
            return False

    def generate_json(
        self, prompt: str, schema: Type[T], image_data: Optional[bytes] = None, image_mime_type: Optional[str] = None
    ) -> T:
        """
        生成结构化 JSON
        :param prompt: 文本提示词
        :param schema: Pydantic 模型类
        :param image_data: 可选的图片数据（bytes）
        :param image_mime_type: 图片 MIME 类型（如 image/jpeg）
        :return: 解析后的模型实例
        """
        if not self._is_available():
            raise ValueError(
                "OpenAI Vision Provider not available: missing OPENAI_API_KEY or openai package"
            )

        client = self._get_client()

        # 构建消息
        messages = []
        if image_data:
            # Vision 模式：包含图片
            image_base64 = base64.b64encode(image_data).decode("utf-8")
            messages.append({
                "role": "user",
                "content": [
                    {"type": "text", "text": prompt},
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": f"data:{image_mime_type or 'image/jpeg'};base64,{image_base64}"
                        },
                    },
                ],
            })
        else:
            # 纯文本模式
            messages.append({"role": "user", "content": prompt})

        # 添加系统提示：要求返回严格 JSON
        system_prompt = f"""You are a JSON generator. You must respond with ONLY valid JSON that matches this schema:
{schema.model_json_schema()}

Do not include any markdown code blocks, explanations, or extra text. Return ONLY the JSON object."""

        try:
            response = client.chat.completions.create(
                model=self.model,
                messages=[
                    {"role": "system", "content": system_prompt},
                    *messages,
                ],
                response_format={"type": "json_object"},  # 强制 JSON 模式
                temperature=0.3,  # 降低随机性
            )

            content = response.choices[0].message.content.strip()

            # 尝试解析 JSON
            try:
                # 移除可能的 markdown 代码块标记
                if content.startswith("```"):
                    lines = content.split("\n")
                    content = "\n".join(lines[1:-1]) if len(lines) > 2 else content

                json_data = json.loads(content)
                return schema.model_validate(json_data)
            except (json.JSONDecodeError, ValidationError) as e:
                raise ValueError(f"Failed to parse JSON from OpenAI response: {e}\nResponse: {content[:200]}")

        except Exception as e:
            raise ValueError(f"OpenAI API call failed: {e}")

    def generate_text(self, prompt: str, image_data: Optional[bytes] = None, image_mime_type: Optional[str] = None) -> str:
        """
        生成纯文本
        :param prompt: 文本提示词
        :param image_data: 可选的图片数据（bytes）
        :param image_mime_type: 图片 MIME 类型
        :return: 生成的文本
        """
        if not self._is_available():
            raise ValueError(
                "OpenAI Vision Provider not available: missing OPENAI_API_KEY or openai package"
            )

        client = self._get_client()

        messages = []
        if image_data:
            image_base64 = base64.b64encode(image_data).decode("utf-8")
            messages.append({
                "role": "user",
                "content": [
                    {"type": "text", "text": prompt},
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": f"data:{image_mime_type or 'image/jpeg'};base64,{image_base64}"
                        },
                    },
                ],
            })
        else:
            messages.append({"role": "user", "content": prompt})

        try:
            response = client.chat.completions.create(
                model=self.model,
                messages=messages,
                temperature=0.7,
            )
            return response.choices[0].message.content.strip()
        except Exception as e:
            raise ValueError(f"OpenAI API call failed: {e}")

