import json
from pathlib import Path

import anyio


def test_chat_graph_interrupt_and_resume(monkeypatch, project_ids, test_paths):
    base_dir = Path(test_paths["base_dir"])
    settings_path = base_dir / "ai_settings.json"
    settings_path.write_text(
        json.dumps({"api_key": "k", "base_url": "http://localhost", "model": "m", "temperature": 0.1, "timeout_seconds": 5}),
        encoding="utf-8",
    )

    import app.routers.ai_shared as ai_shared
    import app.services.chat_graph as chat_graph
    from app.database import SessionLocal
    from app.services.context_store import ContextStore
    from app.routers.ai_chat import ChatActRequest

    async def _fake_chat(*, settings, messages):
        return json.dumps(
            {
                "intent_summary": "先入库再生成大纲",
                "steps": [{"action_key": "memory_extract_changeset", "why": "入库"}, {"action_key": "outline_generate", "why": "大纲"}],
                "final_action_key": "outline_generate",
            },
            ensure_ascii=False,
        )

    monkeypatch.setattr(ai_shared._chat_client, "chat", _fake_chat, raising=True)

    class _FakeIndexer:
        def index_series_bible(self, project_id, version="v1"):
            return None

    class _FakeRetriever:
        def retrieve_for_task(self, project_id, task_description):
            return {}

        def format_for_prompt(self, retrieval_results):
            return {}

    import app.services.memory_indexer as mem_indexer
    import app.services.memory_retriever as mem_retriever

    monkeypatch.setattr(mem_indexer, "MemoryIndexer", _FakeIndexer, raising=True)
    monkeypatch.setattr(mem_retriever, "get_memory_retriever", lambda: _FakeRetriever(), raising=True)

    async def _fake_execute_action_step(
        *,
        action_key,
        in_text,
        project_id_pk,
        project_uuid,
        episode_id,
        run_id,
        artifacts,
        cards,
        db,
    ):
        if action_key == "memory_extract_changeset":
            cards.append(
                {
                    "type": "review_changeset",
                    "changeset_id": "cs_test_1",
                    "title": "需要确认：更新设定/事件（ChangeSet）",
                    "summary": "- 实体：0",
                    "actions": [
                        {"action": "approve_changeset", "label": "确认提交", "changeset_id": "cs_test_1"},
                        {"action": "reject_changeset", "label": "驳回", "changeset_id": "cs_test_1"},
                    ],
                }
            )
            return "已生成待审阅变更单：cs_test_1"
        if action_key == "outline_generate":
            artifacts["outline"] = "OUT"
            return "OUT"
        return f"ok:{action_key}"

    monkeypatch.setattr(chat_graph, "_execute_action_step", _fake_execute_action_step, raising=True)

    db = SessionLocal()
    try:
        run_id = "run_test_interrupt_1"
        req = ChatActRequest(
            project_id=project_ids["uuid"],
            episode_id=1,
            current_action_key="",
            message="请把设定入库并生成大纲",
            ui_context={"master_script": "", "current_input": ""},
            debug=False,
            run_id=run_id,
        )

        anyio.run(lambda: chat_graph.run_chat_graph(req=req, db=db, emit_stages=True, run_id=run_id))

        store = ContextStore()
        interrupt_path = store.stage_path(project_id=project_ids["pk"], run_id=run_id, stage_name="chat.interrupt")
        final_path = store.stage_path(project_id=project_ids["pk"], run_id=run_id, stage_name="chat.final")
        assert Path(interrupt_path).exists()
        assert Path(final_path).exists()

        anyio.run(lambda: chat_graph.resume_chat_graph(project_id_pk=project_ids["pk"], run_id=run_id, decision="approved", db=db))

        fin = json.loads(Path(final_path).read_text(encoding="utf-8"))
        assert fin.get("created_run") is not None
        assert fin.get("run_id") == run_id
    finally:
        db.close()


def test_episode_execute_asset_ingest_plan_and_apply(project_ids):
    from app import models
    from app.database import SessionLocal
    from app.routers.ai_episode_execute import _apply_asset_ingest_plan, _build_asset_ingest_plan
    from app.services.context_store import ContextStore

    db = SessionLocal()
    try:
        pid = int(project_ids["pk"])
        existing = models.Character(
            project_id=pid,
            name="张三",
            description="old",
            base_prompt="old",
            category="persona_visual",
        )
        db.add(existing)
        db.commit()
        db.refresh(existing)

        assets_visual_dna = {
            "characters": [
                {"name": "张三", "description": "new", "visual_dna": {"face": "x"}, "stable_diffusion_tags": "a,b"},
                {"name": "李四", "description": "", "visual_dna": {"hair_style": "y"}, "stable_diffusion_tags": "c,d"},
            ],
            "props": [{"name": "玉佩", "description": "old jade", "stable_diffusion_tags": "jade,pendant"}],
            "locations": [{"name": "青云山", "description": "", "stable_diffusion_tags": "mountain,cloud"}],
            "series_style": {"lighting_style": "soft", "stable_diffusion_tags": "cinematic"},
        }

        plan = _build_asset_ingest_plan(db=db, project_id_pk=pid, assets_visual_dna=assets_visual_dna)
        preview = plan.get("preview") or {}
        assert preview.get("create_count") == 3
        assert preview.get("update_count") == 1
        assert preview.get("visual_dna_count") == 2
        assert preview.get("series_style") is True

        apply_res = _apply_asset_ingest_plan(
            db=db,
            project_id_pk=pid,
            plan={"items": plan.get("items") or [], "series_style": plan.get("series_style")},
        )
        assert apply_res.get("created") == 3
        assert apply_res.get("updated") == 1
        assert apply_res.get("visual_dna_written") == 2
        assert apply_res.get("series_style") is True

        zhang = db.query(models.Character).filter(models.Character.project_id == pid, models.Character.name == "张三").first()
        li = db.query(models.Character).filter(models.Character.project_id == pid, models.Character.name == "李四").first()
        prop = db.query(models.Character).filter(models.Character.project_id == pid, models.Character.name == "玉佩").first()
        loc = db.query(models.Character).filter(models.Character.project_id == pid, models.Character.name == "青云山").first()
        assert zhang and zhang.description == "new" and zhang.base_prompt == "a,b"
        assert li and li.category == "persona_visual"
        assert prop and prop.category == "prop"
        assert loc and loc.category == "background"

        store = ContextStore()
        vd1 = store.get_visual_dna(project_id=pid, item_id=int(zhang.id), version="v1")
        vd2 = store.get_visual_dna(project_id=pid, item_id=int(li.id), version="v1")
        assert isinstance(vd1, dict) and vd1.get("stable_diffusion_tags") == "a,b"
        assert isinstance(vd2, dict) and vd2.get("stable_diffusion_tags") == "c,d"
        sb = store.get_series_bible(project_id=pid, version="v1") or {}
        assert isinstance(sb, dict) and isinstance(sb.get("series_style"), dict)
    finally:
        db.close()


def test_episode_execute_skips_outline_and_assets_confirm(monkeypatch, project_ids):
    import json
    from pathlib import Path
    from types import SimpleNamespace
    import anyio

    from app import models
    from app.database import SessionLocal
    from app.services.chat_graph import StageEmitter
    from app.services.context_store import ContextStore
    import app.routers.ai_episode_execute as exe

    async def _fake_chat(*, settings, messages):
        return json.dumps(
            {
                "logline": "x",
                "characters": [],
                "act_1": "",
                "act_2": "",
                "act_3": "",
                "key_beats": [],
            },
            ensure_ascii=False,
        )

    async def _fake_llm_json(*, system_prompt, user_prompt, project_id_pk, run_id, raw_stage):
        if raw_stage == "episode_split_episodes.raw":
            return {"episodes": []}
        return {"characters": [], "props": [], "locations": [], "series_style": {}}

    def _fake_mask_settings(raw):
        return SimpleNamespace(
            has_api_key=True,
            base_url="http://localhost",
            model="m",
            temperature=0.1,
            max_tokens=256,
            timeout_seconds=5,
        )

    monkeypatch.setattr(
        exe,
        "_read_settings_raw",
        lambda: {"api_key": "k", "base_url": "http://localhost", "model": "m", "temperature": 0.1, "max_tokens": 256, "timeout_seconds": 5},
        raising=True,
    )
    monkeypatch.setattr(exe, "_mask_settings", _fake_mask_settings, raising=True)
    monkeypatch.setattr(exe._chat_client, "chat", _fake_chat, raising=True)
    monkeypatch.setattr(exe, "_llm_json", _fake_llm_json, raising=True)

    db = SessionLocal()
    try:
        pid = int(project_ids["pk"])
        ep = models.Episode(project_id=pid, title="E3", order=3, description="S")
        db.add(ep)
        db.commit()
        db.refresh(ep)

        run_id = "run_test_ep_execute_no_outline_assets_confirm"
        emitter = StageEmitter(project_id=pid, run_id=run_id, emit_stages=False)
        artifacts = {"script_text": "第一段。\n\n第二段。"}

        anyio.run(
            lambda: exe._run_until_interrupt(
                project_id_pk=pid,
                project_uuid=str(project_ids["uuid"]),
                episode_id=int(ep.id),
                run_id=run_id,
                script_text=str(artifacts["script_text"]),
                start_step_index=0,
                artifacts=artifacts,
                emitter=emitter,
                db=db,
            )
        )

        store = ContextStore()
        interrupt_path = store.stage_path(project_id=pid, run_id=run_id, stage_name="chat.interrupt")
        data = json.loads(Path(interrupt_path).read_text(encoding="utf-8"))
        assert data.get("kind") == "confirm_split"
        resume_state = data.get("resume_state") or {}
        resume_artifacts = resume_state.get("artifacts") or {}
        assert "outline" in resume_artifacts
        assert "assets_visual_dna" in resume_artifacts
    finally:
        db.close()


def test_episode_execute_step4_creates_changeset_and_interrupt(monkeypatch, project_ids):
    import json
    from pathlib import Path

    import anyio

    import conftest
    from app import models
    from app.database import SessionLocal
    from app.services.chat_graph import StageEmitter
    from app.services.context_store import ContextStore

    import app.routers.ai_episode_execute as exe

    store = conftest.FakeMemoryStore()
    monkeypatch.setattr(exe, "get_memory_store", lambda: store, raising=True)

    db = SessionLocal()
    try:
        pid = int(project_ids["pk"])
        ep = models.Episode(project_id=pid, title="E1", order=1, description="S")
        db.add(ep)
        db.commit()
        db.refresh(ep)

        run_id = "run_test_ep_execute_ingest_1"
        emitter = StageEmitter(project_id=pid, run_id=run_id, emit_stages=False)
        artifacts = {
            "script_text": "第一段。\n\n第二段。",
            "assets_visual_dna": {
                "characters": [{"name": "张三", "description": "d", "visual_dna": {"face": "x"}, "stable_diffusion_tags": "a,b"}],
                "props": [{"name": "玉佩", "description": "p", "stable_diffusion_tags": "jade"}],
                "locations": [{"name": "青云山", "description": "l", "stable_diffusion_tags": "mountain"}],
            },
            "split_episodes": {},
            "outline": "",
        }

        anyio.run(
            lambda: exe._run_until_interrupt(
                project_id_pk=pid,
                project_uuid=str(project_ids["uuid"]),
                episode_id=int(ep.id),
                run_id=run_id,
                script_text=str(artifacts["script_text"]),
                start_step_index=3,
                artifacts=artifacts,
                emitter=emitter,
                db=db,
            )
        )

        csid = str(artifacts.get("changeset_id") or "").strip()
        assert csid
        payload = store.get_changeset(csid)["payload"]
        assert payload.get("schema_version") == "changeset.v0"
        for k in [
            "project_id",
            "episode_id",
            "story_order_base",
            "evidences",
            "entities",
            "snapshots",
            "events",
            "state_changes",
            "time_constraints",
            "time_blocks",
            "conflicts",
            "materialize",
        ]:
            assert k in payload

        ctx = ContextStore()
        interrupt_path = ctx.stage_path(project_id=pid, run_id=run_id, stage_name="chat.interrupt")
        assert interrupt_path
        data = json.loads(Path(interrupt_path).read_text(encoding="utf-8"))
        assert data.get("kind") == "confirm_ingest"
        assert str(data.get("changeset_id") or "").strip() == csid
        rs = data.get("resume_state") or {}
        assert isinstance(rs, dict)
        ra = rs.get("artifacts") or {}
        assert isinstance(ra, dict)
        assert str(ra.get("changeset_id") or "").strip() == csid
        assert isinstance(ra.get("asset_ingest_plan"), dict)
    finally:
        db.close()


def test_episode_execute_confirm_emits_log(client, project_ids):
    from app import models
    from app.database import SessionLocal
    from app.services.context_store import ContextStore
    import app.routers.ai_episode_execute as exe

    db = SessionLocal()
    try:
        pid = int(project_ids["pk"])
        ep = models.Episode(project_id=pid, title="E2", order=2, description="S")
        db.add(ep)
        db.commit()
        db.refresh(ep)

        run_id = "run_test_ep_confirm_1"
        exe._write_interrupt_state(
            project_id_pk=pid,
            run_id=run_id,
            interrupt={
                "kind": "confirm_outline",
                "resume_state": {
                    "project_id_pk": pid,
                    "project_id": project_ids["uuid"],
                    "episode_id": int(ep.id),
                    "run_id": run_id,
                    "script_text": "",
                    "step_index": 1,
                    "artifacts": {},
                },
            },
        )

        res = client.post(
            f"/ai/episode-execute/{int(ep.id)}/confirm",
            json={"decision": "rejected", "run_id": run_id},
        )
        assert res.status_code == 200

        store = ContextStore()
        stages = store.list_stages(project_id=pid, run_id=run_id)
        log_names = [s["name"] for s in stages if str(s.get("name") or "").startswith("log.")]
        found = False
        for name in log_names:
            data = store.read_stage(project_id=pid, run_id=run_id, stage_name=name) or {}
            if data.get("stage") == "episode_execute.confirm":
                found = True
                break
        assert found
    finally:
        db.close()
