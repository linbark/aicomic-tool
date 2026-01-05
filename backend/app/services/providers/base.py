"""
LLM Provider 抽象接口：可插拔的 LLM 实现
"""
from abc import ABC, abstractmethod
from typing import TypeVar, Generic
from pydantic import BaseModel

T = TypeVar("T", bound=BaseModel)


class LLMProvider(ABC):
    """
    LLM Provider 接口：所有 Agent 通过此接口调用 LLM，便于后续替换实现
    """

    @abstractmethod
    def generate_json(self, prompt: str, schema: type[T]) -> T:
        """
        生成结构化 JSON（按 schema 约束）
        :param prompt: 提示词
        :param schema: Pydantic 模型类
        :return: 解析后的模型实例
        """
        pass

    @abstractmethod
    def generate_text(self, prompt: str) -> str:
        """
        生成纯文本
        :param prompt: 提示词
        :return: 生成的文本
        """
        pass

    @property
    @abstractmethod
    def provider_name(self) -> str:
        """Provider 名称（用于 meta 记录）"""
        pass

