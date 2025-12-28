# API 接口文档

## 基础信息

- **Base URL**: `http://127.0.0.1:{port}` (端口由 Tauri 动态分配)
- **Content-Type**: `application/json`
- **文件上传**: `multipart/form-data`

## 项目管理 API

### 获取所有项目
```http
GET /projects/
```

**响应**:
```json
[
  {
    "id": 1,
    "name": "项目名称",
    "description": "项目描述"
  }
]
```

### 创建项目
```http
POST /projects/
Content-Type: application/json

{
  "name": "项目名称",
  "description": "项目描述（可选）"
}
```

**响应**: 创建的项目对象

### 更新项目
```http
PATCH /projects/{project_id}
Content-Type: application/json

{
  "name": "新名称（可选）",
  "description": "新描述（可选）"
}
```

### 删除项目
```http
DELETE /projects/{project_id}
```

**响应**:
```json
{
  "message": "Project '{name}' and all associated data deleted successfully"
}
```

## 资产条目 API

### 获取资产条目列表
```http
GET /projects/{project_id}/asset-items?category={category}
```

**查询参数**:
- `category` (可选): 资产分类，如 `persona_visual`, `background` 等

**响应**:
```json
[
  {
    "id": 1,
    "name": "资产名称",
    "description": "描述",
    "base_prompt": "基础提示词",
    "category": "persona_visual",
    "avatar_asset_id": 123,
    "assets": [
      {
        "id": 123,
        "file_path": "项目名/characters/xxx.png",
        "file_type": "image",
        "meta_data": {"width": 1920, "height": 1080},
        "is_favorite": true,
        "created_at": "2024-01-01T00:00:00"
      }
    ]
  }
]
```

### 创建资产条目
```http
POST /projects/{project_id}/asset-items
Content-Type: application/json

{
  "name": "资产名称",
  "description": "描述（可选）",
  "base_prompt": "基础提示词（可选）",
  "category": "persona_visual"
}
```

### 更新资产条目
```http
PATCH /projects/asset-items/{item_id}
Content-Type: application/json

{
  "name": "新名称（可选）",
  "description": "新描述（可选）",
  "base_prompt": "新提示词（可选）",
  "category": "新分类（可选）",
  "avatar_path": "文件路径（可选）"
}
```

### 删除资产条目
```http
DELETE /projects/asset-items/{item_id}
```

## 资源文件 API

### 为资产条目上传资源
```http
POST /assets/asset-item/{item_id}
Content-Type: multipart/form-data

file: [文件]
```

**支持的文件类型**:
- 图片: `.jpg`, `.jpeg`, `.png`, `.webp`, `.gif`
- 视频: `.mp4`, `.mov`, `.avi`, `.webm`
- 音频: `.mp3`, `.wav`, `.m4a`, `.aac`, `.flac`, `.ogg`
- 文档: `.txt`, `.md`, `.json`, `.pdf`, `.doc`, `.docx`

**响应**: Asset 对象

### 为镜头上传资源
```http
POST /assets/shot/{shot_id}/upload
Content-Type: multipart/form-data

file: [文件]
```

### 删除资源文件
```http
DELETE /assets/{asset_id}
```

## 剧本管理 API

### 获取完整剧本结构
```http
GET /storyboard/project/{project_id}
```

**响应**:
```json
[
  {
    "id": 1,
    "title": "第一集",
    "order": 1,
    "description": "集描述",
    "scenes": [
      {
        "id": 1,
        "title": "场景标题",
        "sequence_number": 1,
        "description": "场景描述",
        "action_text": "画面描述",
        "prompt": "Stable Diffusion Prompt",
        "shots": [
          {
            "id": 1,
            "sequence_number": 1,
            "title": "镜头标题",
            "action_text": "画面描述",
            "prompt": "Prompt",
            "status": "draft",
            "assets": [],
            "video_path": null
          }
        ]
      }
    ]
  }
]
```

### Episode（集）管理

#### 创建集
```http
POST /storyboard/project/{project_id}/episode
Content-Type: application/json

{
  "title": "集标题",
  "order": 1,
  "description": "集描述（可选）"
}
```

#### 更新集
```http
PATCH /storyboard/episode/{episode_id}
Content-Type: application/json

{
  "title": "新标题（可选）",
  "description": "新描述（可选）"
}
```

#### 删除集
```http
DELETE /storyboard/episode/{episode_id}
```

### Scene（场）管理

#### 创建场
```http
POST /storyboard/episode/{episode_id}/scene
Content-Type: application/json

{
  "title": "场标题",
  "sequence_number": 1,
  "description": "场描述（可选）",
  "action_text": "画面描述（可选）",
  "prompt": "Prompt（可选）"
}
```

#### 更新场
```http
PATCH /storyboard/scene/{scene_id}
Content-Type: application/json

{
  "title": "新标题（可选）",
  "description": "新描述（可选）",
  "action_text": "新画面描述（可选）",
  "prompt": "新Prompt（可选）"
}
```

#### 删除场
```http
DELETE /storyboard/scene/{scene_id}
```

### Shot（镜头）管理

#### 创建镜头
```http
POST /storyboard/scene/{scene_id}/shot
Content-Type: application/json

{
  "sequence_number": 1,
  "title": "镜头标题（可选）",
  "action_text": "画面描述（可选）",
  "prompt": "Prompt（可选）",
  "status": "draft"
}
```

#### 更新镜头
```http
PATCH /storyboard/shot/{shot_id}
Content-Type: application/json

{
  "title": "新标题（可选）",
  "action_text": "新画面描述（可选）",
  "prompt": "新Prompt（可选）",
  "status": "in_progress",
  "selected_asset_id": 123
}
```

#### 删除镜头
```http
DELETE /storyboard/shot/{shot_id}
```

### 视频上传

#### 上传镜头视频
```http
POST /storyboard/shot/{shot_id}/video
Content-Type: multipart/form-data

file: [视频文件]
```

**响应**: 更新后的 Shot 对象（包含 `video_path`）

## 事件系统 API

### 获取项目事件列表
```http
GET /events/project/{project_id}
```

**响应**:
```json
[
  {
    "id": 1,
    "name": "事件名称",
    "color": "#3B82F6",
    "description": "事件描述",
    "graph_data": null,
    "nodes": [
      {
        "id": 1,
        "target_type": "scene",
        "target_id": 1,
        "description": "节点描述"
      }
    ]
  }
]
```

### 创建事件
```http
POST /events/project/{project_id}
Content-Type: application/json

{
  "name": "事件名称",
  "color": "#3B82F6",
  "description": "事件描述（可选）"
}
```

### 更新事件
```http
PATCH /events/{event_id}
Content-Type: application/json

{
  "name": "新名称（可选）",
  "color": "#FF0000（可选）",
  "description": "新描述（可选）",
  "graph_data": {...}（可选）
}
```

### 创建/更新事件节点
```http
POST /events/nodes/{event_id}
Content-Type: application/json

{
  "target_type": "scene",
  "target_id": 1,
  "description": "节点描述"
}
```

**target_type 可选值**: `"episode"`, `"scene"`, `"shot"`

### 获取事件矩阵数据
```http
GET /events/matrix/{project_id}
```

**响应**:
```json
{
  "events": [
    {
      "id": 1,
      "name": "事件名称",
      "color": "#3B82F6"
    }
  ],
  "nodes": [
    {
      "id": 1,
      "event_id": 1,
      "target_type": "scene",
      "target_id": 1,
      "description": "节点描述"
    }
  ]
}
```

## 文件访问

### 静态文件服务
所有上传的文件可通过以下 URL 访问：

```
GET /files/{相对路径}
```

例如：
```
GET /files/项目名/characters/image.png
GET /files/项目名/storyboard/episode_1/scene_1/shot_1/assets/video.mp4
```

## 错误响应

所有错误响应格式：
```json
{
  "detail": "错误信息描述"
}
```

**常见 HTTP 状态码**:
- `200`: 成功
- `400`: 请求参数错误
- `404`: 资源不存在
- `500`: 服务器内部错误

## 兼容性接口

为了向后兼容，以下旧接口仍然可用，但建议使用新接口：

- `GET /projects/{project_id}/characters` → 使用 `GET /projects/{project_id}/asset-items`
- `POST /projects/{project_id}/characters` → 使用 `POST /projects/{project_id}/asset-items`
- `POST /assets/character/{char_id}` → 使用 `POST /assets/asset-item/{item_id}`

