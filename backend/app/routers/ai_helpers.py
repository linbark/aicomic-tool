import json
import time
import uuid
import logging
from typing import Any, Dict, Optional, Tuple

from pydantic import BaseModel, Field

# 引入依赖
from ..services.json_extract import extract_json_any
from ..services.context_store import ContextStore

import os
log_dir = os.path.join(os.path.dirname(__file__), "..", "logs")
os.makedirs(log_dir, exist_ok=True)
log_file = os.path.join(log_dir, "ai_helpers.log")

# 初始化 logger
logger = logging.getLogger(__name__)
logger.setLevel(logging.DEBUG)
handler = logging.FileHandler(log_file)
handler.setLevel(logging.DEBUG)
formatter = logging.Formatter('%(asctime)s - %(name)s - %(levelname)s - %(message)s')
handler.setFormatter(formatter)
logger.addHandler(handler)

# 实例化一个 store 对象供 _log_ui 使用
# ContextStore 主要是文件操作，开销很小，可以在这里独立实例化
_store = ContextStore()

def _normalize_level(level: Any) -> str:
    s = str(level or "INFO").strip().upper()
    if s in ("WARNING",):
        s = "WARN"
    if s not in ("DEBUG", "INFO", "WARN", "ERROR"):
        s = "INFO"
    return s

class LogPayload(BaseModel):
    project_id: int
    run_id: str
    stage: str = "ui.log"
    summary: str = ""
    data: Dict[str, Any] = Field(default_factory=dict)
    ts_ms: Optional[int] = None
    at_ms: Optional[int] = None
    level: Optional[str] = None
    text: Optional[str] = None
    timestamp: Optional[float] = None

def _extract_target_and_message(args: Tuple[Any, ...]) -> Tuple[Optional[int], Optional[str], Any, Optional[str]]:
    if len(args) == 4:
        project_id, run_id, message, level = args
        return int(project_id), str(run_id), message, str(level)
    if len(args) == 3:
        project_id, run_id, message = args
        return int(project_id), str(run_id), message, None
    if len(args) == 2:
        message, level = args
        if isinstance(message, BaseModel):
            message = message.model_dump(exclude_none=True)
        if isinstance(message, dict) and "project_id" in message and "run_id" in message:
            return int(message["project_id"]), str(message["run_id"]), message, str(level)
        return None, None, message, str(level)
    if len(args) == 1:
        (message,) = args
        if isinstance(message, BaseModel):
            message = message.model_dump(exclude_none=True)
        if isinstance(message, dict) and "project_id" in message and "run_id" in message:
            return int(message["project_id"]), str(message["run_id"]), message, None
        return None, None, message, None
    raise TypeError("log_ui expects (project_id, run_id, message[, level]) or (payload[, level])")

def _coerce_log_payload(message: Any, *, ts_ms: int, level: str) -> dict:
    if isinstance(message, dict):
        payload = dict(message)
        stage = payload.get("stage")
        summary = payload.get("summary")
        if stage is None:
            payload["stage"] = "ui.log"
        if summary is None:
            payload["summary"] = str(payload.get("text") or payload.get("message") or "")
    else:
        payload = {"stage": "ui.log", "summary": str(message)}
    payload["ts_ms"] = int(ts_ms)
    payload["level"] = level
    payload["text"] = str(payload.get("summary") or payload.get("text") or "")
    payload["timestamp"] = float(ts_ms) / 1000.0
    return payload

def safe_parse_json(text: str) -> Any:
    """
    尝试从 LLM 输出中提取并解析 JSON。
    针对 'Extra data' 错误进行专门处理（截断到最后一个 }）。
    """
    text = (text or "").strip()
    # 1. 尝试直接解析
    try:
        return extract_json_any(text)
    except Exception:
        pass

    # 2. 如果失败，尝试手动截取最外层的 {...}
    # 往往是因为 LLM 在 json 后又输出了 "Note: ..."
    start = text.find("{")
    end = text.rfind("}")
    if start != -1 and end != -1 and end > start:
        candidate = text[start : end + 1]
        try:
            return json.loads(candidate)
        except Exception:
            pass
            
    # 3. 还是失败，抛出原始文本以便调试（或者返回 None 由调用方处理）
    raise ValueError(f"无法解析 JSON: {text[:200]}...")


def log_ui(*args: Any):
    """
    生成一个 log.xxxx.json 文件，前端轮询时会读取并显示在 Debug 窗口中。
    使用微秒级时间戳确保排序。
    """
    project_id, run_id, message, level = _extract_target_and_message(args)
    if project_id is None or run_id is None:
        logger.error(f"[UI_LOG] Missing project_id/run_id, drop log: {message}")
        return

    if isinstance(message, dict) and level is None:
        inferred = message.get("level")
        if inferred is not None:
            level = str(inferred)

    ts = int(time.time() * 1000000)
    ts_ms = int(ts / 1000)
    salt = uuid.uuid4().hex[:4]
    stage_name = f"log.{ts}.{salt}"
    
    lvl = _normalize_level(level)
    data = _coerce_log_payload(message, ts_ms=ts_ms, level=lvl)
    
    # 记录到后台日志（方便排查）
    logger.info(f"[UI_LOG] Run {run_id}: {data.get('text')}")
    
    # 写入存储
    try:
        _store.snapshot_stage(project_id=project_id, run_id=run_id, stage_name=stage_name, data=data)
    except Exception as e:
        logger.error(f"Failed to write UI log: {e}")
