<template>
  <div :class="embedded ? '' : 'container mx-auto px-4 py-8'">
    <div v-if="!embedded" class="mb-6">
      <h1 class="text-3xl font-bold mb-1">Tool Wiring</h1>
      <p class="text-base-content/70">
        Which devices read, switch and watch each tool, and the interlocks that gate them.
      </p>
    </div>

    <div v-if="error" class="alert alert-error mb-4" role="alert">{{ error }}</div>

    <div v-if="loading" class="flex justify-center py-8">
      <span class="loading loading-spinner loading-lg" aria-label="Loading"></span>
    </div>

    <!-- Guarded on `!error`: a failed load leaves the list empty, and showing
         the empty state next to the error would report a server failure as a
         fact about the space. -->
    <div v-else-if="wiredTools.length === 0 && !error" class="alert">
      <span>
        No tool has modules bound yet. Bind a reader, a power controller or a sensor to a tool to
        start.
      </span>
    </div>

    <div v-else class="space-y-6">
      <div v-for="tool in wiredTools" :key="tool.tool_id" class="card bg-base-200 shadow">
        <div class="card-body">
          <div class="flex items-center justify-between gap-4 flex-wrap">
            <h2 class="card-title">{{ toolName(tool) }}</h2>
            <!--
              The fail-safe verdict is stated plainly rather than hidden behind a
              tooltip. A tool whose plug cannot de-energize itself is one where
              the coordinator's cut is a mitigation, not an interlock, and whoever
              is authoring rules for it needs to know that while they do it.
            -->
            <span
              class="badge"
              :class="tool.power_fails_safe ? 'badge-success' : 'badge-warning'"
              :data-testid="`fail-safe-${tool.tool_id}`"
            >
              {{ tool.power_fails_safe ? 'Power fails safe' : 'Power cannot fail safe' }}
            </span>
          </div>

          <p v-if="!tool.power_fails_safe" class="text-sm text-base-content/70">
            No power module bound to this tool reaches a safe state on its own when it loses
            contact. Cuts still work while the link is up, but treat them as a mitigation rather
            than an interlock.
          </p>

          <h3 class="font-semibold mt-4">Modules</h3>
          <div v-if="tool.modules.length === 0" class="text-sm text-base-content/70">
            None bound.
          </div>
          <table v-else class="table table-sm">
            <thead>
              <tr>
                <th>Role</th>
                <th>Name</th>
                <th>On disconnect</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="m in tool.modules" :key="m.id">
                <td>
                  <span class="badge badge-outline">{{ m.role }}</span>
                </td>
                <td>{{ m.name }}</td>
                <td>
                  <span :class="m.on_disconnect === 'fail_off' ? 'text-success' : ''">
                    {{ m.on_disconnect }}
                  </span>
                </td>
              </tr>
            </tbody>
          </table>

          <h3 class="font-semibold mt-4">Interlocks</h3>
          <div v-if="tool.interlocks.length === 0" class="text-sm text-base-content/70">
            None configured.
          </div>
          <table v-else class="table table-sm">
            <thead>
              <tr>
                <th>Kind</th>
                <th>Condition</th>
                <th>Enforced at</th>
                <th>Latches</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="i in tool.interlocks" :key="i.id">
                <td>
                  <span class="badge" :class="i.kind === 'trip' ? 'badge-error' : 'badge-info'">{{
                    i.kind === 'trip' ? 'trip' : 'start gate'
                  }}</span>
                </td>
                <td>{{ i.condition }}</td>
                <td>
                  <!--
                    The tier is worth showing next to the rule: `edge` means the
                    trip travels through the coordinator, which is a different
                    survivability story from `firmware`, and the person reading
                    this is the one who has to know which they have.
                  -->
                  <span class="badge badge-outline" :data-testid="`enforcement-${i.id}`">{{
                    i.enforcement
                  }}</span>
                </td>
                <td>{{ i.latch ? 'yes' : 'no' }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { useReloadOnReactivate } from '@/composables/useReloadOnReactivate'
import { toolModulesApi, toolsApi } from '@/utils/api'
import type { ToolModuleStateTool } from '@/types'
import type { Tool } from '@/types/tools'

defineProps<{ embedded?: boolean }>()

const loading = ref(true)
const error = ref('')
const tools = ref<ToolModuleStateTool[]>([])
const catalog = ref<Tool[]>([])

const wiredTools = computed(() => tools.value)

/** Prefer the tool's real name; fall back to whatever identifier we have. */
function toolName(t: ToolModuleStateTool): string {
  const match = catalog.value.find((c) => c.id === t.tool_id)
  return match?.name ?? t.external_id ?? t.tool_id
}

async function load() {
  loading.value = true
  error.value = ''
  try {
    const [snapshot, all] = await Promise.all([toolModulesApi.state(), toolsApi.getTools()])
    if (snapshot?.success && snapshot.data) tools.value = snapshot.data.tools ?? []
    if (all?.success && all.data) catalog.value = all.data
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load tool wiring'
  } finally {
    loading.value = false
  }
}

useReloadOnReactivate(load)
</script>
