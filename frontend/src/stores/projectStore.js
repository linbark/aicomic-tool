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
      const { data } = await api.getProjects();
      this.projects = data;
      // 默认选中第一个
      if (this.projects.length > 0 && !this.currentProjectId) {
        this.selectProject(this.projects[0].id);
      }
    },
    
    async selectProject(id) {
      this.currentProjectId = id;
      this.currentScene = null;
      this.currentShot = null;
      await Promise.all([
        this.fetchScript(),
        this.fetchEvents(),
        this.fetchAssetItems('persona')
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