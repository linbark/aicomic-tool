"""
AI 路由聚合入口（/ai）。

说明：
- 该文件仅负责聚合各功能子模块的 APIRouter，避免单文件过大。
- 具体实现拆分在同目录下的 `ai_*.py`。
"""

from fastapi import APIRouter

from . import (
    ai_basic,
    ai_chat,
    ai_context,
    ai_episode_execute,
    ai_prompts,
    ai_runs_files,
    ai_visual_dna,
    ai_workflows,
    ai_writing,
)

# 创建主路由，统一添加 /ai 前缀
router = APIRouter(prefix="/ai", tags=["AI (DeepSeek)"])

# 1. 基础能力与设置 (/settings, /test, /prompts)
router.include_router(ai_basic.router)
router.include_router(ai_prompts.router)

# 2. 写作与编排 (/chat, /outline, /script, /workflows)
router.include_router(ai_writing.router)
router.include_router(ai_chat.router)
router.include_router(ai_episode_execute.router)
router.include_router(ai_workflows.router)

# 3. 上下文与产物 (/context, /runs-files, /visual-dna)
router.include_router(ai_context.router)
router.include_router(ai_runs_files.router)
router.include_router(ai_visual_dna.router)

