<template>
  <div class="container mx-auto px-4 py-8 max-w-4xl">
    <div class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/admin" class="link">Admin</router-link></li>
        <li>Mailing List</li>
      </ul>
    </div>

    <h1 class="text-3xl font-bold mb-6">Groups.io Mailing List</h1>

    <div v-if="errorMessage" class="alert alert-error mb-4" role="alert">
      <span>{{ errorMessage }}</span>
    </div>

    <div class="card bg-base-100 shadow-xl mb-6">
      <div class="card-body">
        <h2 class="card-title">Status</h2>
        <div class="stats stats-vertical sm:stats-horizontal">
          <div class="stat">
            <div class="stat-title">Members intended on the list</div>
            <div class="stat-value text-primary">{{ status?.intended_count ?? '-' }}</div>
            <div class="stat-desc">Active, verified, not opted out</div>
          </div>
          <div class="stat">
            <div class="stat-title">Last run</div>
            <div class="stat-value text-lg">{{ lastRunLabel }}</div>
            <div class="stat-desc">{{ lastRunWhen }}</div>
          </div>
        </div>
        <div class="card-actions justify-end mt-4">
          <button class="btn btn-primary reconcile-now" :disabled="reconciling" @click="reconcile">
            <span v-if="reconciling" class="loading loading-spinner loading-sm"></span>
            {{ reconciling ? 'Reconciling...' : 'Reconcile now' }}
          </button>
        </div>
      </div>
    </div>

    <div class="card bg-base-100 shadow-xl">
      <div class="card-body">
        <h2 class="card-title">Recent runs</h2>
        <div class="overflow-x-auto">
          <table class="table table-sm">
            <thead>
              <tr>
                <th>Started</th>
                <th>Result</th>
                <th>Added</th>
                <th>Removed</th>
                <th>Opted out</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="run in status?.recent_runs || []" :key="run.id">
                <td>{{ formatTime(run.started_at) }}</td>
                <td>
                  <span v-if="run.ok" class="badge badge-success">ok</span>
                  <span v-else class="badge badge-error" :title="run.error || ''">failed</span>
                </td>
                <td>{{ run.added }}</td>
                <td>{{ run.removed }}</td>
                <td>{{ run.opted_out }}</td>
              </tr>
              <tr v-if="!status?.recent_runs?.length">
                <td colspan="5" class="text-base-content/60">No runs recorded yet.</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { apiClient } from '@/utils/api'

interface SyncRun {
  id: string
  started_at: string
  finished_at: string
  added: number
  removed: number
  opted_out: number
  ok: boolean
  error: string | null
}

interface GroupsioStatus {
  enabled: boolean
  intended_count: number
  recent_runs: SyncRun[]
}

interface ReconcileOutcome {
  added: number
  removed: number
  opted_out: number
  ok: boolean
  error: string | null
}

const status = ref<GroupsioStatus | null>(null)
const reconciling = ref(false)
const errorMessage = ref<string | null>(null)

const lastRun = computed<SyncRun | null>(() => status.value?.recent_runs?.[0] ?? null)
const lastRunLabel = computed(() => {
  if (!lastRun.value) return 'never'
  return lastRun.value.ok ? 'ok' : 'failed'
})
const lastRunWhen = computed(() =>
  lastRun.value ? formatTime(lastRun.value.started_at) : 'no runs yet'
)

function formatTime(iso: string): string {
  const d = new Date(iso)
  return isNaN(d.getTime()) ? iso : d.toLocaleString()
}

async function fetchStatus() {
  errorMessage.value = null
  try {
    const res = await apiClient.get<GroupsioStatus>('/admin/groupsio/status')
    if (res.success && res.data) {
      status.value = res.data
    } else {
      throw new Error(res.error || 'Failed to load status')
    }
  } catch (err: any) {
    errorMessage.value = err.message || 'Failed to load status'
  }
}

async function reconcile() {
  reconciling.value = true
  errorMessage.value = null
  try {
    const res = await apiClient.post<ReconcileOutcome>('/admin/groupsio/reconcile')
    if (!res.success || !res.data) {
      throw new Error(res.error || 'Reconcile failed')
    }
    // Refresh first (it clears errorMessage), then surface a not-ok outcome so
    // the refresh cannot wipe the message we are trying to show.
    await fetchStatus()
    if (!res.data.ok) {
      errorMessage.value = `Reconcile did not complete: ${res.data.error || 'unknown error'}`
    }
  } catch (err: any) {
    errorMessage.value = err.message || 'Reconcile failed'
  } finally {
    reconciling.value = false
  }
}

onMounted(fetchStatus)
</script>
