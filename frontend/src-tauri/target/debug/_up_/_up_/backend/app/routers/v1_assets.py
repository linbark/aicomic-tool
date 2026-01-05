from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy.orm import Session

from .. import schemas
from ..database import get_db
from ..services.visual_asset_ingestion import ingest_visual_assets


router = APIRouter(prefix="/api/v1/assets", tags=["Assets v1 (接口契约版)"])


@router.post("/ingest", response_model=schemas.VisualProfileLibrary)
def ingest_assets_to_visual_profile_v1(
    payload: schemas.VisualAssetIngestRequest,
    db: Session = Depends(get_db),
):
    """
    v1 契约路径：/api/v1/assets/ingest
    与 /assets/ingest 逻辑一致，用于后续版本化与前后端对齐。
    """
    try:
        return ingest_visual_assets(payload=payload, db=db)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"ingest failed: {e}")


