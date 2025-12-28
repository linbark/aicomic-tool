<template>
  <div class="flex h-screen bg-gray-100 text-gray-800 text-sm font-sans">
    
    <aside class="w-16 bg-gray-900 flex flex-col items-center py-6 space-y-6 z-20 flex-shrink-0">
      <div class="text-white font-bold text-xl mb-2">AI</div>
      
      <router-link to="/script" active-class="bg-blue-600 text-white" class="p-3 rounded-xl text-gray-400 hover:text-white transition">
        <span class="text-xs font-bold">剧本</span>
      </router-link>
      
      <router-link to="/events" active-class="bg-blue-600 text-white" class="p-3 rounded-xl text-gray-400 hover:text-white transition">
        <span class="text-xs font-bold">事件</span>
      </router-link>
      
      <router-link to="/assets" active-class="bg-blue-600 text-white" class="p-3 rounded-xl text-gray-400 hover:text-white transition">
        <span class="text-xs font-bold">资产</span>
      </router-link>
    </aside>

    <aside class="w-64 bg-white border-r border-gray-200 flex flex-col flex-shrink-0">
      <div class="p-4 border-b bg-gray-50">
        <div class="flex justify-between items-center mb-2">
           <label class="text-[10px] font-bold text-gray-400 uppercase">Current Project</label>
           <div class="flex gap-2">
             <button 
               v-if="store.currentProjectId" 
               @click="handleDeleteProject" 
               class="text-red-600 hover:text-red-700 text-xs"
               title="删除当前项目"
             >
               🗑️
             </button>
             <button class="text-blue-600 hover:underline text-xs" @click="showCreateModal = true">+ New</button>
           </div>
        </div>
        <select :value="store.currentProjectId" @change="e => store.selectProject(Number(e.target.value))" 
                class="w-full border border-gray-300 rounded p-1.5 text-xs bg-white focus:ring-2 focus:ring-blue-500 outline-none">
          <option v-for="p in store.projects" :key="p.id" :value="p.id">{{ p.name }}</option>
        </select>
      </div>
      
      <div class="flex-1 overflow-y-auto p-2">
         <div v-if="$route.path === '/script'">
            <ScriptTree />
         </div>
         <div v-else-if="$route.path === '/events'">
            <EventList />
         </div>
      </div>
    </aside>

    <main class="flex-1 flex flex-col overflow-hidden relative">
      <router-view></router-view>
    </main>

    <!-- 新建项目模态框 -->
    <div v-if="showCreateModal" class="fixed inset-0 bg-black/50 z-50 flex items-center justify-center backdrop-blur-sm" @click="showCreateModal = false">
      <div class="bg-white rounded-xl shadow-2xl w-96 p-6" @click.stop>
        <h3 class="text-lg font-bold text-gray-800 mb-4">新建项目</h3>
        <div class="space-y-4">
          <div>
            <label class="text-xs font-bold text-gray-500">项目名称</label>
            <input 
              v-model="newProjectName" 
              @keyup.enter="handleCreateProject"
              class="w-full border p-2 rounded text-sm focus:ring-2 ring-blue-500 outline-none"
              placeholder="请输入项目名称"
              autofocus
            >
          </div>
        </div>
        <div v-if="createError" class="mt-2 text-red-600 text-xs">{{ createError }}</div>
        <div class="flex justify-end gap-2 mt-6">
          <button @click="showCreateModal = false; createError = ''" class="px-4 py-2 text-gray-500 hover:bg-gray-100 rounded text-sm">取消</button>
          <button @click="handleCreateProject" :disabled="isCreating" class="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded text-sm shadow-sm disabled:opacity-50 disabled:cursor-not-allowed">
            {{ isCreating ? '创建中...' : '确定' }}
          </button>
        </div>
      </div>
    </div>

  </div>
</template>

<script setup>
import { onMounted, ref } from 'vue';
import { useProjectStore } from './stores/projectStore';
import ScriptTree from './components/ScriptTree.vue';
import EventList from './components/EventList.vue'; // 简单列表组件
import api, { getApiBaseUrl } from './api/client';

const store = useProjectStore();
const showCreateModal = ref(false);
const newProjectName = ref('');
const createError = ref('');
const isCreating = ref(false);

onMounted(() => {
  // #region agent log
  fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'App.vue:onMounted',message:'App.vue onMounted called',data:{},timestamp:Date.now(),sessionId:'debug-session',runId:'run4',hypothesisId:'F'})}).catch(()=>{});
  // #endregion
  store.init();
});

const handleCreateProject = async () => {
  // #region agent log
  fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'App.vue:handleCreateProject',message:'handleCreateProject called',data:{name:newProjectName.value,hasName:!!newProjectName.value},timestamp:Date.now(),sessionId:'debug-session',runId:'run3',hypothesisId:'A'})}).catch(()=>{});
  // #endregion
  const name = newProjectName.value?.trim();
  if(!name) {
    createError.value = '请输入项目名称';
    return;
  }
  createError.value = '';
  isCreating.value = true;
  // #region agent log
  const baseUrl = typeof window !== 'undefined' ? (window.__AICOMIC_API_BASE_URL__ || 'not-set') : 'not-window';
  fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'App.vue:before api.createProject',message:'before api.createProject',data:{name,baseUrl,currentBaseURL:getApiBaseUrl()},timestamp:Date.now(),sessionId:'debug-session',runId:'run3',hypothesisId:'B'})}).catch(()=>{});
  // #endregion
  try {
    const result = await api.createProject({ name });
    // #region agent log
    fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'App.vue:api.createProject success',message:'api.createProject success',data:{result:result?.data,status:result?.status},timestamp:Date.now(),sessionId:'debug-session',runId:'run3',hypothesisId:'C'})}).catch(()=>{});
    // #endregion
    // #region agent log
    fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'App.vue:before store.init',message:'before store.init',data:{},timestamp:Date.now(),sessionId:'debug-session',runId:'run3',hypothesisId:'E'})}).catch(()=>{});
    // #endregion
    await store.init(); // 刷新
    // #region agent log
    fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'App.vue:store.init completed',message:'store.init completed',data:{projectCount:store.projects?.length},timestamp:Date.now(),sessionId:'debug-session',runId:'run3',hypothesisId:'E'})}).catch(()=>{});
    // #endregion
    showCreateModal.value = false;
    newProjectName.value = '';
    createError.value = '';
  } catch (error) {
    // #region agent log
    fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'App.vue:api.createProject error',message:'api.createProject error',data:{error:error?.message,stack:error?.stack,response:error?.response?.data,status:error?.response?.status},timestamp:Date.now(),sessionId:'debug-session',runId:'run3',hypothesisId:'C'})}).catch(()=>{});
    // #endregion
    console.error('创建项目失败:', error);
    createError.value = '创建项目失败: ' + (error?.response?.data?.detail || error?.message || '未知错误');
  } finally {
    isCreating.value = false;
  }
};

const handleDeleteProject = async () => {
  if (!store.currentProjectId) return;
  
  const project = store.projects.find(p => p.id === store.currentProjectId);
  if (!project) return;
  
  if (!confirm(`⚠️ 确定要删除项目 "${project.name}" 吗？\n此操作将永久删除该项目及其所有数据（剧本、资产、事件等），且无法撤销！`)) {
    return;
  }
  
  try {
    await api.deleteProject(store.currentProjectId);
    await store.init(); // 刷新项目列表
    // 如果删除后还有项目，自动选择第一个
    if (store.projects.length > 0) {
      await store.selectProject(store.projects[0].id);
    } else {
      store.currentProjectId = null;
    }
  } catch (error) {
    console.error('删除项目失败:', error);
    alert('删除项目失败: ' + (error?.response?.data?.detail || error?.message || '未知错误'));
  }
};
</script>