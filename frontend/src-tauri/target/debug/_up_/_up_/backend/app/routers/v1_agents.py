from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy.orm import Session

from .. import schemas
from ..database import get_db
from ..services.visual_asset_ingestion import ingest_visual_assets
from ..services.agents import run_narrative_architect, run_beat_sheet_agent, run_screenwriter, run_storyboard_translator, run_qc_inspector
from ..services.providers import create_provider


router = APIRouter(prefix="/api/v1/agents", tags=["Agents v1 (接口契约版)"])


@router.post("/run", response_model=schemas.AgentResponse)
def run_agent(req: schemas.AgentRequest, db: Session = Depends(get_db)):
    """
    契约入口：POST /api/v1/agents/run
    当前实现：
    - VisualAssetIngestor：已实现（无 LLM 保底）
    - NarrativeArchitect：已实现（P0）
    - StoryboardTranslator：已实现（P0）
    - QCInspector：已实现（P0）
    - 其他 agent：返回 NOT_IMPLEMENTED（后续逐步补齐）
    """
    provider, _ = create_provider(db=db)

    if req.agent == "VisualAssetIngestor":
        try:
            payload = schemas.VisualAssetIngestRequest.model_validate(req.input)
            out = ingest_visual_assets(payload=payload, db=db, provider=provider)
            return schemas.AgentResponse(
                request_id=req.request_id,
                agent=req.agent,
                status="ok",
                output=out.model_dump(),
            )
        except ValueError as e:
            return schemas.AgentResponse(
                request_id=req.request_id,
                agent=req.agent,
                status="error",
                errors=[schemas.AgentError(code="VALIDATION_FAILED", message=str(e))],
            )
        except Exception as e:
            raise HTTPException(status_code=500, detail=f"agent run failed: {e}")

    elif req.agent == "NarrativeArchitect":
        return run_narrative_architect(req, provider=provider)

    elif req.agent == "BeatSheetAgent":
        return run_beat_sheet_agent(req, provider=provider)

    elif req.agent == "Screenwriter":
        return run_screenwriter(req, provider=provider)

    elif req.agent == "StoryboardTranslator":
        return run_storyboard_translator(req, provider=provider)

    elif req.agent == "QCInspector":
        return run_qc_inspector(req, provider=provider)

    return schemas.AgentResponse(
        request_id=req.request_id,
        agent=req.agent,
        status="error",
        errors=[schemas.AgentError(code="NOT_IMPLEMENTED", message=f"Agent not implemented: {req.agent}")],
    )
