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

