<template>
  <div class="flex h-full flex-col bg-gray-50">
    <!-- 顶部导航与元数据 -->
    <div class="bg-white border-b p-4 shadow-sm z-10 flex flex-col gap-3">
        <div class="flex justify-between items-start">
            <div class="flex items-center gap-4">
                 <router-link to="/events" class="text-gray-400 hover:text-gray-600">← Back</router-link>
                 <h2 class="text-lg font-bold text-gray-800" v-if="currentEvent">{{ currentEvent.name }}</h2>
                 <div class="px-2 py-0.5 rounded text-xs text-white" :style="{ background: currentEvent?.color }" v-if="currentEvent">
                    Event
                 </div>
            </div>
            <div class="flex items-center gap-2">
                 <button 
                    @click="showAddSceneModal = true"
                    class="bg-blue-600 hover:bg-blue-700 text-white px-4 py-2 rounded text-xs font-bold shadow-sm transition flex items-center gap-2"
                 >
                    <span>+</span> 增加关联场
                 </button>
            </div>
        </div>
        
        <!-- 简单的描述编辑 -->
        <div class="flex gap-4" v-if="currentEvent">
            <input 
                v-model="currentEvent.name" 
                @change="updateEventMeta"
                class="border-b border-transparent hover:border-gray-300 focus:border-blue-500 outline-none font-bold text-gray-700 bg-transparent"
            />
            <input 
                type="color" 
                v-model="currentEvent.color" 
                @change="updateEventMeta"
                class="w-6 h-6 border rounded cursor-pointer"
            />
        </div>
    </div>

    <!-- 画布区域 (只读, 可缩放) -->
    <div class="flex-1 w-full h-full relative bg-slate-50">
        <VueFlow
          v-if="elements.length > 0"
          v-model="elements"
          :default-zoom="1"
          :min-zoom="0.2"
          :max-zoom="4"
          :nodes-draggable="false"
          :nodes-connectable="false"
          :elements-selectable="false"
          fit-view-on-init
          class="w-full h-full"
        >
          <Background pattern-color="#ddd" gap="40" />
          <Controls :show-interactive="false" />
          
          <!-- 自定义节点: 圆形场节点 -->
          <template #node-scene-circle="props">
            <div 
                class="w-20 h-20 rounded-full flex flex-col items-center justify-center text-center shadow-md border-4 bg-white transition-transform hover:scale-105"
                :style="{ borderColor: currentEvent.color }"
            >
                <div class="text-[10px] font-bold text-gray-400 uppercase">EP {{ props.data.epOrder }}</div>
                <div class="text-sm font-black text-gray-800">Scene {{ props.data.sceneSeq }}</div>
            </div>
          </template>
        </VueFlow>
        
        <div v-else class="flex-1 h-full flex items-center justify-center text-gray-400">
            <div class="text-center">
                <p class="mb-2">暂无关联场</p>
                <button @click="showAddSceneModal = true" class="text-blue-500 hover:underline">点击添加</button>
            </div>
        </div>
    </div>

    <!-- 添加关联场弹窗 -->
    <div v-if="showAddSceneModal" class="fixed inset-0 bg-black/50 z-50 flex items-center justify-center">
        <div class="bg-white rounded-lg shadow-xl w-[600px] max-h-[80vh] flex flex-col">
            <div class="p-4 border-b flex justify-between items-center bg-gray-50 rounded-t-lg">
                <h3 class="font-bold text-gray-700">选择关联场 (Select Scenes)</h3>
                <button @click="showAddSceneModal = false" class="text-gray-400 hover:text-gray-600 text-xl">×</button>
            </div>
            
            <div class="p-4 overflow-y-auto flex-1 space-y-4">
                <div v-for="ep in store.episodes" :key="ep.id" class="border rounded p-3">
                    <div class="font-bold text-xs text-gray-500 mb-2">EP{{ ep.order }} - {{ ep.title }}</div>
                    <div class="grid grid-cols-4 gap-2">
                        <button
                            v-for="scene in ep.scenes"
                            :key="scene.id"
                            @click="toggleSelectScene(scene.id)"
                            :disabled="isSceneLinked(scene.id)"
                            class="p-2 rounded text-xs border text-center transition flex flex-col items-center justify-center h-16 relative"
                            :class="[
                                isSceneLinked(scene.id) 
                                    ? 'bg-gray-100 text-gray-400 border-gray-100 cursor-not-allowed'
                                    : tempSelectedScenes.has(scene.id)
                                        ? 'bg-blue-50 border-blue-500 text-blue-700 ring-1 ring-blue-500'
                                        : 'bg-white border-gray-200 hover:border-blue-300 hover:shadow-sm'
                            ]"
                        >
                            <span class="font-bold">Scene {{ scene.sequence_number }}</span>
                            <span class="text-[10px] truncate w-full text-gray-500 mt-1">{{ scene.title }}</span>
                            
                            <!-- 已关联标记 -->
                            <span v-if="isSceneLinked(scene.id)" class="absolute top-1 right-1 text-[8px] text-gray-400">Linked</span>
                        </button>
                    </div>
                </div>
            </div>
            
            <div class="p-4 border-t bg-gray-50 rounded-b-lg flex justify-end gap-2">
                <button @click="showAddSceneModal = false" class="px-4 py-2 text-xs text-gray-600 hover:bg-gray-200 rounded">取消</button>
                <button 
                    @click="saveSelectedScenes" 
                    :disabled="tempSelectedScenes.size === 0"
                    class="px-4 py-2 text-xs bg-blue-600 text-white rounded hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                    确认添加 ({{ tempSelectedScenes.size }})
                </button>
            </div>
        </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, watch } from 'vue';
import { useRoute } from 'vue-router';
import { VueFlow } from '@vue-flow/core';
import { Background } from '@vue-flow/background';
import { Controls } from '@vue-flow/controls';
import '@vue-flow/core/dist/style.css';
import '@vue-flow/controls/dist/style.css';

import { useProjectStore } from '../stores/projectStore';
import api from '../api/client';

const store = useProjectStore();
// 如果没有传入 ID，默认取第一个或空
const currentEvent = ref(null);
const elements = ref([]);
const showAddSceneModal = ref(false);
const tempSelectedScenes = ref(new Set());

// 初始化
onMounted(async () => {
    if (!store.currentProjectId) await store.init();
    await Promise.all([store.fetchScript(), store.fetchEvents()]);
    
    // 默认选中第一个事件用于演示，实际应从 Route Params 获取
    // 这里为了配合 EventMatrixView 的 "Edit Single" 按钮，我们需要一种方式传递选中的事件
    // 简单起见，如果 store 里有 events，默认选第一个，或者通过 URL query ?id=xxx
    const urlParams = new URLSearchParams(window.location.search);
    const idFromUrl = urlParams.get('id');
    
    if (idFromUrl) {
        currentEvent.value = store.events.find(e => e.id == idFromUrl);
    } else if (store.events.length > 0) {
        currentEvent.value = store.events[0];
    }
    
    buildGraph();
});

// 计算属性
const isSceneLinked = (sceneId) => {
    if (!currentEvent.value) return false;
    return (currentEvent.value.nodes || []).some(n => n.target_type === 'scene' && n.target_id === sceneId);
};

// 操作逻辑
const toggleSelectScene = (sceneId) => {
    if (tempSelectedScenes.value.has(sceneId)) {
        tempSelectedScenes.value.delete(sceneId);
    } else {
        tempSelectedScenes.value.add(sceneId);
    }
};

const saveSelectedScenes = async () => {
    if (!currentEvent.value) return;
    
    try {
        const promises = Array.from(tempSelectedScenes.value).map(sceneId => {
            return api.upsertEventNode(currentEvent.value.id, {
                target_type: 'scene',
                target_id: sceneId,
                description: ''
            });
        });
        
        await Promise.all(promises);
        
        // 刷新
        await store.fetchEvents();
        const updated = store.events.find(e => e.id === currentEvent.value.id);
        if (updated) currentEvent.value = updated;
        
        tempSelectedScenes.value.clear();
        showAddSceneModal.value = false;
        buildGraph();
    } catch (e) {
        console.error('Failed to add scenes', e);
        alert('添加失败');
    }
};

const updateEventMeta = async () => {
    if (!currentEvent.value) return;
    await api.updateEvent(currentEvent.value.id, {
        name: currentEvent.value.name,
        color: currentEvent.value.color
    });
};

// 自动构建图
// 将关联的 Scene 按剧本顺序排列
const buildGraph = () => {
    if (!currentEvent.value) return;
    
    const nodes = [];
    const edges = [];
    
    // 1. 提取所有关联的 Scenes
    const linkedSceneIds = (currentEvent.value.nodes || [])
        .filter(n => n.target_type === 'scene')
        .map(n => n.target_id);
        
    if (linkedSceneIds.length === 0) {
        elements.value = [];
        return;
    }
    
    // 2. 在 episodes 中找到这些 Scenes 并排序
    const sortedScenes = [];
    store.episodes.forEach(ep => {
        const scenes = (ep.scenes || []).slice().sort((a,b) => a.sequence_number - b.sequence_number);
        scenes.forEach(s => {
            if (linkedSceneIds.includes(s.id)) {
                sortedScenes.push({
                    id: s.id,
                    epOrder: ep.order,
                    sceneSeq: s.sequence_number,
                    title: s.title
                });
            }
        });
    });
    
    // 3. 生成节点和连线 (水平布局)
    const spacingX = 200;
    const startX = 100;
    const startY = 200;
    
    sortedScenes.forEach((s, index) => {
        // Node
        nodes.push({
            id: `scene-${s.id}`,
            type: 'scene-circle',
            position: { x: startX + index * spacingX, y: startY },
            data: { epOrder: s.epOrder, sceneSeq: s.sceneSeq }
        });
        
        // Edge (连向前一个)
        if (index > 0) {
            edges.push({
                id: `e-${index-1}-${index}`,
                source: `scene-${sortedScenes[index-1].id}`,
                target: `scene-${s.id}`,
                animated: true,
                style: { stroke: currentEvent.value.color, strokeWidth: 3 }
            });
        }
    });
    
    elements.value = [...nodes, ...edges];
};

watch(() => currentEvent.value, buildGraph);
</script>