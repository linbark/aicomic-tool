<template>
    <div class="flex flex-col h-full bg-gray-50">
      <div class="p-6 pb-4 flex justify-between items-center bg-white border-b sticky top-0 z-10">
          <div>
              <h2 class="text-xl font-bold text-gray-800">角色资产库</h2>
              <p class="text-xs text-gray-400">Project Character Assets</p>
          </div>
          <button @click="openEditModal(null)" class="bg-blue-600 hover:bg-blue-700 text-white px-3 py-1.5 rounded text-xs flex items-center gap-2 transition">
              <span>+</span> 新建角色
          </button>
      </div>
  
      <div class="flex-1 overflow-y-auto p-6 space-y-6">
          
          <div v-if="(store.characters || []).length === 0" class="text-center text-gray-400 py-20 border-2 border-dashed rounded-xl">
              暂无角色，请点击右上角创建
          </div>
  
          <div v-for="char in (store.characters || [])" :key="char.id" class="bg-white rounded-xl border border-gray-200 shadow-sm flex overflow-hidden min-h-[200px]">
              
              <div class="w-1/3 min-w-[280px] bg-gray-50 p-5 border-r border-gray-100 flex flex-col relative group">
                  <div class="absolute top-2 right-2 flex gap-2 opacity-0 group-hover:opacity-100 transition">
                      <button @click="openEditModal(char)" class="p-1 text-gray-400 hover:text-blue-600" title="编辑内容">
                          <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"></path><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"></path></svg>
                      </button>
                      <button @click="deleteChar(char)" class="p-1 text-gray-400 hover:text-red-500" title="删除角色及文件">
                          <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>
                      </button>
                  </div>
                  
                  <div class="mb-4 mt-1">
                      <h3 class="text-lg font-bold text-gray-800">{{ char.name }}</h3>
                      <p class="text-[10px] text-gray-400 mt-1">Asset Count: {{ (char.assets || []).length }}</p>
                  </div>
                  
                  <div class="space-y-3 flex-1">
                      <div>
                          <label class="text-[10px] font-bold text-gray-400 uppercase">Description</label>
                          <p class="text-xs text-gray-600 leading-relaxed line-clamp-4 bg-white p-2 rounded border border-gray-100 mt-1">
                              {{ char.description || '暂无描述...' }}
                          </p>
                      </div>
                      <div>
                          <label class="text-[10px] font-bold text-gray-400 uppercase">Base Prompt</label>
                          <p class="text-[10px] text-blue-600 font-mono bg-blue-50 p-2 rounded border border-blue-100 mt-1 break-all line-clamp-2">
                              {{ char.base_prompt || 'N/A' }}
                          </p>
                      </div>
                  </div>
  
                  <div class="mt-4 pt-4 border-t border-gray-200">
                    <label class="cursor-pointer bg-white border border-dashed border-gray-300 hover:border-blue-500 hover:text-blue-600 text-gray-500 text-xs rounded py-2 w-full flex items-center justify-center gap-2 transition">
                        <span>📤 上传素材 (图/视/文)</span>
                        <input type="file" class="hidden" multiple accept="image/*,video/*,.txt,.md,.pdf,.doc,.docx" @change="(e) => handleUpload(char.id, e)">
                    </label>
                  </div>
              </div>
  
              <<div class="flex-1 bg-white min-w-0 flex flex-col border-l border-gray-100 justify-center">
                
                <div v-if="!char.assets || char.assets.length === 0" class="flex-1 flex items-center justify-center text-gray-300 text-xs italic p-5">
                    暂无图片或视频，请从左侧上传
                </div>
                
                <div v-else class="w-full overflow-x-auto p-4 flex gap-3 items-center">
                    
                    <div v-for="asset in (char.assets || [])" :key="asset.id" 
                        class="h-48 aspect-[3/4] flex-shrink-0 rounded-lg overflow-hidden border border-gray-200HV relative group cursor-pointer bg-gray-100 shadow-sm"
                        @click="openLightbox(asset)">
                        
                        <img v-if="asset.file_type === 'image'" :src="getFileUrl(asset.file_path)" class="w-full h-full object-cover transition duration-300 group-hover:scale-105">
                        
                        <video v-else-if="asset.file_type === 'video'" :src="getFileUrl(asset.file_path)" class="w-full h-full object-cover" preload="metadata" muted></video>
                        
                        <div v-else class="w-full h-full flex flex-col items-center justify-center bg-gray-50 text-gray-500 p-4">
                            <span class="text-4xl mb-2">📄</span>
                            <span class="text-[10px] break-all text-center line-clamp-3">{{ asset.file_path.split('/').pop() }}</span>
                        </div>

                        <div v-if="asset.file_type === 'video'" class="absolute inset-0 flex items-center justify-center pointer-events-none">
                            <div class="bg-black/50 rounded-full p-2 backdrop-blur-sm">
                                <span class="text-white text-xs">▶</span>
                            </div>
                        </div>

                        <!-- 删除按钮 (Hover显示) -->
                        <div class="absolute top-2 right-2 opacity-0 group-hover:opacity-100 transition z-10">
                            <button @click.stop="deleteAsset(asset)" class="bg-white/90 hover:bg-red-500 hover:text-white text-gray-500 rounded-full p-1.5 shadow-sm backdrop-blur-sm transition-colors" title="删除文件">
                                <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>
                            </button>
                        </div>

                    </div>

                </div>
            </div>
          </div>
      </div>
  
      <div v-if="showModal" class="fixed inset-0 bg-black/50 z-50 flex items-center justify-center backdrop-blur-sm">
          <div class="bg-white rounded-xl shadow-2xl w-96 p-6">
              <h3 class="text-lg font-bold text-gray-800 mb-4">{{ isEditing ? '编辑角色' : '新建角色' }}</h3>
              <div class="space-y-4">
                  <div>
                      <label class="text-xs font-bold text-gray-500">Name</label>
                      <input v-model="form.name" class="w-full border p-2 rounded text-sm focus:ring-2 ring-blue-500 outline-none">
                  </div>
                  <div>
                      <label class="text-xs font-bold text-gray-500">Description</label>
                      <textarea v-model="form.description" class="w-full border p-2 rounded text-sm h-24 focus:ring-2 ring-blue-500 outline-none resize-none"></textarea>
                  </div>
                  <div>
                      <label class="text-xs font-bold text-gray-500">Base Prompt</label>
                      <textarea v-model="form.base_prompt" class="w-full border p-2 rounded text-xs font-mono h-24 bg-gray-50 focus:ring-2 ring-blue-500 outline-none resize-none"></textarea>
                  </div>
              </div>
              <div class="flex justify-end gap-2 mt-6">
                  <button @click="showModal = false" class="px-4 py-2 text-gray-500 hover:bg-gray-100 rounded text-sm">取消</button>
                  <button @click="submit" class="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded text-sm shadow-sm">确定</button>
              </div>
          </div>
      </div>
  
      <div v-if="lightboxAsset" class="fixed inset-0 bg-black/90 z-[60] flex items-center justify-center" @click="lightboxAsset = null">
        <div class="relative max-w-5xl max-h-screen p-4" @click.stop>
            <div class="absolute -top-10 right-0 flex gap-4">
                <button @click="deleteAssetFromLightbox(lightboxAsset)" class="text-white hover:text-red-400 text-sm flex items-center gap-1 bg-black/50 px-3 py-1 rounded backdrop-blur-sm">
                    <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>
                    删除
                </button>
                <button @click="lightboxAsset = null" class="text-white hover:text-gray-300 text-2xl leading-none">✕</button>
            </div>
            
            <img v-if="lightboxAsset.file_type === 'image'" :src="getFileUrl(lightboxAsset.file_path)" class="max-w-full max-h-[90vh] rounded shadow-2xl">
            <video v-else-if="lightboxAsset.file_type === 'video'" :src="getFileUrl(lightboxAsset.file_path)" controls autoplay class="max-w-full max-h-[90vh] rounded shadow-2xl"></video>
            
            <div v-else class="bg-white p-10 rounded text-center">
                <div class="text-6xl mb-4">📄</div>
                <p class="mb-4">文档文件无法直接预览</p>
                <a :href="getFileUrl(lightboxAsset.file_path)" target="_blank" class="bg-blue-600 text-white px-4 py-2 rounded text-sm hover:bg-blue-700">下载/在新标签页打开</a>
            </div>

            <div class="mt-2 text-center text-gray-400 text-xs font-mono">
                {{ lightboxAsset.file_path }}
            </div>
        </div>
      </div>
  
    </div>
  </template>
  
  <script setup>
  import { ref, onMounted, watch } from 'vue';
  import { useProjectStore } from '../stores/projectStore';
  import api from '../api/client';
  import axios from 'axios';
  
  const store = useProjectStore();
  const showModal = ref(false);
  const isEditing = ref(false);
  const form = ref({});
  const lightboxAsset = ref(null);
  
  // 初始化与监听
  onMounted(() => {
      if (store.currentProjectId) store.fetchCharacters();
  });
  
  watch(() => store.currentProjectId, (newId) => {
      if (newId) store.fetchCharacters();
      else store.characters = [];
  });
  
  // --- 增删改操作 ---
  const openEditModal = (char) => {
      if (!store.currentProjectId) return alert("请先选择项目");
      isEditing.value = !!char;
      // 如果是编辑，复制现有数据；如果是新建，清空表单
      form.value = char 
          ? { id: char.id, name: char.name, description: char.description, base_prompt: char.base_prompt } 
          : { name: '', description: '', base_prompt: '' };
      showModal.value = true;
  };
  
  const submit = async () => {
      if(!form.value.name) return;
      try {
          if(isEditing.value) {
              // 调用编辑接口
              await axios.patch(`http://localhost:8000/projects/characters/${form.value.id}`, form.value);
          } else {
              // 调用新建接口
              await axios.post(`http://localhost:8000/projects/${store.currentProjectId}/characters`, form.value);
          }
          await store.fetchCharacters();
          showModal.value = false;
      } catch(e) { 
          console.error(e);
          alert("操作失败"); 
      }
  };
  
  const deleteChar = async (char) => {
      // 提示用户这会删除文件
      if(!confirm(`⚠️ 警告：确定删除角色 "${char.name}" 吗？\n此操作将永久删除该角色下的所有图片/视频文件！`)) return;
      try {
          await axios.delete(`http://localhost:8000/projects/characters/${char.id}`);
          store.fetchCharacters();
      } catch(e) {
          alert("删除失败");
      }
  };
  
  // --- 文件上传 ---
  const handleUpload = async (charId, event) => {
      const files = event.target.files;
      if (!files || files.length === 0) return;
  
      for (let i = 0; i < files.length; i++) {
          const file = files[i];
          const formData = new FormData();
          formData.append('file', file);
          try {
              await api.uploadCharacterAsset(charId, formData);
          } catch (e) {
              console.error("Upload failed", e);
              alert(`文件 ${file.name} 上传失败`);
          }
      }
      await store.fetchCharacters();
      event.target.value = '';
  };

  const deleteAsset = async (asset) => {
    if(!confirm("确定要删除这张图片/视频吗？此操作无法撤销。")) return;
    
    try {
        // 调用后端删除接口
        await axios.delete(`http://localhost:8000/projects/assets/${asset.id}`);
        // 刷新列表
        await store.fetchCharacters();
    } catch (e) {
        console.error(e);
        alert("删除失败，请检查控制台");
    }
  };

  const deleteAssetFromLightbox = async (asset) => {
      if(!confirm("确定要删除这张图片/视频吗？此操作无法撤销。")) return;
      try {
          await axios.delete(`http://localhost:8000/projects/assets/${asset.id}`);
          await store.fetchCharacters();
          lightboxAsset.value = null;
      } catch (e) {
          console.error(e);
          alert("删除失败，请检查控制台");
      }
  };

  
  // --- 工具 ---
  const getFileUrl = (path) => {
      if (!path) return '';
      const baseUrl = 'http://localhost:8000'; 
      return `${baseUrl}/files/${path}`;
  };
  const openLightbox = (asset) => {
      lightboxAsset.value = asset;
  };
  </script>