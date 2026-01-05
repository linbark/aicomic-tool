"""
BeatSheetAgent：从 source_text + SeriesBible 生成 BeatSheet
P1 阶段：规则保底实现（按段落/句号分块生成 beats）
"""
from typing import Optional

from .. import schemas
from ..providers import LLMProvider, LocalRuleProvider


def run_beat_sheet_agent(
    req: schemas.AgentRequest,
    provider: Optional[LLMProvider] = None,
) -> schemas.AgentResponse:
    """
    BeatSheetAgent：生成 BeatSheet
    - 输入：source_text +（可选）series_bible
    - 输出：BeatSheet
    """
    if provider is None:
        provider = LocalRuleProvider()

    warnings: list[str] = []
    errors: list[schemas.AgentError] = []

    # 解析输入
    source_text = req.input.get("source_text", "")
    series_bible = None
    if "series_bible" in req.context and req.context.get("series_bible") is not None:
        try:
            series_bible = schemas.SeriesBible.model_validate(req.context["series_bible"])
        except Exception as e:
            errors.append(
                schemas.AgentError(
                    code="INVALID_SERIES_BIBLE",
                    message=f"Failed to parse series_bible: {e}",
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

    if not source_text:
        return schemas.AgentResponse(
            request_id=req.request_id,
            agent=req.agent,
            status="error",
            errors=[
                schemas.AgentError(
                    code="MISSING_SOURCE_TEXT",
                    message="source_text is required for BeatSheetAgent",
                )
            ],
            warnings=warnings,
        )

    try:
        # P1 保底策略：按段落/句号分块生成 beats
        # 1. 分段（按双换行或段落）
        paragraphs = []
        for para in source_text.split("\n\n"):
            para = para.strip()
            if para:
                paragraphs.append(para)

        # 如果只有一个段落，按句号分割
        if len(paragraphs) == 1:
            sentences = [s.strip() for s in paragraphs[0].split("。") if s.strip()]
            # 每 2-3 句合并为一个段落
            merged = []
            for i in range(0, len(sentences), 2):
                merged.append("。".join(sentences[i : i + 2]) + "。")
            paragraphs = merged

        # 2. 生成 beats（6-12 个）
        beats: list[schemas.Beat] = []
        beat_types = [
            "开场",
            "激励事件",
            "第一转折点",
            "中点",
            "危机点",
            "高潮",
            "结局",
        ]

        # 确保至少有 6 个 beats
        num_beats = max(6, min(12, len(paragraphs)))
        if len(paragraphs) < num_beats:
            # 如果段落不够，重复最后一个段落
            while len(paragraphs) < num_beats:
                paragraphs.append(paragraphs[-1] if paragraphs else "继续发展")

        for i in range(num_beats):
            beat_id = f"beat_{i+1}"
            beat_type = beat_types[i % len(beat_types)]
            para_text = paragraphs[i] if i < len(paragraphs) else f"场景 {i+1}"

            # 估算 panels（每段约 2-4 格）
            estimated_panels = min(4, max(2, len(para_text) // 50))

            # 标记高潮/转折点为 page_turn_candidate
            page_turn_candidate = beat_type in ["高潮", "危机点", "第一转折点"]

            # 情绪电荷（简化：高潮为正，危机为负，其他交替）
            if beat_type == "高潮":
                emotion = schemas.BeatEmotionCharge(start="+", end="+")
            elif beat_type == "危机点":
                emotion = schemas.BeatEmotionCharge(start="-", end="-")
            else:
                emotion = schemas.BeatEmotionCharge(
                    start="+" if i % 2 == 0 else "-",
                    end="+" if (i + 1) % 2 == 0 else "-",
                )

            beat = schemas.Beat(
                id=beat_id,
                type=beat_type,
                emotion_charge=emotion,
                visual_focus=para_text[:50] + "..." if len(para_text) > 50 else para_text,
                estimated_panels=estimated_panels,
                page_turn_candidate=page_turn_candidate,
            )
            beats.append(beat)

        # 3. 生成 arc_summary
        arc_summary = f"故事包含 {len(beats)} 个节拍，从开场到结局的完整弧光。"

        beat_sheet = schemas.BeatSheet(
            arc_summary=arc_summary,
            beats=beats,
        )

        return schemas.AgentResponse(
            request_id=req.request_id,
            agent=req.agent,
            status="ok",
            output=beat_sheet.model_dump(),
            warnings=warnings,
            meta={"provider": provider.provider_name, "beats_count": len(beats)},
        )

    except Exception as e:
        return schemas.AgentResponse(
            request_id=req.request_id,
            agent=req.agent,
            status="error",
            errors=[
                schemas.AgentError(
                    code="BEAT_SHEET_AGENT_FAILED",
                    message=f"Failed to generate BeatSheet: {e}",
                )
            ],
            warnings=warnings,
        )

