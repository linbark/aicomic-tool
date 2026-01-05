# LLM Provider 配置指南

## 概述

从 P3 开始，LLM Provider 配置已从环境变量迁移到数据库配置系统，支持前端界面配置和管理多个 Provider。

## API 端点

### 1. 获取所有 Provider 配置

```http
GET /api/v1/config/providers
```

响应：
```json
[
  {
    "id": 1,
    "provider_name": "openai",
    "is_active": true,
    "config_data": {
      "api_key": "sk-...",
      "model": "gpt-4o-mini",
      "base_url": "https://api.openai.com/v1"
    },
    "notes": "OpenAI GPT-4 Vision",
    "created_at": "2024-01-01T00:00:00",
    "updated_at": "2024-01-01T00:00:00"
  }
]
```

### 2. 获取当前激活的 Provider

```http
GET /api/v1/config/providers/active
```

### 3. 获取指定 Provider 配置

```http
GET /api/v1/config/providers/{provider_name}
```

### 4. 创建 Provider 配置

```http
POST /api/v1/config/providers
Content-Type: application/json

{
  "provider_name": "openai",
  "is_active": true,
  "config_data": {
    "api_key": "sk-your-key-here",
    "model": "gpt-4o-mini",
    "base_url": "https://api.openai.com/v1"
  },
  "notes": "OpenAI GPT-4 Vision Provider"
}
```

### 5. 更新 Provider 配置

```http
PUT /api/v1/config/providers/{provider_name}
Content-Type: application/json

{
  "is_active": true,
  "config_data": {
    "api_key": "sk-new-key-here",
    "model": "gpt-4o"
  },
  "notes": "Updated config"
}
```

### 6. 激活 Provider

```http
POST /api/v1/config/providers/{provider_name}/activate
```

这会自动取消其他 Provider 的激活状态，并激活指定的 Provider。

### 7. 删除 Provider 配置

```http
DELETE /api/v1/config/providers/{provider_name}
```

## Provider 配置格式

### OpenAI Vision Provider

```json
{
  "provider_name": "openai",
  "is_active": true,
  "config_data": {
    "api_key": "sk-...",
    "model": "gpt-4o-mini",
    "base_url": "https://api.openai.com/v1"
  }
}
```

**字段说明：**
- `api_key` (必需): OpenAI API Key
- `model` (可选): 模型名称，默认 `gpt-4o-mini`
- `base_url` (可选): API Base URL，默认 `https://api.openai.com/v1`

### 未来支持的 Provider

系统设计支持扩展，后续可添加：
- Claude (Anthropic)
- Gemini (Google)
- 其他自定义 Provider

## 前端集成示例

### Vue.js 示例

```javascript
// 获取所有配置
async function getProviders() {
  const response = await fetch('/api/v1/config/providers');
  return await response.json();
}

// 创建 OpenAI 配置
async function createOpenAIConfig(apiKey, model = 'gpt-4o-mini') {
  const response = await fetch('/api/v1/config/providers', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      provider_name: 'openai',
      is_active: true,
      config_data: {
        api_key: apiKey,
        model: model,
        base_url: 'https://api.openai.com/v1'
      },
      notes: 'OpenAI GPT-4 Vision'
    })
  });
  return await response.json();
}

// 激活 Provider
async function activateProvider(providerName) {
  const response = await fetch(`/api/v1/config/providers/${providerName}/activate`, {
    method: 'POST'
  });
  return await response.json();
}
```

## 降级机制

如果没有激活的 Provider 配置，系统会自动降级到 `LocalRuleProvider`，确保工作流可以正常运行（但不会调用真实的 LLM API）。

## 数据库迁移

配置表会在应用启动时自动创建（通过 SQLAlchemy `Base.metadata.create_all`）。

如果需要手动迁移，可以运行：

```python
from backend.app.database import engine, Base
from backend.app import models

Base.metadata.create_all(bind=engine)
```

## 安全注意事项

1. **API Key 存储**：API Key 存储在数据库中，请确保数据库访问权限受到保护
2. **前端显示**：建议在前端显示 API Key 时进行部分掩码（如 `sk-...****`）
3. **HTTPS**：生产环境请使用 HTTPS 传输配置数据

