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

> Tauri 壳在 `frontend/src-tauri/`，但 UI 已切换到 `react-frontend/`。

```bash
cd frontend
npm install

# 如需指定 Python（用于桌面端内置后端）
export AICOMIC_PYTHON="/absolute/path/to/python3"

npm run tauri:dev
```

## 构建

```bash
cd frontend
npm run tauri:build
```

## 迁移说明

- 旧 Vue 前端仍保留在 `frontend/src/`，作为迁移期间的回退与参考。
- 新 React 前端在 `react-frontend/`，后续页面会逐步迁移替换。