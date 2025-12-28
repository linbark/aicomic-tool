import { defineStore } from 'pinia';
import api from '../api/client';

export const useProjectStore = defineStore('project', {
  state: () => ({
    projects: [],
    currentProjectId: null,
    
    // 数据缓存
    episodes: [], // 剧本树
    events: [],   // 事件列表
    assetItems: [], // 资产条目列表（项目级，按分类查询）
        
    // UI 状态
    currentScene: null,
    currentShot: null,
  }),
  
  getters: {
    currentProject: (state) => state.projects.find(p => p.id === state.currentProjectId),
  },
  
  actions: {
    async init() {
      // #region agent log
      fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'projectStore.js:24',message:'store.init called',data:{},timestamp:Date.now(),sessionId:'debug-session',runId:'run1',hypothesisId:'E'})}).catch(()=>{});
      // #endregion
      try {
        const { data } = await api.getProjects();
        // #region agent log
        fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'projectStore.js:27',message:'getProjects response',data:{projectCount:data?.length,projects:data},timestamp:Date.now(),sessionId:'debug-session',runId:'run1',hypothesisId:'E'})}).catch(()=>{});
        // #endregion
        this.projects = data;
        // 默认选中第一个
        if (this.projects.length > 0 && !this.currentProjectId) {
          this.selectProject(this.projects[0].id);
        }
      } catch (error) {
        // #region agent log
        fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'projectStore.js:33',message:'store.init error',data:{error:error?.message,status:error?.response?.status},timestamp:Date.now(),sessionId:'debug-session',runId:'run1',hypothesisId:'E'})}).catch(()=>{});
        // #endregion
        console.error('获取项目列表失败:', error);
      }
    },
    
    async selectProject(id) {
      this.currentProjectId = id;
      this.currentScene = null;
      this.currentShot = null;
      await Promise.all([
        this.fetchScript(),
        this.fetchEvents(),
        this.fetchAssetItems('persona_visual')
      ]);
    },
    
    async fetchScript() {
      if (!this.currentProjectId) return;
      const { data } = await api.getScript(this.currentProjectId);
      this.episodes = data;
    },
    
    async fetchEvents() {
      if (!this.currentProjectId) return;
      const { data } = await api.getEvents(this.currentProjectId);
      this.events = data;
    },

    async fetchAssetItems(category) {
      if (!this.currentProjectId) return;
      const { data } = await api.getAssetItems(this.currentProjectId, category);
      this.assetItems = data;
    },
    
    // --- 操作 ---
    async saveShot(shot) {
      await api.updateShot(shot.id, shot);
      // 局部更新（优化体验，不刷新整个树）
      const ep = this.episodes.find(e => e.scenes.some(s => s.id === shot.scene_id));
      const sc = ep?.scenes.find(s => s.id === shot.scene_id);
      const target = sc?.shots.find(s => s.id === shot.id);
      if (target) Object.assign(target, shot);
    }
  }
});