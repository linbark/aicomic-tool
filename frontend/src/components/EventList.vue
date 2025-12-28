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

    <button @click="showEventModal = true" class="w-full text-center py-2 text-xs text-gray-400 border border-dashed rounded hover:border-blue-400 hover:text-blue-500 transition mt-4">
       + New Event
    </button>
  </div>

  <!-- 新建事件模态框 -->
  <div v-if="showEventModal" class="fixed inset-0 bg-black/50 z-50 flex items-center justify-center backdrop-blur-sm" @click="showEventModal = false">
    <div class="bg-white rounded-xl shadow-2xl w-96 p-6" @click.stop>
      <h3 class="text-lg font-bold text-gray-800 mb-4">新建事件</h3>
      <div class="space-y-4">
        <div>
          <label class="text-xs font-bold text-gray-500">事件名称</label>
          <input 
            v-model="eventName" 
            @keyup.enter="handleCreateEvent"
            class="w-full border p-2 rounded text-sm focus:ring-2 ring-blue-500 outline-none"
            placeholder="请输入事件名称"
            autofocus
          >
        </div>
        <div>
          <label class="text-xs font-bold text-gray-500">颜色</label>
          <div class="flex gap-2 mt-1">
            <button
              v-for="color in colorOptions"
              :key="color"
              @click="selectedColor = color"
              :class="['w-8 h-8 rounded-full border-2 transition', selectedColor === color ? 'border-gray-800 scale-110' : 'border-gray-300']"
              :style="{background: color}"
              :title="color"
            ></button>
          </div>
        </div>
      </div>
      <div v-if="eventError" class="mt-2 text-red-600 text-xs">{{ eventError }}</div>
      <div class="flex justify-end gap-2 mt-6">
        <button @click="showEventModal = false; eventError = ''; eventName = ''" class="px-4 py-2 text-gray-500 hover:bg-gray-100 rounded text-sm">取消</button>
        <button @click="handleCreateEvent" :disabled="isCreatingEvent" class="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded text-sm shadow-sm disabled:opacity-50 disabled:cursor-not-allowed">
          {{ isCreatingEvent ? '创建中...' : '确定' }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref } from 'vue';
import { useProjectStore } from '../stores/projectStore';
import api from '../api/client';

const store = useProjectStore();

const showEventModal = ref(false);
const eventName = ref('');
const selectedColor = ref('#3B82F6');
const eventError = ref('');
const isCreatingEvent = ref(false);

const colorOptions = [
  '#3B82F6', // 蓝色
  '#EF4444', // 红色
  '#10B981', // 绿色
  '#F59E0B', // 黄色
  '#8B5CF6', // 紫色
  '#EC4899', // 粉色
  '#06B6D4', // 青色
  '#F97316', // 橙色
];

const handleCreateEvent = async () => {
  if (!store.currentProjectId) {
    eventError.value = '请先选择一个项目';
    return;
  }
  
  const name = eventName.value?.trim();
  if (!name) {
    eventError.value = '请输入事件名称';
    return;
  }
  
  eventError.value = '';
  isCreatingEvent.value = true;
  
  try {
    await api.createEvent(store.currentProjectId, { name, color: selectedColor.value });
    await store.fetchEvents();
    showEventModal.value = false;
    eventName.value = '';
    selectedColor.value = '#3B82F6';
  } catch (error) {
    console.error('创建事件失败:', error);
    eventError.value = '创建事件失败: ' + (error?.response?.data?.detail || error?.message || '未知错误');
  } finally {
    isCreatingEvent.value = false;
  }
};
</script>