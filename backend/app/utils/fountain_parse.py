"""
Fountain 脚本轻量解析器：识别场景、动作块、对话
P2 阶段：最小实现，可预测的解析结果
"""
from typing import List, Dict, Any
import re


class FountainElement:
    """Fountain 元素基类"""
    pass


class FountainScene(FountainElement):
    """场景"""
    def __init__(self, title: str, line_number: int):
        self.title = title
        self.line_number = line_number
        self.action_lines: List[str] = []
        self.dialogues: List[Dict[str, str]] = []  # [{"speaker": "...", "text": "..."}]


class FountainAction(FountainElement):
    """动作块"""
    def __init__(self, lines: List[str], line_start: int):
        self.lines = lines
        self.line_start = line_start


class FountainDialogue(FountainElement):
    """对话"""
    def __init__(self, speaker: str, text: str, line_number: int):
        self.speaker = speaker
        self.text = text
        self.line_number = line_number


def fountain_parse(text: str) -> Dict[str, Any]:
    """
    解析 Fountain 脚本
    返回：{
        "scenes": [FountainScene],
        "actions": [FountainAction],
        "dialogues": [FountainDialogue],
    }
    """
    lines = text.split("\n")
    scenes: List[FountainScene] = []
    actions: List[FountainAction] = []
    dialogues: List[FountainDialogue] = []
    
    current_scene: FountainScene | None = None
    current_action_lines: List[str] = []
    current_action_start = -1
    
    scene_title_pattern = re.compile(r"^(INT\.|EXT\.)\s+[A-Z][A-Z0-9\s]+$")
    
    for i, line in enumerate(lines):
        stripped = line.strip()
        
        # 场景标题
        if stripped.startswith("INT.") or stripped.startswith("EXT."):
            # 结束当前动作块
            if current_action_lines and current_action_start >= 0:
                actions.append(FountainAction(current_action_lines, current_action_start))
                current_action_lines = []
                current_action_start = -1
            
            # 创建新场景
            current_scene = FountainScene(stripped, i)
            scenes.append(current_scene)
            continue
        
        # 空行：结束当前动作块
        if not stripped:
            if current_action_lines and current_action_start >= 0:
                actions.append(FountainAction(current_action_lines, current_action_start))
                if current_scene:
                    current_scene.action_lines.extend(current_action_lines)
                current_action_lines = []
                current_action_start = -1
            continue
        
        # 对话行（包含冒号，且不是场景标题）
        if ":" in stripped and not stripped.startswith(("INT.", "EXT.", ".")):
            # 结束当前动作块
            if current_action_lines and current_action_start >= 0:
                actions.append(FountainAction(current_action_lines, current_action_start))
                if current_scene:
                    current_scene.action_lines.extend(current_action_lines)
                current_action_lines = []
                current_action_start = -1
            
            # 解析对话
            parts = stripped.split(":", 1)
            if len(parts) == 2:
                speaker = parts[0].strip()
                dialogue_text = parts[1].strip()
                dialogue = FountainDialogue(speaker, dialogue_text, i)
                dialogues.append(dialogue)
                if current_scene:
                    current_scene.dialogues.append({"speaker": speaker, "text": dialogue_text})
            continue
        
        # 动作行（其他所有行）
        if current_action_start < 0:
            current_action_start = i
        current_action_lines.append(stripped)
    
    # 处理最后一个动作块
    if current_action_lines and current_action_start >= 0:
        actions.append(FountainAction(current_action_lines, current_action_start))
        if current_scene:
            current_scene.action_lines.extend(current_action_lines)
    
    return {
        "scenes": scenes,
        "actions": actions,
        "dialogues": dialogues,
    }

