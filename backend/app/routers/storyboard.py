from fastapi import APIRouter, Depends, HTTPException,  File, UploadFile
from sqlalchemy.orm import Session
from typing import List
import shutil
import os
import uuid
from sqlalchemy.orm import joinedload



# 引入我们定义好的数据库模型和Pydantic模型
from .. import models, schemas
from ..database import get_db # 假设你有一个 get_db 依赖项

router = APIRouter(
    prefix="/storyboard",  # 👈 修改这里：从 "/script" 改为 "/storyboard"
    tags=["Script Backbone (剧本骨架)"]
)

VIDEO_DIR = "user_projects/videos"

# 1. 获取项目的剧本结构
@router.get("/project/{project_id}", response_model=List[schemas.EpisodeRead])
def get_full_script(project_id: int, db: Session = Depends(get_db)):
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

# 3. 创建场
@router.post("/episode/{episode_id}/scene", response_model=schemas.SceneRead)
def create_scene(episode_id: int, scene: schemas.SceneCreate, db: Session = Depends(get_db)):
    if scene.sequence_number is None:
        last_scene = db.query(models.Scene)\
            .filter(models.Scene.episode_id == episode_id)\
            .order_by(models.Scene.sequence_number.desc())\
            .first()
        new_seq = (last_scene.sequence_number + 1) if last_scene else 1
    else:
        new_seq = scene.sequence_number

    new_title = scene.title
    if not new_title:
        new_title = f"Scene {new_seq}"

    db_scene = models.Scene(episode_id=episode_id, sequence_number=new_seq, title=new_title)
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
    
# 4. 镜头管理
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

@router.post("/scene/{scene_id}/shot", response_model=schemas.ShotRead)
def create_shot(scene_id: int, shot: schemas.ShotCreate, db: Session = Depends(get_db)):
    db_shot = models.Shot(**shot.dict(), scene_id=scene_id)
    db.add(db_shot)
    db.commit()
    db.refresh(db_shot)
    return db_shot

# =======================
# 删除接口 (包含物理文件清理)
# =======================

@router.delete("/episode/{episode_id}")
def delete_episode(episode_id: int, db: Session = Depends(get_db)):
    db_ep = db.query(models.Episode).filter(models.Episode.id == episode_id).first()
    if not db_ep:
        raise HTTPException(status_code=404, detail="Episode not found")
    db.delete(db_ep)
    db.commit()
    return {"message": "Episode deleted"}

@router.delete("/scene/{scene_id}")
def delete_scene(scene_id: int, db: Session = Depends(get_db)):
    db_scene = db.query(models.Scene).filter(models.Scene.id == scene_id).first()
    if not db_scene:
        raise HTTPException(status_code=404, detail="Scene not found")
    
    # 2. 物理删除逻辑
    project_name = None
    episode_id = None
    try:
        project_name = db_scene.episode.project.name
        episode_id = db_scene.episode.id
    except Exception:
        pass

    # 删除对应场次的文件夹：data/{project}/storyboard/episode_{id}/scene_{id}
    if project_name and episode_id:
        base_data_dir = os.path.join(os.getcwd(), "data")
        scene_dir = os.path.join(
            base_data_dir,
            project_name,
            "storyboard",
            f"episode_{episode_id}",
            f"scene_{scene_id}",
        )
        if os.path.exists(scene_dir):
            try:
                shutil.rmtree(scene_dir)
                print(f"[Delete] Removed scene folder: {scene_dir}")
            except Exception as e:
                print(f"[Delete] Failed to remove scene folder {scene_dir}: {e}")

    db.delete(db_scene)
    db.commit()
    return {"message": "Scene deleted"}

@router.delete("/shot/{shot_id}")
def delete_shot(shot_id: int, db: Session = Depends(get_db)):
    # 1. 查询镜头 (预加载 assets 以便删除文件)
    db_shot = db.query(models.Shot).options(joinedload(models.Shot.assets)).filter(models.Shot.id == shot_id).first()
    
    if not db_shot:
        raise HTTPException(status_code=404, detail="Shot not found")
    
    # 2. 物理删除逻辑
    base_data_dir = os.path.join(os.getcwd(), "data")

    # A. 删除 Asset 文件 (图片/文档)
    if db_shot.assets:
        for asset in db_shot.assets:
            if asset.file_path:
                # 拼接完整路径。注意：file_path 是相对路径 "Project/storyboard/..."
                file_full_path = os.path.join(base_data_dir, asset.file_path)
                try:
                    if os.path.exists(file_full_path):
                        os.remove(file_full_path)
                        print(f"[Delete] Removed asset file: {file_full_path}")
                except Exception as e:
                    print(f"[Error] Failed to remove asset file {file_full_path}: {e}")

    # B. 删除视频文件 (Video)
    if db_shot.video_path:
        video_full_path = os.path.join(base_data_dir, db_shot.video_path)
        try:
            if os.path.exists(video_full_path):
                os.remove(video_full_path)
                print(f"[Delete] Removed shot video: {video_full_path}")
        except Exception as e:
            print(f"[Error] Failed to remove shot video {video_full_path}: {e}")

    # 3. 数据库删除
    db.delete(db_shot)
    db.commit()
    return {"message": "Shot and associated files deleted"}

# =======================
# 上传视频接口
# =======================
@router.post("/shot/{shot_id}/video", response_model=schemas.ShotRead)
def upload_shot_video(shot_id: int, file: UploadFile = File(...), db: Session = Depends(get_db)):
    # 1. 完整关联查询
    db_shot = db.query(models.Shot).filter(models.Shot.id == shot_id).first()
    if not db_shot:
        raise HTTPException(status_code=404, detail="Shot not found")
    
    # 关联对象
    scene = db.query(models.Scene).filter(models.Scene.id == db_shot.scene_id).first()
    episode = db.query(models.Episode).filter(models.Episode.id == scene.episode_id).first()
    project = db.query(models.Project).filter(models.Project.id == episode.project_id).first()
    
    project_name = project.name if project else "unknown_project"

    # 2. 构建层级路径: data/{Project}/storyboard/episode_{id}/scene_{id}/shot_{id}/video/
    hierarchy_path = os.path.join(
        "storyboard",
        f"episode_{episode.id}",
        f"scene_{scene.id}",
        f"shot_{db_shot.id}",
        "video"
    )
    
    # 绝对路径用于保存
    DATA_ROOT = os.path.join(os.getcwd(), "data")
    save_dir = os.path.join(DATA_ROOT, project_name, hierarchy_path)
    os.makedirs(save_dir, exist_ok=True) 

    # 3. 保存文件
    file_ext = os.path.splitext(file.filename)[1] or ".mp4"
    # 视频可以使用 uuid 或固定名字 (比如 main_video.mp4)，这里用 uuid 防止浏览器缓存问题
    new_filename = f"{uuid.uuid4()}{file_ext}"
    file_abs_path = os.path.join(save_dir, new_filename)

    with open(file_abs_path, "wb") as buffer:
        shutil.copyfileobj(file.file, buffer)

    # 4. 更新数据库 (存储相对路径)
    relative_path_part = os.path.join(project_name, hierarchy_path, new_filename)
    relative_path = relative_path_part.replace("\\", "/") # 修正分隔符
    
    db_shot.video_path = relative_path
    
    db.commit()
    db.refresh(db_shot)
    
    return db_shot