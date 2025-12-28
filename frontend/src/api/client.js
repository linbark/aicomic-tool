import axios from 'axios';

let _apiBaseUrl =
  (typeof window !== 'undefined' && window.__AICOMIC_API_BASE_URL__) ||
  (import.meta?.env?.VITE_API_BASE_URL) ||
  'http://localhost:8000';

export function setApiBaseUrl(url) {
  if (!url) return;
  _apiBaseUrl = url;
  apiClient.defaults.baseURL = url;
}

export function getApiBaseUrl() {
  return _apiBaseUrl;
}

export function getFileUrl(path) {
  if (!path) return '';
  return `${getApiBaseUrl()}/files/${path}`;
}

// 初始化 axios 实例（baseURL 可在运行时被 setApiBaseUrl 更新）
const apiClient = axios.create({
  baseURL: _apiBaseUrl,
  headers: {
    'Content-Type': 'application/json',
  },
});

// Tauri 启动后会通过 window 事件注入端口
if (typeof window !== 'undefined') {
  window.addEventListener('aicomic-api-base-url', (e) => {
    const url = e?.detail;
    setApiBaseUrl(url);
  });
  if (window.__AICOMIC_API_BASE_URL__) {
    setApiBaseUrl(window.__AICOMIC_API_BASE_URL__);
  }
}

export default {
  // 项目
  getProjects: () => apiClient.get('/projects/'),
  createProject: (data) => apiClient.post('/projects/', data),
  updateProject: (id, data) => apiClient.patch(`/projects/${id}`, data),
  
  // 剧本 (Episode/Scene/Shot)
  getScript: (projectId) => apiClient.get(`/storyboard/project/${projectId}`),
  createEpisode: (projectId, data) => apiClient.post(`/storyboard/project/${projectId}/episode`, data),
  createScene: (episodeId, data) => apiClient.post(`/storyboard/episode/${episodeId}/scene`, data),
  createShot: (sceneId, data) => apiClient.post(`/storyboard/scene/${sceneId}/shot`, data),
  updateShot: (shotId, data) => apiClient.patch(`/storyboard/shot/${shotId}`, data),
  updateScene: (sceneId, data) => apiClient.patch(`/storyboard/scene/${sceneId}`, data),
  
  // 资产
  addAsset: (shotId, filePath) => apiClient.post(`/assets/shot/${shotId}?file_path=${filePath}`),
  
  // 事件
  getEvents: (projectId) => apiClient.get(`/events/project/${projectId}`),
  createEvent: (projectId, data) => apiClient.post(`/events/project/${projectId}`, data),
  updateEvent: (eventId, data) => apiClient.patch(`/events/${eventId}`, data),
  upsertEventNode: (eventId, data) => apiClient.post(`/events/nodes/${eventId}`, data),
  
  // 人设
  // 资产条目（项目级）
  getAssetItems: (projectId, category) => {
    const qs = category ? `?category=${encodeURIComponent(category)}` : '';
    return apiClient.get(`/projects/${projectId}/asset-items${qs}`);
  },
  createAssetItem: (projectId, data) => apiClient.post(`/projects/${projectId}/asset-items`, data),
  updateAssetItem: (itemId, data) => apiClient.patch(`/projects/asset-items/${itemId}`, data),
  deleteAssetItem: (itemId) => apiClient.delete(`/projects/asset-items/${itemId}`),
  // 兼容旧接口（可逐步移除）
  getCharacters: (projectId) => apiClient.get(`/projects/${projectId}/characters`),
  
  // 上传资产条目素材
  uploadAssetItemAsset: (itemId, formData) => {
    return apiClient.post(`/assets/asset-item/${itemId}`, formData, {
      headers: { 'Content-Type': 'multipart/form-data' }
    });
  },
  // 兼容旧接口
  uploadCharacterAsset: (charId, formData) => {
    return apiClient.post(`/assets/character/${charId}`, formData, {
      headers: { 'Content-Type': 'multipart/form-data' }
    });
  },

  // 镜头素材上传
  uploadShotAsset: (shotId, formData) => {
    return apiClient.post(`/assets/shot/${shotId}/upload`, formData, {
      headers: { 'Content-Type': 'multipart/form-data' }
    });
  },
  
  uploadShotVideo(shotId, formData) {
    return apiClient.post(`/assets/shot/${shotId}/video`, formData, {
      headers: {
        'Content-Type': 'multipart/form-data'
      }
    });
  },

  // 新增删除接口
  deleteEpisode(id) {
    return apiClient.delete(`/storyboard/episode/${id}`);
  },
  deleteScene(id) {
    return apiClient.delete(`/storyboard/scene/${id}`);
  },
  deleteShot(id) {
    return apiClient.delete(`/storyboard/shot/${id}`);
  },

  // 资产文件删除（Asset 表）
  deleteProjectAsset(assetId) {
    return apiClient.delete(`/projects/assets/${assetId}`);
  }
};