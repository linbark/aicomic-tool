"""
JSON Patch 引擎单元测试（RFC6902）
"""
import sys
from pathlib import Path

# 添加项目根目录到路径
project_root = Path(__file__).parent.parent.parent.parent
sys.path.insert(0, str(project_root))

from backend.app.utils.json_patch import apply_json_patch, JSONPatchError


def test_replace_dict():
    """测试 replace 操作（dict）"""
    obj = {"a": 1, "b": {"c": 2}}
    patches = [{"op": "replace", "path": "/a", "value": 10}]
    result = apply_json_patch(obj, patches)
    assert result["a"] == 10
    print("✅ test_replace_dict passed")


def test_replace_nested():
    """测试 replace 操作（嵌套）"""
    obj = {"a": {"b": {"c": 1}}}
    patches = [{"op": "replace", "path": "/a/b/c", "value": 2}]
    result = apply_json_patch(obj, patches)
    assert result["a"]["b"]["c"] == 2
    print("✅ test_replace_nested passed")


def test_add_dict():
    """测试 add 操作（dict）"""
    obj = {"a": 1}
    patches = [{"op": "add", "path": "/b", "value": 2}]
    result = apply_json_patch(obj, patches)
    assert result["b"] == 2
    print("✅ test_add_dict passed")


def test_add_list():
    """测试 add 操作（list）"""
    obj = [1, 2, 3]
    patches = [{"op": "add", "path": "/1", "value": 99}]
    result = apply_json_patch(obj, patches)
    assert result == [1, 99, 2, 3]
    print("✅ test_add_list passed")


def test_remove_dict():
    """测试 remove 操作（dict）"""
    obj = {"a": 1, "b": 2}
    patches = [{"op": "remove", "path": "/a"}]
    result = apply_json_patch(obj, patches)
    assert "a" not in result
    assert result["b"] == 2
    print("✅ test_remove_dict passed")


def test_list_index():
    """测试 list index 路径"""
    obj = {"items": [{"id": 1}, {"id": 2}]}
    patches = [{"op": "replace", "path": "/items/0/id", "value": 10}]
    result = apply_json_patch(obj, patches)
    assert result["items"][0]["id"] == 10
    print("✅ test_list_index passed")


def test_complex_path():
    """测试复杂路径（list + dict 混合）"""
    obj = {
        "prompt_packs": [
            {"dialect": "midjourney_v6", "items": [{"panel_id": "p1", "prompt": "old"}]}
        ]
    }
    patches = [{"op": "replace", "path": "/prompt_packs/0/items/0/prompt", "value": "new"}]
    result = apply_json_patch(obj, patches)
    assert result["prompt_packs"][0]["items"][0]["prompt"] == "new"
    print("✅ test_complex_path passed")


def test_error_handling():
    """测试错误处理"""
    obj = {"a": 1}
    
    # 路径不存在
    try:
        patches = [{"op": "replace", "path": "/nonexistent", "value": 2}]
        apply_json_patch(obj, patches)
        assert False, "Should raise error"
    except (KeyError, JSONPatchError):
        print("✅ test_error_handling passed")


if __name__ == "__main__":
    print("JSON Patch Engine Unit Tests")
    print("=" * 60)
    
    try:
        test_replace_dict()
        test_replace_nested()
        test_add_dict()
        test_add_list()
        test_remove_dict()
        test_list_index()
        test_complex_path()
        test_error_handling()
        
        print("\n" + "=" * 60)
        print("All JSON Patch tests passed!")
    except Exception as e:
        print(f"\n❌ Test failed: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)

