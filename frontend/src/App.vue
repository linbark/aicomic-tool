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
           <button class="text-blue-600 hover:underline text-xs" @click="createProject">+ New</button>
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

  </div>
</template>

<script setup>
import { onMounted } from 'vue';
import { useProjectStore } from './stores/projectStore';
import ScriptTree from './components/ScriptTree.vue';
import EventList from './components/EventList.vue'; // 简单列表组件
import api from './api/client';

const store = useProjectStore();

onMounted(() => {
  store.init();
});

const createProject = async () => {
  const name = prompt("请输入项目名称:");
  if(name) {
    await api.createProject({ name });
    store.init(); // 刷新
  }
};
</script>