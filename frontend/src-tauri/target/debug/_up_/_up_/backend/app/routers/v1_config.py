"""
LLM Provider 配置管理 API
支持前端配置和管理多个 LLM Provider
"""
from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy.orm import Session
from typing import List, Optional

from .. import schemas, models
from ..database import get_db

router = APIRouter(prefix="/api/v1/config", tags=["LLM Provider Config"])


@router.get("/providers", response_model=List[schemas.LLMProviderConfigRead])
def list_providers(db: Session = Depends(get_db)):
    """获取所有 LLM Provider 配置"""
    configs = db.query(models.LLMProviderConfig).all()
    return configs


@router.get("/providers/active", response_model=Optional[schemas.LLMProviderConfigRead])
def get_active_provider(db: Session = Depends(get_db)):
    """获取当前激活的 LLM Provider 配置"""
    config = (
        db.query(models.LLMProviderConfig)
        .filter(models.LLMProviderConfig.is_active == True)
        .first()
    )
    return config


@router.get("/providers/{provider_name}", response_model=schemas.LLMProviderConfigRead)
def get_provider(provider_name: str, db: Session = Depends(get_db)):
    """获取指定名称的 LLM Provider 配置"""
    config = (
        db.query(models.LLMProviderConfig)
        .filter(models.LLMProviderConfig.provider_name == provider_name)
        .first()
    )
    if not config:
        raise HTTPException(status_code=404, detail=f"Provider '{provider_name}' not found")
    return config


@router.post("/providers", response_model=schemas.LLMProviderConfigRead)
def create_provider_config(
    config: schemas.LLMProviderConfigCreate,
    db: Session = Depends(get_db),
):
    """创建新的 LLM Provider 配置"""
    # 检查是否已存在同名配置
    existing = (
        db.query(models.LLMProviderConfig)
        .filter(models.LLMProviderConfig.provider_name == config.provider_name)
        .first()
    )
    if existing:
        raise HTTPException(
            status_code=400,
            detail=f"Provider '{config.provider_name}' already exists. Use PUT to update.",
        )
    
    # 如果设置为激活，先取消其他激活的配置
    if config.is_active:
        db.query(models.LLMProviderConfig).update({"is_active": False})
    
    db_config = models.LLMProviderConfig(
        provider_name=config.provider_name,
        is_active=config.is_active,
        config_data=config.config_data,
        notes=config.notes,
    )
    db.add(db_config)
    db.commit()
    db.refresh(db_config)
    return db_config


@router.put("/providers/{provider_name}", response_model=schemas.LLMProviderConfigRead)
def update_provider_config(
    provider_name: str,
    config_update: schemas.LLMProviderConfigUpdate,
    db: Session = Depends(get_db),
):
    """更新 LLM Provider 配置"""
    db_config = (
        db.query(models.LLMProviderConfig)
        .filter(models.LLMProviderConfig.provider_name == provider_name)
        .first()
    )
    if not db_config:
        raise HTTPException(status_code=404, detail=f"Provider '{provider_name}' not found")
    
    # 如果设置为激活，先取消其他激活的配置
    if config_update.is_active is True:
        db.query(models.LLMProviderConfig).filter(
            models.LLMProviderConfig.provider_name != provider_name
        ).update({"is_active": False})
    
    # 更新字段
    if config_update.is_active is not None:
        db_config.is_active = config_update.is_active
    if config_update.config_data is not None:
        db_config.config_data = config_update.config_data
    if config_update.notes is not None:
        db_config.notes = config_update.notes
    
    db.commit()
    db.refresh(db_config)
    return db_config


@router.delete("/providers/{provider_name}")
def delete_provider_config(provider_name: str, db: Session = Depends(get_db)):
    """删除 LLM Provider 配置"""
    db_config = (
        db.query(models.LLMProviderConfig)
        .filter(models.LLMProviderConfig.provider_name == provider_name)
        .first()
    )
    if not db_config:
        raise HTTPException(status_code=404, detail=f"Provider '{provider_name}' not found")
    
    db.delete(db_config)
    db.commit()
    return {"message": f"Provider '{provider_name}' deleted"}


@router.post("/providers/{provider_name}/activate", response_model=schemas.LLMProviderConfigRead)
def activate_provider(provider_name: str, db: Session = Depends(get_db)):
    """激活指定的 LLM Provider（取消其他激活）"""
    db_config = (
        db.query(models.LLMProviderConfig)
        .filter(models.LLMProviderConfig.provider_name == provider_name)
        .first()
    )
    if not db_config:
        raise HTTPException(status_code=404, detail=f"Provider '{provider_name}' not found")
    
    # 取消所有其他激活
    db.query(models.LLMProviderConfig).update({"is_active": False})
    
    # 激活当前配置
    db_config.is_active = True
    db.commit()
    db.refresh(db_config)
    return db_config

