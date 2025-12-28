import os
from pathlib import Path

from sqlalchemy import create_engine
from sqlalchemy.ext.declarative import declarative_base
from sqlalchemy.orm import sessionmaker

def _build_sqlite_url() -> str:
    """
    支持桌面版通过环境变量指定 DB 路径：
    - AICOMIC_DB_PATH: /abs/path/to/database.db 或相对路径
    """
    db_path = os.environ.get("AICOMIC_DB_PATH")
    if db_path:
        p = Path(db_path).expanduser()
        p.parent.mkdir(parents=True, exist_ok=True)
        # sqlite URL：相对路径 sqlite:///relative.db；绝对路径 sqlite:////abs/path.db
        return f"sqlite:///{p}"
    # 默认仍使用项目根目录下的 database.db
    return "sqlite:///./database.db"

# 定义 SQLite 数据库的地址
SQLALCHEMY_DATABASE_URL = _build_sqlite_url()

# 创建引擎
# connect_args={"check_same_thread": False} 是 SQLite 必须的配置
engine = create_engine(
    SQLALCHEMY_DATABASE_URL, connect_args={"check_same_thread": False}
)

# 创建会话工厂
SessionLocal = sessionmaker(autocommit=False, autoflush=False, bind=engine)

# 创建 Base 类，所有的 Model 都继承自它
Base = declarative_base()

# 获取数据库会话的依赖函数 (用于 FastAPI)
def get_db():
    db = SessionLocal()
    try:
        yield db
    finally:
        db.close()
