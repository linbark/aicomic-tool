import os
import shutil
from fastapi import APIRouter, Depends, HTTPException, UploadFile, File
from sqlalchemy.orm import Session
from .. import models, schemas
from ..database import get_db
from ..utils.metadata_parser import extract_metadata
from ..models import Project, Character, Asset, Shot

router = APIRouter(
    prefix="/assets",
    tags=["Assets (资源管理)"]
)

# 配置根存储目录
DATA_ROOT = os.path.join(os.getcwd(), "data")

def get_project_name(db: Session, project_id: int):
    project = db.query(models.Project).filter(models.Project.id == project_id).first()
    if not project:
        raise HTTPException(status_code=404, detail="Project not found")
    return project.name

# 1. 为角色上传图片/视频 (三视图等)
@router.post("/character/{char_id}", response_model=schemas.AssetRead)
async def upload_character_asset(
    char_id: int, 
    file: UploadFile = File(...), 
    db: Session = Depends(get_db)
):
    # 1. 获取角色和项目信息
    char = db.query(models.Character).filter(models.Character.id == char_id).first()
    if not char:
        raise HTTPException(status_code=404, detail="Character not found")
    
    project_name = get_project_name(db, char.project_id)
    
    # 2. 构建目标路径: data/{project_name}/characters/
    # 注意：为了兼容操作系统路径，使用 os.path.join
    save_dir = os.path.join(DATA_ROOT, project_name, "characters")
    os.makedirs(save_dir, exist_ok=True)
    
    # 3. 保存文件
    file_path = os.path.join(save_dir, file.filename)
    
    # 写入文件
    with open(file_path, "wb") as buffer:
        shutil.copyfileobj(file.file, buffer)
        
    # 4. 判断类型
    file_type = "image"
    if file.content_type and file.content_type.startswith("video"):
        file_type = "video"
        
    # 5. 提取元数据 (如果是图片)
    meta = {}
    if file_type == "image":
        meta = extract_metadata(file_path)

    # 6. 存入数据库
    # 存储相对路径： "项目名/characters/文件名" 方便前端拼接 /files/
    relative_path = f"{project_name}/characters/{file.filename}"
    
    db_asset = models.Asset(
        character_id=char.id,
        file_path=relative_path,
        file_type=file_type,
        meta_data=meta,
        is_favorite=True # 默认上传的都算精选
    )
    
    db.add(db_asset)
    db.commit()
    db.refresh(db_asset)
    return db_asset

# 2. 为镜头关联资产 (保持原有逻辑，但适配新路径)
# 注意：这里我们保留原来的 POST 接口，但建议前端改成上传模式，或者你继续用本地路径粘贴模式
# 为了演示，这里假设你还是用"粘贴路径"的方式，或者你可以参考上面的 upload 写一个 create_shot_asset
@router.post("/shot/{shot_id}", response_model=schemas.AssetRead)
def register_asset_for_shot(
    shot_id: int, 
    file_path: str, 
    db: Session = Depends(get_db)
):
    # 这里逻辑暂时不变，如果用户手动粘贴路径，需要确保路径在 data/ 下
    # 建议后续也将此改为 upload 模式，存到 data/{project_name}/{ep_scene_shot}/ 下
    
    # 简单兼容：如果 file_path 是绝对路径，尝试读取
    meta = extract_metadata(file_path)
    
    db_asset = models.Asset(
        shot_id=shot_id,
        file_path=file_path,
        file_type="image",
        meta_data=meta
    )
    
    db.add(db_asset)
    db.commit()
    db.refresh(db_asset)
    return db_asset

@router.post("/shot/{shot_id}/video", response_model=schemas.ShotRead)
async def upload_shot_video(
    shot_id: int, 
    file: UploadFile = File(...), 
    db: Session = Depends(get_db)
):
    # 1. 查找分镜
    shot = db.query(models.Shot).filter(models.Shot.id == shot_id).first()
    if not shot:
        raise HTTPException(status_code=404, detail="Shot not found")
    
    # 2. 获取项目名用于构建路径
    # 需要关联查询找到 project_name，路径稍微绕一点 Shot -> Scene -> Episode -> Project
    # 简单起见，我们先反查或者直接用 ID 命名文件夹，这里为了文件结构好看，我们尝试查一下
    scene = db.query(models.Scene).filter(models.Scene.id == shot.scene_id).first()
    episode = db.query(models.Episode).filter(models.Episode.id == scene.episode_id).first()
    project = db.query(models.Project).filter(models.Project.id == episode.project_id).first()
    
    project_name = project.name if project else "unknown_project"
    
    # 3. 构建保存路径: data/{project_name}/videos/
    save_dir = os.path.join(DATA_ROOT, project_name, "videos")
    os.makedirs(save_dir, exist_ok=True)
    
    # 4. 生成文件名 (建议保留原后缀，前面加 shot_id 防止重名)
    file_ext = os.path.splitext(file.filename)[1]
    new_filename = f"shot_{shot_id}_video{file_ext}"
    file_full_path = os.path.join(save_dir, new_filename)
    
    # 5. 保存文件
    with open(file_full_path, "wb") as buffer:
        shutil.copyfileobj(file.file, buffer)
        
    # 6. 更新数据库 Shot 记录
    # 存储相对路径 "项目名/videos/文件名"
    relative_path = f"{project_name}/videos/{new_filename}"
    shot.video_path = relative_path
    
    db.commit()
    db.refresh(shot)
    
    return shot