"""
QCInspector Agent：检查连续性/逻辑/约束，输出 QCReport
P2 阶段：RFC6902 JSON Pointer fixes + 可修项覆盖
P3 阶段：ordered_tokens 锁定策略校验
"""
import re
import json
from typing import Optional, List

from .. import schemas
from ..providers import LLMProvider, LocalRuleProvider


def _fix_missing_visual_dna(
    prompt_packs: list[schemas.PromptPack],
    series_bible: schemas.SeriesBible,
    mode: str,
) -> list[schemas.QCFix]:
    """
    生成 Visual DNA 锁定缺失的 fixes
    返回：QCFix 列表（JSON Pointer path）
    """
    fixes: list[schemas.QCFix] = []
    if mode == "lint_only" or not series_bible.characters:
        return fixes

    main_char = series_bible.characters[0]
    visual_dna = main_char.visual_dna
    if not visual_dna:
        return fixes

    for pack_idx, pack in enumerate(prompt_packs):
        for item_idx, item in enumerate(pack.items):
            if not item.locked_visual_dna_included or visual_dna not in item.prompt:
                # 生成 fix：在 prompt 前置插入 visual_dna
                new_prompt = f"Character Visual DNA: {visual_dna}\n{item.prompt}"
                patch_ops = [
                    schemas.JSONPatchOp(
                        op="replace",
                        path=f"/prompt_packs/{pack_idx}/items/{item_idx}/prompt",
                        value=new_prompt,
                    ),
                    schemas.JSONPatchOp(
                        op="replace",
                        path=f"/prompt_packs/{pack_idx}/items/{item_idx}/locked_visual_dna_included",
                        value=True,
                    ),
                ]
                fixes.append(
                    schemas.QCFix(
                        description=f"Add visual_dna to prompt for {pack.dialect}/{item.panel_id}",
                        before_ref=f"/prompt_packs/{pack_idx}/items/{item_idx}",
                        after_patch=patch_ops,
                    )
                )
    return fixes


def _fix_missing_json_consistency(
    prompt_packs: list[schemas.PromptPack],
    visual_profile_library: Optional[schemas.VisualProfileLibrary],
    required_fields: list[str],
    mode: str,
) -> list[schemas.QCFix]:
    """
    生成 JSON 一致性层缺失的 fixes
    返回：QCFix 列表（JSON Pointer path）
    """
    fixes: list[schemas.QCFix] = []
    if mode == "lint_only" or not visual_profile_library or not required_fields:
        return fixes

    if not visual_profile_library.profiles:
        return fixes

    profile = visual_profile_library.profiles[0]
    core = profile.character_core

    # 提取 required_fields 的值
    extracted_parts: list[str] = []
    for field_path in required_fields:
        # 简化解析：$.character_core.visual_dna.xxx
        if field_path.startswith("$.character_core.visual_dna."):
            field_name = field_path.replace("$.character_core.visual_dna.", "")
            value = getattr(core.visual_dna, field_name, None)
            if value:
                extracted_parts.append(f"{field_name}: {value}")

    if not extracted_parts:
        return fixes

    extracted_text = ", ".join(extracted_parts)

    for pack_idx, pack in enumerate(prompt_packs):
        for item_idx, item in enumerate(pack.items):
            if item.locked_visual_profile_included is not True:
                # 生成 fix：在 prompt 中添加 required_fields
                new_prompt = f"{item.prompt}\nVisual Profile Fields: {extracted_text}"
                patch_ops = [
                    schemas.JSONPatchOp(
                        op="replace",
                        path=f"/prompt_packs/{pack_idx}/items/{item_idx}/prompt",
                        value=new_prompt,
                    ),
                    schemas.JSONPatchOp(
                        op="replace",
                        path=f"/prompt_packs/{pack_idx}/items/{item_idx}/locked_visual_profile_included",
                        value=True,
                    ),
                ]
                fixes.append(
                    schemas.QCFix(
                        description=f"Add required_fields to prompt for {pack.dialect}/{item.panel_id}",
                        before_ref=f"/prompt_packs/{pack_idx}/items/{item_idx}",
                        after_patch=patch_ops,
                    )
                )
    return fixes


def _tokenize_visual_dna(visual_dna: str) -> List[str]:
    """
    对 visual_dna 字符串进行 token 化
    P3 简单实现：按 JSON 标点/空白/常见分隔符切分
    """
    if not visual_dna:
        return []

    # 移除 JSON 结构标记（引号、括号等），保留内容
    # 先尝试解析为 JSON（如果是 JSON 字符串）
    try:
        parsed = json.loads(visual_dna)
        # 如果是 JSON，提取所有字符串值
        tokens = []
        def extract_strings(obj):
            if isinstance(obj, str):
                # 对字符串按空白和标点分割
                parts = re.split(r'[\s,;:]+', obj)
                tokens.extend([p.strip() for p in parts if p.strip()])
            elif isinstance(obj, dict):
                for v in obj.values():
                    extract_strings(v)
            elif isinstance(obj, list):
                for item in obj:
                    extract_strings(item)
        extract_strings(parsed)
        return [t for t in tokens if len(t) > 1]  # 过滤单字符
    except:
        # 不是 JSON，按空白和标点分割
        parts = re.split(r'[\s,;:(){}[\]"\']+', visual_dna)
        return [p.strip() for p in parts if p.strip() and len(p.strip()) > 1]


def _check_ordered_tokens_in_prompt(tokens: List[str], prompt: str) -> tuple[bool, List[str]]:
    """
    检查 tokens 是否按顺序在 prompt 中出现（允许间隔）
    返回: (是否通过, 缺失的 tokens)
    """
    if not tokens:
        return True, []

    prompt_lower = prompt.lower()
    missing_tokens: List[str] = []
    last_pos = -1

    for token in tokens:
        token_lower = token.lower()
        # 查找 token 在 prompt 中的位置（从上次位置之后开始）
        pos = prompt_lower.find(token_lower, last_pos + 1)
        if pos == -1:
            missing_tokens.append(token)
        else:
            last_pos = pos

    return len(missing_tokens) == 0, missing_tokens


def _fix_ordered_tokens_missing(
    prompt_packs: list[schemas.PromptPack],
    series_bible: schemas.SeriesBible,
    mode: str,
) -> list[schemas.QCFix]:
    """
    生成 ordered_tokens 缺失的 fixes
    返回：QCFix 列表（JSON Pointer path）
    """
    fixes: list[schemas.QCFix] = []
    if mode == "lint_only" or not series_bible.characters:
        return fixes

    main_char = series_bible.characters[0]
    visual_dna = main_char.visual_dna
    if not visual_dna:
        return fixes

    # Token 化 visual_dna
    tokens = _tokenize_visual_dna(visual_dna)
    if not tokens:
        return fixes

    for pack_idx, pack in enumerate(prompt_packs):
        for item_idx, item in enumerate(pack.items):
            passed, missing = _check_ordered_tokens_in_prompt(tokens, item.prompt)
            if not passed:
                # 生成 fix：在 prompt 前置插入缺失的 tokens
                missing_text = ", ".join(missing)
                new_prompt = f"Character Visual DNA tokens: {missing_text}\n{item.prompt}"
                patch_ops = [
                    schemas.JSONPatchOp(
                        op="replace",
                        path=f"/prompt_packs/{pack_idx}/items/{item_idx}/prompt",
                        value=new_prompt,
                    ),
                ]
                fixes.append(
                    schemas.QCFix(
                        description=f"Add missing ordered tokens to prompt for {pack.dialect}/{item.panel_id}: {missing_text}",
                        before_ref=f"/prompt_packs/{pack_idx}/items/{item_idx}",
                        after_patch=patch_ops,
                    )
                )
    return fixes


def _fix_bubble_text_limit(
    storyboard: schemas.Storyboard,
    limit: int,
    mode: str,
) -> list[schemas.QCFix]:
    """
    生成气泡超字数的 fixes（截断）
    返回：QCFix 列表（JSON Pointer path）
    """
    fixes: list[schemas.QCFix] = []
    if mode == "lint_only":
        return fixes

    for scene_idx, scene in enumerate(storyboard.scenes):
        for panel_idx, panel in enumerate(scene.panels):
            for bubble_idx, bubble in enumerate(panel.dialogues):
                if len(bubble.text) > limit:
                    truncated = bubble.text[:limit] + "..."
                    patch_ops = [
                        schemas.JSONPatchOp(
                            op="replace",
                            path=f"/storyboard/scenes/{scene_idx}/panels/{panel_idx}/dialogues/{bubble_idx}/text",
                            value=truncated,
                        ),
                    ]
                    fixes.append(
                        schemas.QCFix(
                            description=f"Truncate dialogue in {panel.panel_id} from {len(bubble.text)} to {limit} chars",
                            before_ref=f"/storyboard/scenes/{scene_idx}/panels/{panel_idx}/dialogues/{bubble_idx}",
                            after_patch=patch_ops,
                        )
                    )
    return fixes


def run_qc_inspector(
    req: schemas.AgentRequest,
    provider: Optional[LLMProvider] = None,
) -> schemas.AgentResponse:
    """
    QCInspector：检查 artifacts 的约束合规性
    - 输入：context 中包含要检查的 artifacts（series_bible, storyboard, prompt_packs 等）
    - 输出：QCReport（含 RFC6902 JSON Pointer fixes）
    """
    if provider is None:
        provider = LocalRuleProvider()

    warnings: list[str] = []
    errors: list[schemas.AgentError] = []
    checks: list[schemas.QCCheck] = []
    constraints = req.constraints
    mode = req.input.get("mode", "lint_only")

    # 解析 context 中的 artifacts
    storyboard = None
    prompt_packs: list[schemas.PromptPack] = []
    series_bible = None
    fountain_script = None
    visual_profile_library = None

    if "fountain_script" in req.context and req.context.get("fountain_script") is not None:
        try:
            fountain_script = schemas.FountainScript.model_validate(req.context["fountain_script"])
        except Exception:
            pass  # 可选

    if "storyboard" in req.context:
        try:
            storyboard = schemas.Storyboard.model_validate(req.context["storyboard"])
        except Exception as e:
            errors.append(
                schemas.AgentError(
                    code="INVALID_STORYBOARD",
                    message=f"Failed to parse storyboard: {e}",
                )
            )

    if "prompt_packs" in req.context:
        try:
            packs_data = req.context["prompt_packs"]
            if isinstance(packs_data, list):
                prompt_packs = [schemas.PromptPack.model_validate(p) for p in packs_data]
        except Exception as e:
            errors.append(
                schemas.AgentError(
                    code="INVALID_PROMPT_PACKS",
                    message=f"Failed to parse prompt_packs: {e}",
                )
            )

    if "series_bible" in req.context and req.context.get("series_bible") is not None:
        try:
            series_bible = schemas.SeriesBible.model_validate(req.context["series_bible"])
        except Exception:
            pass  # 可选

    if "visual_profile_library" in req.context and req.context.get("visual_profile_library") is not None:
        try:
            visual_profile_library = schemas.VisualProfileLibrary.model_validate(
                req.context["visual_profile_library"]
            )
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

    # P2 检查点

    # 0. Fountain Script Lint 汇总
    if fountain_script and fountain_script.lint:
        lint_result = fountain_script.lint
        if not lint_result.is_valid:
            checks.append(
                schemas.QCCheck(
                    name="fountain_lint_errors",
                    result="fail",
                    details=f"Fountain script has {len(lint_result.errors)} lint errors",
                    evidence=[e.message for e in lint_result.errors[:5]],
                    fixes=[],  # P2 暂不自动修复 fountain lint
                )
            )

    # 1. Visual DNA 锁定标志检查 + fixes
    if constraints.visual_dna_locking.enabled:
        check_name = "visual_dna_locking_flags"
        all_passed = True
        failed_items: list[str] = []

        for pack in prompt_packs:
            for item in pack.items:
                if not item.locked_visual_dna_included:
                    all_passed = False
                    failed_items.append(f"{pack.dialect}/{item.panel_id}")

        fixes = _fix_missing_visual_dna(prompt_packs, series_bible, mode) if series_bible else []

        checks.append(
            schemas.QCCheck(
                name=check_name,
                result="pass" if all_passed else "fail",
                details=f"Checked {sum(len(p.items) for p in prompt_packs)} prompt items"
                + (f"; failed: {', '.join(failed_items)}" if failed_items else ""),
                evidence=failed_items if failed_items else [],
                fixes=fixes,
            )
        )

    # 2. JSON 一致性层锁定标志检查 + fixes
    if constraints.json_consistency.enabled:
        check_name = "json_consistency_locking_flags"
        all_passed = True
        failed_items: list[str] = []

        for pack in prompt_packs:
            for item in pack.items:
                if item.locked_visual_profile_included is not True:
                    all_passed = False
                    failed_items.append(f"{pack.dialect}/{item.panel_id}")

        fixes = (
            _fix_missing_json_consistency(
                prompt_packs,
                visual_profile_library,
                constraints.json_consistency.required_fields,
                mode,
            )
            if visual_profile_library
            else []
        )

        checks.append(
            schemas.QCCheck(
                name=check_name,
                result="pass" if all_passed else "fail",
                details=f"Checked {sum(len(p.items) for p in prompt_packs)} prompt items"
                + (f"; failed: {', '.join(failed_items)}" if failed_items else ""),
                evidence=failed_items if failed_items else [],
                fixes=fixes,
            )
        )

    # 3. 气泡字数检查 + fixes
    if storyboard:
        check_name = "bubble_text_limit"
        all_passed = True
        failed_bubbles: list[str] = []
        limit = constraints.bubble_text_limit_zh

        for scene in storyboard.scenes:
            for panel in scene.panels:
                for bubble in panel.dialogues:
                    if len(bubble.text) > limit:
                        all_passed = False
                        failed_bubbles.append(f"{panel.panel_id}: '{bubble.text[:20]}...' ({len(bubble.text)} chars)")

        fixes = _fix_bubble_text_limit(storyboard, limit, mode)

        checks.append(
            schemas.QCCheck(
                name=check_name,
                result="pass" if all_passed else "fail",
                details=f"Checked dialogues; limit={limit} chars"
                + (f"; {len(failed_bubbles)} exceeded limit" if failed_bubbles else ""),
                evidence=failed_bubbles[:5],  # 只显示前 5 个
                fixes=fixes,
            )
        )

    # 4. Visual DNA 实际包含性检查（如果启用 verbatim 或 ordered_tokens）
    if constraints.visual_dna_locking.enabled and series_bible:
        policy = constraints.visual_dna_locking.policy
        if policy == "verbatim":
            check_name = "visual_dna_verbatim_inclusion"
            all_passed = True
            failed_items: list[str] = []

            if series_bible.characters:
                main_char = series_bible.characters[0]
                visual_dna = main_char.visual_dna

                for pack in prompt_packs:
                    for item in pack.items:
                        # 检查 prompt 中是否包含 visual_dna 子串
                        if visual_dna and visual_dna not in item.prompt:
                            all_passed = False
                            failed_items.append(f"{pack.dialect}/{item.panel_id}")

            # 复用 _fix_missing_visual_dna（如果缺失）
            fixes = _fix_missing_visual_dna(prompt_packs, series_bible, mode) if not all_passed else []

            checks.append(
                schemas.QCCheck(
                    name=check_name,
                    result="pass" if all_passed else "fail",
                    details=f"Checked verbatim inclusion of visual_dna in prompts"
                    + (f"; {len(failed_items)} missing" if failed_items else ""),
                    evidence=failed_items[:5],
                    fixes=fixes,
                )
            )
        elif policy == "ordered_tokens":
            check_name = "visual_dna_ordered_tokens"
            all_passed = True
            failed_items: list[str] = []
            missing_tokens_evidence: list[str] = []

            if series_bible.characters:
                main_char = series_bible.characters[0]
                visual_dna = main_char.visual_dna
                if visual_dna:
                    tokens = _tokenize_visual_dna(visual_dna)

                    for pack in prompt_packs:
                        for item in pack.items:
                            passed, missing = _check_ordered_tokens_in_prompt(tokens, item.prompt)
                            if not passed:
                                all_passed = False
                                failed_items.append(f"{pack.dialect}/{item.panel_id}")
                                missing_tokens_evidence.append(
                                    f"{pack.dialect}/{item.panel_id}: missing {', '.join(missing[:3])}"
                                )

            fixes = _fix_ordered_tokens_missing(prompt_packs, series_bible, mode) if not all_passed else []

            checks.append(
                schemas.QCCheck(
                    name=check_name,
                    result="pass" if all_passed else "fail",
                    details=f"Checked ordered tokens inclusion of visual_dna in prompts"
                    + (f"; {len(failed_items)} failed" if failed_items else ""),
                    evidence=missing_tokens_evidence[:5],
                    fixes=fixes,
                )
            )

    # 计算 summary
    all_checks_passed = all(c.result == "pass" for c in checks)
    pass_count = sum(1 for c in checks if c.result == "pass")
    total_count = len(checks)
    score = (pass_count / total_count * 100) if total_count > 0 else 100.0

    qc_report = schemas.QCReport(
        summary=schemas.QCReportSummary(pass_=all_checks_passed, score=score),
        checks=checks,
        rounds=0,  # 由 workflow 设置
    )

    if not all_checks_passed:
        warnings.append(f"QC checks failed: {total_count - pass_count}/{total_count}")

    return schemas.AgentResponse(
        request_id=req.request_id,
        agent=req.agent,
        status="ok",
        output=qc_report.model_dump(),
        warnings=warnings,
        meta={"provider": provider.provider_name, "mode": mode},
    )
