# backend/app/main.py
import os
from fastapi import FastAPI
from fastapi.staticfiles import StaticFiles
from fastapi.middleware.cors import CORSMiddleware
from .routers import storyboard, assets, projects, events
from .database import engine, Base
from sqlalchemy import text

Base.metadata.create_all(bind=engine)

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

ensure_characters_category_column()

app = FastAPI(title="AI Comic Studio")

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

DATA_DIR = os.path.join(os.getcwd(), "data")
os.makedirs(DATA_DIR, exist_ok=True)
app.mount("/files", StaticFiles(directory=DATA_DIR), name="files")

app.include_router(storyboard.router)
app.include_router(assets.router)
app.include_router(projects.router)
app.include_router(events.router)

@app.get("/")
def read_root():
    return {"message": "Server is running"}