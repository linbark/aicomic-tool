"""
P1 测试脚本：手工验证 Manju Workflow
用法：
    python -m backend.app.devtools.test_workflow
或：
    cd backend && python -m app.devtools.test_workflow
"""
import sys
import os
import json
from pathlib import Path

# 添加项目根目录到路径
project_root = Path(__file__).parent.parent.parent.parent
sys.path.insert(0, str(project_root))

from backend.app.schemas import (
    ManjuWorkflowRequest,
    Constraints,
    JsonConsistencyConstraint,
    RefinementLoopConstraint,
    VisualDNALockingConstraint,
)
from backend.app.services.manju_workflow import run_manju_workflow
from backend.app.database import SessionLocal
from backend.app import models


def test_workflow_no_assets():
    """测试 1：无 assets，只用 source_text"""
    print("=" * 60)
    print("Test 1: Workflow without assets (source_text only)")
    print("=" * 60)

    db = SessionLocal()
    try:
        req = ManjuWorkflowRequest(
            request_id="test_no_assets_001",
            source_text="这是一个测试故事。主角走在街上。突然，他看到了一个神秘的人。",
            assets=[],
            constraints=Constraints(),
        )

        resp = run_manju_workflow(req, db)
        print(f"\nStatus: {resp.status}")
        print(f"Warnings: {resp.warnings}")
        if resp.errors:
            print(f"Errors: {[e.message for e in resp.errors]}")

        print(f"\nSeriesBible: {resp.series_bible.title if resp.series_bible else 'None'}")
        print(f"BeatSheet: {len(resp.beat_sheet.beats) if resp.beat_sheet else 0} beats")
        print(f"FountainScript: {len(resp.fountain_script.text) if resp.fountain_script else 0} chars")
        print(f"Storyboard scenes: {len(resp.storyboard.scenes) if resp.storyboard else 0}")
        print(f"PromptPacks: {len(resp.prompt_packs)}")
        print(f"QCReport pass: {resp.qc_report.summary.pass_ if resp.qc_report else 'None'}")
        if resp.qc_report:
            print(f"QCReport rounds: {resp.qc_report.rounds}")

        print("\n✅ Test 1 completed")
    finally:
        db.close()


def test_workflow_with_assets():
    """测试 2：有 assets（需要实际图片路径）"""
    print("\n" + "=" * 60)
    print("Test 2: Workflow with assets")
    print("=" * 60)

    # 检查是否有测试图片
    data_dir = os.environ.get("AICOMIC_DATA_DIR") or os.path.join(project_root, "data")
    test_images = list(Path(data_dir).glob("*.jpg")) + list(Path(data_dir).glob("*.png"))

    if not test_images:
        print("⚠️  No test images found in data/ directory. Skipping Test 2.")
        print("   To run this test, add some .jpg or .png files to the data/ directory.")
        return

    db = SessionLocal()
    try:
        # 使用第一个找到的图片
        test_image = test_images[0]
        image_ref = str(test_image.relative_to(Path(data_dir))) if Path(data_dir) in test_image.parents else str(test_image)

        req = ManjuWorkflowRequest(
            request_id="test_with_assets_001",
            source_text="这是一个带图片反推的测试。",
            assets=[
                {
                    "id": "test_char_1",
                    "name": "Test Character",
                    "image_ref": image_ref,
                }
            ],
            constraints=Constraints(
                json_consistency=JsonConsistencyConstraint(
                    enabled=True,
                    required_fields=["$.character_core.visual_dna.face"],
                )
            ),
        )

        resp = run_manju_workflow(req, db)
        print(f"\nStatus: {resp.status}")
        print(f"Warnings: {resp.warnings}")
        if resp.errors:
            print(f"Errors: {[e.message for e in resp.errors]}")

        print(f"\nVisualProfileLibrary: {len(resp.visual_profile_library.profiles) if resp.visual_profile_library else 0} profiles")
        print(f"SeriesBible characters: {len(resp.series_bible.characters) if resp.series_bible else 0}")
        print(f"BeatSheet: {len(resp.beat_sheet.beats) if resp.beat_sheet else 0} beats")
        print(f"FountainScript: {len(resp.fountain_script.text) if resp.fountain_script else 0} chars")
        print(f"Storyboard panels: {sum(len(s.panels) for s in resp.storyboard.scenes) if resp.storyboard else 0}")
        print(f"PromptPacks: {len(resp.prompt_packs)}")
        print(f"QCReport pass: {resp.qc_report.summary.pass_ if resp.qc_report else 'None'}")
        if resp.qc_report:
            print(f"QCReport rounds: {resp.qc_report.rounds}")

        print("\n✅ Test 2 completed")
    finally:
        db.close()


def test_workflow_refinement_loop():
    """测试 3：开启 refinement_loop（验证至少 1 轮修正发生）"""
    print("\n" + "=" * 60)
    print("Test 3: Workflow with refinement_loop enabled")
    print("=" * 60)

    db = SessionLocal()
    try:
        req = ManjuWorkflowRequest(
            request_id="test_refinement_loop_001",
            source_text="这是一个测试故事。主角走在街上。突然，他看到了一个神秘的人。",
            assets=[],
            constraints=Constraints(
                refinement_loop=RefinementLoopConstraint(
                    enabled=True,
                    max_rounds=3,
                )
            ),
        )

        resp = run_manju_workflow(req, db)
        print(f"\nStatus: {resp.status}")
        print(f"Warnings: {resp.warnings}")
        if resp.errors:
            print(f"Errors: {[e.message for e in resp.errors]}")

        print(f"\nSeriesBible: {resp.series_bible.title if resp.series_bible else 'None'}")
        print(f"BeatSheet: {len(resp.beat_sheet.beats) if resp.beat_sheet else 0} beats")
        print(f"FountainScript: {len(resp.fountain_script.text) if resp.fountain_script else 0} chars")
        print(f"Storyboard scenes: {len(resp.storyboard.scenes) if resp.storyboard else 0}")
        print(f"PromptPacks: {len(resp.prompt_packs)}")
        print(f"QCReport pass: {resp.qc_report.summary.pass_ if resp.qc_report else 'None'}")
        if resp.qc_report:
            print(f"QCReport rounds: {resp.qc_report.rounds}")
            print(f"QCReport checks: {len(resp.qc_report.checks)}")
            for check in resp.qc_report.checks:
                print(f"  - {check.name}: {check.result} ({len(check.fixes)} fixes)")

        # 验证 refinement_loop 是否工作
        if resp.qc_report and resp.qc_report.rounds > 1:
            print("\n✅ Refinement loop worked: multiple rounds executed")
        else:
            print("\n⚠️  Refinement loop may not have executed multiple rounds")

        print("\n✅ Test 3 completed")
    finally:
        db.close()


def test_workflow_refinement_loop_with_fault():
    """测试 4：refinement_loop + fault injection（故意制造 fail，验证 loop 修复）"""
    print("\n" + "=" * 60)
    print("Test 4: Workflow with refinement_loop + fault injection")
    print("=" * 60)

    db = SessionLocal()
    try:
        # 创建一个会触发 QC fail 的场景：超长对话
        source_text = "这是一个测试故事。主角走在街上。突然，他看到了一个神秘的人。"
        
        req = ManjuWorkflowRequest(
            request_id="test_refinement_loop_fault_001",
            source_text=source_text,
            assets=[],
            constraints=Constraints(
                bubble_text_limit_zh=10,  # 故意设置很小的限制
                refinement_loop=RefinementLoopConstraint(
                    enabled=True,
                    max_rounds=3,
                )
            ),
            options={"debug_fault_injection": True},  # 标记为 fault injection 测试
        )

        resp = run_manju_workflow(req, db)
        print(f"\nStatus: {resp.status}")
        print(f"Warnings: {resp.warnings}")
        if resp.errors:
            print(f"Errors: {[e.message for e in resp.errors]}")

        print(f"\nSeriesBible: {resp.series_bible.title if resp.series_bible else 'None'}")
        print(f"BeatSheet: {len(resp.beat_sheet.beats) if resp.beat_sheet else 0} beats")
        print(f"FountainScript: {len(resp.fountain_script.text) if resp.fountain_script else 0} chars")
        print(f"Storyboard scenes: {len(resp.storyboard.scenes) if resp.storyboard else 0}")
        print(f"PromptPacks: {len(resp.prompt_packs)}")
        print(f"QCReport pass: {resp.qc_report.summary.pass_ if resp.qc_report else 'None'}")
        if resp.qc_report:
            print(f"QCReport rounds: {resp.qc_report.rounds}")
            print(f"QCReport checks: {len(resp.qc_report.checks)}")
            total_fixes = sum(len(check.fixes) for check in resp.qc_report.checks)
            print(f"Total fixes generated: {total_fixes}")
            for check in resp.qc_report.checks:
                if check.fixes:
                    print(f"  - {check.name}: {check.result} ({len(check.fixes)} fixes)")

        # 验证 refinement_loop 是否工作
        if resp.qc_report:
            if resp.qc_report.rounds > 1:
                print("\n✅ Refinement loop worked: multiple rounds executed")
            if resp.meta.get("patches_applied", 0) > 0:
                print(f"✅ Patches applied: {resp.meta.get('patches_applied', 0)}")
            else:
                print("\n⚠️  No patches were applied (may be expected if all checks passed)")

        print("\n✅ Test 4 completed")
    finally:
        db.close()


def test_workflow_p3_ordered_tokens():
    """测试 5：P3 ordered_tokens 锁定策略 + json_consistency + 图片"""
    print("\n" + "=" * 60)
    print("Test 5: P3 ordered_tokens + json_consistency + image")
    print("=" * 60)

    # 检查是否有测试图片
    data_dir = os.environ.get("AICOMIC_DATA_DIR") or os.path.join(project_root, "data")
    test_images = list(Path(data_dir).glob("*.jpg")) + list(Path(data_dir).glob("*.png"))

    if not test_images:
        print("⚠️  No test images found in data/ directory. Skipping Test 5.")
        print("   To run this test, add some .jpg or .png files to the data/ directory.")
        return

    db = SessionLocal()
    try:
        test_image = test_images[0]
        image_ref = str(test_image.relative_to(Path(data_dir))) if Path(data_dir) in test_image.parents else str(test_image)

        req = ManjuWorkflowRequest(
            request_id="test_p3_ordered_tokens_001",
            source_text="这是一个测试故事，用于验证 ordered_tokens 锁定策略。",
            assets=[
                {
                    "id": "test_char_1",
                    "name": "Test Character",
                    "image_ref": image_ref,
                }
            ],
            constraints=Constraints(
                json_consistency=JsonConsistencyConstraint(
                    enabled=True,
                    required_fields=["$.character_core.visual_dna.face", "$.character_core.visual_dna.hair_style"],
                ),
                visual_dna_locking=VisualDNALockingConstraint(
                    enabled=True,
                    policy="ordered_tokens",  # P3 新策略
                ),
                refinement_loop=RefinementLoopConstraint(
                    enabled=True,
                    max_rounds=3,
                ),
            ),
        )

        resp = run_manju_workflow(req, db)
        print(f"\nStatus: {resp.status}")
        print(f"Warnings: {resp.warnings}")
        if resp.errors:
            print(f"Errors: {[e.message for e in resp.errors]}")

        print(f"\nVisualProfileLibrary: {len(resp.visual_profile_library.profiles) if resp.visual_profile_library else 0} profiles")
        if resp.visual_profile_library and resp.visual_profile_library.profiles:
            profile = resp.visual_profile_library.profiles[0]
            print(f"  Profile ID: {profile.id}")
            print(f"  Visual DNA face: {profile.character_core.visual_dna.face}")
            print(f"  Visual DNA hair: {profile.character_core.visual_dna.hair_style}")
            print(f"  Notes: {profile.notes}")

        print(f"\nSeriesBible characters: {len(resp.series_bible.characters) if resp.series_bible else 0}")
        if resp.series_bible and resp.series_bible.characters:
            char = resp.series_bible.characters[0]
            print(f"  Character visual_dna: {char.visual_dna[:100] if char.visual_dna else 'None'}...")

        print(f"\nPromptPacks: {len(resp.prompt_packs)}")
        for pack in resp.prompt_packs:
            print(f"  {pack.dialect}: {len(pack.items)} items")
            if pack.items:
                item = pack.items[0]
                print(f"    First item params: {item.params}")
                print(f"    First item negative_prompt: {item.negative_prompt[:50] if item.negative_prompt else 'None'}...")

        print(f"\nQCReport pass: {resp.qc_report.summary.pass_ if resp.qc_report else 'None'}")
        if resp.qc_report:
            print(f"QCReport rounds: {resp.qc_report.rounds}")
            print(f"QCReport checks: {len(resp.qc_report.checks)}")
            for check in resp.qc_report.checks:
                print(f"  - {check.name}: {check.result}")
                if check.name == "visual_dna_ordered_tokens":
                    print(f"    Evidence: {check.evidence[:3]}")
                    print(f"    Fixes: {len(check.fixes)}")

        print("\n✅ Test 5 completed")
    finally:
        db.close()


def test_workflow_p3_no_key_fallback():
    """测试 6：P3 无配置降级路径（验证 LocalRuleProvider 正常工作）"""
    print("\n" + "=" * 60)
    print("Test 6: P3 no config fallback (LocalRuleProvider)")
    print("=" * 60)

    db = SessionLocal()
    try:
        # 确保没有激活的配置
        db.query(models.LLMProviderConfig).update({"is_active": False})
        db.commit()

        req = ManjuWorkflowRequest(
            request_id="test_p3_no_key_001",
            source_text="这是一个测试，验证无配置时的降级路径。",
            assets=[],
            constraints=Constraints(),
        )

        resp = run_manju_workflow(req, db)
        print(f"\nStatus: {resp.status}")
        print(f"Warnings: {resp.warnings}")
        if resp.errors:
            print(f"Errors: {[e.message for e in resp.errors]}")

        # 检查是否使用了 LocalRuleProvider
        provider_name = resp.meta.get("provider", "unknown")
        print(f"\nProvider used: {provider_name}")
        if provider_name == "local_rules":
            print("✅ Fallback to LocalRuleProvider worked")
        else:
            print(f"⚠️  Expected 'local_rules', got '{provider_name}'")

        print(f"\nSeriesBible: {resp.series_bible.title if resp.series_bible else 'None'}")
        print(f"Storyboard scenes: {len(resp.storyboard.scenes) if resp.storyboard else 0}")
        print(f"PromptPacks: {len(resp.prompt_packs)}")

        print("\n✅ Test 6 completed")
    finally:
        db.close()


if __name__ == "__main__":
    print("Manju Workflow P3 Test Suite")
    print("=" * 60)
    print("\nLLM Provider Configuration:")
    print("  Provider configs are now stored in database (llm_provider_configs table)")
    print("  Use API endpoints to configure:")
    print("    POST /api/v1/config/providers - Create provider config")
    print("    PUT /api/v1/config/providers/{name} - Update provider config")
    print("    POST /api/v1/config/providers/{name}/activate - Activate provider")
    print("=" * 60)

    try:
        test_workflow_no_assets()
        test_workflow_with_assets()
        test_workflow_refinement_loop()
        test_workflow_refinement_loop_with_fault()
        test_workflow_p3_ordered_tokens()
        test_workflow_p3_no_key_fallback()
        print("\n" + "=" * 60)
        print("All tests completed!")
    except Exception as e:
        print(f"\n❌ Test failed with error: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)

