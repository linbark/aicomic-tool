<template>
  <div class="space-y-4">
    <div v-for="ep in store.episodes" :key="ep.id">
      <div class="flex items-center text-xs font-bold text-gray-800 mb-2 px-2 group cursor-pointer">
        <span class="text-gray-400 mr-1 text-[10px]">EP{{ ep.order }}</span>
        <span class="group-hover:text-blue-600">{{ ep.title }}</span>
      </div>
      
      <div class="pl-3 border-l-2 border-gray-100 ml-2 space-y-0.5">
        <div v-for="scene in ep.scenes" :key="scene.id" 
             @click="store.currentScene = scene; store.currentShot = null;"
             :class="['cursor-pointer px-2 py-1.5 rounded text-xs truncate transition flex items-center', 
                      store.currentScene?.id === scene.id ? 'bg-blue-50 text-blue-700 font-bold' : 'text-gray-500 hover:bg-gray-50 hover:text-gray-700']">
             <span class="mr-2 opacity-50 text-[10px] w-4 text-right">{{ scene.sequence_number }}</span>
             {{ scene.title }}
        </div>
        <button @click="addScene(ep.id)" class="px-2 py-1 text-[10px] text-gray-300 hover:text-blue-500 block w-full text-left ml-6">
           + Add Scene
        </button>
      </div>
    </div>
    
    <button @click="addEpisode" class="w-full text-center py-2 text-xs text-gray-400 border border-dashed rounded hover:border-blue-400 hover:text-blue-500 transition mt-4">
       + New Episode
    </button>
  </div>
</template>

<script setup>
import { useProjectStore } from '../stores/projectStore';
import api from '../api/client';
const store = useProjectStore();

const addEpisode = async () => {
   if (!store.currentProjectId) {
     alert("请先选择一个项目");
     return;
   }
   const title = prompt("请输入集标题:");
   if(title) {
     await api.createEpisode(store.currentProjectId, { title, order: store.episodes.length + 1 });
     await store.fetchScript();
   }
};

const addScene = async (epId) => {
   const title = prompt("请输入场标题 (如: 草庙村·日):");
   if(title) {
     // sequence_number 由后端自动计算
     await api.createScene(epId, { title }); 
     store.fetchScript();
   }
};
</script>