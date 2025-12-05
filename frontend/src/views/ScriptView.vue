<template>
  <div class="flex h-full" v-if="store.currentProjectId">
    
    <div class="w-80 bg-white border-r border-gray-200 flex flex-col flex-shrink-0" v-if="store.currentScene">
      <div class="p-3 border-b bg-gray-50 flex justify-between items-center">
         <span class="font-bold text-gray-700 truncate w-48" :title="store.currentScene.title">
            {{ store.currentScene.title }}
         </span>
         <div class="flex gap-1">
            <button class="text-gray-400 hover:text-red-600 px-2" title="删除本场" @click="handleDeleteScene">
               <span class="text-xs">🗑️</span>
            </button>
            <button class="text-blue-600 text-xs hover:bg-blue-50 px-2 py-1 rounded" @click="addShot">+ 加镜</button>
         </div>
      </div>
      
      <div class="flex-1 overflow-y-auto p-3 space-y-3">
         <div v-for="shot in store.currentScene.shots" :key="shot.id"
            @click="store.currentShot = JSON.parse(JSON.stringify(shot))"
            :class="['p-3 rounded border cursor-pointer transition relative group',
                     store.currentShot?.id === shot.id ? 'border-blue-500 bg-blue-50 shadow-sm' : 'border-gray-200 hover:border-blue-300 bg-white']">
            
            <div class="flex justify-between items-start mb-1">
               <span class="font-bold text-xs text-blue-700">#{{ shot.sequence_number }}</span>
               
               <div class="flex items-center gap-2">
                  <span class="text-[10px] text-gray-400 border px-1 rounded">{{ shot.status }}</span>
                  <button @click.stop="handleDeleteShot(shot.id)" class="text-gray-300 hover:text-red-500 opacity-0 group-hover:opacity-100 transition">
                     ×
                  </button>
               </div>
            </div>
         </div>
      </div>
    </div>
    
    <div v-else class="w-80 bg-gray-50 flex items-center justify-center text-gray-400 border-r text-xs">
      请从左侧选择一场戏
    </div>

    <div class="flex-1 flex flex-col bg-gray-50" v-if="store.currentShot">
       <div class="p-4 bg-white border-b shadow-sm flex justify-between items-center">
          <div class="flex items-center gap-3">
             <span class="bg-gray-100 px-2 py-1 rounded text-xs font-mono text-gray-600">SHOT-{{ store.currentShot.sequence_number }}</span>
             <input v-model="store.currentShot.title" placeholder="镜头名称 (可选)" class="border-b focus:border-blue-500 outline-none text-sm w-48">
          </div>
          <button @click="save" class="bg-blue-600 text-white px-4 py-1.5 rounded text-xs hover:bg-blue-700 transition shadow-sm">
            保存修改
          </button>
       </div>

       <div class="grid grid-cols-2 gap-0 border-b border-gray-200">
          <div class="p-4 bg-white border-r">
             <label class="block text-[10px] font-bold text-gray-400 uppercase mb-2">Action (画面描述)</label>
             <textarea v-model="store.currentShot.action_text" class="w-full h-40 text-sm outline-none resize-none placeholder-gray-300" placeholder="描述发生了什么..."></textarea>
          </div>
          <div class="p-4 bg-gray-50">
             <label class="block text-[10px] font-bold text-gray-400 uppercase mb-2">Stable Diffusion Prompt</label>
             <textarea v-model="store.currentShot.prompt" class="w-full h-40 text-xs font-mono bg-transparent outline-none resize-none text-gray-600 placeholder-gray-300" placeholder="English prompt here..."></textarea>
          </div>
       </div>

       <div class="p-4 bg-white border-b border-gray-200">
         <div class="flex justify-between items-center mb-2">
            <label class="block text-[10px] font-bold text-gray-400 uppercase">Video Preview (视频演示)</label>
            
            <div>
               <input type="file" ref="videoInput" accept="video/*" class="hidden" @change="handleVideoUpload">
               <button @click="$refs.videoInput.click()" class="text-xs bg-gray-100 hover:bg-gray-200 text-gray-600 px-3 py-1 rounded transition">
               {{ store.currentShot.video_path ? '替换视频' : '上传视频' }}
               </button>
            </div>
         </div>

         <div v-if="store.currentShot.video_path" class="w-full bg-black rounded overflow-hidden aspect-video relative group">
            <video controls class="w-full h-full object-contain" :src="getFileUrl(store.currentShot.video_path)"></video>
         </div>
         
         <div v-else class="w-full aspect-[16/5] bg-gray-50 border border-dashed border-gray-300 rounded flex items-center justify-center text-gray-400 text-xs cursor-pointer hover:bg-gray-100 transition" @click="$refs.videoInput.click()">
            <span>点击上传 MP4 视频片段</span>
         </div>
       </div>

       <div class="flex-1 p-6 overflow-y-auto">
          <div class="flex justify-between items-center mb-4">
             <h3 class="font-bold text-gray-700 text-xs uppercase flex items-center gap-2">
                <span>Assets Library</span>
                <span class="bg-gray-200 text-gray-600 px-1.5 py-0.5 rounded-full text-[10px]">{{ store.currentShot.assets.length }}</span>
             </h3>
             <div class="flex gap-2">
                <input v-model="newAssetPath" placeholder="Paste local path..." class="border rounded px-2 py-1 text-xs w-64">
                <button @click="addAsset" class="bg-white border hover:bg-gray-50 text-gray-600 px-3 py-1 rounded text-xs">Add</button>
             </div>
          </div>
          
          <div class="grid grid-cols-3 xl:grid-cols-4 gap-4">
             <div v-for="asset in store.currentShot.assets" :key="asset.id" class="group relative aspect-video bg-gray-200 rounded border overflow-hidden hover:shadow-md transition">
                <img v-if="asset.file_path" :src="getFileUrl(asset.file_path)" class="w-full h-full object-cover">
                <div class="absolute inset-0 bg-black/50 opacity-0 group-hover:opacity-100 transition flex items-end p-2">
                   <span class="text-white text-[10px] truncate w-full">{{ asset.file_path }}</span>
                </div>
             </div>
          </div>
       </div>
    </div>
    
    <div v-else class="flex-1 flex items-center justify-center text-gray-300">
       <div class="text-center">
          <div class="text-4xl mb-2">🎬</div>
          <p>Select a shot to edit</p>
       </div>
    </div>
    
  </div>
</template>

<script setup>
import { ref } from 'vue';
import { useProjectStore } from '../stores/projectStore';
import api from '../api/client';

const store = useProjectStore();
const newAssetPath = ref('');
const videoInput = ref(null);

const addShot = async () => {
   // 简化逻辑：直接创建
   if(!store.currentScene) return;
   const nextSeq = store.currentScene.shots.length + 1;
   await api.createShot(store.currentScene.id, { 
      sequence_number: nextSeq,
      action_text: '' 
   });
   await store.fetchScript();
   // 重新选中当前Scene (fetchScript会刷新引用)
   const ep = store.episodes.find(e => e.scenes.some(s => s.id === store.currentScene.id));
   if(ep) store.currentScene = ep.scenes.find(s => s.id === store.currentScene.id);
};

const save = async () => {
  if(store.currentShot) {
    await store.saveShot(store.currentShot);
    alert('已保存');
  }
};

const addAsset = async () => {
  if(newAssetPath.value && store.currentShot) {
    await api.addAsset(store.currentShot.id, newAssetPath.value);
    // 简单刷新
    store.fetchScript();
    newAssetPath.value = '';
  }
};

const getFileUrl = (path) => {
  if (!path) return '';
  const baseUrl = 'http://localhost:8000';
  // 后端通过 /files 静态服务 data 目录，拼接时补上 /files
  return `${baseUrl}/files/${path}`;
};

// 处理视频上传
const handleVideoUpload = async (event) => {
  const file = event.target.files[0];
  if (!file) return;

  if (!store.currentShot) return;

  // 构建 FormData
  const formData = new FormData();
  formData.append('file', file);

  try {
    const { data } = await api.uploadShotVideo(store.currentShot.id, formData);
    
    store.currentShot.video_path = data.video_path;
    alert('视频上传成功！');
    // 刷新数据以获取最新的 shot 信息
    await store.fetchScript();
    // 重新选中当前 shot
    if (store.currentScene) {
      const ep = store.episodes.find(e => e.scenes.some(s => s.id === store.currentScene.id));
      if (ep) {
        const scene = ep.scenes.find(s => s.id === store.currentScene.id);
        if (scene) {
          store.currentScene = scene;
          const shot = scene.shots.find(s => s.id === store.currentShot.id);
          if (shot) {
            store.currentShot = JSON.parse(JSON.stringify(shot));
          }
        }
      }
    }
  } catch (error) {
    console.error('上传失败:', error);
    alert('上传失败');
  } finally {
    // 清空 input，允许重复上传同名文件
    if (videoInput.value) videoInput.value.value = '';
  }
};

// 处理删除镜头
const handleDeleteShot = async (shotId) => {
  if (!confirm('确定要删除这个镜头吗？')) return;
  
  try {
    await api.deleteShot(shotId);
    
    // 如果删除的是当前选中的镜头，清空选中状态
    if (store.currentShot?.id === shotId) {
      store.currentShot = null;
    }
    
    // 刷新数据
    await store.fetchScript();
    
    // 重新定位回当前 Scene (因为 fetchScript 会重置整个树)
    if (store.currentScene) {
      const ep = store.episodes.find(e => e.scenes.some(s => s.id === store.currentScene.id));
      if (ep) {
         store.currentScene = ep.scenes.find(s => s.id === store.currentScene.id);
      }
    }
  } catch (e) {
    console.error(e);
    alert('删除失败');
  }
};

// 处理删除场次
const handleDeleteScene = async () => {
  if (!store.currentScene) return;
  if (!confirm(`确定要删除 "${store.currentScene.title}" 及其所有镜头吗？`)) return;

  try {
    await api.deleteScene(store.currentScene.id);
    store.currentScene = null;
    store.currentShot = null;
    await store.fetchScript();
  } catch (e) {
    console.error(e);
    alert('删除失败');
  }
};
</script>