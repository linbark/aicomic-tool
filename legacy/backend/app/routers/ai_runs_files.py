from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy.orm import Session

from ..database import get_db
from ..services.project_lookup import resolve_project_pk
from .ai_shared import _context_store


router = APIRouter(tags=["AI (DeepSeek)"])


@router.get("/runs-files")
def list_runs_files(project_id: str, db: Session = Depends(get_db)):
    """
    列出项目的所有 run 快照（仅返回 meta 信息）。
    """
    pid = resolve_project_pk(db, project_id)
    runs = _context_store.list_runs(project_id=pid)
    return {"project_id": str(project_id), "runs": runs}


@router.get("/runs-files/{run_id}")
def get_run_file(project_id: str, run_id: str, db: Session = Depends(get_db)):
    """
    读取指定 run 的完整信息（request + response + meta）。
    """
    pid = resolve_project_pk(db, project_id)
    run_data = _context_store.read_run(project_id=pid, run_id=run_id)
    if not run_data:
        raise HTTPException(status_code=404, detail="Run not found")
    return {"project_id": str(project_id), "run_id": run_id, **run_data}


@router.get("/runs-files/{run_id}/stages")
def list_run_stages(project_id: str, run_id: str, db: Session = Depends(get_db)):
    """
    列出该 run 的所有 stage（包含预览与时间戳）。
    """
    pid = resolve_project_pk(db, project_id)
    stages = _context_store.list_stages(project_id=pid, run_id=run_id)
    return {"project_id": str(project_id), "run_id": run_id, "stages": stages}


@router.get("/runs-files/{run_id}/stages/{stage_name}")
def get_run_stage(project_id: str, run_id: str, stage_name: str, db: Session = Depends(get_db)):
    """
    读取指定 stage 的内容。
    """
    pid = resolve_project_pk(db, project_id)
    stage_data = _context_store.read_stage(project_id=pid, run_id=run_id, stage_name=stage_name)
    if stage_data is None:
        raise HTTPException(status_code=404, detail="Stage not found")
    return {"project_id": str(project_id), "run_id": run_id, "stage_name": stage_name, "data": stage_data}
