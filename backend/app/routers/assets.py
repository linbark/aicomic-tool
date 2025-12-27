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

def determine_file_type(content_type: str, filename: str) -> str:
    """辅助函数：根据 MIME 或 后缀判断文件类型"""
    if content_type:
        if content_type.startswith("image"): return "image"
        if content_type.startswith("video"): return "video"
        if content_type.startswith("text"): return "text"
    
    # 后缀回退判断
    ext = os.path.splitext(filename)[1].lower()
    if ext in ['.jpg', '.jpeg', '.png', '.webp', '.gif']: return "image"
    if ext in ['.mp4', '.mov', '.avi', '.webm']: return "video"
    if ext in ['.txt', '.md', '.json', '.pdf', '.doc', '.docx']: return "text"
    
    return "other"

# ==========================================
# 1. 资产条目资源上传 (支持文档)
# ==========================================
@router.post("/asset-item/{item_id}", response_model=schemas.AssetRead)
async def upload_asset_item_asset(
    item_id: int,
    file: UploadFile = File(...), 
    db: Session = Depends(get_db)
):
    item = db.query(models.Character).filter(models.Character.id == item_id).first()
    if not item:
        raise HTTPException(status_code=404, detail="Asset item not found")
    
    project_name = get_project_name(db, item.project_id)
    
    # 按分类决定落盘目录（persona 兼容旧 characters 目录）
    category = (item.category or "persona").lower()
    folder = "characters" if category == "persona" else "backgrounds" if category == "background" else category
    save_dir = os.path.join(DATA_ROOT, project_name, folder)
    os.makedirs(save_dir, exist_ok=True)
    
    file_path = os.path.join(save_dir, file.filename)
    
    with open(file_path, "wb") as buffer:
        shutil.copyfileobj(file.file, buffer)
        
    # --- 修改点：使用通用类型判断 ---
    file_type = determine_file_type(file.content_type, file.filename)
        
    meta = {}
    if file_type == "image":
        meta = extract_metadata(file_path)

    relative_path = f"{project_name}/{folder}/{file.filename}"
    
    db_asset = models.Asset(
        character_id=item.id,
        file_path=relative_path,
        file_type=file_type,
        meta_data=meta,
        is_favorite=True 
    )
    
    db.add(db_asset)
    db.commit()
    db.refresh(db_asset)
    return db_asset

# -----------------------
# 兼容旧接口（前端已切到 /asset-item/{id}；此处仅避免旧客户端断掉）
# -----------------------
@router.post("/character/{char_id}", response_model=schemas.AssetRead)
async def upload_character_asset(
    char_id: int,
    file: UploadFile = File(...),
    db: Session = Depends(get_db),
):
    return await upload_asset_item_asset(item_id=char_id, file=file, db=db)

# ==========================================
# 2. 镜头资源上传
# ==========================================
@router.post("/shot/{shot_id}/upload", response_model=schemas.AssetRead)
async def upload_shot_asset(
    shot_id: int, 
    file: UploadFile = File(...), 
    db: Session = Depends(get_db)
):
    # 1. 获取镜头及完整的层级关系
    shot = db.query(models.Shot).filter(models.Shot.id == shot_id).first()
    if not shot:
        raise HTTPException(status_code=404, detail="Shot not found")
    
    # 关联查询: Shot -> Scene -> Episode -> Project
    # 也可以使用 db.query(models.Project).join(...).filter(...) 的方式
    # 这里为了简单直接查对象
    scene = db.query(models.Scene).filter(models.Scene.id == shot.scene_id).first()
    episode = db.query(models.Episode).filter(models.Episode.id == scene.episode_id).first()
    project = db.query(models.Project).filter(models.Project.id == episode.project_id).first()
    
    project_name = project.name

    # 2. 构建层级路径: data/{Project}/storyboard/episode_{id}/scene_{id}/shot_{id}/assets/
    # 使用 ID 命名文件夹比使用 Title 更安全，因为 Title 会变，ID 不会
    hierarchy_path = os.path.join(
        "storyboard",
        f"episode_{episode.id}",
        f"scene_{scene.id}",
        f"shot_{shot.id}",
        "assets"
    )
    
    save_dir = os.path.join(DATA_ROOT, project_name, hierarchy_path)
    os.makedirs(save_dir, exist_ok=True)

    # 3. 保存文件
    # 既然已经分了 shot_{id} 文件夹，文件名冲突概率大大降低，直接用原名即可
    file_path = os.path.join(save_dir, file.filename)
    
    with open(file_path, "wb") as buffer:
        shutil.copyfileobj(file.file, buffer)

    # 4. 判断类型
    file_type = determine_file_type(file.content_type, file.filename)
    
    # 5. 提取 Metadata
    meta = {}
    if file_type == "image":
        meta = extract_metadata(file_path)

    # 6. 入库 (存储相对路径)
    # Windows下 path separator 是 \, 需要转成 / 供前端 URL 使用
    relative_path_part = os.path.join(project_name, hierarchy_path, file.filename)
    relative_path = relative_path_part.replace("\\", "/")
    
    db_asset = models.Asset(
        shot_id=shot.id,
        file_path=relative_path,
        file_type=file_type,
        meta_data=meta,
        is_favorite=False
    )
    
    db.add(db_asset)
    db.commit()
    db.refresh(db_asset)
    return db_asset

# 2. 为镜头关联资产 (保持原有逻辑，但适配新路径)
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