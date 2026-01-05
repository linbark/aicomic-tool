# AI Comic Tool - 启动指南

## 项目结构

- `backend/` - FastAPI 后端服务
- `frontend/` - Vue.js 前端应用
- `data/` - 图片资源目录
- `database.db` - SQLite 数据库（自动创建）

## 环境要求

### 基础要求（Web 模式）

- Python 3.8+
- Node.js 16+
- npm 或 yarn

### Tauri 模式额外要求

- Rust 工具链（用于构建桌面应用）
  ```bash
  # 安装 Rust
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  
  # macOS 也可以使用 Homebrew
  brew install rust
  ```

## 快速启动

### 方式一：Web 模式（开发）

#### 1. 安装后端依赖

```bash
pip install -r requirements.txt
```

**可选：安装 OpenAI SDK（如果使用 OpenAI Vision Provider）**

```bash
pip install openai
```

#### 2. 安装前端依赖

```bash
cd frontend
npm install
```

#### 3. 启动后端服务

在项目根目录运行：

```bash
uvicorn backend.app.main:app --reload --host 0.0.0.0 --port 8000
```

后端服务将在 `http://localhost:8000` 启动

#### 4. 启动前端服务

在 `frontend/` 目录运行：

```bash
npm run dev
```

前端服务通常会在 `http://localhost:5173` 启动（Vite 默认端口）

### 方式二：Tauri 桌面应用

#### 前置要求

- Rust 工具链（Tauri 需要）
  ```bash
  # macOS/Linux
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  
  # 或使用 Homebrew (macOS)
  brew install rust
  ```

#### 启动步骤

1. **安装依赖**（同上，需要先安装后端和前端依赖）

2. **启动后端服务**（必须，Tauri 应用需要后端 API）

   在项目根目录运行：
   ```bash
   uvicorn backend.app.main:app --reload --host 127.0.0.1 --port 8000
   ```

3. **启动 Tauri 应用**

   在 `frontend/` 目录运行：
   ```bash
   npm run tauri:dev
   ```

   这会：
   - 自动启动 Vite 开发服务器（`http://127.0.0.1:1420`）
   - 打开 Tauri 桌面窗口
   - 支持热重载

#### 构建 Tauri 应用

```bash
cd frontend
npm run tauri:build
```

构建产物会在 `frontend/src-tauri/target/release/` 目录下（根据平台不同，可能是 `.app`、`.exe` 或 `.AppImage`）

## 配置 LLM Provider（可选）

### 方式一：通过 API 配置

```bash
# 创建 OpenAI 配置
curl -X POST http://localhost:8000/api/v1/config/providers \
  -H "Content-Type: application/json" \
  -d '{
    "provider_name": "openai",
    "is_active": true,
    "config_data": {
      "api_key": "sk-your-key-here",
      "model": "gpt-4o-mini"
    },
    "notes": "OpenAI GPT-4 Vision"
  }'
```

### 方式二：通过前端界面配置

访问前端应用，在设置页面配置 LLM Provider。

## 验证安装

### 检查后端服务

访问 `http://localhost:8000/docs` 查看 API 文档

访问 `http://localhost:8000/` 应该返回：
```json
{"message": "Server is running"}
```

### 检查数据库

数据库文件 `database.db` 会在首次启动时自动创建。

## 常用命令

### 后端

```bash
# 开发模式（热重载）
uvicorn backend.app.main:app --reload --host 0.0.0.0 --port 8000

# 生产模式
uvicorn backend.app.main:app --host 0.0.0.0 --port 8000

# 运行测试
python -m backend.app.devtools.test_workflow
```

### 前端

```bash
# Web 开发模式
npm run dev

# 构建生产版本
npm run build

# 预览构建结果
npm run preview

# Tauri 开发模式（需要先启动后端服务）
npm run tauri:dev

# Tauri 构建桌面应用
npm run tauri:build
```

## 环境变量（可选）

```bash
# 数据目录（默认：项目根目录下的 data/）
export AICOMIC_DATA_DIR=/path/to/data
```

## 故障排查

### 后端启动失败

1. 检查 Python 版本：`python --version`（需要 3.8+）
2. 检查依赖是否安装：`pip list | grep fastapi`
3. 检查端口是否被占用：`lsof -i :8000`

### 前端启动失败

1. 检查 Node.js 版本：`node --version`（需要 16+）
2. 删除 `node_modules` 重新安装：`rm -rf node_modules && npm install`
3. 检查端口是否被占用：`lsof -i :5173`

### 数据库问题

如果数据库表结构有问题，可以删除 `database.db` 文件，重启后端服务会自动重建。

## API 文档

启动后端后，访问以下地址查看 API 文档：

- Swagger UI: `http://localhost:8000/docs`
- ReDoc: `http://localhost:8000/redoc`

## 项目功能

- ✅ 多 Agent 工作流编排（Manju Workflow）
- ✅ Visual Asset 图片反推（支持 OpenAI Vision）
- ✅ 剧本生成（Fountain Script）
- ✅ 分镜生成（Storyboard）
- ✅ Prompt 生成（支持 Midjourney/SD/Flux）
- ✅ QC 检查与自动修复（Refinement Loop）
- ✅ LLM Provider 配置管理（前端可配置）
- ✅ Tauri 桌面应用支持

详细功能说明请参考 `docs/` 目录下的文档。

## 注意事项

### Tauri 模式

- Tauri 应用需要后端服务在 `http://127.0.0.1:8000` 运行
- 开发模式下，Tauri 会自动启动 Vite 开发服务器（端口 1420）
- 确保防火墙允许本地连接
- 如果后端服务未启动，前端会显示连接错误

### 端口说明

- **后端 API**: `8000` (Web 模式) 或 `127.0.0.1:8000` (Tauri 模式)
- **前端 Web**: `5173` (Vite 默认)
- **Tauri Dev**: `1420` (Tauri 开发服务器)