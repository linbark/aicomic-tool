<template>
  <div class="flex flex-col h-full bg-gray-50">
     <div class="h-12 bg-white border-b flex items-center px-4 justify-between flex-shrink-0">
        <div class="flex gap-2">
           <button class="px-3 py-1 bg-gray-100 rounded text-xs font-bold text-gray-600 hover:bg-gray-200">集视图</button>
           <button class="px-3 py-1 bg-blue-100 rounded text-xs font-bold text-blue-700">场视图</button>
           <button class="px-3 py-1 bg-gray-100 rounded text-xs font-bold text-gray-600 hover:bg-gray-200">镜视图</button>
        </div>
        <div class="text-xs text-gray-400">Time Scale: 100%</div>
     </div>
     
     <div class="flex-1 relative overflow-auto p-4 flex items-center justify-center">
        <div class="text-center opacity-40">
           <div class="text-6xl mb-4">📊</div>
           <h2 class="text-xl font-bold text-gray-800">Event Matrix System</h2>
           <p class="text-sm text-gray-500 mt-2">Phase 3 开发中...</p>
           <div class="mt-8 text-left bg-white p-4 rounded shadow-sm border text-xs max-w-md">
              <p class="font-bold mb-2">已加载事件数据 (Debug):</p>
              <ul class="list-disc pl-4 space-y-1">
                 <li v-for="evt in store.events" :key="evt.id" class="text-gray-600">
                    <span class="inline-block w-2 h-2 rounded-full mr-2" :style="{background: evt.color}"></span>
                    {{ evt.name }} (ID: {{ evt.id }})
                 </li>
              </ul>
           </div>
        </div>
     </div>
  </div>
</template>

<script setup>
import { useProjectStore } from '../stores/projectStore';
const store = useProjectStore();
</script>