"""
Fountain 脚本 lint 工具：检查格式、约束合规性
P2 阶段：基于 parse 结果的可靠检查
"""
import re
from typing import List

from .. import schemas
from .fountain_parse import fountain_parse


# 反心理描写禁用词表（最小集合，P1）
_ANTI_PSYCHOLOGIZING_WORDS = [
    "想", "觉得", "认为", "感觉", "意识到", "意识到", "明白", "理解",
    "思考", "回忆", "想起", "记得", "忘记", "希望", "担心", "害怕",
    "焦虑", "紧张", "兴奋", "沮丧", "失望", "满意", "后悔",
]


def lint_fountain_script(
    text: str,
    constraints: schemas.Constraints,
) -> schemas.FountainLintResult:
    """
    对 Fountain 脚本进行 lint 检查（基于 parse 结果）
    返回：FountainLintResult（is_valid, errors）
    """
    errors: List[schemas.AgentError] = []
    
    # P2: 使用 parse 结果
    parsed = fountain_parse(text)
    scenes = parsed["scenes"]
    actions = parsed["actions"]
    dialogues = parsed["dialogues"]
    
    lines = text.split("\n")

    # 1. 场景标题格式检查（基于 parse 结果）
    scene_title_pattern = re.compile(r"^(INT\.|EXT\.)\s+[A-Z][A-Z0-9\s]+$")
    for scene in scenes:
        if not scene_title_pattern.match(scene.title):
            errors.append(
                schemas.AgentError(
                    code="FOUNTAIN_SCENE_TITLE_INVALID",
                    message=f"Scene title must be uppercase and follow 'INT./EXT. LOCATION' format: {scene.title}",
                    field_path=f"$.text[{scene.line_number}]",
                    suggestion="Format: INT. LOCATION or EXT. LOCATION (all uppercase)",
                )
            )

    # 2. 动作段行数检查（基于 parse 结果）
    if constraints.action_block_max_lines > 0:
        for action in actions:
            if len(action.lines) > constraints.action_block_max_lines:
                errors.append(
                    schemas.AgentError(
                        code="FOUNTAIN_ACTION_BLOCK_TOO_LONG",
                        message=f"Action block exceeds {constraints.action_block_max_lines} lines: {len(action.lines)} lines",
                        field_path=f"$.text[{action.line_start}:{action.line_start + len(action.lines)}]",
                        suggestion=f"Split action block into multiple blocks (max {constraints.action_block_max_lines} lines)",
                    )
                )

    # 3. 气泡字数检查（基于 parse 结果）
    if constraints.bubble_text_limit_zh > 0:
        for dialogue in dialogues:
            if len(dialogue.text) > constraints.bubble_text_limit_zh:
                errors.append(
                    schemas.AgentError(
                        code="FOUNTAIN_BUBBLE_TEXT_TOO_LONG",
                        message=f"Dialogue exceeds {constraints.bubble_text_limit_zh} chars: {len(dialogue.text)} chars",
                        field_path=f"$.text[{dialogue.line_number}]",
                        suggestion=f"Split dialogue or reduce text (max {constraints.bubble_text_limit_zh} chars)",
                    )
                )

    # 4. 反心理描写检查
    if constraints.anti_psychologizing:
        for i, line in enumerate(lines):
            stripped = line.strip()
            if not stripped:
                continue

            # 检查是否包含禁用词
            for word in _ANTI_PSYCHOLOGIZING_WORDS:
                if word in stripped:
                    errors.append(
                        schemas.AgentError(
                            code="FOUNTAIN_PSYCHOLOGIZING_DETECTED",
                            message=f"Psychological verb detected: '{word}'",
                            field_path=f"$.text[{i}]",
                            suggestion=f"Replace psychological description with visual action. Remove: '{word}'",
                        )
                    )
                    break  # 每行只报第一个

    is_valid = len(errors) == 0

    return schemas.FountainLintResult(
        is_valid=is_valid,
        errors=errors,
    )


