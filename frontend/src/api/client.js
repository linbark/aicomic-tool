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
    // #region agent log
    fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'client.js:33',message:'baseURL event received',data:{url},timestamp:Date.now(),sessionId:'debug-session',runId:'run2',hypothesisId:'B'})}).catch(()=>{});
    // #endregion
    setApiBaseUrl(url);
  });
  // 立即检查是否已有注入的 baseURL（Tauri 可能在页面加载前就注入了）
  if (window.__AICOMIC_API_BASE_URL__) {
    // #region agent log
    fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'client.js:38',message:'baseURL found on window',data:{url:window.__AICOMIC_API_BASE_URL__},timestamp:Date.now(),sessionId:'debug-session',runId:'run2',hypothesisId:'B'})}).catch(()=>{});
    // #endregion
    setApiBaseUrl(window.__AICOMIC_API_BASE_URL__);
  } else {
    // #region agent log
    fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'client.js:41',message:'baseURL not found, polling for injection',data:{defaultUrl:_apiBaseUrl},timestamp:Date.now(),sessionId:'debug-session',runId:'run2',hypothesisId:'B'})}).catch(()=>{});
    // #endregion
    // 轮询检查 baseURL 是否被注入（Tauri 可能在页面加载后才注入）
    let pollCount = 0;
    const pollInterval = setInterval(() => {
      pollCount++;
      if (window.__AICOMIC_API_BASE_URL__) {
        // #region agent log
        fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'client.js:50',message:'baseURL found via polling',data:{url:window.__AICOMIC_API_BASE_URL__,pollCount},timestamp:Date.now(),sessionId:'debug-session',runId:'run2',hypothesisId:'B'})}).catch(()=>{});
        // #endregion
        setApiBaseUrl(window.__AICOMIC_API_BASE_URL__);
        clearInterval(pollInterval);
      } else if (pollCount >= 50) {
        // 5秒后停止轮询
        // #region agent log
        fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'client.js:56',message:'baseURL polling timeout, using default',data:{defaultUrl:_apiBaseUrl},timestamp:Date.now(),sessionId:'debug-session',runId:'run2',hypothesisId:'B'})}).catch(()=>{});
        // #endregion
        clearInterval(pollInterval);
      }
    }, 100);
  }
}

export default {
  // 项目
  getProjects: () => apiClient.get('/projects/'),
  createProject: (data) => {
    // #region agent log
    fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'client.js:45',message:'createProject API call',data:{data,baseURL:apiClient.defaults.baseURL},timestamp:Date.now(),sessionId:'debug-session',runId:'run1',hypothesisId:'B'})}).catch(()=>{});
    // #endregion
    return apiClient.post('/projects/', data).catch((err) => {
      // #region agent log
      fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'client.js:48',message:'createProject API error',data:{error:err?.message,status:err?.response?.status,data:err?.response?.data},timestamp:Date.now(),sessionId:'debug-session',runId:'run1',hypothesisId:'D'})}).catch(()=>{});
      // #endregion
      throw err;
    });
  },
  updateProject: (projectId, data) => apiClient.patch(`/projects/${projectId}`, data),
  deleteProject: (projectId) => apiClient.delete(`/projects/${projectId}`),
  
  // 剧本 (Episode/Scene/Shot)
  getScript: (projectId) => apiClient.get(`/storyboard/project/${projectId}`),
  createEpisode: (projectId, data) => apiClient.post(`/storyboard/project/${projectId}/episode`, data),
  updateEpisode: (episodeId, data) => apiClient.patch(`/storyboard/episode/${episodeId}`, data),
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