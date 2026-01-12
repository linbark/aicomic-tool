import json
import time
import uuid
import logging
from typing import Any

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


def log_ui(project_id: int, run_id: str, message: str, level: str = "INFO"):
    """
    生成一个 log.xxxx.json 文件，前端轮询时会读取并显示在 Debug 窗口中。
    使用微秒级时间戳确保排序。
    """
    # 构造文件名: log.{timestamp_us}.{random}.json
    ts = int(time.time() * 1000000)
    salt = uuid.uuid4().hex[:4]
    stage_name = f"log.{ts}.{salt}"
    
    data = {
        "text": str(message),
        "level": level,
        "timestamp": time.time()
    }
    
    # 记录到后台日志（方便排查）
    logger.info(f"[UI_LOG] Run {run_id}: {message}")
    
    # 写入存储
    try:
        _store.snapshot_stage(project_id=project_id, run_id=run_id, stage_name=stage_name, data=data)
    except Exception as e:
        logger.error(f"Failed to write UI log: {e}")