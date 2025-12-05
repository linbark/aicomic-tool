<template>
  <div class="flex h-full">
    <!-- 左侧事件列表 -->
    <div class="w-64 bg-white border-r flex-shrink-0 flex flex-col">
      <div class="p-4 border-b bg-gray-50 font-bold text-gray-700 text-xs">
        事件列表 (Event List)
      </div>
      <div class="flex-1 overflow-y-auto p-2 space-y-2">
        <div
          v-for="evt in store.events"
          :key="evt.id"
          @click="selectEvent(evt)"
          :class="[
            'p-3 rounded cursor-pointer border transition flex items-center gap-2',
            currentEvent?.id === evt.id
              ? 'bg-blue-50 border-blue-500 shadow-sm'
              : 'bg-white border-gray-200 hover:border-blue-300'
          ]"
        >
          <div
            class="w-3 h-3 rounded-full shadow-sm flex-shrink-0"
            :style="{ background: evt.color }"
          />
          <span class="text-xs font-bold text-gray-700 truncate">{{ evt.name }}</span>
        </div>

        <button
          @click="addEvent"
          class="w-full py-2 border border-dashed rounded text-xs text-gray-400 hover:text-blue-500 hover:border-blue-400 mt-2"
        >
          + 新建事件
        </button>
      </div>
    </div>

    <!-- 右侧编辑区 -->
    <div class="flex-1 flex flex-col bg-gray-50 relative" v-if="currentEvent">
      <!-- 顶部：元数据编辑 -->
      <div class="h-auto bg-white border-b p-4 shadow-sm z-10 flex flex-col gap-3">
        <div class="flex justify-between items-start">
          <div class="flex-1 mr-4">
            <label class="text-[10px] font-bold text-gray-400 uppercase">Event Title</label>
            <input
              v-model="currentEvent.name"
              @change="saveMetadata"
              class="w-full text-lg font-bold border-b border-transparent hover:border-gray-300 focus:border-blue-500 outline-none transition"
            />
          </div>
          <button
            @click="saveGraph"
            class="bg-blue-600 hover:bg-blue-700 text-white px-4 py-1.5 rounded text-xs shadow-sm flex items-center gap-1"
          >
            <span>💾</span> 保存流程
          </button>
        </div>

        <div class="flex gap-4">
          <div class="flex-1">
            <label class="text-[10px] font-bold text-gray-400 uppercase">Description (剧情描述)</label>
            <textarea
              v-model="currentEvent.description"
              @change="saveMetadata"
              class="w-full h-16 mt-1 p-2 border rounded text-xs resize-none focus:ring-1 ring-blue-500 outline-none text-gray-600"
              placeholder="在此描述该事件的前因后果、核心冲突..."
            />
          </div>
        </div>
      </div>

      <!-- 左上角节点工具条 -->
      <div class="absolute top-[140px] left-4 z-20 flex gap-2">
        <div class="bg-white p-1 rounded shadow border flex flex-col gap-1">
          <button
            @click="addNode('start')"
            class="p-2 hover:bg-gray-100 rounded text-xs flex flex-col items-center"
            title="添加开始节点"
          >
            <span class="text-lg">🟢</span>
            <span class="scale-75">Start</span>
          </button>
          <button
            @click="addNode('scene')"
            class="p-2 hover:bg-gray-100 rounded text-xs flex flex-col items-center"
            title="关联剧本场次"
          >
            <span class="text-lg">🎬</span>
            <span class="scale-75">Scene</span>
          </button>
          <button
            @click="addNode('note')"
            class="p-2 hover:bg-gray-100 rounded text-xs flex flex-col items-center"
            title="添加备注"
          >
            <span class="text-lg">📝</span>
            <span class="scale-75">Note</span>
          </button>
        </div>
      </div>

      <!-- Vue Flow 画布 -->
      <div class="flex-1 w-full h-full bg-slate-50">
        <VueFlow
          v-model="elements"
          :default-zoom="1"
          :min-zoom="0.2"
          :max-zoom="4"
          fit-view-on-init
          class="w-full h-full"
          @nodes-initialized="restoreGraph"
        >
          <Background pattern-color="#aaa" gap="20" />
          <Controls />

          <template #node-scene="props">
            <div class="bg-white border-2 border-blue-500 rounded shadow-md min-w-[150px]">
              <div class="bg-blue-500 text-white text-[10px] px-2 py-1 font-bold flex justify-between">
                <span>SCENE LINK</span>
                <button @click="removeNodes([props.id])" class="hover:text-red-200">×</button>
              </div>
              <div class="p-2">
                <select
                  v-model="props.data.sceneId"
                  class="w-full text-xs border rounded p-1 mb-1"
                  @mousedown.stop
                >
                  <option :value="null">-- 选择场次 --</option>
                  <optgroup v-for="ep in store.episodes" :key="ep.id" :label="ep.title">
                    <option v-for="sc in ep.scenes" :key="sc.id" :value="sc.id">
                      {{ sc.sequence_number }}. {{ sc.title }}
                    </option>
                  </optgroup>
                </select>
                <textarea
                  v-model="props.data.desc"
                  placeholder="该场次发生了什么..."
                  class="w-full text-[10px] border rounded p-1 h-12 resize-none"
                  @mousedown.stop
                />
              </div>
              <Handle type="target" position="left" />
              <Handle type="source" position="right" />
            </div>
          </template>

          <template #node-start="props">
            <div
              class="bg-green-500 text-white rounded-full w-12 h-12 flex items-center justify-center shadow-lg font-bold text-xs border-2 border-white"
            >
              START
              <Handle type="source" position="right" />
            </div>
          </template>
        </VueFlow>
      </div>
    </div>

    <!-- 空状态 -->
    <div v-else class="flex-1 flex items-center justify-center text-gray-300">
      <div class="text-center">
        <div class="text-4xl mb-4">🔮</div>
        <p>Select an event to start editing the flow</p>
      </div>
    </div>
  </div>
</template>

<script setup>
import { onMounted, ref, watch } from 'vue';
import { VueFlow, useVueFlow, Handle } from '@vue-flow/core';
import { Background } from '@vue-flow/background';
import { Controls } from '@vue-flow/controls';
import '@vue-flow/core/dist/style.css';
import '@vue-flow/background/dist/style.css';
import '@vue-flow/controls/dist/style.css';

import { useProjectStore } from '../stores/projectStore';
import api from '../api/client';

const store = useProjectStore();
const currentEvent = ref(null);
const elements = ref([]);

const { addNodes, removeNodes, toObject, onConnect, addEdges, setViewport } = useVueFlow();

// 初始化：加载事件和剧本
onMounted(async () => {
  await store.fetchEvents();
  if (store.currentProjectId) {
    await store.fetchScript();
  }
  if (store.events.length > 0) {
    selectEvent(store.events[0]);
  }
});

// 事件选择
const selectEvent = (evt) => {
  currentEvent.value = { ...evt };
  if (evt.graph_data && Array.isArray(evt.graph_data.nodes) && Array.isArray(evt.graph_data.edges)) {
    // Vue Flow v-model 使用扁平元素数组
    elements.value = [...evt.graph_data.nodes, ...evt.graph_data.edges];
    if (evt.graph_data.viewport) {
      setViewport(evt.graph_data.viewport);
    }
  } else {
    elements.value = [
      { id: 'start-1', type: 'start', position: { x: 50, y: 200 }, data: {} },
    ];
  }
};

// 新建事件
const addEvent = async () => {
  if (!store.currentProjectId) return;
  const name = window.prompt('New Event Name:');
  if (name) {
    await api.createEvent(store.currentProjectId, { name });
    await store.fetchEvents();
  }
};

// 添加节点
const addNode = (type) => {
  const id = `node-${Date.now()}`;
  const newNode = {
    id,
    type,
    position: { x: 200, y: 200 + Math.random() * 50 },
    data: { sceneId: null, desc: '' },
    label: type === 'note' ? 'New Note' : undefined,
  };
  addNodes([newNode]);
};

// 连线回调
onConnect((params) => {
  addEdges([params]);
});

// 保存文字元数据
const saveMetadata = async () => {
  if (!currentEvent.value) return;
  await api.updateEvent(currentEvent.value.id, {
    name: currentEvent.value.name,
    description: currentEvent.value.description,
  });
};

// 保存画布状态
const saveGraph = async () => {
  if (!currentEvent.value) return;
  const flowData = toObject();
  await api.updateEvent(currentEvent.value.id, {
    graph_data: flowData,
  });
  const idx = store.events.findIndex((e) => e.id === currentEvent.value.id);
  if (idx !== -1) {
    store.events[idx].graph_data = flowData;
  }
  window.alert('流程图已保存!');
};

// 当节点初始化后，确保使用当前事件数据恢复视图
const restoreGraph = () => {
  if (!currentEvent.value) return;
  if (currentEvent.value.graph_data?.viewport) {
    setViewport(currentEvent.value.graph_data.viewport);
  }
};
</script>

<style>
.vue-flow__node-scene {
  /* 样式已在模板中定义 */
}
</style>

