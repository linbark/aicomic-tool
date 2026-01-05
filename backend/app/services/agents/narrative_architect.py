"""
NarrativeArchitect Agent：从 source_text + VisualProfileLibrary 生成 SeriesBible
"""
from typing import Optional

from .. import schemas
from ..providers import LLMProvider, LocalRuleProvider


def run_narrative_architect(
    req: schemas.AgentRequest,
    provider: Optional[LLMProvider] = None,
) -> schemas.AgentResponse:
    """
    NarrativeArchitect：生成 SeriesBible
    - 输入：source_text + context.visual_profile_library
    - 输出：SeriesBible
    """
    if provider is None:
        provider = LocalRuleProvider()

    warnings: list[str] = []
    errors: list[schemas.AgentError] = []

    # 解析输入
    source_text = req.input.get("source_text", "")
    vpl = None
    if "visual_profile_library" in req.context and req.context.get("visual_profile_library") is not None:
        try:
            vpl = schemas.VisualProfileLibrary.model_validate(req.context["visual_profile_library"])
        except Exception as e:
            errors.append(
                schemas.AgentError(
                    code="INVALID_VISUAL_PROFILE_LIBRARY",
                    message=f"Failed to parse visual_profile_library: {e}",
                )
            )

    if errors:
        return schemas.AgentResponse(
            request_id=req.request_id,
            agent=req.agent,
            status="error",
            errors=errors,
            warnings=warnings,
        )

    # 生成 SeriesBible
    try:
        # P0 策略：从 VisualProfileLibrary 映射角色
        characters: list[schemas.CharacterProfile] = []
        if vpl and vpl.profiles:
            for profile in vpl.profiles:
                # 提取 do_not_change 列表（从 visual_dna 的非空字段）
                do_not_change: list[str] = []
                dna = profile.character_core.visual_dna
                if dna.hair_style:
                    do_not_change.append(f"hair_style: {dna.hair_style}")
                if dna.body_type:
                    do_not_change.append(f"body_type: {dna.body_type}")
                if dna.distinguishing_marks:
                    do_not_change.append(f"distinguishing_marks: {dna.distinguishing_marks}")

                char = schemas.CharacterProfile(
                    id=profile.id,
                    name=profile.name or profile.id,
                    visual_dna=profile.visual_dna_string or "",  # 使用不可变字符串镜像
                    do_not_change=do_not_change,
                )
                characters.append(char)

        # 提取 title（从 source_text 首行或默认值）
        title = "Untitled"
        if source_text:
            lines = source_text.strip().split("\n")
            if lines:
                first_line = lines[0].strip()
                if first_line and len(first_line) < 100:  # 合理长度
                    title = first_line
                else:
                    title = first_line[:97] + "..." if len(first_line) > 100 else first_line

        series_bible = schemas.SeriesBible(
            title=title,
            logline=None,  # P0 暂不生成
            visual_tone=None,  # P0 暂不生成
            characters=characters,
            world_rules=schemas.WorldRules(
                era=None,
                physics=[],
                technology=[],
                taboos=[],  # P0 为空数组
            ),
        )

        if not vpl and not source_text:
            warnings.append("No visual_profile_library or source_text provided; generated minimal SeriesBible")

        return schemas.AgentResponse(
            request_id=req.request_id,
            agent=req.agent,
            status="ok",
            output=series_bible.model_dump(),
            warnings=warnings,
            meta={"provider": provider.provider_name},
        )

    except Exception as e:
        return schemas.AgentResponse(
            request_id=req.request_id,
            agent=req.agent,
            status="error",
            errors=[
                schemas.AgentError(
                    code="NARRATIVE_ARCHITECT_FAILED",
                    message=f"Failed to generate SeriesBible: {e}",
                )
            ],
            warnings=warnings,
        )

