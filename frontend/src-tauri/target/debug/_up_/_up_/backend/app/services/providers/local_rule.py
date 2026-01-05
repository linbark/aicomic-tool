"""
LocalRuleProvider：无 LLM 的保底实现（纯规则/模板）
用于 P0 阶段，确保工作流可运行而不依赖外部服务
"""
from typing import TypeVar
from pydantic import BaseModel, ValidationError

from .base import LLMProvider

T = TypeVar("T", bound=BaseModel)


class LocalRuleProvider(LLMProvider):
    """
    本地规则 Provider：不调用 LLM，使用模板/规则生成保底结果
    """

    @property
    def provider_name(self) -> str:
        return "local_rules"

    def generate_json(self, prompt: str, schema: type[T]) -> T:
        """
        保底实现：尝试从 prompt 中提取信息，或返回默认值
        P0 阶段：大多数情况下返回最小有效实例
        """
        # 尝试创建空实例（如果 schema 允许）
        try:
            # 对于有默认值的字段，创建最小实例
            return schema.model_construct()
        except Exception:
            # 如果失败，尝试用空字典
            try:
                return schema.model_validate({})
            except ValidationError as e:
                # 如果还是失败，返回一个包含错误的占位实例
                # 实际使用中，Agent 应该处理这种情况并回退到规则实现
                raise ValueError(f"LocalRuleProvider cannot generate {schema.__name__}: {e}")

    def generate_text(self, prompt: str) -> str:
        """
        保底实现：返回提示词的摘要或固定模板
        """
        # P0 阶段：简单返回提示词的前 100 字符作为占位
        if len(prompt) <= 100:
            return prompt
        return prompt[:97] + "..."

