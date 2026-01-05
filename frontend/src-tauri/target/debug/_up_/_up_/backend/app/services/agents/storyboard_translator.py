"""
StoryboardTranslator Agent：从 SeriesBible + source_text 生成 Storyboard + PromptPacks
"""
from typing import Optional

from .. import schemas
from ..providers import LLMProvider, LocalRuleProvider
from ...utils.fountain_parse import fountain_parse
from ...utils.controlled_vocab import (
    normalize_shot_size,
    normalize_shot_angle,
    normalize_lighting_style,
    ShotSize,
    ShotAngle,
    LightingStyle,
)
from ...utils.prompt_policy import generate_prompt_params_for_dialect


def _extract_visual_dna_from_profile(
    profile: schemas.VisualProfile,
    required_fields: list[str],
) -> str:
    """
    从 VisualProfile 中提取 required_fields 对应的值，拼成字符串
    """
    parts: list[str] = []
    core = profile.character_core

    for field_path in required_fields:
        # 简化 JSONPath 解析：只支持 $.character_core.visual_dna.xxx
        if field_path.startswith("$.character_core.visual_dna."):
            field_name = field_path.replace("$.character_core.visual_dna.", "")
            value = getattr(core.visual_dna, field_name, None)
            if value:
                parts.append(f"{field_name}: {value}")
        elif field_path.startswith("$.character_core.attire."):
            field_name = field_path.replace("$.character_core.attire.", "")
            if core.attire:
                value = getattr(core.attire, field_name, None)
                if value:
                    parts.append(f"{field_name}: {value}")

    return ", ".join(parts) if parts else ""


def _build_prompt_for_panel(
    panel: schemas.Panel,
    series_bible: schemas.SeriesBible,
    visual_profile_library: Optional[schemas.VisualProfileLibrary],
    constraints: schemas.Constraints,
    dialect: schemas.Dialect,
) -> tuple[str, bool, bool]:
    """
    为单个 panel 构建 prompt，并返回锁定标志
    返回: (prompt_text, locked_visual_dna_included, locked_visual_profile_included)
    """
    prompt_parts: list[str] = []

    # 1. Visual DNA 锁定（如果启用）
    locked_visual_dna_included = False
    if constraints.visual_dna_locking.enabled:
        # 使用主角的 visual_dna（或第一个角色）
        if series_bible.characters:
            main_char = series_bible.characters[0]
            if main_char.visual_dna:
                prompt_parts.append(f"Character Visual DNA: {main_char.visual_dna}")
                locked_visual_dna_included = True

    # 2. JSON 一致性层（如果启用）
    locked_visual_profile_included = False
    if constraints.json_consistency.enabled and visual_profile_library:
        required_fields = constraints.json_consistency.required_fields
        if required_fields and visual_profile_library.profiles:
            # 使用第一个 profile（或匹配的 profile）
            profile = visual_profile_library.profiles[0]
            extracted = _extract_visual_dna_from_profile(profile, required_fields)
            if extracted:
                prompt_parts.append(f"Visual Profile Fields: {extracted}")
                locked_visual_profile_included = True

    # 3. Panel action（场景描述）
    if panel.action:
        prompt_parts.append(f"Scene: {panel.action}")

    # 4. Shot 信息（使用受控词汇表）
    if panel.shot:
        normalized_size = normalize_shot_size(panel.shot.size)
        normalized_angle = normalize_shot_angle(panel.shot.angle)
        prompt_parts.append(f"Shot: {normalized_size}, angle: {normalized_angle}")

    # 5. Lighting（使用受控词汇表）
    if panel.lighting:
        normalized_lighting = normalize_lighting_style(panel.lighting)
        prompt_parts.append(f"Lighting: {normalized_lighting}")

    # 6. Dialogues（如果有）
    if panel.dialogues:
        dialogue_texts = [f"{b.speaker}: {b.text}" for b in panel.dialogues]
        prompt_parts.append(f"Dialogues: {' | '.join(dialogue_texts)}")

    # 根据 dialect 调整格式
    prompt_text = ", ".join(prompt_parts)
    if dialect == "midjourney_v6":
        # Midjourney 风格：可以加 --ar 等参数
        pass  # P0 暂不添加参数
    elif dialect in ["stable_diffusion", "flux"]:
        # SD/Flux 可能需要 negative_prompt
        pass  # P0 暂不生成 negative_prompt

    return prompt_text, locked_visual_dna_included, locked_visual_profile_included


def run_storyboard_translator(
    req: schemas.AgentRequest,
    provider: Optional[LLMProvider] = None,
) -> schemas.AgentResponse:
    """
    StoryboardTranslator：生成 Storyboard + PromptPacks
    - 输入：SeriesBible + source_text（可选）
    - 输出：Storyboard + PromptPacks（按 target.dialects）
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

    visual_profile_library = None
    if "visual_profile_library" in req.context and req.context.get("visual_profile_library") is not None:
        try:
            visual_profile_library = schemas.VisualProfileLibrary.model_validate(
                req.context["visual_profile_library"]
            )
        except Exception:
            pass  # 可选，不报错

    fountain_script = None
    if "fountain_script" in req.context and req.context.get("fountain_script") is not None:
        try:
            fountain_script = schemas.FountainScript.model_validate(req.context["fountain_script"])
        except Exception:
            pass  # 可选，不报错

    # 获取 target（从 input 或默认值）
    dialects = req.input.get("dialects", ["midjourney_v6", "stable_diffusion", "flux"])
    pages = req.input.get("pages", None)
    target_panels = (pages * 4) if pages else 8  # 假设每页 4 格

    if errors:
        return schemas.AgentResponse(
            request_id=req.request_id,
            agent=req.agent,
            status="error",
            errors=errors,
            warnings=warnings,
        )

    if not series_bible:
        return schemas.AgentResponse(
            request_id=req.request_id,
            agent=req.agent,
            status="error",
            errors=[
                schemas.AgentError(
                    code="MISSING_SERIES_BIBLE",
                    message="series_bible is required for StoryboardTranslator",
                )
            ],
            warnings=warnings,
        )

    try:
        # P1 策略：优先从 Fountain 解析，否则回退到 source_text
        panels: list[schemas.Panel] = []
        
        if fountain_script and fountain_script.text:
            # P2: 使用 fountain_parse 解析
            parsed = fountain_parse(fountain_script.text)
            scenes_parsed = parsed["scenes"]
            actions_parsed = parsed["actions"]
            dialogues_parsed = parsed["dialogues"]
            
            panel_id_counter = 1
            
            # 按场景和动作块生成 panels
            for scene in scenes_parsed:
                # 每个动作块生成一个 panel
                for action in actions_parsed:
                    # 找到属于当前场景的对话
                    scene_dialogues: list[schemas.Bubble] = []
                    for dialogue in dialogues_parsed:
                        # 简单匹配：对话行号在场景范围内
                        if scene.line_number <= dialogue.line_number:
                            scene_dialogues.append(
                                schemas.Bubble(speaker=dialogue.speaker, text=dialogue.text)
                            )
                    
                    action_text = "\n".join(action.lines)
                    panels.append(
                        schemas.Panel(
                            panel_id=f"p{panel_id_counter}",
                            shot=schemas.ShotSpec(
                                size=ShotSize.MEDIUM.value,
                                angle=ShotAngle.EYE_LEVEL.value,
                            ),
                            lighting=LightingStyle.NATURAL.value,
                            action=action_text,
                            dialogues=scene_dialogues[:3],  # 每个 panel 最多 3 个对话
                            visual_constraints=[],
                            layout=None,
                            page_hint=None,
                        )
                    )
                    panel_id_counter += 1
                
                # 如果没有动作块，至少生成一个 panel
                if not actions_parsed:
                    panels.append(
                        schemas.Panel(
                            panel_id=f"p{panel_id_counter}",
                            shot=schemas.ShotSpec(
                                size=ShotSize.MEDIUM.value,
                                angle=ShotAngle.EYE_LEVEL.value,
                            ),
                            lighting=LightingStyle.NATURAL.value,
                            action=scene.title,
                            dialogues=[],
                            visual_constraints=[],
                            layout=None,
                            page_hint=None,
                        )
                    )
                    panel_id_counter += 1

        elif source_text:
            # 简单拆句（按句号/换行）
            sentences = []
            for line in source_text.split("\n"):
                line = line.strip()
                if line:
                    # 按句号分割
                    for sent in line.split("。"):
                        sent = sent.strip()
                        if sent:
                            sentences.append(sent)

            # 如果句子不够，用默认描述
            while len(sentences) < target_panels:
                sentences.append(f"Scene {len(sentences) + 1}")

            # 生成 panels（使用受控词汇表）
            for i, sentence in enumerate(sentences[:target_panels]):
                panel_id = f"p{i+1}"
                panels.append(
                    schemas.Panel(
                        panel_id=panel_id,
                        shot=schemas.ShotSpec(
                            size=ShotSize.MEDIUM.value,  # 使用受控词汇表
                            angle=ShotAngle.EYE_LEVEL.value,  # 使用受控词汇表
                        ),
                        lighting=LightingStyle.NATURAL.value,  # 使用受控词汇表
                        action=sentence,
                        dialogues=[],  # P0 暂不生成对话
                        visual_constraints=[],
                        layout=None,
                        page_hint=None,
                    )
                )
        else:
            # 无 source_text：生成默认 panels
            for i in range(target_panels):
                panel_id = f"p{i+1}"
                panels.append(
                    schemas.Panel(
                        panel_id=panel_id,
                        shot=schemas.ShotSpec(
                            size=ShotSize.MEDIUM.value,
                            angle=ShotAngle.EYE_LEVEL.value,
                        ),
                        lighting=LightingStyle.NATURAL.value,
                        action=f"Panel {i+1}",
                        dialogues=[],
                        visual_constraints=[],
                        layout=None,
                        page_hint=None,
                    )
                )

        # 2. 生成 StoryScene
        scene = schemas.StoryScene(scene_id="s1", panels=panels)
        storyboard = schemas.Storyboard(scenes=[scene])

        # 3. 生成 PromptPacks（按 dialects）
        prompt_packs: list[schemas.PromptPack] = []
        constraints = req.constraints

        for dialect in dialects:
            items: list[schemas.PromptItem] = []
            # 生成 dialect 特定的参数（所有 panel 共享基础参数）
            negative_prompt_base, params_base = generate_prompt_params_for_dialect(
                dialect=dialect,
                pages=pages,
                constraints=constraints,
            )

            for panel in panels:
                prompt_text, locked_dna, locked_profile = _build_prompt_for_panel(
                    panel=panel,
                    series_bible=series_bible,
                    visual_profile_library=visual_profile_library,
                    constraints=constraints,
                    dialect=dialect,
                )

                # 使用共享的基础参数（未来可基于 panel 特性微调）
                item = schemas.PromptItem(
                    panel_id=panel.panel_id,
                    prompt=prompt_text,
                    negative_prompt=negative_prompt_base,
                    params=params_base,
                    locked_visual_dna_included=locked_dna,
                    locked_visual_profile_included=locked_profile if constraints.json_consistency.enabled else None,
                )
                items.append(item)

            pack = schemas.PromptPack(
                dialect=dialect,
                items=items,
                json_consistency_enabled=constraints.json_consistency.enabled,
                visual_dna_locking_enabled=constraints.visual_dna_locking.enabled,
            )
            prompt_packs.append(pack)

        # 输出：包含 storyboard 和 prompt_packs
        output = {
            "storyboard": storyboard.model_dump(),
            "prompt_packs": [p.model_dump() for p in prompt_packs],
        }

        return schemas.AgentResponse(
            request_id=req.request_id,
            agent=req.agent,
            status="ok",
            output=output,
            warnings=warnings,
            meta={"provider": provider.provider_name, "panels_count": len(panels)},
        )

    except Exception as e:
        return schemas.AgentResponse(
            request_id=req.request_id,
            agent=req.agent,
            status="error",
            errors=[
                schemas.AgentError(
                    code="STORYBOARD_TRANSLATOR_FAILED",
                    message=f"Failed to generate Storyboard/PromptPacks: {e}",
                )
            ],
            warnings=warnings,
        )

