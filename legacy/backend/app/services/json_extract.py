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
    except json.JSONDecodeError as e:
        # 如果直接解析失败，尝试提取第一个完整的 JSON 对象
        pass
    except Exception:
        pass
    
    # 尝试提取第一个完整的 JSON 对象（而不是最后一个 }）
    # 使用栈来匹配括号，找到第一个完整的 JSON 对象
    def find_first_json_object(text: str) -> tuple[int, int] | None:
        """找到第一个完整的 JSON 对象的位置"""
        start = text.find("{")
        if start == -1:
            return None
        depth = 0
        in_string = False
        escape_next = False
        
        for i in range(start, len(text)):
            char = text[i]
            
            if escape_next:
                escape_next = False
                continue
            
            if char == "\\":
                escape_next = True
                continue
            
            if char == '"' and not escape_next:
                in_string = not in_string
                continue
            
            if in_string:
                continue
            
            if char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
                if depth == 0:
                    return (start, i)
        return None
    
    # 尝试提取第一个完整的对象
    obj_pos = find_first_json_object(s)
    if obj_pos:
        start, end = obj_pos
        try:
            return json.loads(s[start : end + 1])
        except json.JSONDecodeError:
            pass
    
    # 尝试提取数组
    l = s.find("[")
    if l != -1:
        # 找到第一个完整的数组
        depth = 0
        in_string = False
        escape_next = False
        for i in range(l, len(s)):
            char = s[i]
            if escape_next:
                escape_next = False
                continue
            if char == "\\":
                escape_next = True
                continue
            if char == '"' and not escape_next:
                in_string = not in_string
                continue
            if in_string:
                continue
            if char == "[":
                depth += 1
            elif char == "]":
                depth -= 1
                if depth == 0:
                    try:
                        return json.loads(s[l : i + 1])
                    except json.JSONDecodeError:
                        break
    
    # 回退到旧逻辑：尝试提取对象（使用第一个 { 到最后一个 }）
    l = s.find("{")
    r = s.rfind("}")
    if l != -1 and r != -1 and r > l:
        try:
            return json.loads(s[l : r + 1])
        except json.JSONDecodeError as e:
            # 如果还是失败，尝试只解析第一个对象（递归调用）
            # 但先尝试找到第一个完整的对象
            obj_pos = find_first_json_object(s)
            if obj_pos:
                start, end = obj_pos
                try:
                    return json.loads(s[start : end + 1])
                except json.JSONDecodeError:
                    raise ValueError(f"failed to parse json: {str(e)}")
            raise ValueError(f"failed to parse json: {str(e)}")
    
    raise ValueError("failed to parse json")


