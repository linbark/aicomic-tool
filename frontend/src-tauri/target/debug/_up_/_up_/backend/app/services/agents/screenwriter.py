"""
Screenwriter Agent：从 BeatSheet + SeriesBible 生成 FountainScript
P1 阶段：规则保底实现（生成基本 Fountain 格式）
"""
from typing import Optional

from .. import schemas
from ..providers import LLMProvider, LocalRuleProvider
from ...utils.fountain_lint import lint_fountain_script


def run_screenwriter(
    req: schemas.AgentRequest,
    provider: Optional[LLMProvider] = None,
) -> schemas.AgentResponse:
    """
    Screenwriter：生成 FountainScript
    - 输入：beat_sheet + series_bible
    - 输出：FountainScript（含 lint 结果）
    """
    if provider is None:
        provider = LocalRuleProvider()

    warnings: list[str] = []
    errors: list[schemas.AgentError] = []

    # 解析输入
    beat_sheet = None
    if "beat_sheet" in req.context and req.context.get("beat_sheet") is not None:
        try:
            beat_sheet = schemas.BeatSheet.model_validate(req.context["beat_sheet"])
        except Exception as e:
            errors.append(
                schemas.AgentError(
                    code="INVALID_BEAT_SHEET",
                    message=f"Failed to parse beat_sheet: {e}",
                )
            )

    series_bible = None
    if "series_bible" in req.context and req.context.get("series_bible") is not None:
        try:
            series_bible = schemas.SeriesBible.model_validate(req.context["series_bible"])
        except Exception:
            pass  # 可选

    if errors:
        return schemas.AgentResponse(
            request_id=req.request_id,
            agent=req.agent,
            status="error",
            errors=errors,
            warnings=warnings,
        )

    if not beat_sheet:
        return schemas.AgentResponse(
            request_id=req.request_id,
            agent=req.agent,
            status="error",
            errors=[
                schemas.AgentError(
                    code="MISSING_BEAT_SHEET",
                    message="beat_sheet is required for Screenwriter",
                )
            ],
            warnings=warnings,
        )

    try:
        # P1 保底策略：从 BeatSheet 生成基本 Fountain 格式
        fountain_lines: list[str] = []

        # 生成场景标题和动作
        scene_num = 1
        for beat in beat_sheet.beats:
            # 场景标题
            location = "室内" if scene_num % 2 == 1 else "室外"
            fountain_lines.append(f"INT. {location.upper()} - 场景{scene_num}")
            fountain_lines.append("")

            # 动作描述（从 visual_focus）
            action_text = beat.visual_focus
            # 确保不超过 action_block_max_lines
            action_lines = action_text.split("\n")
            max_lines = req.constraints.action_block_max_lines
            if len(action_lines) > max_lines:
                action_lines = action_lines[:max_lines]
                action_text = "\n".join(action_lines)

            fountain_lines.append(action_text)
            fountain_lines.append("")

            # 对话（P1 暂不生成，留空）
            # 后续可以从 source_text 或其他来源提取

            scene_num += 1

        fountain_text = "\n".join(fountain_lines)

        # 执行 lint
        lint_result = lint_fountain_script(fountain_text, req.constraints)

        fountain_script = schemas.FountainScript(
            format="fountain",
            text=fountain_text,
            lint=lint_result,
            is_valid=lint_result.is_valid,
            errors=lint_result.errors,
        )

        if not lint_result.is_valid:
            warnings.append(f"Fountain script has {len(lint_result.errors)} lint errors")

        return schemas.AgentResponse(
            request_id=req.request_id,
            agent=req.agent,
            status="ok",
            output=fountain_script.model_dump(),
            warnings=warnings,
            meta={"provider": provider.provider_name, "scenes_count": scene_num - 1},
        )

    except Exception as e:
        return schemas.AgentResponse(
            request_id=req.request_id,
            agent=req.agent,
            status="error",
            errors=[
                schemas.AgentError(
                    code="SCREENWRITER_FAILED",
                    message=f"Failed to generate FountainScript: {e}",
                )
            ],
            warnings=warnings,
        )

