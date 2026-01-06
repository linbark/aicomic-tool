from __future__ import annotations

import json
import os
import time
import uuid
from dataclasses import dataclass
from typing import Any, Dict, List, Optional

from fastapi import HTTPException

from .app_paths import project_context_dir, project_runs_dir


def new_run_id() -> str:
    return uuid.uuid4().hex


def _read_json(path: str) -> Optional[Dict[str, Any]]:
    if not os.path.exists(path):
        return None
    try:
        with open(path, "r", encoding="utf-8") as f:
            data = json.load(f)
        if isinstance(data, dict):
            return data
        # 对于 context 文件，我们要求是 object（dict）
        return {"_value": data}
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to read json: {e}")


def _write_json(path: str, data: Any) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    try:
        with open(path, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to write json: {e}")


@dataclass(frozen=True)
class ContextFileMeta:
    path: str
    version: str
    updated_at_ms: int


class ContextStore:
    """
    file-first 的一致性上下文存储。

    路径策略：
    - {app_data_dir}/projects/{project_id}/context/...
    - {app_data_dir}/projects/{project_id}/runs/{run_id}/...
    """

    def series_bible_path(self, project_id: int, version: str = "v1") -> str:
        return os.path.join(project_context_dir(project_id), f"series_bible.{version}.json")

    def visual_dna_path(self, project_id: int, item_id: int, version: str = "v1") -> str:
        return os.path.join(project_context_dir(project_id), f"visual_dna.asset_item_{int(item_id)}.{version}.json")

    def get_series_bible(self, project_id: int, version: str = "v1") -> Optional[Dict[str, Any]]:
        return _read_json(self.series_bible_path(project_id, version))

    def put_series_bible(self, project_id: int, data: Dict[str, Any], version: str = "v1") -> ContextFileMeta:
        path = self.series_bible_path(project_id, version)
        _write_json(path, data)
        return ContextFileMeta(path=path, version=version, updated_at_ms=int(time.time() * 1000))

    def get_visual_dna(self, project_id: int, item_id: int, version: str = "v1") -> Optional[Dict[str, Any]]:
        return _read_json(self.visual_dna_path(project_id, item_id, version))

    def put_visual_dna(self, project_id: int, item_id: int, data: Dict[str, Any], version: str = "v1") -> ContextFileMeta:
        path = self.visual_dna_path(project_id, item_id, version)
        _write_json(path, data)
        return ContextFileMeta(path=path, version=version, updated_at_ms=int(time.time() * 1000))

    def snapshot_run(
        self,
        *,
        project_id: int,
        run_id: str,
        request: Dict[str, Any],
        response: Dict[str, Any],
        meta: Optional[Dict[str, Any]] = None,
    ) -> str:
        run_dir = os.path.join(project_runs_dir(project_id), run_id)
        os.makedirs(run_dir, exist_ok=True)

        _write_json(os.path.join(run_dir, "request.json"), request)
        _write_json(os.path.join(run_dir, "response.json"), response)
        _write_json(
            os.path.join(run_dir, "meta.json"),
            {
                "project_id": int(project_id),
                "run_id": run_id,
                "created_at_ms": int(time.time() * 1000),
                **(meta or {}),
            },
        )
        return run_dir

    def stage_path(self, project_id: int, run_id: str, stage_name: str) -> str:
        safe = "".join([c for c in str(stage_name) if c.isalnum() or c in ("_", "-", ".")]) or "stage"
        return os.path.join(project_runs_dir(project_id), run_id, "stages", f"{safe}.json")

    def snapshot_stage(self, *, project_id: int, run_id: str, stage_name: str, data: Any) -> str:
        path = self.stage_path(project_id, run_id, stage_name)
        _write_json(path, data)
        return path

    def read_run_response(self, project_id: int, run_id: str) -> Optional[Dict[str, Any]]:
        path = os.path.join(project_runs_dir(project_id), run_id, "response.json")
        return _read_json(path)

    def list_runs(self, project_id: int) -> List[Dict[str, Any]]:
        """
        列出项目的所有 runs（按时间倒序）。
        返回每个 run 的 meta.json 内容。
        """
        runs_dir = project_runs_dir(project_id)
        if not os.path.exists(runs_dir):
            return []
        out: List[Dict[str, Any]] = []
        for run_id_dir in os.listdir(runs_dir):
            run_path = os.path.join(runs_dir, run_id_dir)
            if not os.path.isdir(run_path):
                continue
            meta_path = os.path.join(run_path, "meta.json")
            meta = _read_json(meta_path)
            if isinstance(meta, dict):
                out.append(meta)
        # 按 created_at_ms 倒序
        out.sort(key=lambda x: x.get("created_at_ms", 0), reverse=True)
        return out

    def read_run(self, project_id: int, run_id: str) -> Optional[Dict[str, Any]]:
        """
        读取完整的 run 信息（request + response + meta）。
        """
        run_dir = os.path.join(project_runs_dir(project_id), run_id)
        if not os.path.exists(run_dir):
            return None
        request = _read_json(os.path.join(run_dir, "request.json"))
        response = _read_json(os.path.join(run_dir, "response.json"))
        meta = _read_json(os.path.join(run_dir, "meta.json"))
        return {
            "request": request or {},
            "response": response or {},
            "meta": meta or {},
        }

    def list_stages(self, project_id: int, run_id: str) -> List[str]:
        """
        列出该 run 的所有 stage 文件名（不含 .json 后缀）。
        """
        stages_dir = os.path.join(project_runs_dir(project_id), run_id, "stages")
        if not os.path.exists(stages_dir):
            return []
        out: List[str] = []
        for fname in os.listdir(stages_dir):
            if fname.endswith(".json"):
                out.append(fname[:-5])  # 去掉 .json
        return sorted(out)

    def read_stage(self, project_id: int, run_id: str, stage_name: str) -> Optional[Any]:
        """
        读取指定 stage 的内容。
        """
        path = self.stage_path(project_id, run_id, stage_name)
        if not os.path.exists(path):
            return None
        try:
            with open(path, "r", encoding="utf-8") as f:
                import json as _json
                return _json.load(f)
        except Exception:
            return None


