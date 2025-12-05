from fastapi import APIRouter, Depends, HTTPException,  File, UploadFile
from sqlalchemy.orm import Session
from typing import List
import shutil
import os
import uuid


# 引入我们定义好的数据库模型和Pydantic模型
from .. import models, schemas
from ..database import get_db # 假设你有一个 get_db 依赖项

router = APIRouter(
    prefix="/storyboard",  # 👈 修改这里：从 "/script" 改为 "/storyboard"
    tags=["Script Backbone (剧本骨架)"]
)

VIDEO_DIR = "user_projects/videos"

# 1. 获取项目的剧本结构 (Episode -> Scene -> Shot)
@router.get("/project/{project_id}", response_model=List[schemas.EpisodeRead])
def get_full_script(project_id: int, db: Session = Depends(get_db)):
    # 这里的查询会比较重，获取了整个项目的所有集、场、镜
    # 实际生产中可能需要按集懒加载，但本地工具没关系
    # 使用 joinedload 预加载关系数据，避免 N+1 查询问题
    from sqlalchemy.orm import joinedload
    episodes = db.query(models.Episode)\
                 .options(
                     joinedload(models.Episode.scenes).joinedload(models.Scene.shots).joinedload(models.Shot.assets)
                 )\
                 .filter(models.Episode.project_id == project_id)\
                 .order_by(models.Episode.order).all()
    return episodes

# 2. 创建集
@router.post("/project/{project_id}/episode", response_model=schemas.EpisodeRead)
def create_episode(project_id: int, episode: schemas.EpisodeCreate, db: Session = Depends(get_db)):
    db_ep = models.Episode(**episode.dict(), project_id=project_id)
    db.add(db_ep)
    db.commit()
    db.refresh(db_ep)
    return db_ep

# 3. 创建场 (现在需要 episode_id)
@router.post("/episode/{episode_id}/scene", response_model=schemas.SceneRead)
def create_scene(episode_id: int, scene: schemas.SceneCreate, db: Session = Depends(get_db)):
    # 1. 自动计算 sequence_number (如果前端没传)
    if scene.sequence_number is None:
        # 查询该集下序号最大的场次
        last_scene = db.query(models.Scene)\
            .filter(models.Scene.episode_id == episode_id)\
            .order_by(models.Scene.sequence_number.desc())\
            .first()
        
        # 如果有上一场，则 +1；否则从 1 开始
        new_seq = (last_scene.sequence_number + 1) if last_scene else 1
    else:
        new_seq = scene.sequence_number

    # 2. 自动生成标题 (如果前端没传)
    # 格式：Scene 1, Scene 2...
    new_title = scene.title
    if not new_title:
        new_title = f"Scene {new_seq}"

    # 3. 创建数据库对象
    db_scene = models.Scene(
        episode_id=episode_id,
        sequence_number=new_seq,
        title=new_title
    )
    
    db.add(db_scene)
    db.commit()
    db.refresh(db_scene)
    return db_scene

@router.patch("/scene/{scene_id}", response_model=schemas.SceneRead)
def update_scene(scene_id: int, scene_update: schemas.SceneUpdate, db: Session = Depends(get_db)):
    db_scene = db.query(models.Scene).filter(models.Scene.id == scene_id).first()
    if not db_scene:
        raise HTTPException(status_code=404, detail="Scene not found")
    
    if scene_update.title is not None:
        db_scene.title = scene_update.title
        
    db.commit()
    db.refresh(db_scene)
    return db_scene
    
# 修改某个镜头的内容 (记录修改)
@router.patch("/shot/{shot_id}", response_model=schemas.ShotRead)
def update_shot(shot_id: int, shot_update: schemas.ShotUpdate, db: Session = Depends(get_db)):
    db_shot = db.query(models.Shot).filter(models.Shot.id == shot_id).first()
    if not db_shot:
        raise HTTPException(status_code=404, detail="Shot not found")
    
    update_data = shot_update.dict(exclude_unset=True)
    for key, value in update_data.items():
        setattr(db_shot, key, value)
    
    db.commit()
    db.refresh(db_shot)
    return db_shot

# 3. 创建一个新的镜头 (比如需要加戏)
@router.post("/scene/{scene_id}/shot", response_model=schemas.ShotRead)
def create_shot(scene_id: int, shot: schemas.ShotCreate, db: Session = Depends(get_db)):
    db_shot = models.Shot(**shot.dict(), scene_id=scene_id)
    db.add(db_shot)
    db.commit()
    db.refresh(db_shot)
    return db_shot

@router.delete("/episode/{episode_id}")
def delete_episode(episode_id: int, db: Session = Depends(get_db)):
    db_ep = db.query(models.Episode).filter(models.Episode.id == episode_id).first()
    if not db_ep:
        raise HTTPException(status_code=404, detail="Episode not found")
    
    db.delete(db_ep) # 级联删除会自动处理 Scene 和 Shot
    db.commit()
    return {"message": "Episode deleted"}

@router.delete("/scene/{scene_id}")
def delete_scene(scene_id: int, db: Session = Depends(get_db)):
    db_scene = db.query(models.Scene).filter(models.Scene.id == scene_id).first()
    if not db_scene:
        raise HTTPException(status_code=404, detail="Scene not found")
    
    db.delete(db_scene) # 级联删除会自动处理 Shot
    db.commit()
    return {"message": "Scene deleted"}

@router.delete("/shot/{shot_id}")
def delete_shot(shot_id: int, db: Session = Depends(get_db)):
    db_shot = db.query(models.Shot).filter(models.Shot.id == shot_id).first()
    if not db_shot:
        raise HTTPException(status_code=404, detail="Shot not found")
    
    db.delete(db_shot)
    db.commit()
    return {"message": "Shot deleted"}

@router.post("/shot/{shot_id}/video", response_model=schemas.ShotRead)
def upload_shot_video(shot_id: int, file: UploadFile = File(...), db: Session = Depends(get_db)):
    # 1. 查询镜头信息，并预加载关联直到 Project
    # 路径：Shot -> Scene -> Episode -> Project
    # 注意：这里假设你的 model 关系定义完善。如果报错，可能需要手动 join 查询。
    db_shot = db.query(models.Shot).filter(models.Shot.id == shot_id).first()
    if not db_shot:
        raise HTTPException(status_code=404, detail="Shot not found")
    
    # 2. 获取项目名称 (需要防空处理)
    try:
        project_name = db_shot.scene.episode.project.name
    except AttributeError:
        # 如果关系链断裂，使用默认文件夹
        project_name = "unknown_project"

    # 3. 构建目标路径: data/{project_name}/storyBoards/videos
    # 建议对 project_name 做简单的去特殊字符处理，防止路径报错
    safe_project_name = "".join([c for c in project_name if c.isalnum() or c in (' ', '_', '-')]).strip()
    
    base_dir = os.path.join("data", safe_project_name, "storyBoards", "videos")
    os.makedirs(base_dir, exist_ok=True) # 自动创建多级目录

    # 4. 生成文件名并保存
    file_ext = os.path.splitext(file.filename)[1] or ".mp4"
    new_filename = f"{uuid.uuid4()}{file_ext}"
    file_path = os.path.join(base_dir, new_filename)

    with open(file_path, "wb") as buffer:
        shutil.copyfileobj(file.file, buffer)

    # 5. 更新数据库 (存储相对路径，方便前端映射)
    # 存入数据库的格式例如: data/MyProject/storyBoards/videos/abc.mp4
    # 注意：Windows下路径分隔符可能是反斜杠，建议统一为正斜杠以便前端处理
    db_shot.video_path = file_path.replace("\\", "/")
    
    db.commit()
    db.refresh(db_shot)
    
    return db_shot