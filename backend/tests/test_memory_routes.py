import os
import json
from app.workflows.memory_schemas import TimeConstraint, TimeBlock


def test_time_constraints_crud(client, project_ids):
    c = TimeConstraint(project_id=project_ids["pk"], relation="before", from_event_id="e1", to_event_id="e2")
    resp = client.post("/memory/time-constraint", json={"constraint": c.model_dump(), "run_id": "test_run_id"})
    assert resp.status_code == 200
    cid = resp.json()["id"]
    r2 = client.get("/memory/time-constraints", params={"project_id": project_ids["uuid"], "run_id": "test_run_id"})
    assert r2.status_code == 200
    items = r2.json()["items"]
    assert any(i["id"] == cid for i in items)


def test_time_blocks_crud(client, project_ids):
    b = TimeBlock(project_id=project_ids["pk"], name="flashback")
    resp = client.post("/memory/time-block", json={"block": b.model_dump(), "run_id": "test_run_id"})
    assert resp.status_code == 200
    bid = resp.json()["id"]
    r2 = client.get("/memory/time-blocks", params={"project_id": project_ids["uuid"], "run_id": "test_run_id"})
    assert r2.status_code == 200
    items = r2.json()["items"]
    assert any(i["id"] == bid for i in items)


def test_changeset_crud(client, project_ids):
    payload = {"schema_version": "changeset.v0", "proposed": {"entities": []}}
    r1 = client.post("/memory/changeset", json={"project_id": project_ids["uuid"], "payload": payload, "run_id": "test_run_id"})
    assert r1.status_code == 200
    cid = r1.json()["changeset_id"]
    rlist = client.get("/memory/changesets", params={"project_id": project_ids["uuid"], "run_id": "test_run_id"})
    assert rlist.status_code == 200
    assert any(i["changeset_id"] == cid for i in rlist.json()["items"])
    rget = client.get(f"/memory/changeset/{cid}", params={"run_id": "test_run_id"})
    assert rget.status_code == 200
    rappr = client.post(f"/memory/changeset/{cid}/approve", json={"reviewer": "tester", "run_id": "test_run_id"})
    assert rappr.status_code == 200 and rappr.json()["message"] == "approved"
    rrej = client.post(f"/memory/changeset/{cid}/reject", json={"reviewer": "tester", "run_id": "test_run_id"})
    assert rrej.status_code == 200 and rrej.json()["message"] == "rejected"


def test_conflict_crud(client, project_ids):
    r1 = client.post(
        "/memory/conflict",
        json={
            "project_id": project_ids["uuid"],
            "conflict_type": "entity_name_mismatch",
            "entity_id": "char_1",
            "old_claim": {"name": "A"},
            "new_claim": {"name": "B"},
            "run_id": "test_run_id",
        },
    )
    assert r1.status_code == 200
    fid = r1.json()["conflict_id"]
    rlist = client.get("/memory/conflicts", params={"project_id": project_ids["uuid"], "run_id": "test_run_id"})
    assert rlist.status_code == 200
    assert any(i["conflict_id"] == fid for i in rlist.json()["items"])
    rres = client.post(f"/memory/conflict/{fid}/resolve", json={"resolved_by": "tester", "status": "resolved", "run_id": "test_run_id"})
    assert rres.status_code == 200 and rres.json()["message"] == "resolved"


def test_evidence_ingest(client, project_ids):
    r = client.post(
        "/memory/evidence/ingest",
        json={"project_id": project_ids["uuid"], "text": "他说：你好世界。", "max_quote_chars": 200, "tags": ["t1"], "run_id": "test_run_id"},
    )
    assert r.status_code == 200
    data = r.json()
    assert data["count"] >= 1
    assert isinstance(data["evidence_ids"], list)
    assert len(data["evidence_ids"]) == data["count"]


def test_changeset_from_payload_defaults(client, project_ids):
    r = client.post(
        "/memory/changeset/from-payload",
        json={
            "project_id": project_ids["uuid"],
            "payload": {"proposed": {"entities": []}},
            "default_materialize_static_bible": True,
            "run_id": "test_run_id",
        },
    )
    assert r.status_code == 200
    assert isinstance(r.json()["changeset_id"], str) and r.json()["changeset_id"]


def test_extract_changeset_mocked(client, project_ids, tmp_path, monkeypatch):
    settings_path = tmp_path / "ai_settings.json"
    base_dir = tmp_path / "data"
    monkeypatch.setenv("AICOMIC_DATA_DIR", str(base_dir))
    base_dir.mkdir(parents=True, exist_ok=True)
    with open(settings_path, "w", encoding="utf-8") as f:
        f.write(json.dumps({"api_key": "k", "base_url": "http://localhost", "model": "m", "temperature": 0.1, "timeout_seconds": 5}))
    from app.services.app_paths import ai_settings_path as _p
    assert _p() == str(settings_path)
    import app.routers.memory as mem_router
    async def _fake_extract(**kwargs):
        return {"schema_version": "changeset.v0", "proposed": {"entities": []}}, {"trace": "ok"}
    def _fake_resolve(store, project_id, payload):
        return payload, {"resolver": "ok"}
    mem_router.extract_changeset_v0_with_llm_with_trace = _fake_extract
    mem_router.resolve_changeset_entities_with_trace = _fake_resolve
    r = client.post(
        "/memory/extract/changeset",
        json={"project_id": project_ids["uuid"], "text": "他走进房间。", "create_changeset": True, "debug": True, "run_id": "test_run_id"},
    )
    assert r.status_code == 200
    body = r.json()
    assert body["payload"]["schema_version"] == "changeset.v0"
    assert isinstance(body.get("evidence_ids"), list)
