"""
Manju Workflow Orchestrator：编排多个 Agent 的调用与上下文注入
P1 阶段：串联 ingest -> narrative -> beat -> screenwriter -> storyboard -> qc -> refinement_loop
"""
from __future__ import annotations

from sqlalchemy.orm import Session

from .. import schemas
from .visual_asset_ingestion import ingest_visual_assets
from .agents import (
    run_narrative_architect,
    run_beat_sheet_agent,
    run_screenwriter,
    run_storyboard_translator,
    run_qc_inspector,
)
from .providers import create_provider
from ..utils.json_patch import apply_json_patch


def run_manju_workflow(req: schemas.ManjuWorkflowRequest, db: Session) -> schemas.ManjuWorkflowResponse:
    """
    Manju Workflow：完整编排
    - 可选：先 ingest assets -> VisualProfileLibrary
    - NarrativeArchitect -> SeriesBible
    - BeatSheetAgent -> BeatSheet
    - Screenwriter -> FountainScript
    - StoryboardTranslator -> Storyboard + PromptPacks
    - QCInspector -> QCReport
    - 若启用 refinement_loop：QC fail -> 应用 fixes -> 再 QC（最多 max_rounds）
    """
    warnings: list[str] = []
    errors: list[schemas.AgentError] = []
    provider, provider_warnings = create_provider(db=db)
    warnings.extend(provider_warnings)

    # 1. VisualAssetIngestor（如果提供了 assets）
    vpl = None
    if req.assets:
        ingest_req = schemas.VisualAssetIngestRequest(
            assets=req.assets,
            schema_version=req.constraints.json_consistency.schema_version,
            extract_mode="character_plus_technical",
            allow_overrides=req.constraints.json_consistency.allow_overrides,
        )
        try:
            vpl = ingest_visual_assets(payload=ingest_req, db=db, provider=provider)
        except Exception as e:
            errors.append(schemas.AgentError(code="VISUAL_INGEST_FAILED", message=str(e)))

    if errors:
        return schemas.ManjuWorkflowResponse(
            request_id=req.request_id,
            status="error",
            errors=errors,
            warnings=warnings,
            visual_profile_library=vpl,
        )

    # 2. NarrativeArchitect
    series_bible = None
    context = {}
    if vpl:
        context["visual_profile_library"] = vpl.model_dump()
    agent_req = schemas.AgentRequest(
        request_id=f"{req.request_id}_narrative",
        agent="NarrativeArchitect",
        version="0.1",
        input={"source_text": req.source_text},
        context=context,
        constraints=req.constraints,
    )
    narrative_resp = run_narrative_architect(agent_req, provider=provider)
    if narrative_resp.status == "ok" and narrative_resp.output:
        try:
            series_bible = schemas.SeriesBible.model_validate(narrative_resp.output)
        except Exception as e:
            errors.append(
                schemas.AgentError(
                    code="NARRATIVE_ARCHITECT_PARSE_FAILED",
                    message=f"Failed to parse SeriesBible: {e}",
                )
            )
    else:
        errors.extend(narrative_resp.errors)
        warnings.extend(narrative_resp.warnings)

    if errors:
        return schemas.ManjuWorkflowResponse(
            request_id=req.request_id,
            status="error",
            errors=errors,
            warnings=warnings,
            visual_profile_library=vpl,
            series_bible=series_bible,
        )

    # 3. BeatSheetAgent
    beat_sheet = None
    if series_bible:
        context = {
            "series_bible": series_bible.model_dump(),
        }
        agent_req = schemas.AgentRequest(
            request_id=f"{req.request_id}_beat",
            agent="BeatSheetAgent",
            version="0.1",
            input={"source_text": req.source_text},
            context=context,
            constraints=req.constraints,
        )
        beat_resp = run_beat_sheet_agent(agent_req, provider=provider)
        if beat_resp.status == "ok" and beat_resp.output:
            try:
                beat_sheet = schemas.BeatSheet.model_validate(beat_resp.output)
            except Exception as e:
                errors.append(
                    schemas.AgentError(
                        code="BEAT_SHEET_AGENT_PARSE_FAILED",
                        message=f"Failed to parse BeatSheet: {e}",
                    )
                )
        else:
            errors.extend(beat_resp.errors)
            warnings.extend(beat_resp.warnings)

    # 4. Screenwriter
    fountain_script = None
    if beat_sheet:
        context = {
            "beat_sheet": beat_sheet.model_dump(),
            "series_bible": series_bible.model_dump() if series_bible else None,
        }
        if series_bible:
            context["series_bible"] = series_bible.model_dump()
        agent_req = schemas.AgentRequest(
            request_id=f"{req.request_id}_screenwriter",
            agent="Screenwriter",
            version="0.1",
            input={},
            context=context,
            constraints=req.constraints,
        )
        screenwriter_resp = run_screenwriter(agent_req, provider=provider)
        if screenwriter_resp.status == "ok" and screenwriter_resp.output:
            try:
                fountain_script = schemas.FountainScript.model_validate(screenwriter_resp.output)
            except Exception as e:
                errors.append(
                    schemas.AgentError(
                        code="SCREENWRITER_PARSE_FAILED",
                        message=f"Failed to parse FountainScript: {e}",
                    )
                )
        else:
            errors.extend(screenwriter_resp.errors)
            warnings.extend(screenwriter_resp.warnings)

    # 5. StoryboardTranslator
    storyboard = None
    prompt_packs: list[schemas.PromptPack] = []
    if series_bible:
        context = {
            "series_bible": series_bible.model_dump(),
        }
        if vpl:
            context["visual_profile_library"] = vpl.model_dump()
        if fountain_script:
            context["fountain_script"] = fountain_script.model_dump()
        agent_req = schemas.AgentRequest(
            request_id=f"{req.request_id}_storyboard",
            agent="StoryboardTranslator",
            version="0.1",
            input={
                "source_text": req.source_text,
                "dialects": req.target.dialects,
                "pages": req.target.pages,
            },
            context=context,
            constraints=req.constraints,
        )
        storyboard_resp = run_storyboard_translator(agent_req, provider=provider)
        if storyboard_resp.status == "ok" and storyboard_resp.output:
            try:
                output = storyboard_resp.output
                if "storyboard" in output:
                    storyboard = schemas.Storyboard.model_validate(output["storyboard"])
                if "prompt_packs" in output:
                    prompt_packs = [schemas.PromptPack.model_validate(p) for p in output["prompt_packs"]]
            except Exception as e:
                errors.append(
                    schemas.AgentError(
                        code="STORYBOARD_TRANSLATOR_PARSE_FAILED",
                        message=f"Failed to parse Storyboard/PromptPacks: {e}",
                    )
                )
        else:
            errors.extend(storyboard_resp.errors)
            warnings.extend(storyboard_resp.warnings)

    if errors:
        return schemas.ManjuWorkflowResponse(
            request_id=req.request_id,
            status="error",
            errors=errors,
            warnings=warnings,
            visual_profile_library=vpl,
            series_bible=series_bible,
            beat_sheet=beat_sheet,
            fountain_script=fountain_script,
            storyboard=storyboard,
            prompt_packs=prompt_packs,
        )

    # 6. QCInspector + Refinement Loop
    qc_report = None
    max_rounds = req.constraints.refinement_loop.max_rounds if req.constraints.refinement_loop.enabled else 1
    current_round = 0
    patches_applied_count = 0

    while current_round < max_rounds:
        current_round += 1
        context = {}
        if series_bible:
            context["series_bible"] = series_bible.model_dump()
        if storyboard:
            context["storyboard"] = storyboard.model_dump()
        if prompt_packs:
            context["prompt_packs"] = [p.model_dump() for p in prompt_packs]
        if fountain_script:
            context["fountain_script"] = fountain_script.model_dump()

        mode = "auto_fix" if req.constraints.refinement_loop.enabled and current_round < max_rounds else "lint_only"
        agent_req = schemas.AgentRequest(
            request_id=f"{req.request_id}_qc_round{current_round}",
            agent="QCInspector",
            version="0.1",
            input={"mode": mode},
            context=context,
            constraints=req.constraints,
        )
        qc_resp = run_qc_inspector(agent_req, provider=provider)
        if qc_resp.status == "ok" and qc_resp.output:
            try:
                qc_report = schemas.QCReport.model_validate(qc_resp.output)
                qc_report.rounds = current_round
            except Exception as e:
                errors.append(
                    schemas.AgentError(
                        code="QC_INSPECTOR_PARSE_FAILED",
                        message=f"Failed to parse QCReport: {e}",
                    )
                )
                break
        else:
            errors.extend(qc_resp.errors)
            warnings.extend(qc_resp.warnings)
            break

        # 如果所有检查都通过，退出循环
        if qc_report and qc_report.summary.pass_:
            break

        # 应用 fixes（仅当启用 refinement_loop 且不是最后一轮）
        if req.constraints.refinement_loop.enabled and current_round < max_rounds and qc_report:
            fixes_applied = False
            round_patches_count = 0

            # 收集所有 fixes 的 patches
            all_patches: list[dict] = []
            for check in qc_report.checks:
                if check.result == "fail" and check.fixes:
                    for fix in check.fixes:
                        for patch_op in fix.after_patch:
                            all_patches.append(patch_op.model_dump())

            # 按路径分组应用（storyboard 和 prompt_packs 分开处理）
            storyboard_patches: list[dict] = []
            prompt_packs_patches: list[dict] = []

            for patch in all_patches:
                path = patch.get("path", "")
                if path.startswith("/storyboard"):
                    storyboard_patches.append(patch)
                elif path.startswith("/prompt_packs"):
                    prompt_packs_patches.append(patch)

            # 应用 storyboard patches
            if storyboard_patches and storyboard:
                try:
                    storyboard_dict = storyboard.model_dump()
                    storyboard_dict = apply_json_patch(storyboard_dict, storyboard_patches)
                    storyboard = schemas.Storyboard.model_validate(storyboard_dict)
                    fixes_applied = True
                    round_patches_count += len(storyboard_patches)
                except Exception as e:
                    warnings.append(f"Failed to apply storyboard patches: {e}")

            # 应用 prompt_packs patches
            if prompt_packs_patches and prompt_packs:
                try:
                    packs_dict = [p.model_dump() for p in prompt_packs]
                    packs_dict = apply_json_patch(packs_dict, prompt_packs_patches)
                    prompt_packs = [schemas.PromptPack.model_validate(p) for p in packs_dict]
                    fixes_applied = True
                    round_patches_count += len(prompt_packs_patches)
                except Exception as e:
                    warnings.append(f"Failed to apply prompt_packs patches: {e}")

            if not fixes_applied:
                # 没有可应用的 fixes，退出循环
                break

            # 记录本轮 patch 数量到 warnings（便于调试）并累加总数
            if round_patches_count > 0:
                patches_applied_count += round_patches_count
                warnings.append(f"Round {current_round}: Applied {round_patches_count} patches")

    # 汇总响应
    return schemas.ManjuWorkflowResponse(
        request_id=req.request_id,
        status="error" if errors else "ok",
        warnings=warnings,
        errors=errors,
        visual_profile_library=vpl,
        series_bible=series_bible,
        beat_sheet=beat_sheet,
        fountain_script=fountain_script,
        storyboard=storyboard,
        prompt_packs=prompt_packs,
        qc_report=qc_report,
        meta={
            "implemented_agents": [
                "VisualAssetIngestor",
                "NarrativeArchitect",
                "BeatSheetAgent",
                "Screenwriter",
                "StoryboardTranslator",
                "QCInspector",
            ],
            "pending_agents": [],
            "provider": provider.provider_name,
            "refinement_rounds": current_round,
            "patches_applied": patches_applied_count if req.constraints.refinement_loop.enabled else 0,
        },
    )
