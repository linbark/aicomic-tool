import json
import re
from typing import Any


def extract_json_any(text: str, expected_hint: str | None = None) -> Any:
    """
    从 LLM 返回中提取 JSON（容错）：去 code fence、截取首个 [...] 或 {...}。
    与旧实现保持一致，供 workflows / 原子接口复用。
    expected_hint: 仅用于兼容旧调用方，不参与解析逻辑。
    """
    if text is None:
        raise ValueError("empty response")
    s = str(text).strip()
    # 去 code fence
    s = re.sub(r"^```(?:json)?\s*", "", s, flags=re.IGNORECASE)
    s = re.sub(r"\s*```$", "", s)
    # 先直接 parse
    try:
        return json.loads(s)
    except Exception:
        pass
    # 尝试提取数组
    l = s.find("[")
    r = s.rfind("]")
    if l != -1 and r != -1 and r > l:
        return json.loads(s[l : r + 1])
    # 尝试提取对象
    l = s.find("{")
    r = s.rfind("}")
    if l != -1 and r != -1 and r > l:
        return json.loads(s[l : r + 1])
    raise ValueError("failed to parse json")


