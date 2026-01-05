from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy.orm import Session

from .. import schemas
from ..database import get_db
from ..services.manju_workflow import run_manju_workflow

router = APIRouter(prefix="/api/v1/workflows", tags=["Workflows v1 (契约版)"])


@router.post("/manju/run", response_model=schemas.ManjuWorkflowResponse)
def run_manju(req: schemas.ManjuWorkflowRequest, db: Session = Depends(get_db)):
    """
    v1 工作流入口：POST /api/v1/workflows/manju/run
    
    完整工作流编排：
    - VisualAssetIngestor（可选）
    - NarrativeArchitect -> SeriesBible
    - BeatSheetAgent -> BeatSheet
    - Screenwriter -> FountainScript
    - StoryboardTranslator -> Storyboard + PromptPacks
    - QCInspector -> QCReport（含 refinement_loop）
    """
    try:
        return run_manju_workflow(req=req, db=db)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"workflow run failed: {e}")
