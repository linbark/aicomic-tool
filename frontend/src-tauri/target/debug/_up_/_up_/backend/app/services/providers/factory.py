"""
Provider Factory：根据数据库配置选择并创建 LLM Provider
失败时自动降级到 LocalRuleProvider
"""
from typing import Optional
from sqlalchemy.orm import Session

from .base import LLMProvider
from .local_rule import LocalRuleProvider
from .openai_vision import OpenAIVisionProvider
from .. import models


def create_provider(db: Optional[Session] = None) -> tuple[LLMProvider, list[str]]:
    """
    创建 LLM Provider（根据数据库配置）
    返回: (provider, warnings)
    
    :param db: 数据库会话（如果为 None，则使用 LocalRuleProvider）
    """
    warnings: list[str] = []
    
    if db is None:
        warnings.append("No database session provided, using LocalRuleProvider")
        return LocalRuleProvider(), warnings
    
    # 查找激活的 provider 配置
    active_config = (
        db.query(models.LLMProviderConfig)
        .filter(models.LLMProviderConfig.is_active == True)
        .first()
    )
    
    if not active_config:
        warnings.append("No active LLM provider configured, using LocalRuleProvider")
        return LocalRuleProvider(), warnings
    
    provider_name = active_config.provider_name.lower()
    config_data = active_config.config_data or {}
    
    if provider_name == "openai" or provider_name == "openai_vision":
        try:
            provider = OpenAIVisionProvider(config_data=config_data)
            # 检查是否真正可用
            if provider._is_available():
                return provider, warnings
            else:
                warnings.append(
                    f"OpenAI provider configured but not available (missing api_key or package). Falling back to LocalRuleProvider."
                )
        except Exception as e:
            warnings.append(
                f"Failed to initialize OpenAI provider: {e}. Falling back to LocalRuleProvider."
            )
    else:
        warnings.append(
            f"Unknown provider: {provider_name}. Falling back to LocalRuleProvider."
        )
    
    # 默认或降级：使用 LocalRuleProvider
    return LocalRuleProvider(), warnings

