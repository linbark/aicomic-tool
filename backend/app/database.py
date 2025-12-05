from sqlalchemy import create_engine
from sqlalchemy.ext.declarative import declarative_base
from sqlalchemy.orm import sessionmaker

# 定义 SQLite 数据库的地址
# 这里的 ./database.db 表示会在项目根目录生成数据库文件
SQLALCHEMY_DATABASE_URL = "sqlite:///./database.db"

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
