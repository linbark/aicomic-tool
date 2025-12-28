# AI Comic Tool 项目文档

## 项目概述

AI Comic Tool 是一个专为**配音漫剧/视频**制作设计的桌面应用程序，使用 Tauri 框架构建，集成了 Vue.js 前端和 FastAPI 后端。该工具支持项目级别的剧本管理、资产库管理、事件系统等功能，帮助创作者高效地组织和制作内容。

## 技术架构

### 前端技术栈
- **框架**: Vue.js 3 (Composition API)
- **状态管理**: Pinia
- **路由**: Vue Router
- **构建工具**: Vite
- **UI 框架**: Tailwind CSS
- **桌面框架**: Tauri (Rust)

### 后端技术栈
- **框架**: FastAPI (Python)
- **数据库**: SQLite
- **ORM**: SQLAlchemy
- **数据验证**: Pydantic

### 桌面应用
- **框架**: Tauri
- **语言**: Rust
- **平台**: macOS (当前支持)

## 项目结构

```
aicomic-tool/
├── backend/                 # FastAPI 后端
│   └── app/
│       ├── main.py         # 应用入口、路由注册、数据库迁移
│       ├── models.py       # SQLAlchemy 数据模型
│       ├── schemas.py      # Pydantic 数据验证模型
│       ├── database.py     # 数据库连接配置
│       └── routers/        # API 路由模块
│           ├── projects.py    # 项目和资产条目管理
│           ├── storyboard.py  # 剧本骨架管理 (Episode/Scene/Shot)
│           ├── assets.py      # 资源文件上传管理
│           └── events.py      # 事件系统管理
├── frontend/               # Vue.js 前端
│   ├── src/
│   │   ├── App.vue         # 主应用组件
│   │   ├── router/         # 路由配置
│   │   ├── stores/         # Pinia 状态管理
│   │   ├── views/          # 页面视图
│   │   │   ├── ScriptView.vue      # 剧本编辑视图
│   │   │   ├── EventMatrixView.vue # 事件矩阵视图
│   │   │   ├── EventFlowView.vue   # 事件流程图视图
│   │   │   └── AssetLibraryView.vue # 资产库视图
│   │   ├── components/     # 组件
│   │   │   ├── ScriptTree.vue      # 剧本树组件
│   │   │   └── EventList.vue       # 事件列表组件
│   │   └── api/            # API 客户端
│   └── src-tauri/          # Tauri 配置和 Rust 代码
│       └── src/
│           └── main.rs     # Tauri 主程序（后端进程管理）
└── docs/                   # 项目文档
```

## 核心功能模块

### 1. 项目管理

#### 功能描述
- 创建、编辑、删除项目
- 项目切换和选择
- 项目数据隔离（每个项目有独立的数据目录）

#### API 接口
- `GET /projects/` - 获取所有项目列表
- `POST /projects/` - 创建新项目
- `PATCH /projects/{project_id}` - 更新项目信息
- `DELETE /projects/{project_id}` - 删除项目及其所有关联数据

#### 数据模型
```python
class Project:
    id: int
    name: str
    description: Optional[str]
    created_at: datetime
```

### 2. 资产库管理

#### 功能描述
资产库用于管理项目中的各类资源，支持 12 种分类：

1. **persona_visual** - 人设（视觉）
2. **persona_voice** - 人设（声音）
3. **background** - 背景
4. **element** - 元素
5. **prop** - 道具
6. **pose** - 姿态
7. **vfx** - 特效
8. **layout** - 布局
9. **audio_music** - 音频（音乐）
10. **audio_sfx** - 音频（音效）
11. **branding** - 品牌
12. **ai_preset** - AI 预设

每个资产条目可以包含多个资源文件（图片、视频、音频、文档等）。

#### API 接口
- `GET /projects/{project_id}/asset-items?category={category}` - 获取资产条目列表（支持分类筛选）
- `POST /projects/{project_id}/asset-items` - 创建资产条目
- `PATCH /projects/asset-items/{item_id}` - 更新资产条目
- `DELETE /projects/asset-items/{item_id}` - 删除资产条目及其所有文件
- `POST /assets/asset-item/{item_id}` - 为资产条目上传资源文件
- `DELETE /assets/{asset_id}` - 删除单个资源文件

#### 数据模型
```python
class Character (AssetItem):
    id: int
    project_id: int
    name: str
    description: Optional[str]
    base_prompt: Optional[str]  # 基础提示词
    category: str  # 资产分类
    avatar_asset_id: Optional[int]  # 头像资源ID
    assets: List[Asset]  # 关联的资源文件列表

class Asset:
    id: int
    character_id: Optional[int]  # 关联的资产条目ID
    shot_id: Optional[int]  # 关联的镜头ID
    file_path: str  # 文件相对路径
    file_type: str  # 文件类型: image/video/audio/text/other
    meta_data: Optional[dict]  # 元数据（图片尺寸等）
    is_favorite: bool  # 是否收藏
    created_at: datetime
```

#### 文件存储结构
```
data/
└── {项目名称}/
    ├── characters/      # 人设（视觉）资源
    ├── voices/         # 人设（声音）资源
    ├── backgrounds/    # 背景资源
    ├── elements/       # 元素资源
    ├── props/          # 道具资源
    ├── poses/          # 姿态资源
    ├── vfx/            # 特效资源
    ├── layout/         # 布局资源
    ├── audio_music/    # 音频（音乐）资源
    ├── audio_sfx/      # 音频（音效）资源
    ├── branding/       # 品牌资源
    ├── ai_preset/      # AI 预设资源
    └── storyboard/     # 剧本相关资源（见剧本管理部分）
```

### 3. 剧本管理

#### 功能描述
剧本采用四级层级结构：**Project → Episode（集）→ Scene（场）→ Shot（镜头）**

- **Episode（集）**: 最高层级，包含标题和剧本内容描述
- **Scene（场）**: 包含标题、剧本内容、画面描述（Action）、Stable Diffusion Prompt
- **Shot（镜头）**: 最细粒度，包含标题、画面描述、对话、Prompt、状态、关联的资源文件

#### API 接口

##### Episode（集）管理
- `GET /storyboard/project/{project_id}` - 获取项目的完整剧本结构
- `POST /storyboard/project/{project_id}/episode` - 创建新集
- `PATCH /storyboard/episode/{episode_id}` - 更新集信息
- `DELETE /storyboard/episode/{episode_id}` - 删除集（级联删除所有关联的 Scene 和 Shot）

##### Scene（场）管理
- `POST /storyboard/episode/{episode_id}/scene` - 创建新场
- `PATCH /storyboard/scene/{scene_id}` - 更新场信息
- `DELETE /storyboard/scene/{scene_id}` - 删除场（级联删除所有关联的 Shot 和文件）

##### Shot（镜头）管理
- `POST /storyboard/scene/{scene_id}/shot` - 创建新镜头
- `PATCH /storyboard/shot/{shot_id}` - 更新镜头信息
- `DELETE /storyboard/shot/{shot_id}` - 删除镜头及其关联的资源文件

##### 资源上传
- `POST /assets/shot/{shot_id}/upload` - 为镜头上传资源文件（图片/视频/音频等）
- `POST /storyboard/shot/{shot_id}/video` - 上传镜头视频文件

#### 数据模型
```python
class Episode:
    id: int
    project_id: int
    title: str
    order: int  # 排序序号
    description: Optional[str]  # 剧本内容描述
    action_text: Optional[str]  # 画面描述（保留字段，前端不显示）
    prompt: Optional[str]  # Stable Diffusion Prompt（保留字段，前端不显示）
    scenes: List[Scene]

class Scene:
    id: int
    episode_id: int
    sequence_number: int  # 场序号
    title: str
    description: Optional[str]  # 剧本内容描述
    action_text: Optional[str]  # 画面描述
    dialogue: Optional[str]  # 对话内容（保留字段，前端不显示）
    prompt: Optional[str]  # Stable Diffusion Prompt
    shots: List[Shot]

class Shot:
    id: int
    scene_id: int
    sequence_number: int  # 镜头序号
    title: Optional[str]  # 镜头名称
    action_text: Optional[str]  # 画面描述
    dialogue: Optional[str]  # 对话内容（保留字段，前端不显示）
    prompt: Optional[str]  # Stable Diffusion Prompt
    negative_prompt: Optional[str]  # 负面提示词
    selected_asset_id: Optional[int]  # 选中的资源ID
    status: str  # 状态: draft/in_progress/done
    video_path: Optional[str]  # 视频文件路径
    assets: List[Asset]  # 关联的资源文件
```

#### 文件存储结构
```
data/
└── {项目名称}/
    └── storyboard/
        └── episode_{episode_id}/
            └── scene_{scene_id}/
                └── shot_{shot_id}/
                    ├── assets/     # 镜头资源文件
                    └── video/      # 镜头视频文件
```

### 4. 事件系统

#### 功能描述
事件系统用于标记和追踪剧本中的关键事件，支持多粒度关联（Episode/Scene/Shot）。

#### API 接口
- `GET /events/project/{project_id}` - 获取项目的所有事件
- `POST /events/project/{project_id}` - 创建新事件
- `PATCH /events/{event_id}` - 更新事件信息
- `POST /events/nodes/{event_id}` - 创建或更新事件节点（关联到 Episode/Scene/Shot）
- `GET /events/matrix/{project_id}` - 获取事件矩阵数据（用于矩阵视图）

#### 数据模型
```python
class Event:
    id: int
    project_id: int
    name: str  # 事件名称
    color: str  # 显示颜色（十六进制）
    start_time_sort_key: int  # Y 轴排序键
    description: Optional[str]  # 事件描述
    graph_data: Optional[dict]  # Vue Flow 节点/连线数据
    nodes: List[EventNode]  # 关联的节点列表

class EventNode:
    id: int
    event_id: int
    target_type: str  # "episode" | "scene" | "shot"
    target_id: int  # 目标对象的ID
    description: str  # 节点描述
```

## 前端视图

### 1. 剧本视图 (ScriptView)
- **路径**: `/script`
- **功能**: 
  - 左侧显示剧本树（Episode → Scene）
  - 中间显示当前 Scene 的 Shot 列表
  - 右侧显示选中 Scene 或 Shot 的编辑界面
  - 支持编辑 Scene 的 Action 和 Prompt
  - 支持编辑 Shot 的 Action、Prompt、状态、关联资源
  - 支持上传和预览视频文件

### 2. 事件矩阵视图 (EventMatrixView)
- **路径**: `/events`
- **功能**: 
  - 显示事件列表
  - 创建新事件（带颜色选择）
  - 事件与剧本结构的关联矩阵（开发中）

### 3. 事件流程图视图 (EventFlowView)
- **路径**: `/events/flow`
- **功能**: 
  - 使用 Vue Flow 显示事件流程图（开发中）

### 4. 资产库视图 (AssetLibraryView)
- **路径**: `/assets`
- **功能**: 
  - 按分类显示资产条目
  - 上传和管理资源文件
  - 支持分类筛选

## 桌面应用特性

### Tauri 集成
- **自动后端管理**: Tauri 应用启动时自动启动 FastAPI 后端进程
- **动态端口选择**: 自动选择可用端口，避免冲突
- **数据目录**: 使用系统应用数据目录存储数据库和文件
  - macOS: `~/Library/Application Support/com.ljc.aicomictool/`
- **进程管理**: 应用关闭时自动终止后端进程

### 环境变量
后端通过环境变量配置：
- `AICOMIC_DB_PATH`: 数据库文件路径
- `AICOMIC_DATA_DIR`: 数据文件存储目录
- `PYTHONPATH`: Python 模块搜索路径

## 数据库迁移

项目使用轻量级 SQLite 迁移机制（无 Alembic）：
- `ensure_characters_category_column()`: 为 `characters` 表添加 `category` 列
- `ensure_episode_scene_description_columns()`: 为 `episodes` 和 `scenes` 表添加描述和编辑字段

迁移在应用启动时自动执行，确保数据库结构与代码模型一致。

## API 文档

### 基础 URL
- **开发环境**: `http://localhost:8000`
- **桌面应用**: 动态端口（由 Tauri 注入到前端）

### 通用响应格式
- **成功**: 返回对应的数据模型对象
- **错误**: 返回 `{"detail": "错误信息"}`，HTTP 状态码 400/404/500

### CORS
后端配置了 CORS 中间件，允许所有来源的请求（开发环境）。

## 开发指南

### 启动开发环境

#### 后端
```bash
cd backend
uvicorn app.main:app --reload --host 0.0.0.0 --port 8000
```

#### 前端
```bash
cd frontend
npm install
npm run dev
```

#### 桌面应用
```bash
cd frontend
npm run tauri dev
```

### 构建桌面应用
```bash
cd frontend
npm run tauri build
```

### 依赖安装

#### Python 依赖
```bash
pip install -r requirements.txt
```

#### Node.js 依赖
```bash
cd frontend
npm install
```

#### Rust 依赖
Tauri 会自动管理 Rust 依赖，首次构建时会自动下载。

## 数据存储

### 数据库
- **文件**: SQLite 数据库文件
- **位置**: 
  - 开发环境: 项目根目录 `database.db`
  - 桌面应用: `{应用数据目录}/database.db`

### 文件存储
- **根目录**: 
  - 开发环境: `./data/`
  - 桌面应用: `{应用数据目录}/data/`
- **结构**: 按项目名称和分类组织

## 注意事项

1. **文件路径**: 所有文件路径在数据库中存储为相对路径（相对于 `DATA_DIR`）
2. **级联删除**: 删除项目/集/场/镜头时会自动删除关联的文件和数据库记录
3. **端口冲突**: 桌面应用会自动选择可用端口，开发环境需手动指定
4. **Python 环境**: 确保系统 PATH 中包含 `python3` 或 `python` 命令
5. **依赖安装**: 桌面应用需要系统级安装 Python 依赖（`pip install -r requirements.txt`）

## 未来规划

- [ ] 事件矩阵视图完整实现
- [ ] 事件流程图视图完整实现
- [ ] 资产库分类筛选和搜索
- [ ] 批量上传功能
- [ ] 导出/导入项目功能
- [ ] 多平台支持（Windows、Linux）

## 许可证

（待补充）

