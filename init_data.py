# init_data.py
from backend.app.database import SessionLocal, engine
from backend.app import models

# 1. 重置数据库 (删除旧表建新表)
models.Base.metadata.drop_all(bind=engine)
models.Base.metadata.create_all(bind=engine)

db = SessionLocal()

# 2. 建项目
project = models.Project(name="诛仙·青云志", description="张小凡的故事")
db.add(project)
db.commit()

# 3. 建集 (Episode) - 新增层级
ep1 = models.Episode(project_id=project.id, title="第一集：草庙村惨案", order=1)
ep2 = models.Episode(project_id=project.id, title="第二集：初入青云", order=2)
db.add_all([ep1, ep2])
db.commit()

# 4. 建场 (Scene)
s1 = models.Scene(episode_id=ep1.id, sequence_number=1, title="草庙村·雨夜")
s2 = models.Scene(episode_id=ep1.id, sequence_number=2, title="破庙·普智激战")
db.add_all([s1, s2])
db.commit()

# 5. 建镜 (Shot)
shots = [
    models.Shot(scene_id=s1.id, sequence_number=1, title="全景", action_text="雷雨交加...", prompt="storm"),
    models.Shot(scene_id=s1.id, sequence_number=2, title="特写", action_text="小凡惊醒...", prompt="boy wake up"),
]
db.add_all(shots)
db.commit()

# 6. 建事件 (Event) - Phase 1 测试
evt1 = models.Event(project_id=project.id, name="张小凡的悲剧命运", color="#EF4444")
evt2 = models.Event(project_id=project.id, name="普智的执念", color="#F59E0B")
db.add_all([evt1, evt2])
db.commit()

print("新架构数据初始化完成！")
db.close()