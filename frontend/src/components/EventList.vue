<template>
  <div class="space-y-2">
    <div v-for="evt in store.events" :key="evt.id" 
         class="flex items-center p-2 rounded hover:bg-gray-50 cursor-pointer border border-transparent hover:border-gray-200 transition"
         title="点击事件 (功能开发中)">
      <div class="w-3 h-3 rounded-full mr-2 flex-shrink-0 shadow-sm" :style="{background: evt.color}"></div>
      <span class="text-xs text-gray-700 font-bold truncate">{{ evt.name }}</span>
    </div>
    
    <div v-if="store.events.length === 0" class="text-center text-gray-300 text-[10px] py-4">
      暂无事件
    </div>

    <button @click="addEvent" class="w-full text-center py-2 text-xs text-gray-400 border border-dashed rounded hover:border-blue-400 hover:text-blue-500 transition mt-4">
       + New Event
    </button>
  </div>
</template>

<script setup>
import { useProjectStore } from '../stores/projectStore';
import api from '../api/client';

const store = useProjectStore();

const addEvent = async () => {
  const name = prompt("请输入事件名称:");
  if (name) {
    // 默认创建一个蓝色事件
    await api.createEvent(store.currentProjectId, { name, color: '#3B82F6' });
    // 刷新事件列表
    store.fetchEvents();
  }
};
</script>