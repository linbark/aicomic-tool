<template>
   <div class="flex h-full" v-if="store.currentProjectId">
     
     <!-- 中间栏：Episode 选中时显示 Scene 列表；Scene 选中时显示 Shot 列表 -->
     <div class="w-80 bg-white border-r border-gray-200 flex flex-col flex-shrink-0">
       <!-- Episode -> Scene 列表 -->
       <div v-if="store.currentEpisode && !store.currentScene" class="flex flex-col h-full">
         <div class="p-3 border-b bg-gray-50 flex justify-between items-center">
           <span class="font-bold text-gray-700 truncate w-56" :title="store.currentEpisode.title">
             {{ store.currentEpisode.title }}
           </span>
           <span class="text-[10px] text-gray-400">Scenes</span>
         </div>
         <div class="flex-1 overflow-y-auto p-3 space-y-2">
           <div v-for="scene in (store.currentEpisode.scenes || [])" :key="scene.id"
                @click="store.selectScene(scene)"
                :class="['px-3 py-2 rounded border cursor-pointer transition flex items-center justify-between group',
                         store.currentScene?.id === scene.id ? 'border-blue-500 bg-blue-50' : 'border-gray-200 hover:border-blue-300 bg-white']">
             <div class="min-w-0">
               <div class="text-xs font-bold text-gray-800 truncate">
                 <span class="text-[10px] text-gray-400 mr-2">#{{ scene.sequence_number }}</span>
                 {{ scene.title }}
               </div>
               <div class="text-[10px] text-gray-400 truncate" v-if="scene.description">{{ scene.description }}</div>
             </div>
             <span class="text-gray-300 group-hover:text-blue-500 transition text-xs">›</span>
           </div>
           <div v-if="!store.currentEpisode.scenes || store.currentEpisode.scenes.length === 0" class="text-center text-gray-300 text-xs py-10">
             暂无场景，请在左侧点击 “+ Add Scene”
           </div>
         </div>
       </div>

       <!-- Scene -> Shot 列表（保持原逻辑） -->
       <div v-else-if="store.currentScene" class="flex flex-col h-full">
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
               <div class="text-[10px] text-gray-500 truncate" v-if="shot.action_text">{{ shot.action_text }}</div>
            </div>
            
            <div v-if="!store.currentScene.shots || store.currentScene.shots.length === 0" class="text-center text-gray-300 text-xs py-10">
               暂无镜头，点击右上角添加
            </div>
         </div>
       </div>

       <!-- 未选择任何内容 -->
       <div v-else class="h-full bg-gray-50 flex items-center justify-center text-gray-400 border-r text-xs">
         请从左侧选择一集或一场
       </div>
     </div>
 
     <!-- Episode 详情（当选中 Episode 但未选中 Scene 时） -->
     <div class="flex-1 flex flex-col bg-gray-50" v-if="store.currentEpisode && !store.currentScene">
       <div class="p-4 bg-white border-b shadow-sm flex justify-between items-center">
         <div class="flex items-center gap-3 min-w-0">
           <span class="bg-gray-100 px-2 py-1 rounded text-xs font-mono text-gray-600">EP{{ store.currentEpisode.order }}</span>
           <span class="font-bold text-gray-800 truncate" :title="store.currentEpisode.title">{{ store.currentEpisode.title }}</span>
         </div>
         <button @click="saveEpisodeScript" class="bg-blue-600 text-white px-4 py-1.5 rounded text-xs hover:bg-blue-700 transition shadow-sm">
           保存修改
         </button>
       </div>

       <div class="flex-1 p-6 overflow-y-auto">
         <div class="flex items-center justify-between mb-2">
           <label class="block text-[10px] font-bold text-gray-400 uppercase">剧本内容</label>
           <button
             @click="handleAutoSplit"
             :disabled="isSplitting || !episodeDescription.trim()"
             class="text-xs px-3 py-1 rounded border bg-white hover:bg-gray-50 text-gray-600 disabled:opacity-50 disabled:cursor-not-allowed"
             title="自动分场"
           >
             {{ isSplitting ? '分场中...' : '自动分场' }}
           </button>
         </div>
         <textarea
           v-model="episodeDescription"
           class="w-full h-[40vh] text-sm bg-white border rounded p-3 outline-none resize-none focus:ring-2 ring-blue-500"
           placeholder="编写本集的剧本内容..."
         ></textarea>

         <div class="mt-4">
           <div class="flex items-center justify-between mb-2">
             <label class="block text-[10px] font-bold text-gray-400 uppercase">
               分场预览
               <span v-if="splitScenesPreview.length" class="ml-2 text-gray-400 font-normal">({{ splitScenesPreview.length }} 场)</span>
             </label>
             <div class="flex items-center gap-3">
               <label class="flex items-center gap-2 text-xs text-gray-500 select-none">
                 <input type="checkbox" v-model="overwriteOnImport" class="accent-blue-600">
                 覆盖导入（先清空本集已有场）
               </label>
               <button
                 @click="handleImportScenes"
                 :disabled="isImporting || splitScenesPreview.length === 0 || !store.currentEpisode?.id"
                 class="text-xs px-3 py-1 rounded bg-blue-600 hover:bg-blue-700 text-white disabled:opacity-50 disabled:cursor-not-allowed"
                 title="一键导入为场"
               >
                 {{ isImporting ? '导入中...' : '一键导入' }}
               </button>
             </div>
           </div>

           <div v-if="splitError" class="text-xs text-red-600 mb-2">{{ splitError }}</div>
           <div v-if="splitScenesPreview.length === 0" class="text-xs text-gray-300 border border-dashed rounded p-4 bg-white">
             点击右上角“自动分场”生成预览，然后点击“一键导入”创建场景。
           </div>

           <div v-else class="space-y-3 max-h-[28vh] overflow-y-auto pr-1">
             <div v-for="(sc, idx) in splitScenesPreview" :key="sc._key" class="bg-white border rounded p-3">
               <div class="flex items-center gap-2 mb-2">
                 <span class="text-[10px] text-gray-400 font-mono">SC{{ idx + 1 }}</span>
                 <input
                   v-model="sc.title"
                   class="flex-1 border rounded px-2 py-1 text-sm outline-none focus:ring-2 ring-blue-500"
                   placeholder="场标题"
                 />
               </div>
               <textarea
                 v-model="sc.description"
                 class="w-full border rounded p-2 text-sm outline-none resize-none focus:ring-2 ring-blue-500"
                 rows="4"
                 placeholder="该场的文本内容..."
               ></textarea>
             </div>
           </div>
         </div>
       </div>
     </div>

     <!-- Scene 剧本（当选中 Scene 但没有选中 Shot 时） -->
     <div class="flex-1 flex flex-col bg-gray-50" v-else-if="store.currentScene && !store.currentShot">
        <div class="p-4 bg-white border-b shadow-sm flex justify-between items-center">
           <div class="flex items-center gap-3 min-w-0">
              <span class="bg-gray-100 px-2 py-1 rounded text-xs font-mono text-gray-600">SCENE-{{ store.currentScene.sequence_number }}</span>
              <span class="font-bold text-gray-800 truncate" :title="store.currentScene.title">{{ store.currentScene.title }}</span>
           </div>
           <button @click="saveSceneScript" class="bg-blue-600 text-white px-4 py-1.5 rounded text-xs hover:bg-blue-700 transition shadow-sm">
             保存修改
           </button>
        </div>

        <div class="flex-1 p-6 overflow-y-auto">
          <div class="flex items-center justify-between mb-2">
            <label class="block text-[10px] font-bold text-gray-400 uppercase">剧本内容</label>
            <button
              @click="handleAutoStoryboard"
              :disabled="isStoryboardSplitting || !sceneDescription.trim()"
              class="text-xs px-3 py-1 rounded border bg-white hover:bg-gray-50 text-gray-600 disabled:opacity-50 disabled:cursor-not-allowed"
              title="自动分镜"
            >
              {{ isStoryboardSplitting ? '分镜中...' : '自动分镜' }}
            </button>
          </div>
          <textarea
            v-model="sceneDescription"
            class="w-full h-[40vh] text-sm bg-white border rounded p-3 outline-none resize-none focus:ring-2 ring-blue-500"
            placeholder="编写本场的剧本内容..."
          ></textarea>

          <div class="mt-4">
            <div class="flex items-center justify-between mb-2">
              <label class="block text-[10px] font-bold text-gray-400 uppercase">
                分镜预览
                <span v-if="storyboardPreview.length" class="ml-2 text-gray-400 font-normal">({{ storyboardPreview.length }} 镜)</span>
              </label>
              <div class="flex items-center gap-3">
                <label class="flex items-center gap-2 text-xs text-gray-500 select-none">
                  <input type="checkbox" v-model="overwriteShotsOnImport" class="accent-blue-600">
                  覆盖导入（先清空本场已有镜头）
                </label>
                <button
                  @click="handleImportShots"
                  :disabled="isStoryboardImporting || storyboardPreview.length === 0 || !store.currentScene?.id"
                  class="text-xs px-3 py-1 rounded bg-blue-600 hover:bg-blue-700 text-white disabled:opacity-50 disabled:cursor-not-allowed"
                  title="一键导入为镜头"
                >
                  {{ isStoryboardImporting ? '导入中...' : '一键导入' }}
                </button>
              </div>
            </div>

            <div v-if="storyboardError" class="text-xs text-red-600 mb-2">{{ storyboardError }}</div>
            <div v-if="storyboardPreview.length === 0" class="text-xs text-gray-300 border border-dashed rounded p-4 bg-white">
              点击右上角“自动分镜”生成预览，然后点击“一键导入”创建镜头。
            </div>

            <div v-else class="space-y-3 max-h-[28vh] overflow-y-auto pr-1">
              <div v-for="(sh, idx) in storyboardPreview" :key="sh._key" class="bg-white border rounded p-3">
                <div class="flex items-center gap-2 mb-2">
                  <span class="text-[10px] text-gray-400 font-mono">SHOT{{ idx + 1 }}</span>
                  <input
                    v-model="sh.title"
                    class="flex-1 border rounded px-2 py-1 text-sm outline-none focus:ring-2 ring-blue-500"
                    placeholder="镜头标题（可选）"
                  />
                </div>
                <textarea
                  v-model="sh.action_text"
                  class="w-full border rounded p-2 text-sm outline-none resize-none focus:ring-2 ring-blue-500"
                  rows="4"
                  placeholder="镜头内容（将写入 Action）..."
                ></textarea>
              </div>
            </div>
          </div>
        </div>
     </div>

     <!-- Shot 编辑界面 -->
     <div class="flex-1 flex flex-col bg-gray-50" v-else-if="store.currentShot">
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
              <textarea v-model="store.currentShot.action_text" class="w-full h-32 text-sm outline-none resize-none placeholder-gray-300" placeholder="描述发生了什么..."></textarea>
           </div>
           <div class="p-4 bg-gray-50">
              <label class="block text-[10px] font-bold text-gray-400 uppercase mb-2">Stable Diffusion Prompt</label>
              <textarea v-model="store.currentShot.prompt" class="w-full h-32 text-xs font-mono bg-transparent outline-none resize-none text-gray-600 placeholder-gray-300" placeholder="English prompt here..."></textarea>
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
 
          <div v-if="store.currentShot.video_path" class="w-full bg-black rounded overflow-hidden aspect-[21/9] relative group">
             <video controls class="w-full h-full object-contain" :src="getFileUrl(store.currentShot.video_path)"></video>
          </div>
        </div>
 
        <div class="flex-1 p-6 overflow-y-auto">
           <div class="flex justify-between items-center mb-4">
              <h3 class="font-bold text-gray-700 text-xs uppercase flex items-center gap-2">
                 <span>Assets Library</span>
                 <span class="bg-gray-200 text-gray-600 px-1.5 py-0.5 rounded-full text-[10px]">{{ store.currentShot.assets?.length || 0 }}</span>
              </h3>
              <div class="flex gap-2 items-center">
                 <input v-model="newAssetPath" placeholder="Paste local path..." class="border rounded px-2 py-1 text-xs w-32">
                 <button @click="addAsset" class="bg-white border hover:bg-gray-50 text-gray-600 px-2 py-1 rounded text-xs">Link</button>
                 
                 <label class="cursor-pointer bg-blue-50 border border-blue-200 hover:bg-blue-100 text-blue-600 px-2 py-1 rounded text-xs flex items-center gap-1 transition">
                     <span>+ Upload</span>
                     <input type="file" multiple class="hidden" @change="handleShotAssetUpload">
                 </label>
              </div>
           </div>
           
           <div class="grid grid-cols-3 xl:grid-cols-4 gap-4">
              <div v-for="asset in (store.currentShot.assets || [])" :key="asset.id" class="group relative aspect-video bg-gray-100 rounded border overflow-hidden hover:shadow-md transition cursor-pointer" @click="openFile(asset)">
                 
                 <img v-if="asset.file_type === 'image'" :src="getFileUrl(asset.file_path)" class="w-full h-full object-cover">
                 
                 <div v-else-if="asset.file_type === 'video'" class="w-full h-full relative">
                      <video :src="getFileUrl(asset.file_path)" class="w-full h-full object-cover"></video>
                      <div class="absolute inset-0 flex items-center justify-center bg-black/20">
                          <span class="text-white text-xs">▶</span>
                      </div>
                 </div>
             
                 <div v-else class="w-full h-full flex flex-col items-center justify-center p-2 text-gray-400">
                     <span class="text-2xl">📄</span>
                     <span class="text-[8px] mt-1 break-all text-center leading-tight">{{ getFileName(asset.file_path) }}</span>
                 </div>
                 
                 <div class="absolute inset-0 bg-black/60 opacity-0 group-hover:opacity-100 transition flex items-end p-2">
                    <span class="text-white text-[10px] truncate w-full">{{ getFileName(asset.file_path) }}</span>
                 </div>
              </div>
           </div>
        </div>
     </div>
     
     <div v-else class="flex-1 flex items-center justify-center text-gray-300">
        <div class="text-center">
           <div class="text-4xl mb-2">🎬</div>
           <p>请从左侧选择一集或一场戏</p>
        </div>
     </div>
     
   </div>
 </template>
 
 <script setup>
import { ref, watch } from 'vue';
import { useProjectStore } from '../stores/projectStore';
import api, { getFileUrl as buildFileUrl } from '../api/client';

const store = useProjectStore();
const newAssetPath = ref('');
const videoInput = ref(null);

// Episode/Scene 剧本内容编辑（仅 description）
const episodeDescription = ref('');
const sceneDescription = ref('');

// Episode 自动分场：预览与导入
const splitScenesPreview = ref([]); // [{ _key, title, description }]
const splitError = ref('');
const isSplitting = ref(false);
const isImporting = ref(false);
const overwriteOnImport = ref(false);

// Scene 自动分镜：预览与导入（Shot）
const storyboardPreview = ref([]); // [{ _key, title, action_text }]
const storyboardError = ref('');
const isStoryboardSplitting = ref(false);
const isStoryboardImporting = ref(false);
const overwriteShotsOnImport = ref(false);

watch(() => store.currentEpisode, (ep) => {
  if (ep && !store.currentScene) {
    episodeDescription.value = ep.description || '';
  }
  // 切换集时清理预览，避免混淆
  splitScenesPreview.value = [];
  splitError.value = '';
  overwriteOnImport.value = false;
}, { immediate: true });

watch(() => store.currentScene, (sc) => {
  if (sc) {
    sceneDescription.value = sc.description || '';
  }
  storyboardPreview.value = [];
  storyboardError.value = '';
  overwriteShotsOnImport.value = false;
}, { immediate: true });
 
 // === 基础操作 ===
 
 const addShot = async () => {
    if(!store.currentScene) return;
    // 防止 undefined
    const currentCount = store.currentScene.shots ? store.currentScene.shots.length : 0;
    const nextSeq = currentCount + 1;
    
    try {
      await api.createShot(store.currentScene.id, { 
         sequence_number: nextSeq,
         action_text: '' 
      });
      await store.fetchScript();
      refreshCurrentSceneRef();
    } catch(e) {
      console.error(e);
      alert("添加镜头失败");
    }
 };
 
const save = async () => {
  if(store.currentShot) {
    await store.saveShot(store.currentShot);
    alert('已保存');
  }
};

const saveEpisodeScript = async () => {
  if (!store.currentEpisode?.id) return;
  try {
    await api.updateEpisode(store.currentEpisode.id, { description: episodeDescription.value });
    await store.fetchScript();
    alert('已保存');
  } catch (error) {
    console.error('保存集失败:', error);
    alert('保存失败: ' + (error?.response?.data?.detail || error?.message || '未知错误'));
  }
};

const splitEpisodeToScenes = (text) => {
  const normalized = (text || '').replace(/\r\n/g, '\n').trim();
  if (!normalized) return [];

  const isSeparator = (line) => {
    const t = (line || '').trim();
    return t === '---' || t === '===' || t === '———' || /^-{3,}$/.test(t) || /^={3,}$/.test(t);
  };

  // 形如：场1：xx / 场景 2 - xx / Scene 3: xx / 【场】xx
  const headerRe = /^\s*(?:【\s*)?(?:场景|场|Scene)\s*([0-9一二三四五六七八九十]+)?\s*(?:[：:\-—]\s*)?(.+?)?\s*(?:】\s*)?$/i;
  const lines = normalized.split('\n');
  const hasHeader = lines.some(l => headerRe.test(l.trim()));

  const out = [];
  if (hasHeader) {
    let current = null;
    for (const rawLine of lines) {
      const line = rawLine ?? '';
      const trimmed = line.trim();
      if (isSeparator(trimmed)) {
        if (current) {
          const desc = current.lines.join('\n').trim();
          if (desc || current.title) out.push({ title: current.title, description: desc });
        }
        current = null;
        continue;
      }
      const m = trimmed.match(headerRe);
      if (m) {
        if (current) {
          const desc = current.lines.join('\n').trim();
          if (desc || current.title) out.push({ title: current.title, description: desc });
        }
        const titleRaw = (m[2] || '').trim();
        const nextIndex = out.length + 1;
        current = { title: titleRaw || `场${nextIndex}`, lines: [] };
      } else {
        if (!current) current = { title: `场${out.length + 1}`, lines: [] };
        current.lines.push(line);
      }
    }
    if (current) {
      const desc = current.lines.join('\n').trim();
      if (desc || current.title) out.push({ title: current.title, description: desc });
    }
    return out.filter(s => (s.description || '').trim().length > 0);
  }

  // 无明确“场标题行”时：按空行块切分
  const blocks = normalized
    .split(/\n{2,}/)
    .map(b => b.trim())
    .filter(Boolean);

  return blocks.map((block, i) => {
    const bLines = block.split('\n');
    const m = bLines[0]?.trim().match(headerRe);
    if (m) {
      const titleRaw = (m[2] || '').trim();
      return {
        title: titleRaw || `场${i + 1}`,
        description: bLines.slice(1).join('\n').trim(),
      };
    }
    return { title: `场${i + 1}`, description: block };
  }).filter(s => (s.description || '').trim().length > 0);
};

const handleAutoSplit = async () => {
  if (isSplitting.value) return;
  splitError.value = '';
  isSplitting.value = true;
  try {
    const text = episodeDescription.value;
    let scenes = [];
    // 优先走 AI，失败回退到本地规则
    try {
      const { data } = await api.aiSplitScenes({ text, max_scenes: 50 });
      scenes = (data || []).map((it, idx) => ({
        title: (it.title || `场${idx + 1}`).trim(),
        description: (it.description || '').trim(),
      })).filter(s => s.description || s.title);
    } catch (e) {
      scenes = splitEpisodeToScenes(text);
    }

    if (!scenes.length) {
      splitScenesPreview.value = [];
      splitError.value = '未能分出场景：请确认剧本内容不为空。';
      return;
    }
    splitScenesPreview.value = scenes.map((s, idx) => ({
      _key: `${Date.now()}-${idx}`,
      title: (s.title || `场${idx + 1}`).trim(),
      description: (s.description || '').trim(),
    }));
  } catch (e) {
    console.error('自动分场失败:', e);
    splitError.value = '自动分场失败：' + (e?.message || '未知错误');
    splitScenesPreview.value = [];
  } finally {
    isSplitting.value = false;
  }
};

const handleImportScenes = async () => {
  if (isImporting.value) return;
  if (!store.currentEpisode?.id) return;
  if (!splitScenesPreview.value.length) return;

  splitError.value = '';
  isImporting.value = true;

  try {
    if (overwriteOnImport.value && (store.currentEpisode.scenes?.length || 0) > 0) {
      const ok = confirm('⚠️ 覆盖导入将删除本集已有的所有场与镜头及相关文件，且不可撤销。确定继续吗？');
      if (!ok) return;

      // 先删除已有 scenes（后端会级联删除 shots 并清理文件）
      for (const existing of (store.currentEpisode.scenes || [])) {
        await api.deleteScene(existing.id);
      }
      await store.fetchScript();
      // 重新定位当前 episode（避免引用变化）
      store.selectEpisode(store.currentEpisode.id);
    }

    for (let i = 0; i < splitScenesPreview.value.length; i++) {
      const sc = splitScenesPreview.value[i];
      const title = (sc.title || `场${i + 1}`).trim();
      const description = (sc.description || '').trim();

      const created = await api.createScene(store.currentEpisode.id, { title });
      const sceneId = created?.data?.id;
      if (sceneId && description) {
        await api.updateScene(sceneId, { description });
      }
    }
    await store.fetchScript();
    // 保持停留在当前 Episode（不自动跳入某个 Scene）
    store.selectEpisode(store.currentEpisode.id);
    alert('导入完成');
  } catch (e) {
    console.error('导入失败:', e);
    splitError.value = '导入失败：' + (e?.response?.data?.detail || e?.message || '未知错误');
  } finally {
    isImporting.value = false;
  }
};

const splitSceneToShots = (text) => {
  const normalized = (text || '').replace(/\r\n/g, '\n').trim();
  if (!normalized) return [];

  const isSeparator = (line) => {
    const t = (line || '').trim();
    return t === '---' || t === '===' || t === '———' || /^-{3,}$/.test(t) || /^={3,}$/.test(t);
  };

  // 形如：镜头1：xx / 镜头 2 - xx / Shot 3: xx / 【镜头】xx
  const headerRe = /^\s*(?:【\s*)?(?:镜头|Shot)\s*([0-9一二三四五六七八九十]+)?\s*(?:[：:\-—]\s*)?(.+?)?\s*(?:】\s*)?$/i;
  const lines = normalized.split('\n');
  const hasHeader = lines.some(l => headerRe.test(l.trim()));

  const out = [];
  if (hasHeader) {
    let current = null;
    for (const rawLine of lines) {
      const line = rawLine ?? '';
      const trimmed = line.trim();
      if (isSeparator(trimmed)) {
        if (current) {
          const action = current.lines.join('\n').trim();
          if (action || current.title) out.push({ title: current.title, action_text: action });
        }
        current = null;
        continue;
      }
      const m = trimmed.match(headerRe);
      if (m) {
        if (current) {
          const action = current.lines.join('\n').trim();
          if (action || current.title) out.push({ title: current.title, action_text: action });
        }
        const titleRaw = (m[2] || '').trim();
        current = { title: titleRaw || '', lines: [] };
      } else {
        if (!current) current = { title: '', lines: [] };
        current.lines.push(line);
      }
    }
    if (current) {
      const action = current.lines.join('\n').trim();
      if (action || current.title) out.push({ title: current.title, action_text: action });
    }
    return out.filter(s => (s.action_text || '').trim().length > 0);
  }

  // 无明确“镜头标题行”时：按空行块切分
  const blocks = normalized
    .split(/\n{2,}/)
    .map(b => b.trim())
    .filter(Boolean);

  return blocks.map((block, i) => {
    const bLines = block.split('\n');
    const m = bLines[0]?.trim().match(headerRe);
    if (m) {
      const titleRaw = (m[2] || '').trim();
      return {
        title: titleRaw || '',
        action_text: bLines.slice(1).join('\n').trim(),
      };
    }
    return { title: '', action_text: block };
  }).filter(s => (s.action_text || '').trim().length > 0);
};

const handleAutoStoryboard = async () => {
  if (isStoryboardSplitting.value) return;
  storyboardError.value = '';
  isStoryboardSplitting.value = true;
  try {
    const text = sceneDescription.value;
    let shots = [];
    // 优先走 AI，失败回退到本地规则
    try {
      const { data } = await api.aiSplitShots({ text, max_shots: 80 });
      shots = (data || []).map((it) => ({
        title: (it.title || '').trim(),
        action_text: (it.action_text || '').trim(),
      })).filter(s => s.action_text);
    } catch (e) {
      shots = splitSceneToShots(text);
    }

    if (!shots.length) {
      storyboardPreview.value = [];
      storyboardError.value = '未能分出镜头：请确认剧本内容不为空。';
      return;
    }
    storyboardPreview.value = shots.map((s, idx) => ({
      _key: `${Date.now()}-${idx}`,
      title: (s.title || '').trim(),
      action_text: (s.action_text || '').trim(),
    }));
  } catch (e) {
    console.error('自动分镜失败:', e);
    storyboardError.value = '自动分镜失败：' + (e?.message || '未知错误');
    storyboardPreview.value = [];
  } finally {
    isStoryboardSplitting.value = false;
  }
};

const handleImportShots = async () => {
  if (isStoryboardImporting.value) return;
  if (!store.currentScene?.id) return;
  if (!storyboardPreview.value.length) return;

  storyboardError.value = '';
  isStoryboardImporting.value = true;

  try {
    if (overwriteShotsOnImport.value && (store.currentScene.shots?.length || 0) > 0) {
      const ok = confirm('⚠️ 覆盖导入将删除本场已有的所有镜头及相关文件，且不可撤销。确定继续吗？');
      if (!ok) return;

      for (const existing of (store.currentScene.shots || [])) {
        await api.deleteShot(existing.id);
      }
      store.currentShot = null;
      await store.fetchScript();
      store.selectScene(store.currentScene.id);
    }

    const baseSeq = overwriteShotsOnImport.value ? 1 : ((store.currentScene.shots?.length || 0) + 1);
    for (let i = 0; i < storyboardPreview.value.length; i++) {
      const sh = storyboardPreview.value[i];
      const title = (sh.title || '').trim() || null;
      const action_text = (sh.action_text || '').trim();
      // action_text 在后端模型里是必填（Text 非 nullable），这里保证不为空
      const safeAction = action_text || ' ';

      await api.createShot(store.currentScene.id, {
        sequence_number: baseSeq + i,
        title,
        action_text: safeAction,
        prompt: ''
      });
    }

    await store.fetchScript();
    store.selectScene(store.currentScene.id);
    alert('导入完成');
  } catch (e) {
    console.error('导入镜头失败:', e);
    storyboardError.value = '导入失败：' + (e?.response?.data?.detail || e?.message || '未知错误');
  } finally {
    isStoryboardImporting.value = false;
  }
};
const saveSceneScript = async () => {
  if (!store.currentScene?.id) return;
  try {
    await api.updateScene(store.currentScene.id, { description: sceneDescription.value });
    await store.fetchScript();
    refreshCurrentSceneRef();
    alert('已保存');
  } catch (error) {
    console.error('保存场失败:', error);
    alert('保存失败: ' + (error?.response?.data?.detail || error?.message || '未知错误'));
  }
};
 
 // === 资产管理 ===
 
 const addAsset = async () => {
   if(newAssetPath.value && store.currentShot) {
     try {
         await api.addAsset(store.currentShot.id, newAssetPath.value);
         await store.fetchScript();
         refreshCurrentShotRef();
         newAssetPath.value = '';
     } catch(e) {
         alert("添加失败");
     }
   }
 };
 
 const handleShotAssetUpload = async (event) => {
   const files = event.target.files;
   if (!files || files.length === 0) return;
   if (!store.currentShot) return;
 
   for (let i = 0; i < files.length; i++) {
     const file = files[i];
     const formData = new FormData();
     formData.append('file', file);
     
     try {
         await api.uploadShotAsset(store.currentShot.id, formData);
     } catch (e) {
         console.error("Asset upload failed", e);
         alert(`文件 ${file.name} 上传失败`);
     }
   }
   
   await store.fetchScript();
   refreshCurrentShotRef();
   event.target.value = ''; 
 };
 
 // === 视频专用处理 ===
 
 const handleVideoUpload = async (event) => {
   const file = event.target.files[0];
   if (!file) return;
   if (!store.currentShot) return;
 
   const formData = new FormData();
   formData.append('file', file);
 
   try {
     const { data } = await api.uploadShotVideo(store.currentShot.id, formData);
     // 更新本地视图
     store.currentShot.video_path = data.video_path;
     alert('视频上传成功！');
     await store.fetchScript();
     refreshCurrentShotRef();
   } catch (error) {
     console.error('上传失败:', error);
     alert('上传失败');
   } finally {
     if (videoInput.value) videoInput.value.value = '';
   }
 };
 
 // === 删除逻辑 ===
 
 const handleDeleteShot = async (shotId) => {
   if (!confirm('确定要删除这个镜头吗？')) return;
   
   try {
     await api.deleteShot(shotId);
     
     if (store.currentShot?.id === shotId) {
       store.currentShot = null;
     }
     
     await store.fetchScript();
     refreshCurrentSceneRef();
   } catch (e) {
     console.error(e);
     alert('删除失败');
   }
 };
 
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
 
 // === 工具函数 ===
 
 // 刷新当前Scene引用 (因为 store.episodes 整个被替换了)
 const refreshCurrentSceneRef = () => {
     if (store.currentScene) {
       const ep = store.episodes.find(e => e.scenes.some(s => s.id === store.currentScene.id));
       if (ep) {
          store.currentScene = ep.scenes.find(s => s.id === store.currentScene.id);
       } else {
          store.currentScene = null;
       }
     }
 }
 
 // 刷新当前Shot引用
 const refreshCurrentShotRef = () => {
     refreshCurrentSceneRef();
     if (store.currentScene && store.currentShot) {
         const shot = store.currentScene.shots.find(s => s.id === store.currentShot.id);
         if (shot) {
             store.currentShot = JSON.parse(JSON.stringify(shot));
         } else {
             store.currentShot = null;
         }
     }
 }
 
 const getFileUrl = (path) => buildFileUrl(path);
 
 const getFileName = (path) => {
     if (!path) return '';
     return path.split(/[\/\\]/).pop();
 }
 const openFile = (asset) => {
     window.open(getFileUrl(asset.file_path), '_blank');
 }
 </script>