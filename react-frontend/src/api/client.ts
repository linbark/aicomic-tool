import axios from 'axios'
import type {
  AiSettingsRead,
  AiSettingsUpdate,
  AiTestResponse,
  AssetItemRead,
  AiActionRunCreate,
  AiActionRunRead,
  ApplyScriptWorkflowRequest,
  ApplyStoryboardWorkflowRequest,
  EpisodeRead,
  EventCreate,
  EventRead,
  EventUpdate,
  OutlineOptimizeRequest,
  OutlineOptimizeResponse,
  PromptTemplateCreate,
  PromptTemplateRead,
  PromptTemplateUpsert,
  ProjectCreate,
  ProjectBase,
  ProjectUpdate,
  ScriptGenerateRequest,
  ScriptGenerateResponse,
  SplitSceneItem,
  SplitShotItem,
  WorkflowScriptRequest,
  WorkflowScriptResponse,
  WorkflowStoryboardRequest,
  WorkflowStoryboardResponse,
} from './types'

let _apiBaseUrl =
  (typeof window !== 'undefined' && window.__AICOMIC_API_BASE_URL__) ||
  (import.meta.env?.VITE_API_BASE_URL as string | undefined) ||
  'http://localhost:8000'

export function setApiBaseUrl(url: string) {
  const next = (url || '').trim()
  if (!next) return
  _apiBaseUrl = next
  apiClient.defaults.baseURL = next
}

export function getApiBaseUrl() {
  return _apiBaseUrl
}

export function getFileUrl(path: string) {
  if (!path) return ''
  return `${getApiBaseUrl()}/files/${path}`
}

const apiClient = axios.create({
  baseURL: _apiBaseUrl,
  headers: {
    'Content-Type': 'application/json',
  },
})

// Tauri 启动后会通过 window 事件注入端口（见 `frontend/src-tauri/src/main.rs`）
if (typeof window !== 'undefined') {
  window.addEventListener('aicomic-api-base-url', (e: Event) => {
    const ce = e as CustomEvent<string>
    if (ce?.detail) setApiBaseUrl(ce.detail)
  })

  // 立即检查是否已有注入的 baseURL（Tauri 可能在页面加载前就注入了）
  if (window.__AICOMIC_API_BASE_URL__) {
    setApiBaseUrl(window.__AICOMIC_API_BASE_URL__)
  } else {
    // 轮询检查 baseURL 是否被注入（Tauri 可能在页面加载后才注入）
    let pollCount = 0
    const pollInterval = window.setInterval(() => {
      pollCount++
      if (window.__AICOMIC_API_BASE_URL__) {
        setApiBaseUrl(window.__AICOMIC_API_BASE_URL__)
        window.clearInterval(pollInterval)
      } else if (pollCount >= 50) {
        window.clearInterval(pollInterval)
      }
    }, 100)
  }
}

export const api = {
  // 项目
  getProjects: () => apiClient.get<ProjectBase[]>('/projects/'),
  createProject: (data: ProjectCreate) => apiClient.post<ProjectBase>('/projects/', data),
  updateProject: (projectId: number, data: ProjectUpdate) => apiClient.patch<ProjectBase>(`/projects/${projectId}`, data),
  deleteProject: (projectId: number) => apiClient.delete(`/projects/${projectId}`),

  // 剧本 (Episode/Scene/Shot)
  getScript: (projectId: number) => apiClient.get<EpisodeRead[]>(`/storyboard/project/${projectId}`),
  createEpisode: (projectId: number, data: unknown) => apiClient.post(`/storyboard/project/${projectId}/episode`, data),
  updateEpisode: (episodeId: number, data: unknown) => apiClient.patch(`/storyboard/episode/${episodeId}`, data),
  createScene: (episodeId: number, data: unknown) => apiClient.post(`/storyboard/episode/${episodeId}/scene`, data),
  createShot: (sceneId: number, data: unknown) => apiClient.post(`/storyboard/scene/${sceneId}/shot`, data),
  updateShot: (shotId: number, data: unknown) => apiClient.patch(`/storyboard/shot/${shotId}`, data),
  updateScene: (sceneId: number, data: unknown) => apiClient.patch(`/storyboard/scene/${sceneId}`, data),

  deleteEpisode: (id: number) => apiClient.delete(`/storyboard/episode/${id}`),
  deleteScene: (id: number) => apiClient.delete(`/storyboard/scene/${id}`),
  deleteShot: (id: number) => apiClient.delete(`/storyboard/shot/${id}`),

  // 资产
  addAsset: (shotId: number, filePath: string) => apiClient.post(`/assets/shot/${shotId}?file_path=${filePath}`),

  uploadAssetItemAsset: (itemId: number, formData: FormData) =>
    apiClient.post(`/assets/asset-item/${itemId}`, formData, {
      headers: { 'Content-Type': 'multipart/form-data' },
    }),

  uploadShotAsset: (shotId: number, formData: FormData) =>
    apiClient.post(`/assets/shot/${shotId}/upload`, formData, {
      headers: { 'Content-Type': 'multipart/form-data' },
    }),

  uploadShotVideo: (shotId: number, formData: FormData) =>
    apiClient.post(`/assets/shot/${shotId}/video`, formData, {
      headers: { 'Content-Type': 'multipart/form-data' },
    }),

  // 事件
  getEvents: (projectId: number) => apiClient.get<EventRead[]>(`/events/project/${projectId}`),
  createEvent: (projectId: number, data: EventCreate) => apiClient.post<EventRead>(`/events/project/${projectId}`, data),
  updateEvent: (eventId: number, data: EventUpdate) => apiClient.patch<EventRead>(`/events/${eventId}`, data),
  deleteEvent: (eventId: number) => apiClient.delete(`/events/${eventId}`),
  upsertEventNode: (eventId: number, data: { target_type: 'scene' | 'episode' | 'shot'; target_id: number; description: string }) =>
    apiClient.post(`/events/nodes/${eventId}`, data),

  // 人设/资产条目（项目级）
  getAssetItems: (projectId: number, category?: string) => {
    const qs = category ? `?category=${encodeURIComponent(category)}` : ''
    return apiClient.get<AssetItemRead[]>(`/projects/${projectId}/asset-items${qs}`)
  },
  createAssetItem: (projectId: number, data: unknown) => apiClient.post(`/projects/${projectId}/asset-items`, data),
  updateAssetItem: (itemId: number, data: unknown) => apiClient.patch(`/projects/asset-items/${itemId}`, data),
  deleteAssetItem: (itemId: number) => apiClient.delete(`/projects/asset-items/${itemId}`),
  getCharacters: (projectId: number) => apiClient.get(`/projects/${projectId}/characters`),

  // AI
  getAiSettings: () => apiClient.get<AiSettingsRead>(`/ai/settings`),
  updateAiSettings: (data: AiSettingsUpdate) => apiClient.put<AiSettingsRead>(`/ai/settings`, data),
  testAi: () => apiClient.post<AiTestResponse>(`/ai/test`),
  aiSplitScenes: (data: { text: string; max_scenes?: number }) => apiClient.post<SplitSceneItem[]>(`/ai/split-scenes`, data),
  aiSplitShots: (data: { text: string; max_shots?: number }) => apiClient.post<SplitShotItem[]>(`/ai/split-shots`, data),
  aiOutlineGenerate: (data: { text: string }) => apiClient.post<{ text: string }>(`/ai/outline-generate`, data),
  aiOutlineOptimize: (data: OutlineOptimizeRequest) => apiClient.post<OutlineOptimizeResponse>(`/ai/outline-optimize`, data),
  aiGenerateScript: (data: ScriptGenerateRequest) => apiClient.post<ScriptGenerateResponse>(`/ai/generate-script`, data),
  aiScriptOptimize: (data: { text: string }) => apiClient.post<{ text: string }>(`/ai/script-optimize`, data),
  aiWorkflowScript: (data: WorkflowScriptRequest) => apiClient.post<WorkflowScriptResponse>(`/ai/workflows/script`, data),
  aiWorkflowStoryboard: (data: WorkflowStoryboardRequest) => apiClient.post<WorkflowStoryboardResponse>(`/ai/workflows/storyboard`, data),
  aiApplyWorkflowScript: (data: ApplyScriptWorkflowRequest) => apiClient.post(`/ai/workflows/script/apply`, data),
  aiApplyWorkflowStoryboard: (data: ApplyStoryboardWorkflowRequest) => apiClient.post(`/ai/workflows/storyboard/apply`, data),
  getPromptTemplates: () => apiClient.get<PromptTemplateRead[]>(`/ai/prompts`),
  createPromptTemplate: (data: PromptTemplateCreate) => apiClient.post<PromptTemplateRead>(`/ai/prompts`, data),
  upsertPromptTemplate: (key: string, data: PromptTemplateUpsert) => apiClient.put<PromptTemplateRead>(`/ai/prompts/${encodeURIComponent(key)}`, data),
  resetPromptTemplate: (key: string) => apiClient.post<PromptTemplateRead>(`/ai/prompts/${encodeURIComponent(key)}/reset`),
  deletePromptTemplate: (key: string) => apiClient.delete(`/ai/prompts/${encodeURIComponent(key)}`),

  // AI Runs（按钮输出历史）
  getAiRuns: (params: { project_id: number; episode_id?: number; action_key?: string; limit?: number }) => {
    const qs = new URLSearchParams()
    qs.set('project_id', String(params.project_id))
    if (typeof params.episode_id === 'number') qs.set('episode_id', String(params.episode_id))
    if (params.action_key) qs.set('action_key', params.action_key)
    if (typeof params.limit === 'number') qs.set('limit', String(params.limit))
    return apiClient.get<AiActionRunRead[]>(`/ai/runs?${qs.toString()}`)
  },
  createAiRun: (data: AiActionRunCreate) => apiClient.post<AiActionRunRead>(`/ai/runs`, data),

  // 资产文件删除（Asset 表）
  deleteProjectAsset: (assetId: number) => apiClient.delete(`/projects/assets/${assetId}`),
}

export default api


