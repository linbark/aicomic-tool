from __future__ import annotations

import uuid as _uuid

from fastapi import HTTPException
from sqlalchemy.orm import Session

from .. import models


def ensure_project_uuid(db: Session, project: models.Project) -> str:
    """
    确保项目存在 uuid（对外 ID）。
    返回 uuid（hex 字符串）。
    """
    u = (getattr(project, "uuid", None) or "").strip()
    if u:
        return u
    project.uuid = _uuid.uuid4().hex
    db.add(project)
    db.commit()
    db.refresh(project)
    return str(project.uuid)


def resolve_project(db: Session, project_id: str) -> models.Project:
    """
    将对外 project_id（UUID hex 字符串）解析为 Project ORM。

    兼容：
    - 若传入的是纯数字字符串，则尝试按旧的自增 id 查找（便于平滑升级）。
    """
    pid = (project_id or "").strip()
    if not pid:
        raise HTTPException(status_code=400, detail="project_id 不能为空")

    # 优先按 uuid 查
    p = db.query(models.Project).filter(models.Project.uuid == pid).first()
    if p:
        ensure_project_uuid(db, p)
        return p

    # 兼容旧的 int id
    if pid.isdigit():
        p = db.query(models.Project).filter(models.Project.id == int(pid)).first()
        if p:
            ensure_project_uuid(db, p)
            return p

    raise HTTPException(status_code=404, detail="Project not found")


def resolve_project_pk(db: Session, project_id: str) -> int:
    """把对外 project_id 解析为内部主键（int）。"""
    return int(resolve_project(db, project_id).id)


