# backend/app/main.py
import os
import signal
import sys
import faulthandler
import uuid
from fastapi import FastAPI
from fastapi.staticfiles import StaticFiles
from fastapi.middleware.cors import CORSMiddleware
from .routers import storyboard, assets, projects, events, ai, ai_runs, memory
from .database import engine, Base
from sqlalchemy import text

Base.metadata.create_all(bind=engine)

def ensure_projects_uuid_column():
    """
    轻量 SQLite 迁移（无 Alembic）：
    - 若 projects 表缺少 uuid 列，则补齐
    - 为历史项目回填 uuid（hex 字符串）
    """
    try:
        with engine.begin() as conn:
            cols = conn.execute(text("PRAGMA table_info(projects)")).fetchall()
            col_names = {row[1] for row in cols}  # (cid, name, type, notnull, dflt_value, pk)
            if "uuid" not in col_names:
                conn.execute(text("ALTER TABLE projects ADD COLUMN uuid TEXT"))
            # 为 uuid 建索引/唯一约束（SQLite 通过唯一索引实现）
            conn.execute(text("CREATE UNIQUE INDEX IF NOT EXISTS ux_projects_uuid ON projects (uuid)"))
            # 回填历史数据
            rows = conn.execute(text("SELECT id FROM projects WHERE uuid IS NULL OR uuid = ''")).fetchall()
            for (pid,) in rows:
                conn.execute(
                    text("UPDATE projects SET uuid = :u WHERE id = :id"),
                    {"u": uuid.uuid4().hex, "id": int(pid)},
                )
    except Exception as e:
        print(f"[Migration][Warning] ensure_projects_uuid_column failed: {e}")

def ensure_characters_category_column():
    """
    轻量 SQLite 迁移（无 Alembic）：
    - 若 characters 表缺少 category 列，则补齐，并将历史数据默认置为 persona
    """
    try:
        with engine.begin() as conn:
            cols = conn.execute(text("PRAGMA table_info(characters)")).fetchall()
            col_names = {row[1] for row in cols}  # (cid, name, type, notnull, dflt_value, pk)
            if "category" not in col_names:
                conn.execute(text("ALTER TABLE characters ADD COLUMN category TEXT NOT NULL DEFAULT 'persona_visual'"))
                # 保险起见：把历史 NULL 补齐（正常情况下 NOT NULL + DEFAULT 已覆盖）
                conn.execute(text("UPDATE characters SET category='persona_visual' WHERE category IS NULL"))
            # 历史兼容：将旧值 persona 归一化为 persona_visual
            conn.execute(text("UPDATE characters SET category='persona_visual' WHERE category='persona'"))
    except Exception as e:
        # 不阻断服务启动，但打印错误便于排查
        print(f"[Migration][Warning] ensure_characters_category_column failed: {e}")

def ensure_episode_scene_description_columns():
    """
    轻量 SQLite 迁移：
    - 为 episodes 和 scenes 表添加 description 列
    - 为 episodes 表添加 action_text、prompt 列
    - 为 scenes 表添加 action_text、dialogue、prompt 列
    """
    try:
        with engine.begin() as conn:
            # 检查并添加 episodes 的各个列
            ep_cols = conn.execute(text("PRAGMA table_info(episodes)")).fetchall()
            ep_col_names = {row[1] for row in ep_cols}
            if "description" not in ep_col_names:
                conn.execute(text("ALTER TABLE episodes ADD COLUMN description TEXT"))
            if "action_text" not in ep_col_names:
                conn.execute(text("ALTER TABLE episodes ADD COLUMN action_text TEXT"))
            if "prompt" not in ep_col_names:
                conn.execute(text("ALTER TABLE episodes ADD COLUMN prompt TEXT"))
            
            # 检查并添加 scenes 的各个列
            sc_cols = conn.execute(text("PRAGMA table_info(scenes)")).fetchall()
            sc_col_names = {row[1] for row in sc_cols}
            if "description" not in sc_col_names:
                conn.execute(text("ALTER TABLE scenes ADD COLUMN description TEXT"))
            if "action_text" not in sc_col_names:
                conn.execute(text("ALTER TABLE scenes ADD COLUMN action_text TEXT"))
            if "dialogue" not in sc_col_names:
                conn.execute(text("ALTER TABLE scenes ADD COLUMN dialogue TEXT"))
            if "prompt" not in sc_col_names:
                conn.execute(text("ALTER TABLE scenes ADD COLUMN prompt TEXT"))
    except Exception as e:
        print(f"[Migration][Warning] ensure_episode_scene_description_columns failed: {e}")

def ensure_ai_action_runs_table():
    """
    轻量 SQLite 迁移：
    - 创建 ai_action_runs 表（用于按钮输出历史）
    说明：Base.metadata.create_all 通常已会创建表；这里再加一道 IF NOT EXISTS，确保老环境可用。
    """
    try:
        with engine.begin() as conn:
            conn.execute(
                text(
                    """
CREATE TABLE IF NOT EXISTS ai_action_runs (
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL,
  target_type TEXT NOT NULL DEFAULT 'episode',
  target_id INTEGER NOT NULL,
  action_key TEXT NOT NULL,
  input_text TEXT,
  output_text TEXT NOT NULL,
  meta_data JSON,
  created_at DATETIME DEFAULT (CURRENT_TIMESTAMP),
  FOREIGN KEY(project_id) REFERENCES projects (id)
);
"""
                )
            )
            conn.execute(text("CREATE INDEX IF NOT EXISTS ix_ai_action_runs_project_id ON ai_action_runs (project_id)"))
            conn.execute(text("CREATE INDEX IF NOT EXISTS ix_ai_action_runs_target ON ai_action_runs (target_type, target_id)"))
            conn.execute(text("CREATE INDEX IF NOT EXISTS ix_ai_action_runs_action_key ON ai_action_runs (action_key)"))
            conn.execute(text("CREATE INDEX IF NOT EXISTS ix_ai_action_runs_created_at ON ai_action_runs (created_at)"))
    except Exception as e:
        print(f"[Migration][Warning] ensure_ai_action_runs_table failed: {e}")

def ensure_episodes_execute_columns():
    try:
        with engine.begin() as conn:
            ep_cols = conn.execute(text("PRAGMA table_info(episodes)")).fetchall()
            ep_col_names = {row[1] for row in ep_cols}
            if "script_locked" not in ep_col_names:
                conn.execute(text("ALTER TABLE episodes ADD COLUMN script_locked BOOLEAN NOT NULL DEFAULT 0"))
            if "script_locked_at" not in ep_col_names:
                conn.execute(text("ALTER TABLE episodes ADD COLUMN script_locked_at DATETIME"))
            if "last_exec_run_id" not in ep_col_names:
                conn.execute(text("ALTER TABLE episodes ADD COLUMN last_exec_run_id TEXT"))
            if "exec_status" not in ep_col_names:
                conn.execute(text("ALTER TABLE episodes ADD COLUMN exec_status TEXT NOT NULL DEFAULT 'idle'"))
            if "exec_artifacts" not in ep_col_names:
                conn.execute(text("ALTER TABLE episodes ADD COLUMN exec_artifacts JSON"))
    except Exception as e:
        print(f"[Migration][Warning] ensure_episodes_execute_columns failed: {e}")

ensure_characters_category_column()
ensure_episode_scene_description_columns()
ensure_episodes_execute_columns()
ensure_ai_action_runs_table()
ensure_projects_uuid_column()

app = FastAPI(title="AI Comic Studio")

# ==========================
# Debug: Ctrl-C dump stacks
# ==========================
try:
    faulthandler.enable(all_threads=True)
except Exception:
    pass

_old_sigint = None
try:
    _old_sigint = signal.getsignal(signal.SIGINT)
except Exception:
    _old_sigint = None

def _sigint_dump_stacks(signum, frame):  # type: ignore
    try:
        print("\n[Debug] SIGINT received, dumping all thread stacks...\n", file=sys.stderr, flush=True)
        faulthandler.dump_traceback(file=sys.stderr, all_threads=True)
    except Exception as e:
        try:
            print(f"[Debug] dump_traceback failed: {e}", file=sys.stderr, flush=True)
        except Exception:
            pass
    # chain to old handler if exists; otherwise let default KeyboardInterrupt happen
    try:
        if callable(_old_sigint):
            _old_sigint(signum, frame)  # type: ignore
    except Exception:
        pass

try:
    signal.signal(signal.SIGINT, _sigint_dump_stacks)
except Exception:
    pass

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

DATA_DIR = os.environ.get("AICOMIC_DATA_DIR") or os.path.join(os.getcwd(), "data")
os.makedirs(DATA_DIR, exist_ok=True)
app.mount("/files", StaticFiles(directory=DATA_DIR), name="files")

app.include_router(storyboard.router)
app.include_router(assets.router)
app.include_router(projects.router)
app.include_router(events.router)
app.include_router(ai.router)
app.include_router(ai_runs.router)
app.include_router(memory.router)

@app.get("/")
def read_root():
    return {"message": "Server is running"}
