<template>
  <div class="space-y-4">
    <div v-for="ep in store.episodes" :key="ep.id">
      <div class="flex items-center justify-between text-xs font-bold text-gray-800 mb-2 px-2 group">
        <div class="flex items-center cursor-pointer flex-1" @click="openEpisodeEditModal(ep)">
          <span class="text-gray-400 mr-1 text-[10px]">EP{{ ep.order }}</span>
          <span class="group-hover:text-blue-600">{{ ep.title }}</span>
        </div>
        <button 
          @click.stop="openEpisodeEditModal(ep)" 
          class="opacity-0 group-hover:opacity-100 text-gray-400 hover:text-blue-600 transition px-1"
          title="编辑集"
        >
          ✏️
        </button>
      </div>
      
      <div class="pl-3 border-l-2 border-gray-100 ml-2 space-y-0.5">
        <div v-for="scene in ep.scenes" :key="scene.id" 
             class="group flex items-center"
             :class="['cursor-pointer px-2 py-1.5 rounded text-xs truncate transition', 
                      store.currentScene?.id === scene.id ? 'bg-blue-50 text-blue-700 font-bold' : 'text-gray-500 hover:bg-gray-50 hover:text-gray-700']">
          <div class="flex items-center flex-1" @click="store.currentScene = scene; store.currentShot = null;">
            <span class="mr-2 opacity-50 text-[10px] w-4 text-right">{{ scene.sequence_number }}</span>
            {{ scene.title }}
          </div>
          <button 
            @click.stop="openSceneEditModal(scene)" 
            class="opacity-0 group-hover:opacity-100 text-gray-400 hover:text-blue-600 transition px-1 ml-1"
            title="编辑场"
          >
            ✏️
          </button>
        </div>
        <button @click="openSceneModal(ep.id)" class="px-2 py-1 text-[10px] text-gray-300 hover:text-blue-500 block w-full text-left ml-6">
           + Add Scene
        </button>
      </div>
    </div>
    
    <button @click="showEpisodeModal = true" class="w-full text-center py-2 text-xs text-gray-400 border border-dashed rounded hover:border-blue-400 hover:text-blue-500 transition mt-4">
       + New Episode
    </button>
  </div>

  <!-- 新建集模态框 -->
  <div v-if="showEpisodeModal" class="fixed inset-0 bg-black/50 z-50 flex items-center justify-center backdrop-blur-sm" @click="showEpisodeModal = false">
    <div class="bg-white rounded-xl shadow-2xl w-96 p-6" @click.stop>
      <h3 class="text-lg font-bold text-gray-800 mb-4">新建集</h3>
      <div class="space-y-4">
        <div>
          <label class="text-xs font-bold text-gray-500">集标题</label>
          <input 
            v-model="episodeTitle" 
            @keyup.enter="handleCreateEpisode"
            class="w-full border p-2 rounded text-sm focus:ring-2 ring-blue-500 outline-none"
            placeholder="请输入集标题"
            autofocus
          >
        </div>
      </div>
      <div v-if="episodeError" class="mt-2 text-red-600 text-xs">{{ episodeError }}</div>
      <div class="flex justify-end gap-2 mt-6">
        <button @click="showEpisodeModal = false; episodeError = ''; episodeTitle = ''" class="px-4 py-2 text-gray-500 hover:bg-gray-100 rounded text-sm">取消</button>
        <button @click="handleCreateEpisode" :disabled="isCreatingEpisode" class="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded text-sm shadow-sm disabled:opacity-50 disabled:cursor-not-allowed">
          {{ isCreatingEpisode ? '创建中...' : '确定' }}
        </button>
      </div>
    </div>
  </div>

  <!-- 新建场模态框 -->
  <div v-if="showSceneModal" class="fixed inset-0 bg-black/50 z-50 flex items-center justify-center backdrop-blur-sm" @click="showSceneModal = false">
    <div class="bg-white rounded-xl shadow-2xl w-96 p-6" @click.stop>
      <h3 class="text-lg font-bold text-gray-800 mb-4">新建场</h3>
      <div class="space-y-4">
        <div>
          <label class="text-xs font-bold text-gray-500">场标题</label>
          <input 
            v-model="sceneTitle" 
            @keyup.enter="handleCreateScene"
            class="w-full border p-2 rounded text-sm focus:ring-2 ring-blue-500 outline-none"
            placeholder="请输入场标题 (如: 草庙村·日)"
            autofocus
          >
        </div>
      </div>
      <div v-if="sceneError" class="mt-2 text-red-600 text-xs">{{ sceneError }}</div>
      <div class="flex justify-end gap-2 mt-6">
        <button @click="showSceneModal = false; sceneError = ''; sceneTitle = ''" class="px-4 py-2 text-gray-500 hover:bg-gray-100 rounded text-sm">取消</button>
        <button @click="handleCreateScene" :disabled="isCreatingScene" class="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded text-sm shadow-sm disabled:opacity-50 disabled:cursor-not-allowed">
          {{ isCreatingScene ? '创建中...' : '确定' }}
        </button>
      </div>
    </div>
  </div>

  <!-- 编辑集模态框 -->
  <div v-if="showEpisodeEditModal" class="fixed inset-0 bg-black/50 z-50 flex items-center justify-center backdrop-blur-sm" @click="showEpisodeEditModal = false">
    <div class="bg-white rounded-xl shadow-2xl w-[600px] max-h-[90vh] flex flex-col" @click.stop>
      <div class="p-6 border-b">
        <h3 class="text-lg font-bold text-gray-800">编辑集</h3>
      </div>
      <div class="flex-1 overflow-y-auto p-6 space-y-4">
        <div>
          <label class="text-xs font-bold text-gray-500">集标题</label>
          <input 
            v-model="editingEpisode.title" 
            class="w-full border p-2 rounded text-sm focus:ring-2 ring-blue-500 outline-none"
            placeholder="请输入集标题"
          >
        </div>
        <div>
          <label class="text-xs font-bold text-gray-500">剧本内容</label>
          <textarea 
            v-model="editingEpisode.description" 
            class="w-full border p-2 rounded text-sm focus:ring-2 ring-blue-500 outline-none resize-none"
            placeholder="编写集的剧本内容..."
            rows="12"
          ></textarea>
        </div>
      </div>
      <div v-if="episodeEditError" class="px-6 text-red-600 text-xs">{{ episodeEditError }}</div>
      <div class="flex justify-end gap-2 p-6 border-t">
        <button @click="showEpisodeEditModal = false; episodeEditError = ''" class="px-4 py-2 text-gray-500 hover:bg-gray-100 rounded text-sm">取消</button>
        <button @click="handleUpdateEpisode" :disabled="isUpdatingEpisode" class="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded text-sm shadow-sm disabled:opacity-50 disabled:cursor-not-allowed">
          {{ isUpdatingEpisode ? '保存中...' : '保存' }}
        </button>
      </div>
    </div>
  </div>

  <!-- 编辑场模态框 -->
  <div v-if="showSceneEditModal" class="fixed inset-0 bg-black/50 z-50 flex items-center justify-center backdrop-blur-sm" @click="showSceneEditModal = false">
    <div class="bg-white rounded-xl shadow-2xl w-[600px] max-h-[90vh] flex flex-col" @click.stop>
      <div class="p-6 border-b">
        <h3 class="text-lg font-bold text-gray-800">编辑场</h3>
      </div>
      <div class="flex-1 overflow-y-auto p-6 space-y-4">
        <div>
          <label class="text-xs font-bold text-gray-500">场标题</label>
          <input 
            v-model="editingScene.title" 
            class="w-full border p-2 rounded text-sm focus:ring-2 ring-blue-500 outline-none"
            placeholder="请输入场标题"
          >
        </div>
        <div>
          <label class="text-xs font-bold text-gray-500">剧本内容</label>
          <textarea 
            v-model="editingScene.description" 
            class="w-full border p-2 rounded text-sm focus:ring-2 ring-blue-500 outline-none resize-none"
            placeholder="编写场的剧本内容..."
            rows="12"
          ></textarea>
        </div>
      </div>
      <div v-if="sceneEditError" class="px-6 text-red-600 text-xs">{{ sceneEditError }}</div>
      <div class="flex justify-end gap-2 p-6 border-t">
        <button @click="showSceneEditModal = false; sceneEditError = ''" class="px-4 py-2 text-gray-500 hover:bg-gray-100 rounded text-sm">取消</button>
        <button @click="handleUpdateScene" :disabled="isUpdatingScene" class="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded text-sm shadow-sm disabled:opacity-50 disabled:cursor-not-allowed">
          {{ isUpdatingScene ? '保存中...' : '保存' }}
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

const showEpisodeModal = ref(false);
const episodeTitle = ref('');
const episodeError = ref('');
const isCreatingEpisode = ref(false);

const showSceneModal = ref(false);
const sceneTitle = ref('');
const sceneError = ref('');
const isCreatingScene = ref(false);
const currentEpisodeId = ref(null);

const showEpisodeEditModal = ref(false);
const editingEpisode = ref({ id: null, title: '', description: '' });
const episodeEditError = ref('');
const isUpdatingEpisode = ref(false);

const showSceneEditModal = ref(false);
const editingScene = ref({ id: null, title: '', description: '' });
const sceneEditError = ref('');
const isUpdatingScene = ref(false);

const handleCreateEpisode = async () => {
  if (!store.currentProjectId) {
    episodeError.value = '请先选择一个项目';
    return;
  }
  
  const title = episodeTitle.value?.trim();
  if (!title) {
    episodeError.value = '请输入集标题';
    return;
  }
  
  episodeError.value = '';
  isCreatingEpisode.value = true;
  
  try {
    await api.createEpisode(store.currentProjectId, { title, order: store.episodes.length + 1 });
    await store.fetchScript();
    showEpisodeModal.value = false;
    episodeTitle.value = '';
  } catch (error) {
    console.error('创建集失败:', error);
    episodeError.value = '创建集失败: ' + (error?.response?.data?.detail || error?.message || '未知错误');
  } finally {
    isCreatingEpisode.value = false;
  }
};

const openSceneModal = (epId) => {
  currentEpisodeId.value = epId;
  showSceneModal.value = true;
};

const handleCreateScene = async () => {
  if (!currentEpisodeId.value) {
    sceneError.value = '缺少集ID';
    return;
  }
  
  const title = sceneTitle.value?.trim();
  if (!title) {
    sceneError.value = '请输入场标题';
    return;
  }
  
  sceneError.value = '';
  isCreatingScene.value = true;
  
  try {
    await api.createScene(currentEpisodeId.value, { title });
    await store.fetchScript();
    showSceneModal.value = false;
    sceneTitle.value = '';
    currentEpisodeId.value = null;
  } catch (error) {
    console.error('创建场失败:', error);
    sceneError.value = '创建场失败: ' + (error?.response?.data?.detail || error?.message || '未知错误');
  } finally {
    isCreatingScene.value = false;
  }
};

const openEpisodeEditModal = (episode) => {
  editingEpisode.value = {
    id: episode.id,
    title: episode.title || '',
    description: episode.description || ''
  };
  showEpisodeEditModal.value = true;
};

const handleUpdateEpisode = async () => {
  if (!editingEpisode.value.id) {
    episodeEditError.value = '缺少集ID';
    return;
  }
  
  episodeEditError.value = '';
  isUpdatingEpisode.value = true;
  
  try {
    await api.updateEpisode(editingEpisode.value.id, {
      title: editingEpisode.value.title,
      description: editingEpisode.value.description
    });
    await store.fetchScript();
    showEpisodeEditModal.value = false;
  } catch (error) {
    console.error('更新集失败:', error);
    episodeEditError.value = '更新集失败: ' + (error?.response?.data?.detail || error?.message || '未知错误');
  } finally {
    isUpdatingEpisode.value = false;
  }
};

const openSceneEditModal = (scene) => {
  editingScene.value = {
    id: scene.id,
    title: scene.title || '',
    description: scene.description || ''
  };
  showSceneEditModal.value = true;
};

const handleUpdateScene = async () => {
  if (!editingScene.value.id) {
    sceneEditError.value = '缺少场ID';
    return;
  }
  
  sceneEditError.value = '';
  isUpdatingScene.value = true;
  
  try {
    await api.updateScene(editingScene.value.id, {
      title: editingScene.value.title,
      description: editingScene.value.description
    });
    await store.fetchScript();
    showSceneEditModal.value = false;
  } catch (error) {
    console.error('更新场失败:', error);
    sceneEditError.value = '更新场失败: ' + (error?.response?.data?.detail || error?.message || '未知错误');
  } finally {
    isUpdatingScene.value = false;
  }
};
</script>