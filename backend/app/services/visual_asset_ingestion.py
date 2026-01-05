import os
import mimetypes
from typing import Optional

from sqlalchemy.orm import Session, joinedload

from .. import models, schemas
from ..utils.metadata_parser import extract_metadata
from ..utils.visual_profile import (
    extract_dominant_palette,
    resolve_image_path,
    guess_face_stub,
    build_visual_dna_string_from_profile_dict,
)
from .providers import LLMProvider, LocalRuleProvider


def ingest_visual_assets(
    payload: schemas.VisualAssetIngestRequest,
    db: Session,
    data_root: Optional[str] = None,
    provider: Optional[LLMProvider] = None,
) -> schemas.VisualProfileLibrary:
    """
    VisualAssetIngestor（支持 Vision/LLM 反推）：
    - 输入：assets 引用（asset_id 或 image_ref）
    - 输出：VisualProfileLibrary（严格 JSON）
    - 副作用：若引用了 asset_id，则把生成的 visual_profile 写入 Asset.meta_data（可追溯）
    - 若 provider 可用且支持 Vision：调用 LLM 反推角色特征
    """
    if data_root is None:
        data_root = os.environ.get("AICOMIC_DATA_DIR") or os.path.join(os.getcwd(), "data")

    lib = schemas.VisualProfileLibrary(schema_version=payload.schema_version, profiles=[])

    for ref in payload.assets:
        asset = None
        image_ref = ref.image_ref
        name = ref.name
        profile_id = ref.id

        if ref.asset_id is not None:
            asset = (
                db.query(models.Asset)
                .options(joinedload(models.Asset.character))
                .filter(models.Asset.id == ref.asset_id)
                .first()
            )
            if not asset:
                raise ValueError(f"Asset not found: {ref.asset_id}")
            if asset.file_type != "image":
                raise ValueError(f"Asset is not image: {ref.asset_id} ({asset.file_type})")

            image_ref = asset.file_path
            if name is None and asset.character is not None:
                name = asset.character.name
            if profile_id is None:
                profile_id = str(ref.asset_id)

        if not image_ref:
            raise ValueError("Missing image_ref (or asset_id)")

        image_path = resolve_image_path(data_root, image_ref)
        if not os.path.exists(image_path):
            raise ValueError(f"Image not found on disk: {image_ref}")

        palette = []
        if payload.extract_mode == "character_plus_technical":
            palette = extract_dominant_palette(image_path, k=6)

        meta = extract_metadata(image_path) if (asset is None or asset.meta_data is None) else (asset.meta_data or {})
        sd_prompt = None
        try:
            # metadata_parser 可能提取到 SD WebUI 的 prompt / negative_prompt
            sd_prompt = meta.get("prompt")
        except Exception:
            sd_prompt = None

        # 基础 profile（本地提取）
        profile = schemas.VisualProfile(
            id=profile_id or image_ref,
            name=name,
            source=schemas.VisualProfileSource(type="image", ref=image_ref),
            character_core=schemas.CharacterCoreJson(
                visual_dna=schemas.VisualDNAJson(face=guess_face_stub()),
                attire=None,
            ),
            technical_specs=schemas.TechnicalSpecsJson(color_palette=palette) if palette else None,
            stable_diffusion_tags=sd_prompt,
            notes="generated_by_local_ingestor",
        )

        # 尝试使用 Vision/LLM 反推（如果 provider 可用）
        if provider and provider.provider_name != "local_rules":
            try:
                # 读取图片数据
                with open(image_path, "rb") as f:
                    image_data = f.read()

                # 推断 MIME 类型
                mime_type, _ = mimetypes.guess_type(image_path)
                if not mime_type:
                    mime_type = "image/jpeg"

                # 构建 Vision prompt
                vision_prompt = f"""Analyze this character image and extract visual characteristics.

Fill in the VisualProfile schema with:
- character_core.visual_dna.face: Describe facial features (e.g., "round face, large eyes, small nose")
- character_core.visual_dna.body_type: Body shape/build (e.g., "slim", "athletic", "average")
- character_core.visual_dna.hair_style: Hair style and color (e.g., "short black hair", "long blonde ponytail")
- character_core.visual_dna.distinguishing_marks: Notable features (e.g., "scar on left cheek", "glasses")
- character_core.attire.base_layer: Main clothing (e.g., "school uniform", "casual t-shirt and jeans")
- character_core.attire.accessories: List of accessories (e.g., ["glasses", "watch"])

Keep existing technical_specs.color_palette if present.
Return ONLY the JSON object matching VisualProfile schema."""

                # 调用 Vision API
                if hasattr(provider, "generate_json"):
                    try:
                        llm_profile = provider.generate_json(
                            prompt=vision_prompt,
                            schema=schemas.VisualProfile,
                            image_data=image_data,
                            image_mime_type=mime_type,
                        )

                        # 合并 LLM 结果到现有 profile（保留本地提取的 palette 等）
                        profile_dict = profile.model_dump()
                        llm_dict = llm_profile.model_dump()

                        # 合并 character_core
                        if llm_dict.get("character_core"):
                            if llm_dict["character_core"].get("visual_dna"):
                                profile_dict["character_core"]["visual_dna"].update(
                                    llm_dict["character_core"]["visual_dna"]
                                )
                            if llm_dict["character_core"].get("attire"):
                                profile_dict["character_core"]["attire"] = llm_dict["character_core"]["attire"]

                        # 保留本地提取的 technical_specs（palette）
                        if profile_dict.get("technical_specs") and profile_dict["technical_specs"].get("color_palette"):
                            if llm_dict.get("technical_specs"):
                                llm_dict["technical_specs"]["color_palette"] = profile_dict["technical_specs"]["color_palette"]
                                profile_dict["technical_specs"] = llm_dict["technical_specs"]
                        elif llm_dict.get("technical_specs"):
                            profile_dict["technical_specs"] = llm_dict["technical_specs"]

                        # 保留其他字段（id, name, source, stable_diffusion_tags）
                        profile_dict["notes"] = "generated_by_vision_llm"
                        if llm_dict.get("stable_diffusion_tags"):
                            profile_dict["stable_diffusion_tags"] = llm_dict["stable_diffusion_tags"]

                        # 重新构建 profile
                        profile = schemas.VisualProfile.model_validate(profile_dict)
                    except Exception as e:
                        # Vision 调用失败，使用本地结果
                        profile.notes = f"generated_by_local_ingestor (vision_failed: {str(e)[:50]})"
            except Exception as e:
                # 读取图片或其他错误，使用本地结果
                profile.notes = f"generated_by_local_ingestor (error: {str(e)[:50]})"

        # 生成确定性的 visual_dna_string（用于后续 SeriesBible/提示词锁定）
        profile.visual_dna_string = build_visual_dna_string_from_profile_dict(profile.model_dump())

        lib.profiles.append(profile)

        # 写回 meta_data（仅当 asset_id 引用时）
        if asset is not None:
            merged = dict(asset.meta_data or {})
            merged["visual_profile_schema_version"] = payload.schema_version
            merged["visual_profile"] = profile.model_dump()
            asset.meta_data = merged
            db.add(asset)

    db.commit()
    return lib


