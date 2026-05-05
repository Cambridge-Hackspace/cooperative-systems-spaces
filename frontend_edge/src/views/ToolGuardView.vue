<template>
  <div>
    <div class="flex items-center justify-between mb-6">
      <h1 class="text-2xl font-bold">Tool Status</h1>
      <div class="flex items-center gap-2 text-sm text-gray-500">
        <span
          class="inline-block w-2 h-2 rounded-full"
          :class="connected ? 'bg-green-500' : 'bg-red-400'"
        ></span>
        <span>{{ lastUpdated ? `Updated ${lastUpdated}` : (connected ? 'Waiting…' : 'Reconnecting…') }}</span>
      </div>
    </div>

      <!-- Loading -->
      <div v-if="loading" class="flex flex-col items-center py-16">
        <span class="loading loading-spinner loading-lg text-primary"></span>
        <p class="mt-4 text-gray-500">Loading tool state...</p>
      </div>

      <!-- Error / no state -->
      <div v-else-if="error" class="alert alert-warning">
        <span>{{ error }}</span>
      </div>

      <!-- Tool list -->
      <div v-else class="flex flex-col gap-3">
        <div
          v-for="tool in tools"
          :key="tool.id"
          class="card bg-base-100 shadow"
          :class="frameClass(tool.status)"
        >
          <div class="card-body py-4 px-5 flex-row items-center gap-4">
            <div
              class="rounded-full shrink-0"
              :class="dotClass(tool.status)"
              style="width: 18px; height: 18px;"
            ></div>
            <div class="flex-1 min-w-0">
              <p class="font-semibold text-base truncate">{{ tool.name }}</p>
              <p class="text-sm" :class="textClass(tool.status)">{{ statusLabel(tool.status) }}</p>
              <p v-if="tool.inuse_by" class="text-xs text-gray-400 truncate">{{ tool.inuse_by }}</p>
            </div>
            <div class="text-xs text-gray-400 shrink-0">
              {{ tool.external_id ?? tool.id.slice(0, 8) }}
            </div>
          </div>
        </div>

        <div v-if="tools.length === 0" class="text-center text-gray-500 py-10">
          No tools found in state.
        </div>
      </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import type { SyncPayload, SyncTool, ToolStatus } from '@/types'

interface ToolEntry extends SyncTool {
  inuse_by?: string
}

const loading = ref(true)
const error = ref<string | null>(null)
const tools = ref<ToolEntry[]>([])
const lastUpdated = ref<string | null>(null)
const connected = ref(false)

let ws: WebSocket | null = null
let reconnectTimer: number | null = null

function applyPayload(payload: SyncPayload) {
  // Build inuse_by map: tool id → full_name of user who has it in use
  const inuseBy = new Map<string, string>()
  for (const user of payload.users) {
    for (const toolId of user.authorized_tool_ids) {
      const tool = payload.tools.find(t => t.id === toolId)
      if (tool?.status === 'in_use' && !inuseBy.has(toolId)) {
        inuseBy.set(toolId, user.full_name)
      }
    }
  }

  tools.value = [...payload.tools]
    .sort((a, b) => a.name.localeCompare(b.name))
    .map(t => ({ ...t, inuse_by: inuseBy.get(t.id) }))

  error.value = null
  lastUpdated.value = new Date().toLocaleTimeString()
  loading.value = false
}

function connect() {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  ws = new WebSocket(`${proto}://${location.host}/api/toolguard/ws`)

  ws.onopen = () => {
    connected.value = true
    error.value = null
    if (reconnectTimer) { clearTimeout(reconnectTimer); reconnectTimer = null }
  }

  ws.onmessage = (evt) => {
    try {
      applyPayload(JSON.parse(evt.data) as SyncPayload)
    } catch {
      console.error('Failed to parse toolguard state:', evt.data)
    }
  }

  ws.onerror = () => {
    error.value = 'WebSocket error'
  }

  ws.onclose = () => {
    connected.value = false
    loading.value = false
    reconnectTimer = window.setTimeout(connect, 3000)
  }
}

function statusLabel(status: ToolStatus): string {
  const labels: Record<ToolStatus, string> = {
    idle: 'Idle', in_use: 'In Use', maintenance: 'Maintenance',
    repair: 'Repair', broken: 'Broken', retired: 'Retired',
  }
  return labels[status] ?? status
}

function dotClass(status: ToolStatus): string {
  const map: Record<ToolStatus, string> = {
    idle: 'bg-green-500', in_use: 'bg-blue-500', maintenance: 'bg-yellow-400',
    repair: 'bg-orange-400', broken: 'bg-red-500', retired: 'bg-gray-400',
  }
  return map[status] ?? 'bg-gray-400'
}

function textClass(status: ToolStatus): string {
  const map: Record<ToolStatus, string> = {
    idle: 'text-green-500', in_use: 'text-blue-400', maintenance: 'text-yellow-400',
    repair: 'text-orange-400', broken: 'text-red-400', retired: 'text-gray-400',
  }
  return map[status] ?? 'text-gray-400'
}

function frameClass(status: ToolStatus): string {
  const map: Record<ToolStatus, string> = {
    idle: 'border-l-4 border-green-500', in_use: 'border-l-4 border-blue-500',
    maintenance: 'border-l-4 border-yellow-400', repair: 'border-l-4 border-orange-400',
    broken: 'border-l-4 border-red-500', retired: 'border-l-4 border-gray-400',
  }
  return map[status] ?? ''
}

onMounted(connect)

onUnmounted(() => {
  if (reconnectTimer) clearTimeout(reconnectTimer)
  ws?.close()
})
</script>
