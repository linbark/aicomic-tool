# AI Comic Tool (星晴)

AI 漫剧创作工具 - 集成记忆系统的智能剧本生成与分镜工具。

## 项目结构

```
aicomic-tool/
├── backend/           # FastAPI 后端
├── react-frontend/    # React + Tauri 前端
│   ├── src/           # React 源码
│   └── src-tauri/     # Tauri 桌面打包配置
├── docs/              # 文档
└── requirements.txt   # Python 依赖
```

## 开发运行

### 后端（FastAPI）

```bash
cd backend
uvicorn backend.app.main:app --reload --host 0.0.0.0 --port 8000
```

### Web 前端（React / Vite）

```bash
cd react-frontend
npm install
npm run dev
```

### 桌面端（Tauri + React）

```bash
cd react-frontend
npm install

# 如需指定 Python（用于桌面端内置后端）
export AICOMIC_PYTHON="/absolute/path/to/python3"

npm run tauri:dev
```

## 构建

### Web 构建

```bash
cd react-frontend
npm run build
```

### 桌面应用构建

```bash
cd react-frontend
npm run tauri:build
```

构建产物位于 `react-frontend/src-tauri/target/release/bundle/`。

## 技术栈

- **后端**: FastAPI + SQLite + Qdrant (向量数据库)
- **前端**: React 19 + TypeScript + Vite
- **桌面**: Tauri (Rust)
- **AI**: DeepSeek API + BGE-M3 (Embedding)
