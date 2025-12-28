# 架构设计文档

## 系统架构概览

AI Comic Tool 采用前后端分离的架构，通过 Tauri 框架将 Web 应用打包为桌面应用。

```
┌─────────────────────────────────────────────────────────┐
│                    Tauri Desktop App                    │
│  ┌──────────────────┐         ┌─────────────────────┐  │
│  │   Vue.js Frontend │  ←→     │  FastAPI Backend    │  │
│  │   (Vite + Pinia)  │  HTTP   │  (Python + SQLite)  │  │
│  └──────────────────┘         └─────────────────────┘  │
│         │                              │                │
│         │                              │                │
│         └──────────┬───────────────────┘                │
│                    │                                    │
│         ┌──────────▼──────────┐                       │
│         │  Application Data    │                       │
│         │  Directory           │                       │
│         │  - database.db       │                       │
│         │  - data/              │                       │
│         └──────────────────────┘                       │
└─────────────────────────────────────────────────────────┘
```

## 数据流

### 1. 应用启动流程

```
Tauri App 启动
    ↓
检测可用端口
    ↓
启动 FastAPI 后端进程
    ↓
等待后端就绪（端口监听）
    ↓
注入 API Base URL 到前端
    ↓
前端初始化并连接后端
```

### 2. 数据请求流程

```
用户操作
    ↓
Vue Component
    ↓
Pinia Store (状态管理)
    ↓
API Client (axios)
    ↓
HTTP Request
    ↓
FastAPI Router
    ↓
SQLAlchemy ORM
    ↓
SQLite Database
    ↓
返回响应
    ↓
更新 Pinia Store
    ↓
更新 Vue Component
```

## 数据模型关系

### 核心实体关系图

```
Project (项目)
├── Character (资产条目) [1:N]
│   └── Asset (资源文件) [1:N]
├── Episode (集) [1:N]
│   └── Scene (场) [1:N]
│       └── Shot (镜头) [1:N]
│           ├── Asset (资源文件) [1:N]
│           └── video_path (视频文件)
└── Event (事件) [1:N]
    └── EventNode (事件节点) [1:N]
        └── 多态关联 → Episode/Scene/Shot
```

### 数据库表结构

#### projects
- `id` (PK)
- `name`
- `description`
- `created_at`

#### characters (资产条目)
- `id` (PK)
- `project_id` (FK → projects.id)
- `name`
- `description`
- `base_prompt`
- `category`
- `avatar_asset_id` (FK → assets.id)

#### assets (资源文件)
- `id` (PK)
- `character_id` (FK → characters.id, nullable)
- `shot_id` (FK → shots.id, nullable)
- `file_path`
- `file_type`
- `meta_data` (JSON)
- `is_favorite`
- `created_at`

#### episodes (集)
- `id` (PK)
- `project_id` (FK → projects.id)
- `title`
- `order`
- `description`
- `action_text`
- `prompt`

#### scenes (场)
- `id` (PK)
- `episode_id` (FK → episodes.id)
- `sequence_number`
- `title`
- `description`
- `action_text`
- `dialogue`
- `prompt`

#### shots (镜头)
- `id` (PK)
- `scene_id` (FK → scenes.id)
- `sequence_number`
- `title`
- `action_text`
- `dialogue`
- `prompt`
- `negative_prompt`
- `selected_asset_id` (FK → assets.id)
- `status`
- `video_path`

#### events (事件)
- `id` (PK)
- `project_id` (FK → projects.id)
- `name`
- `color`
- `start_time_sort_key`
- `description`
- `graph_data` (JSON)

#### event_nodes (事件节点)
- `id` (PK)
- `event_id` (FK → events.id)
- `target_type` (episode/scene/shot)
- `target_id`
- `description`

## 前端架构

### 组件层次结构

```
App.vue (根组件)
├── Sidebar (左侧导航)
│   ├── Project Selector
│   ├── ScriptTree (剧本树)
│   └── EventList (事件列表)
└── Router View
    ├── ScriptView (剧本视图)
    │   ├── Scene List (场列表)
    │   ├── Shot List (镜头列表)
    │   └── Editor (编辑面板)
    ├── EventMatrixView (事件矩阵)
    ├── EventFlowView (事件流程图)
    └── AssetLibraryView (资产库)
```

### 状态管理 (Pinia)

```javascript
projectStore {
  state: {
    projects: [],
    currentProjectId: null,
    episodes: [],
    events: [],
    assetItems: [],
    currentScene: null,
    currentShot: null
  },
  actions: {
    init(),
    selectProject(id),
    fetchScript(),
    fetchEvents(),
    fetchAssetItems(category),
    saveShot(shot)
  }
}
```

### 路由配置

```javascript
routes: [
  { path: '/', redirect: '/script' },
  { path: '/script', component: ScriptView },
  { path: '/events', component: EventMatrixView },
  { path: '/events/flow', component: EventFlowView },
  { path: '/assets', component: AssetLibraryView }
]
```

## 后端架构

### 路由模块化

```
main.py
├── routers/projects.py    # 项目和资产条目
├── routers/storyboard.py   # 剧本管理
├── routers/assets.py       # 资源文件上传
└── routers/events.py       # 事件系统
```

### 依赖注入

FastAPI 使用依赖注入模式：
- `get_db()`: 数据库会话依赖
- 所有路由通过 `Depends(get_db)` 获取数据库连接

### 数据库迁移

轻量级迁移机制（无 Alembic）：
- 在 `main.py` 启动时执行迁移函数
- 使用 SQLite `PRAGMA table_info` 检测列是否存在
- 使用 `ALTER TABLE` 添加缺失的列

## 文件存储架构

### 目录结构

```
{应用数据目录}/
├── database.db
└── data/
    └── {项目名称}/
        ├── characters/          # 人设（视觉）
        ├── voices/             # 人设（声音）
        ├── backgrounds/        # 背景
        ├── elements/           # 元素
        ├── props/              # 道具
        ├── poses/              # 姿态
        ├── vfx/                # 特效
        ├── layout/             # 布局
        ├── audio_music/        # 音频（音乐）
        ├── audio_sfx/          # 音频（音效）
        ├── branding/           # 品牌
        ├── ai_preset/          # AI 预设
        └── storyboard/         # 剧本资源
            └── episode_{id}/
                └── scene_{id}/
                    └── shot_{id}/
                        ├── assets/
                        └── video/
```

### 路径管理

- **存储**: 所有文件路径在数据库中存储为相对路径（相对于 `DATA_DIR`）
- **访问**: 通过 FastAPI 静态文件服务 `/files/{相对路径}` 访问
- **删除**: 删除实体时自动删除关联的文件

## Tauri 集成

### 后端进程管理

```rust
spawn_backend() {
  1. 创建应用数据目录
  2. 设置环境变量 (DB_PATH, DATA_DIR, PYTHONPATH)
  3. 启动 Python 进程运行 uvicorn
  4. 返回子进程句柄
}

wait_port_open() {
  轮询检查端口是否开放
}

inject_base_url() {
  通过 window.eval() 注入 API URL
}
```

### 生命周期管理

```
应用启动
  → spawn_backend()
  → wait_port_open()
  → inject_base_url()
  → 前端连接

应用关闭
  → on_window_event(CloseRequested)
  → kill_backend_process()
  → 关闭窗口
```

## 安全考虑

### 当前实现

1. **CORS**: 开发环境允许所有来源（生产环境需限制）
2. **文件上传**: 无文件类型严格验证（建议添加）
3. **路径遍历**: 文件路径拼接需注意防止路径遍历攻击
4. **SQL 注入**: 使用 SQLAlchemy ORM 自动防护

### 建议改进

1. 添加文件类型和大小验证
2. 添加用户认证和授权
3. 限制 CORS 来源
4. 添加请求速率限制
5. 文件路径验证和清理

## 性能优化

### 已实现

1. **数据库查询优化**: 使用 `joinedload` 预加载关联数据
2. **前端状态缓存**: Pinia Store 缓存数据，减少 API 调用
3. **文件服务**: 使用 FastAPI 静态文件服务，避免重复读取

### 可优化点

1. 添加数据库索引（特别是外键字段）
2. 实现分页查询（大量数据时）
3. 添加前端虚拟滚动（长列表）
4. 实现增量更新（只更新变化的部分）
5. 添加文件缓存策略

## 扩展性设计

### 模块化设计

- 路由模块化，易于添加新功能
- 数据模型清晰，易于扩展字段
- 前端组件化，易于复用

### 可扩展点

1. **插件系统**: 可添加自定义资产分类
2. **导出功能**: 支持导出为不同格式
3. **协作功能**: 多用户支持（需要认证系统）
4. **云同步**: 数据备份和同步
5. **AI 集成**: 自动生成 Prompt、图片等

