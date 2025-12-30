<template>
  <div class="h-full overflow-y-auto bg-gray-50">
    <div class="max-w-3xl mx-auto p-6">
      <div class="flex items-center justify-between mb-6">
        <div>
          <h2 class="text-lg font-bold text-gray-800">AI 配置（DeepSeek）</h2>
          <p class="text-xs text-gray-400 mt-1">配置保存在本机应用数据目录，前端不会直接请求 DeepSeek。</p>
        </div>
        <div class="flex gap-2">
          <button
            @click="handleTest"
            :disabled="isTesting"
            class="px-3 py-2 text-xs rounded border bg-white hover:bg-gray-50 text-gray-700 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {{ isTesting ? '测试中...' : '测试连接' }}
          </button>
          <button
            @click="handleSave"
            :disabled="isSaving"
            class="px-3 py-2 text-xs rounded bg-blue-600 hover:bg-blue-700 text-white disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {{ isSaving ? '保存中...' : '保存配置' }}
          </button>
        </div>
      </div>

      <div v-if="loading" class="text-sm text-gray-400">加载中...</div>

      <div v-else class="bg-white border rounded-xl p-6 space-y-5 shadow-sm">
        <div v-if="error" class="text-xs text-red-600">{{ error }}</div>
        <div v-if="success" class="text-xs text-green-700">{{ success }}</div>

        <div class="grid grid-cols-2 gap-4">
          <div>
            <label class="block text-xs font-bold text-gray-500 mb-1">Base URL</label>
            <input v-model="form.base_url" class="w-full border rounded p-2 text-sm outline-none focus:ring-2 ring-blue-500" placeholder="https://api.deepseek.com">
          </div>
          <div>
            <label class="block text-xs font-bold text-gray-500 mb-1">Model</label>
            <input v-model="form.model" class="w-full border rounded p-2 text-sm outline-none focus:ring-2 ring-blue-500" placeholder="deepseek-chat">
          </div>
        </div>

        <div class="grid grid-cols-3 gap-4">
          <div>
            <label class="block text-xs font-bold text-gray-500 mb-1">temperature</label>
            <input v-model.number="form.temperature" type="number" step="0.1" min="0" max="2"
                   class="w-full border rounded p-2 text-sm outline-none focus:ring-2 ring-blue-500">
          </div>
          <div>
            <label class="block text-xs font-bold text-gray-500 mb-1">max_tokens</label>
            <input v-model.number="form.max_tokens" type="number" step="1" min="64" max="8192"
                   class="w-full border rounded p-2 text-sm outline-none focus:ring-2 ring-blue-500">
          </div>
          <div>
            <label class="block text-xs font-bold text-gray-500 mb-1">timeout(s)</label>
            <input v-model.number="form.timeout_seconds" type="number" step="1" min="5" max="120"
                   class="w-full border rounded p-2 text-sm outline-none focus:ring-2 ring-blue-500">
          </div>
        </div>

        <div>
          <div class="flex items-center justify-between mb-1">
            <label class="block text-xs font-bold text-gray-500">API Key</label>
            <span class="text-[10px]" :class="hasApiKey ? 'text-green-700' : 'text-gray-400'">
              {{ hasApiKey ? '已设置' : '未设置' }}
            </span>
          </div>
          <input
            v-model="apiKeyInput"
            type="password"
            class="w-full border rounded p-2 text-sm outline-none focus:ring-2 ring-blue-500"
            placeholder="输入新的 API Key（留空表示不修改）"
          >
          <label class="mt-2 inline-flex items-center gap-2 text-xs text-gray-500 select-none">
            <input type="checkbox" v-model="clearApiKey" class="accent-blue-600">
            清空 API Key
          </label>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { onMounted, ref } from 'vue';
import api from '../api/client';

const loading = ref(true);
const error = ref('');
const success = ref('');
const isSaving = ref(false);
const isTesting = ref(false);

const hasApiKey = ref(false);
const apiKeyInput = ref('');
const clearApiKey = ref(false);

const form = ref({
  base_url: 'https://api.deepseek.com',
  model: 'deepseek-chat',
  temperature: 0.2,
  max_tokens: 2048,
  timeout_seconds: 30,
});

const load = async () => {
  loading.value = true;
  error.value = '';
  success.value = '';
  try {
    const { data } = await api.getAiSettings();
    hasApiKey.value = !!data?.has_api_key;
    form.value = {
      base_url: data?.base_url || 'https://api.deepseek.com',
      model: data?.model || 'deepseek-chat',
      temperature: Number(data?.temperature ?? 0.2),
      max_tokens: Number(data?.max_tokens ?? 2048),
      timeout_seconds: Number(data?.timeout_seconds ?? 30),
    };
  } catch (e) {
    error.value = '加载失败：' + (e?.response?.data?.detail || e?.message || '未知错误');
  } finally {
    loading.value = false;
  }
};

const handleSave = async () => {
  if (isSaving.value) return;
  isSaving.value = true;
  error.value = '';
  success.value = '';
  try {
    const payload = {
      base_url: form.value.base_url,
      model: form.value.model,
      temperature: form.value.temperature,
      max_tokens: form.value.max_tokens,
      timeout_seconds: form.value.timeout_seconds,
    };
    if (clearApiKey.value) {
      payload.api_key = '';
    } else if (apiKeyInput.value.trim()) {
      payload.api_key = apiKeyInput.value.trim();
    }
    const { data } = await api.updateAiSettings(payload);
    hasApiKey.value = !!data?.has_api_key;
    apiKeyInput.value = '';
    clearApiKey.value = false;
    success.value = '保存成功';
  } catch (e) {
    error.value = '保存失败：' + (e?.response?.data?.detail || e?.message || '未知错误');
  } finally {
    isSaving.value = false;
  }
};

const handleTest = async () => {
  if (isTesting.value) return;
  isTesting.value = true;
  error.value = '';
  success.value = '';
  try {
    const { data } = await api.testAi();
    if (data?.ok) success.value = data?.detail || '连接成功';
    else error.value = data?.detail || '连接失败';
  } catch (e) {
    error.value = '测试失败：' + (e?.response?.data?.detail || e?.message || '未知错误');
  } finally {
    isTesting.value = false;
  }
};

onMounted(load);
</script>


