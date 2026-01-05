"""
JSON Patch 工具：RFC6902 规范实现（支持 JSON Pointer）
支持：add, replace, remove
"""
from typing import Any, Dict, List
import re


class JSONPatchError(Exception):
    """JSON Patch 操作错误"""
    pass


def apply_json_patch(obj: Any, patch_ops: List[Dict[str, Any]]) -> Any:
    """
    对对象应用 JSON Patch 操作（RFC6902）
    支持：add, replace, remove
    """
    result = obj
    if isinstance(obj, dict):
        result = obj.copy()
    elif isinstance(obj, list):
        result = obj.copy()
    else:
        # 对于非 dict/list，尝试转换为 dict（如果是 BaseModel，用 model_dump）
        if hasattr(obj, "model_dump"):
            result = obj.model_dump()
        else:
            result = obj

    for op in patch_ops:
        op_type = op.get("op")
        path = op.get("path", "")
        value = op.get("value")
        from_path = op.get("from")  # 用于 move/copy

        if not path.startswith("/"):
            raise JSONPatchError(f"Invalid JSON Pointer path: {path} (must start with /)")

        try:
            if op_type == "add":
                _apply_add(result, path, value)
            elif op_type == "replace":
                _apply_replace(result, path, value)
            elif op_type == "remove":
                _apply_remove(result, path)
            elif op_type == "move":
                if not from_path:
                    raise JSONPatchError("move operation requires 'from' field")
                _apply_move(result, from_path, path)
            elif op_type == "copy":
                if not from_path:
                    raise JSONPatchError("copy operation requires 'from' field")
                _apply_copy(result, from_path, path)
            else:
                raise JSONPatchError(f"Unsupported operation: {op_type}")
        except (KeyError, IndexError, TypeError) as e:
            raise JSONPatchError(f"Failed to apply {op_type} at {path}: {e}")

    return result


def _parse_json_pointer(path: str) -> List[str]:
    """
    解析 JSON Pointer (RFC6901)
    例如：/a/b/0/c -> ['a', 'b', '0', 'c']
    支持转义：~0 -> ~, ~1 -> /
    """
    if not path.startswith("/"):
        raise JSONPatchError(f"JSON Pointer must start with /: {path}")
    
    if path == "/":
        return []
    
    parts = path[1:].split("/")
    # 处理转义
    decoded = []
    for part in parts:
        part = part.replace("~1", "/").replace("~0", "~")
        decoded.append(part)
    return decoded


def _resolve_pointer(obj: Any, path_parts: List[str], create_missing: bool = False) -> tuple[Any, str]:
    """
    解析 JSON Pointer 路径，返回 (parent_obj, final_key)
    create_missing: 如果为 True，路径不存在时创建中间对象
    """
    current = obj
    if not path_parts:
        return current, ""
    
    for i, part in enumerate(path_parts[:-1]):
        if isinstance(current, dict):
            if part not in current:
                if create_missing:
                    # 判断下一个部分是数字还是字符串，决定创建 list 还是 dict
                    next_part = path_parts[i + 1]
                    if next_part.isdigit():
                        current[part] = []
                    else:
                        current[part] = {}
                else:
                    raise KeyError(f"Path not found: {'/'.join(path_parts[:i+1])}")
            current = current[part]
        elif isinstance(current, list):
            idx = int(part)
            if idx < 0 or idx >= len(current):
                raise IndexError(f"List index out of range: {idx}")
            current = current[idx]
        else:
            raise TypeError(f"Cannot traverse into {type(current).__name__}")
    
    return current, path_parts[-1]


def _apply_add(obj: Any, path: str, value: Any) -> None:
    """应用 add 操作"""
    path_parts = _parse_json_pointer(path)
    if not path_parts:
        # 根路径，直接替换整个对象
        raise JSONPatchError("Cannot add at root path")
    
    parent, key = _resolve_pointer(obj, path_parts[:-1], create_missing=True)
    final_key = path_parts[-1]
    
    if isinstance(parent, dict):
        parent[final_key] = value
    elif isinstance(parent, list):
        idx = int(final_key) if final_key.isdigit() else len(parent)
        if idx < 0 or idx > len(parent):
            raise IndexError(f"List index out of range: {idx}")
        parent.insert(idx, value)
    else:
        raise TypeError(f"Cannot add to {type(parent).__name__}")


def _apply_replace(obj: Any, path: str, value: Any) -> None:
    """应用 replace 操作"""
    path_parts = _parse_json_pointer(path)
    if not path_parts:
        raise JSONPatchError("Cannot replace at root path")
    
    parent, key = _resolve_pointer(obj, path_parts[:-1], create_missing=False)
    final_key = path_parts[-1]
    
    if isinstance(parent, dict):
        if final_key not in parent:
            raise KeyError(f"Key not found: {final_key}")
        parent[final_key] = value
    elif isinstance(parent, list):
        idx = int(final_key)
        if idx < 0 or idx >= len(parent):
            raise IndexError(f"List index out of range: {idx}")
        parent[idx] = value
    else:
        raise TypeError(f"Cannot replace in {type(parent).__name__}")


def _apply_remove(obj: Any, path: str) -> None:
    """应用 remove 操作"""
    path_parts = _parse_json_pointer(path)
    if not path_parts:
        raise JSONPatchError("Cannot remove at root path")
    
    parent, key = _resolve_pointer(obj, path_parts[:-1], create_missing=False)
    final_key = path_parts[-1]
    
    if isinstance(parent, dict):
        if final_key not in parent:
            raise KeyError(f"Key not found: {final_key}")
        del parent[final_key]
    elif isinstance(parent, list):
        idx = int(final_key)
        if idx < 0 or idx >= len(parent):
            raise IndexError(f"List index out of range: {idx}")
        parent.pop(idx)
    else:
        raise TypeError(f"Cannot remove from {type(parent).__name__}")


def _apply_move(obj: Any, from_path: str, to_path: str) -> None:
    """应用 move 操作（先 copy 再 remove）"""
    # 先获取值
    from_parts = _parse_json_pointer(from_path)
    if not from_parts:
        raise JSONPatchError("Cannot move from root path")
    
    from_parent, _ = _resolve_pointer(obj, from_parts[:-1], create_missing=False)
    from_key = from_parts[-1]
    
    if isinstance(from_parent, dict):
        if from_key not in from_parent:
            raise KeyError(f"Key not found: {from_key}")
        value = from_parent[from_key]
    elif isinstance(from_parent, list):
        idx = int(from_key)
        if idx < 0 or idx >= len(from_parent):
            raise IndexError(f"List index out of range: {idx}")
        value = from_parent[idx]
    else:
        raise TypeError(f"Cannot move from {type(from_parent).__name__}")
    
    # 添加到目标位置
    _apply_add(obj, to_path, value)
    # 从源位置删除
    _apply_remove(obj, from_path)


def _apply_copy(obj: Any, from_path: str, to_path: str) -> None:
    """应用 copy 操作"""
    # 先获取值
    from_parts = _parse_json_pointer(from_path)
    if not from_parts:
        raise JSONPatchError("Cannot copy from root path")
    
    from_parent, _ = _resolve_pointer(obj, from_parts[:-1], create_missing=False)
    from_key = from_parts[-1]
    
    if isinstance(from_parent, dict):
        if from_key not in from_parent:
            raise KeyError(f"Key not found: {from_key}")
        value = from_parent[from_key]
    elif isinstance(from_parent, list):
        idx = int(from_key)
        if idx < 0 or idx >= len(from_parent):
            raise IndexError(f"List index out of range: {idx}")
        value = from_parent[idx]
    else:
        raise TypeError(f"Cannot copy from {type(from_parent).__name__}")
    
    # 深拷贝值
    import copy
    value_copy = copy.deepcopy(value)
    
    # 添加到目标位置
    _apply_add(obj, to_path, value_copy)
