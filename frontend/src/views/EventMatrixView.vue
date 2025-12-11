<template>
  <div class="flex flex-col h-full bg-gray-50">
    <!-- 顶部控制区 -->
    <div class="bg-white border-b px-4 py-3 flex items-center justify-between gap-3 flex-wrap shadow-sm z-10">
      <div class="flex items-center gap-3 flex-wrap">
        <span class="text-xs font-bold text-gray-500 uppercase tracking-wide">事件纵览 (Overview)</span>
        <div class="flex items-center gap-2 flex-wrap">
          <label
            v-for="evt in store.events"
            :key="evt.id"
            class="flex items-center gap-1 px-2 py-1 rounded border text-xs cursor-pointer transition select-none"
            :style="{
              borderColor: isEventSelected(evt.id) ? evt.color : '#e5e7eb',
              backgroundColor: isEventSelected(evt.id) ? 'white' : '#f9fafb',
              boxShadow: isEventSelected(evt.id) ? `0 1px 2px ${evt.color}33` : 'none'
            }"
          >
            <input type="checkbox" class="accent-blue-500 w-3 h-3" :value="evt.id" v-model="selectedEventIds" />
            <span class="w-2 h-2 rounded-full" :style="{ background: evt.color }" />
            <span class="text-gray-700 font-medium">{{ evt.name }}</span>
          </label>
        </div>
      </div>
      <div class="flex items-center gap-2 text-xs">
        <button @click="selectAll" class="px-3 py-1 rounded border bg-gray-50 hover:bg-white transition text-gray-600">全选</button>
        <button @click="clearSelection" class="px-3 py-1 rounded border bg-gray-50 hover:bg-white transition text-gray-600">清空</button>
        <div class="h-4 w-px bg-gray-300 mx-1"></div>
        <router-link 
            :to="editLink"
            class="px-3 py-1 rounded text-white transition shadow-sm flex items-center gap-1"
            :class="singleEventSelected ? 'bg-blue-600 hover:bg-blue-700' : 'bg-gray-300 cursor-not-allowed'"
            @click.prevent="!singleEventSelected && void 0"
        >
            <span>Edit Detail</span>
            <span>→</span>
        </router-link>
      </div>
    </div>

    <!-- 网格画布区域 -->
    <div class="flex-1 overflow-auto relative p-8 cursor-grab active:cursor-grabbing" ref="gridContainer" @mousedown="startDrag" @mousemove="onDrag" @mouseup="stopDrag" @mouseleave="stopDrag">
        
      <div class="inline-block relative min-w-full" :style="{ transform: `scale(${scale})`, transformOrigin: '0 0' }">
        
        <!-- 列标题 (Scene 1, Scene 2...) -->
        <div class="flex mb-4 ml-16">
             <div 
                v-for="i in maxSceneCount" 
                :key="i"
                class="w-16 flex-shrink-0 text-center text-xs font-bold text-gray-400 select-none"
                style="margin-right: 80px;" 
             >
                scene {{ i }}
             </div>
        </div>

        <!-- 行 (Episodes) -->
        <div class="space-y-16">
            <div v-for="ep in episodeRows" :key="ep.id" class="flex items-center group">
                <!-- 行头 (EP Name) -->
                <div class="w-16 flex-shrink-0 text-right pr-4 text-xs font-bold text-gray-600 select-none">
                    EP{{ ep.order }}
                </div>

                <!-- 场景点 -->
                <div class="flex items-center">
                    <div 
                        v-for="(scene, idx) in ep.scenes" 
                        :key="scene.id"
                        class="w-16 flex-shrink-0 flex justify-center relative"
                        style="margin-right: 80px;"
                        :ref="el => setSceneRef(scene.id, el)"
                    >
                        <!-- 圆点 -->
                        <div 
                            class="w-10 h-10 rounded-full flex items-center justify-center text-[10px] font-bold transition-all duration-300 z-10"
                            :class="[
                                isSceneActive(scene.id) 
                                    ? 'shadow-md scale-110 text-white' 
                                    : 'bg-gray-100 text-gray-300 border border-gray-200'
                            ]"
                            :style="getSceneStyle(scene.id)"
                            :title="`${ep.title} - ${scene.title}\n${scene.action_text || ''}`"
                        >
                            <span v-if="!isSceneActive(scene.id)">{{ idx + 1 }}</span>
                        </div>
                    </div>
                </div>
            </div>
        </div>

        <!-- 连线层 (SVG) -->
        <svg class="absolute inset-0 pointer-events-none overflow-visible" style="z-index: 0;">
             <path
                v-for="line in linePaths"
                :key="line.id"
                :d="line.d"
                :stroke="line.color"
                :stroke-width="3"
                fill="none"
                stroke-linecap="round"
                stroke-linejoin="round"
                opacity="0.6"
             />
        </svg>

      </div>
    </div>
    
    <!-- 缩放控制 -->
    <div class="absolute bottom-4 right-4 flex flex-col gap-1 bg-white p-1 rounded shadow border z-20">
        <button @click="zoomIn" class="w-8 h-8 flex items-center justify-center hover:bg-gray-100 rounded text-gray-600 font-bold">+</button>
        <button @click="zoomOut" class="w-8 h-8 flex items-center justify-center hover:bg-gray-100 rounded text-gray-600 font-bold">-</button>
        <button @click="resetZoom" class="w-8 h-8 flex items-center justify-center hover:bg-gray-100 rounded text-xs text-gray-600">1:1</button>
    </div>

  </div>
</template>

<script setup>
import { ref, computed, onMounted, nextTick, watch, onBeforeUnmount } from 'vue';
import { useProjectStore } from '../stores/projectStore';

const store = useProjectStore();
const selectedEventIds = ref([]);
const gridContainer = ref(null);
const sceneRefs = new Map();
const linePaths = ref([]);
const scale = ref(1);

// 缩放逻辑
const zoomIn = () => scale.value = Math.min(scale.value + 0.1, 2);
const zoomOut = () => scale.value = Math.max(scale.value - 0.1, 0.5);
const resetZoom = () => scale.value = 1;

// 拖拽逻辑 (简单实现)
const isDragging = ref(false);
const startX = ref(0);
const startY = ref(0);
const scrollLeft = ref(0);
const scrollTop = ref(0);

const startDrag = (e) => {
    isDragging.value = true;
    startX.value = e.pageX - gridContainer.value.offsetLeft;
    startY.value = e.pageY - gridContainer.value.offsetTop;
    scrollLeft.value = gridContainer.value.scrollLeft;
    scrollTop.value = gridContainer.value.scrollTop;
};
const onDrag = (e) => {
    if (!isDragging.value) return;
    e.preventDefault();
    const x = e.pageX - gridContainer.value.offsetLeft;
    const y = e.pageY - gridContainer.value.offsetTop;
    const walkX = (x - startX.value) * 1.5;
    const walkY = (y - startY.value) * 1.5;
    gridContainer.value.scrollLeft = scrollLeft.value - walkX;
    gridContainer.value.scrollTop = scrollTop.value - walkY;
};
const stopDrag = () => isDragging.value = false;

// 数据计算
const episodeRows = computed(() => {
    return (store.episodes || []).map(ep => ({
        ...ep,
        scenes: (ep.scenes || []).slice().sort((a, b) => (a.sequence_number || 0) - (b.sequence_number || 0))
    }));
});

const maxSceneCount = computed(() => {
    if (!episodeRows.value.length) return 5;
    return Math.max(...episodeRows.value.map(ep => ep.scenes.length), 5);
});

const singleEventSelected = computed(() => selectedEventIds.value.length === 1);
const editLink = computed(() => {
    if (singleEventSelected.value) {
        return `/events/flow?id=${selectedEventIds.value[0]}`;
    }
    return '';
});

// Scene -> Event 关联计算
const sceneEventMap = computed(() => {
    const map = new Map(); // sceneId -> [eventId, eventId...]
    (store.events || []).forEach(evt => {
        (evt.nodes || []).forEach(node => {
             // 只关心 scene 类型的节点
             // 兼容旧数据：如果是 shot 类型，找到对应的 scene
             // 但根据用户需求 "事件关联的粒度到场为止"，我们这里优先展示 target_type='scene'
             // 如果旧数据只有 shot，这里可能需要反向查找，但为了简洁，假设数据已经清洗或用户新建
             if (node.target_type === 'scene') {
                 if (!map.has(node.target_id)) map.set(node.target_id, []);
                 map.get(node.target_id).push(evt);
             }
        });
    });
    return map;
});

const isEventSelected = (id) => selectedEventIds.value.includes(id);

const isSceneActive = (sceneId) => {
    const events = sceneEventMap.value.get(sceneId) || [];
    return events.some(e => selectedEventIds.value.includes(e.id));
};

const getSceneStyle = (sceneId) => {
    const events = sceneEventMap.value.get(sceneId) || [];
    const activeEvents = events.filter(e => selectedEventIds.value.includes(e.id));
    
    if (activeEvents.length === 0) return {};
    
    // 如果只有一个事件，显示该颜色
    if (activeEvents.length === 1) {
        return { backgroundColor: activeEvents[0].color };
    }
    
    // 多个事件，显示渐变或分割 (简化处理：显示第一个选中事件的颜色，或深灰色)
    // 更好的方式可能是 Conic Gradient，但这里先显示混合色或第一个
    return { 
        background: `conic-gradient(${activeEvents.map((e, i, arr) => 
            `${e.color} ${i * (360/arr.length)}deg ${(i+1) * (360/arr.length)}deg`
        ).join(', ')})` 
    };
};

const setSceneRef = (id, el) => {
    if (el) sceneRefs.set(id, el);
    else sceneRefs.delete(id);
};

// 连线重建
const rebuildLines = () => {
    if (!gridContainer.value) return;
    
    // 获取被缩放的内容容器 (即包含 SVG 和 行列的那个 div)
    // 它的引用可以通过 gridContainer 的第一个子元素获取，或者给它加个 ref
    // 这里我们假设 gridContainer 的第一个子元素就是 contentContainer
    const contentContainer = gridContainer.value.firstElementChild;
    if (!contentContainer) return;

    const paths = [];
    // 获取容器的绝对位置 (这是缩放后的视觉位置)
    const containerRect = contentContainer.getBoundingClientRect();
    
    // 遍历所有选中的事件
    const activeEvents = store.events.filter(e => selectedEventIds.value.includes(e.id));
    
    activeEvents.forEach((evt, evtIndex) => {
        const relatedSceneIds = [];
        (evt.nodes || []).forEach(node => {
            if (node.target_type === 'scene') relatedSceneIds.push(node.target_id);
        });
        
        const sortedPoints = [];
        // 计算偏移量：让线不完全重叠
        // 比如：总共3个事件，index 0 -> -3, index 1 -> 0, index 2 -> +3
        const offsetStep = 4;
        const offset = (evtIndex - (activeEvents.length - 1) / 2) * offsetStep;
        
        episodeRows.value.forEach(ep => {
            ep.scenes.forEach(scene => {
                if (relatedSceneIds.includes(scene.id)) {
                    const el = sceneRefs.get(scene.id);
                    if (el) {
                        const elRect = el.getBoundingClientRect();
                        
                        const centerX = elRect.left + elRect.width / 2;
                        const centerY = elRect.top + elRect.height / 2;
                        
                        const relativeX = (centerX - containerRect.left) / scale.value;
                        const relativeY = (centerY - containerRect.top) / scale.value;

                        // 应用偏移 (主要在 Y 轴偏移，也可以考虑 X 轴，或者根据连线方向)
                        // 这里简单地在 Y 轴和 X 轴都做一点偏移，或者只做 Y 轴
                        // 考虑到是横向排列的时间轴，Y 轴偏移比较合理
                        sortedPoints.push({ 
                            x: relativeX + offset, 
                            y: relativeY + offset,
                            epOrder: ep.order,
                            sceneSeq: scene.sequence_number
                        });
                    }
                }
            });
        });

        if (sortedPoints.length > 1) {
            let d = `M ${sortedPoints[0].x} ${sortedPoints[0].y}`;
            for (let i = 0; i < sortedPoints.length - 1; i++) {
                const curr = sortedPoints[i];
                const next = sortedPoints[i+1];
                
                // 判断是否同行且不相邻 (中间有间隔节点)
                const isSameRow = curr.epOrder === next.epOrder;
                const isAdjacent = Math.abs(next.sceneSeq - curr.sceneSeq) === 1;

                if (isSameRow && !isAdjacent) {
                    // 使用直角折线 (曼哈顿连线) 绕过
                    // 弯曲幅度：基础幅度(避开圆点半径) + 基于事件索引的偏移
                    const baseAmp = 28; // 圆点半径20px，留一点余量
                    const variableAmp = (Math.floor(evtIndex / 2) % 5) * 6; 
                    
                    // 奇偶交替：偶数向下(+)，奇数向上(-)
                    const direction = evtIndex % 2 === 0 ? 1 : -1;
                    
                    // 计算折线的水平轨道 Y 坐标
                    const turnY = curr.y + (baseAmp + variableAmp) * direction;
                    
                    // 生成直角路径: 垂直移出 -> 水平平移 -> 垂直切入
                    d += ` L ${curr.x} ${turnY} L ${next.x} ${turnY} L ${next.x} ${next.y}`;
                } else if (isSameRow) {
                    // 同行相邻：直线连接 (因为 Y 坐标相同，本质上是水平直线)
                    d += ` L ${next.x} ${next.y}`;
                } else {
                    // 跨行：使用 Z 字形直角连线 (Vertical -> Horizontal -> Vertical)
                    // 取两点中间的 Y 坐标作为水平折线的位置
                    const midY = (curr.y + next.y) / 2;
                    d += ` L ${curr.x} ${midY} L ${next.x} ${midY} L ${next.x} ${next.y}`;
                }
            }
            paths.push({ id: evt.id, color: evt.color, d });
        }
    });

    linePaths.value = paths;
};

const selectAll = () => selectedEventIds.value = store.events.map(e => e.id);
const clearSelection = () => selectedEventIds.value = [];

// 生命周期
onMounted(async () => {
    if (!store.currentProjectId) await store.init();
    await Promise.all([store.fetchScript(), store.fetchEvents()]);
    if (store.events.length && selectedEventIds.value.length === 0) {
        selectedEventIds.value = [store.events[0].id];
    }
    await nextTick();
    rebuildLines();
    window.addEventListener('resize', rebuildLines);
});

onBeforeUnmount(() => window.removeEventListener('resize', rebuildLines));

watch(
    () => [selectedEventIds.value, store.events, store.episodes],
    () => {
        nextTick(rebuildLines);
    },
    { deep: true }
);

watch(scale, () => nextTick(rebuildLines)); // Scale 变化可能不需要重算，因为 SVG 也在 Scale 内，但如果位置不对需要调整
</script>