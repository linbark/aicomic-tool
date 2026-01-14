import os
import json
import tempfile
from pathlib import Path
import sys

import pytest
from fastapi.testclient import TestClient


_TEST_BASE_DIR = Path(tempfile.mkdtemp(prefix="aicomic_test_")).resolve()
_TEST_DATA_DIR = (_TEST_BASE_DIR / "data").resolve()
_TEST_DB_PATH = (_TEST_BASE_DIR / "test.db").resolve()

os.environ["AICOMIC_DATA_DIR"] = str(_TEST_DATA_DIR)
os.environ["AICOMIC_DB_PATH"] = str(_TEST_DB_PATH)
_TEST_DATA_DIR.mkdir(parents=True, exist_ok=True)

# 确保可以直接 import app.*
_BACKEND_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(_BACKEND_DIR))

from app.main import app
from app.database import SessionLocal, Base, engine
from app.models import Project

Base.metadata.create_all(bind=engine)


@pytest.fixture(scope="session")
def client():
    return TestClient(app)


@pytest.fixture(scope="session")
def project_ids():
    db = SessionLocal()
    try:
        p = Project(name="Test Project", description="for memory api tests")
        db.add(p)
        db.commit()
        db.refresh(p)
        return {"pk": int(p.id), "uuid": str(p.uuid)}
    finally:
        db.close()

@pytest.fixture(scope="session")
def test_paths():
    return {"base_dir": str(_TEST_BASE_DIR), "data_dir": str(_TEST_DATA_DIR), "db_path": str(_TEST_DB_PATH)}


class FakeMemoryStore:
    def __init__(self):
        self.time_constraints = {}
        self.time_blocks = {}
        self.changesets = {}
        self.conflicts = {}
        self.evidences = {}

    def upsert_time_constraint(self, constraint):
        self.time_constraints[constraint.id] = constraint
        return constraint.id

    def list_time_constraints(self, project_id, status=None, limit=200):
        items = []
        for c in self.time_constraints.values():
            if int(c.project_id) != int(project_id):
                continue
            if status and getattr(c.status, "value", c.status) != status:
                continue
            items.append(
                {
                    "id": c.id,
                    "relation": c.relation.value,
                    "from_event_id": c.from_event_id,
                    "to_event_id": c.to_event_id,
                    "anchor_id": c.anchor_id,
                    "interval": c.interval,
                    "evidence_ids": c.evidence_ids,
                    "status": c.status.value,
                    "confidence": c.confidence,
                    "source_kind": c.source_kind.value,
                    "created_at_ms": 0,
                    "updated_at_ms": 0,
                }
            )
            if len(items) >= limit:
                break
        return items

    def upsert_time_block(self, block):
        self.time_blocks[block.id] = block
        return block.id

    def list_time_blocks(self, project_id, status=None, limit=200):
        items = []
        for b in self.time_blocks.values():
            if int(b.project_id) != int(project_id):
                continue
            if status and getattr(b.status, "value", b.status) != status:
                continue
            items.append(
                {
                    "id": b.id,
                    "name": b.name,
                    "parent_block_id": b.parent_block_id,
                    "anchor_id": b.anchor_id,
                    "constraint_ids": b.constraint_ids,
                    "event_ids": b.event_ids,
                    "status": b.status.value,
                    "confidence": b.confidence,
                    "source_kind": b.source_kind.value,
                    "created_at_ms": 0,
                    "updated_at_ms": 0,
                }
            )
            if len(items) >= limit:
                break
        return items

    def create_changeset(self, project_id, payload, episode_id=None, review_status="pending_review"):
        cid = f"cs_{len(self.changesets)+1}"
        self.changesets[cid] = {
            "changeset_id": cid,
            "project_id": int(project_id),
            "episode_id": episode_id,
            "payload": payload,
            "review_status": review_status,
            "review_log": [],
        }
        return cid

    def list_changesets(self, project_id, review_status=None, limit=50):
        out = []
        for cs in self.changesets.values():
            if int(cs["project_id"]) != int(project_id):
                continue
            if review_status and cs["review_status"] != review_status:
                continue
            out.append(
                {
                    "changeset_id": cs["changeset_id"],
                    "episode_id": cs["episode_id"],
                    "review_status": cs["review_status"],
                    "created_at_ms": 0,
                    "updated_at_ms": 0,
                }
            )
            if len(out) >= limit:
                break
        return out

    def get_changeset(self, changeset_id):
        cs = self.changesets.get(changeset_id)
        if not cs:
            return None
        return {
            "changeset_id": cs["changeset_id"],
            "project_id": cs["project_id"],
            "episode_id": cs["episode_id"],
            "payload": cs["payload"],
            "review_status": cs["review_status"],
            "review_log": cs["review_log"],
            "created_at_ms": 0,
            "updated_at_ms": 0,
        }

    def apply_changeset(self, changeset_id, reviewer="human", note=None):
        cs = self.changesets[changeset_id]
        cs["review_status"] = "approved"
        cs["review_log"].append({"action": "approved", "reviewer": reviewer, "note": note})

    def reject_changeset(self, changeset_id, reviewer="human", note=None):
        cs = self.changesets[changeset_id]
        cs["review_status"] = "rejected"
        cs["review_log"].append({"action": "rejected", "reviewer": reviewer, "note": note})

    def create_conflict(self, project_id, conflict_type, changeset_id=None, entity_id=None, old_claim=None, new_claim=None, suggested_actions=None, status="open"):
        fid = f"cf_{len(self.conflicts)+1}"
        self.conflicts[fid] = {
            "conflict_id": fid,
            "project_id": int(project_id),
            "changeset_id": changeset_id,
            "conflict_type": conflict_type,
            "entity_id": entity_id,
            "old_claim": old_claim,
            "new_claim": new_claim,
            "suggested_actions": suggested_actions,
            "status": status,
            "resolved_by": None,
            "resolution_note": None,
        }
        return fid

    def list_conflicts(self, project_id, status="open", limit=100):
        out = []
        for cf in self.conflicts.values():
            if int(cf["project_id"]) != int(project_id):
                continue
            if status and cf["status"] != status:
                continue
            out.append(
                {
                    "conflict_id": cf["conflict_id"],
                    "changeset_id": cf["changeset_id"],
                    "conflict_type": cf["conflict_type"],
                    "entity_id": cf["entity_id"],
                    "old_claim_json": json.dumps(cf["old_claim"], ensure_ascii=False) if cf["old_claim"] else None,
                    "new_claim_json": json.dumps(cf["new_claim"], ensure_ascii=False) if cf["new_claim"] else None,
                    "suggested_actions_json": json.dumps(cf["suggested_actions"], ensure_ascii=False) if cf["suggested_actions"] else None,
                    "status": cf["status"],
                    "resolved_by": cf["resolved_by"],
                    "resolution_note": cf["resolution_note"],
                    "created_at_ms": 0,
                    "updated_at_ms": 0,
                }
            )
            if len(out) >= limit:
                break
        return out

    def resolve_conflict(self, conflict_id, resolved_by="human", resolution_note=None, status="resolved"):
        cf = self.conflicts[conflict_id]
        cf["status"] = status
        cf["resolved_by"] = resolved_by
        cf["resolution_note"] = resolution_note

    def upsert_evidence(self, evidence):
        self.evidences[evidence.evidence_id] = evidence
        return evidence.evidence_id

    def list_evidences_by_ids(self, project_id, evidence_ids):
        rows = []
        for eid in evidence_ids:
            ev = self.evidences.get(eid)
            if not ev or int(ev.project_id) != int(project_id):
                continue
            rows.append(
                {
                    "evidence_id": ev.evidence_id,
                    "project_id": ev.project_id,
                    "episode_id": ev.episode_id,
                    "scene_id": ev.scene_id,
                    "quote": ev.quote,
                    "speaker": ev.speaker,
                    "tags": ev.tags,
                }
            )
        return rows


@pytest.fixture(scope="session", autouse=True)
def patch_memory_store():
    fake = FakeMemoryStore()
    import app.routers.memory as mem_router
    orig = mem_router.get_memory_store
    mem_router.get_memory_store = lambda: fake
    yield
    mem_router.get_memory_store = orig
