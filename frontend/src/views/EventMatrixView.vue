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
     
     <div class="matrix-container flex flex-col h-full overflow-hidden">
    
         <div class="controls h-12 border-b flex items-center px-4">
            <div class="tabs">
                  <button @click="setMode('episode')">集</button>
                  <button @click="setMode('scene')">场</button>
            </div>
         </div>

         <div class="matrix-body flex-1 overflow-auto relative bg-gray-50">
            
            <div class="grid-layout" :style="{ '--col-count': timelineColumns.length }">
                  
                  <div class="sticky top-0 left-0 z-30 bg-white border-b border-r w-48 h-10"></div>

                  <div class="contents header-row">
                     <div v-for="col in timelineColumns" :key="col.id" 
                           class="sticky top-0 z-20 bg-gray-100 border-b border-r p-2 text-xs font-bold truncate h-10 flex items-center justify-center">
                        {{ col._label || col.title }}
                     </div>
                  </div>

                  <div v-for="evt in store.events" :key="evt.id" class="contents event-row group">
                     
                     <div class="sticky left-0 z-10 bg-white border-r border-b w-48 p-2 flex items-center group-hover:bg-blue-50">
                        <div class="w-3 h-3 rounded-full mr-2" :style="{background: evt.color}"></div>
                        <span class="text-xs font-bold truncate">{{ evt.name }}</span>
                     </div>

                     <div v-for="col in timelineColumns" :key="col.id" 
                           class="border-r border-b h-16 relative hover:bg-gray-100 transition"
                           @click="openEditor(evt, col)">
                           
                        <div v-if="getNode(evt.id, col.id)" 
                              class="absolute inset-1 rounded p-1 text-[10px] overflow-hidden text-white shadow-sm"
                              :style="{background: evt.color}">
                              {{ getNode(evt.id, col.id).description }}
                        </div>
                        
                        <div v-else class="w-full h-full flex items-center justify-center opacity-0 hover:opacity-100 text-gray-300">
                              +
                        </div>
                     </div>
                  </div>

            </div>
         </div>
     </div>
  </div>
</template>

<script setup>
import { ref, computed } from 'vue';
import { useProjectStore } from '../stores/projectStore';
import api from '../api/client';
const store = useProjectStore();
const currentGranularity = ref('scene');

// X轴：时间轴列 (Columns)
// 这是一个计算属性，根据 currentGranularity 动态生成
const timelineColumns = computed(() => {
   if (currentGranularity.value === 'episode') {
       return store.episodes; // 直接返回集列表
   } 
   else if (currentGranularity.value === 'scene') {
       // 展平所有场：[Ep1-S1, Ep1-S2, ..., Ep2-S1, ...]
       return store.episodes.flatMap(ep => ep.scenes.map(s => ({
           ...s,
           _label: `${ep.order}-${s.sequence_number} ${s.title}`, //用于表头显示
           _epTitle: ep.title // 用于分组表头
       })));
   }
   // ... shot 同理
});

// 数据索引 (Map)
// 作用：快速查找 (event_id, target_id) 是否有内容
// 结构：{ "evt_1_scene_5": { description: "..." }, ... }
const nodeMap = computed(() => {
    const map = {};
    if (store.events) {
        store.events.forEach(event => {
            if (event.nodes) {
                event.nodes.forEach(node => {
                    // 生成唯一 Key
                    const key = `evt_${node.event_id}_${node.target_type}_${node.target_id}`;
                    map[key] = node;
                });
            }
        });
    }
    return map;
});

// 切换视图模式
const setMode = (mode) => {
    currentGranularity.value = mode;
};

// 获取节点数据
const getNode = (eventId, targetId) => {
    const targetType = currentGranularity.value === 'episode' ? 'episode' : 'scene';
    const key = `evt_${eventId}_${targetType}_${targetId}`;
    return nodeMap.value[key] || null;
};

// 打开编辑器
const openEditor = async (event, col) => {
    const existingNode = getNode(event.id, col.id);
    const description = prompt(
        existingNode ? '编辑事件节点描述:' : '添加事件节点描述:',
        existingNode ? existingNode.description : ''
    );
    
    if (description !== null && description.trim() !== '') {
        try {
            const targetType = currentGranularity.value === 'episode' ? 'episode' : 'scene';
            await api.upsertEventNode(event.id, {
                target_type: targetType,
                target_id: col.id,
                description: description.trim()
            });
            // 刷新事件数据
            await store.fetchEvents();
        } catch (e) {
            console.error('保存事件节点失败:', e);
            alert('保存失败');
        }
    }
};
</script>