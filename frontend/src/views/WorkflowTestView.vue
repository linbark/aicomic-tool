<template>
  <div class="p-6 max-w-6xl mx-auto">
    <h1 class="text-2xl font-bold mb-4">Manju Workflow Test (P0)</h1>
    
    <div class="mb-4 space-y-2">
      <label class="block text-sm font-semibold">Source Text</label>
      <textarea
        v-model="sourceText"
        class="w-full h-32 p-2 border rounded"
        placeholder="输入源文本（可选）"
      ></textarea>
    </div>

    <div class="mb-4">
      <label class="block text-sm font-semibold mb-2">Assets (可选，JSON 格式)</label>
      <textarea
        v-model="assetsJson"
        class="w-full h-24 p-2 border rounded font-mono text-xs"
        placeholder='[{"id": "char1", "image_ref": "path/to/image.jpg"}]'
      ></textarea>
    </div>

    <button
      @click="runWorkflow"
      :disabled="loading"
      class="bg-blue-600 text-white px-4 py-2 rounded hover:bg-blue-700 disabled:bg-gray-400"
    >
      {{ loading ? "运行中..." : "运行 Workflow" }}
    </button>

    <div v-if="result" class="mt-6">
      <h2 class="text-xl font-bold mb-2">结果</h2>
      <div class="bg-gray-50 border rounded p-4">
        <div class="mb-2">
          <span class="font-semibold">Status: </span>
          <span :class="result.status === 'ok' ? 'text-green-600' : 'text-red-600'">
            {{ result.status }}
          </span>
        </div>
        <div v-if="result.warnings && result.warnings.length" class="mb-2">
          <span class="font-semibold">Warnings: </span>
          <ul class="list-disc list-inside text-yellow-600">
            <li v-for="w in result.warnings" :key="w">{{ w }}</li>
          </ul>
        </div>
        <div v-if="result.errors && result.errors.length" class="mb-2">
          <span class="font-semibold">Errors: </span>
          <ul class="list-disc list-inside text-red-600">
            <li v-for="e in result.errors" :key="e.code">{{ e.message }}</li>
          </ul>
        </div>
        <details class="mt-4">
          <summary class="cursor-pointer font-semibold">完整 JSON 结果</summary>
          <pre class="mt-2 p-4 bg-white border rounded overflow-auto text-xs">{{ JSON.stringify(result, null, 2) }}</pre>
        </details>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref } from 'vue'
import api from '../api/client'

const sourceText = ref('')
const assetsJson = ref('')
const loading = ref(false)
const result = ref(null)

async function runWorkflow() {
  loading.value = true
  result.value = null

  try {
    const assets = assetsJson.value.trim() ? JSON.parse(assetsJson.value) : []
    
    const request = {
      request_id: `test_${Date.now()}`,
      source_text: sourceText.value || 'Test workflow',
      assets: assets,
      target: {
        pages: 2,
        dialects: ['midjourney_v6', 'stable_diffusion', 'flux']
      },
      constraints: {
        visual_first: true,
        anti_psychologizing: true,
        bubble_text_limit_zh: 30,
        action_block_max_lines: 4,
        visual_dna_locking: {
          enabled: true,
          policy: 'verbatim'
        },
        json_consistency: {
          enabled: false,
          schema_version: 'visual_profile@0.1',
          locking_policy: 'field_whitelist',
          required_fields: [],
          allow_overrides: false
        }
      }
    }

    const response = await api.runManjuWorkflow(request)
    result.value = response.data
  } catch (error) {
    result.value = {
      status: 'error',
      errors: [{ code: 'REQUEST_FAILED', message: error.message || String(error) }]
    }
  } finally {
    loading.value = false
  }
}
</script>

