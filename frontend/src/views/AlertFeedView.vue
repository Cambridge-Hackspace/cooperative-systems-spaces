<template>
  <div class="container mx-auto px-4 py-8">
    <div class="breadcrumbs text-sm mb-4">
      <ul>
        <li><router-link to="/" class="link">Home</router-link></li>
        <li>Alerts</li>
      </ul>
    </div>
    <h1 class="text-3xl font-bold mb-2">Alerts</h1>
    <p class="text-base-content/70 mb-6">
      Audit events worth a person's attention, newest first. Acknowledging records who looked and
      when; it changes nothing else.
    </p>

    <div v-if="!canView" class="alert alert-warning" role="alert">
      <span>You do not hold the <code>alerts.view</code> permission.</span>
    </div>

    <template v-else>
      <div class="flex flex-wrap items-end gap-3 mb-4">
        <div class="form-control">
          <label class="label py-1" for="alert-severity"
            ><span class="label-text">At least</span></label
          >
          <select
            id="alert-severity"
            v-model="minSeverity"
            class="select select-bordered select-sm"
            @change="reload"
          >
            <option v-for="s in severities" :key="s" :value="s">{{ s }}</option>
          </select>
        </div>
        <div class="form-control">
          <label class="label py-1" for="alert-category"
            ><span class="label-text">Category</span></label
          >
          <select
            id="alert-category"
            v-model="category"
            class="select select-bordered select-sm"
            @change="reload"
          >
            <option value="">All</option>
            <option v-for="c in categories" :key="c" :value="c">{{ c }}</option>
          </select>
        </div>
        <div class="form-control">
          <label class="label py-1" for="alert-ack"><span class="label-text">Show</span></label>
          <select
            id="alert-ack"
            v-model="ackFilter"
            class="select select-bordered select-sm"
            @change="reload"
          >
            <option value="unacknowledged">Unacknowledged</option>
            <option value="all">All</option>
            <option value="acknowledged">Acknowledged</option>
          </select>
        </div>
        <button class="btn btn-ghost btn-sm" :disabled="loading" @click="reload">
          <span v-if="loading" class="loading loading-spinner loading-xs"></span>
          Refresh
        </button>
        <span class="text-sm text-base-content/70" data-testid="total">{{ total }} matching</span>
      </div>

      <div v-if="errorMessage" class="alert alert-error mb-4" role="alert">
        <span>{{ errorMessage }}</span>
      </div>

      <p v-if="!loading && !errorMessage && alerts.length === 0" class="text-sm">Nothing here.</p>

      <div v-if="alerts.length" class="overflow-x-auto">
        <table class="table table-sm" data-testid="alerts">
          <thead>
            <tr>
              <th>When</th>
              <th>Severity</th>
              <th>Event</th>
              <th>Details</th>
              <th>Acknowledged</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="a in alerts" :key="a.id" :data-alert-id="a.id" :data-severity="a.severity">
              <td class="whitespace-nowrap text-xs">{{ fmt(a.created_at) }}</td>
              <td>
                <span class="badge badge-sm" :class="severityClass(a.severity)">{{
                  a.severity
                }}</span>
              </td>
              <td>
                <div class="font-mono text-xs">{{ a.event_type }}</div>
                <div class="text-xs text-base-content/60">{{ a.category }}</div>
              </td>
              <td>
                <pre class="text-xs max-w-md overflow-x-auto whitespace-pre-wrap">{{
                  brief(a.event_data)
                }}</pre>
              </td>
              <td class="text-xs">
                <template v-if="a.acknowledgement">
                  {{ fmt(a.acknowledgement.acknowledged_at) }}
                  <div v-if="a.acknowledgement.note" class="italic">
                    {{ a.acknowledgement.note }}
                  </div>
                </template>
                <span v-else class="text-base-content/50">—</span>
              </td>
              <td class="text-right">
                <button
                  v-if="!a.acknowledgement && canAcknowledge"
                  class="btn btn-xs btn-outline"
                  :disabled="busy"
                  @click="acknowledge(a)"
                >
                  Acknowledge
                </button>
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <div v-if="totalPages > 1" class="join mt-4">
        <button class="join-item btn btn-sm" :disabled="page <= 1 || loading" @click="go(page - 1)">
          «
        </button>
        <button class="join-item btn btn-sm btn-disabled">
          Page {{ page }} of {{ totalPages }}
        </button>
        <button
          class="join-item btn btn-sm"
          :disabled="page >= totalPages || loading"
          @click="go(page + 1)"
        >
          »
        </button>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { alertsApi } from '@/utils/api'
import type { Alert } from '@/types'

const authStore = useAuthStore()
const canView = computed(() => authStore.hasPermission('alerts.view'))
const canAcknowledge = computed(() => authStore.hasPermission('alerts.acknowledge'))

const alerts = ref<Alert[]>([])
const total = ref(0)
const page = ref(1)
const totalPages = ref(1)
const perPage = 50
const loading = ref(false)
const busy = ref(false)
const errorMessage = ref('')

const severities = ref<string[]>(['notice', 'warning', 'critical'])
const categories = ref<string[]>([])
const minSeverity = ref('notice')
const category = ref('')
const ackFilter = ref<'unacknowledged' | 'all' | 'acknowledged'>('unacknowledged')

function fmt(iso: string) {
  return new Date(iso).toLocaleString()
}
function brief(data: unknown) {
  const text = JSON.stringify(data)
  return text.length > 240 ? `${text.slice(0, 240)}…` : text
}
function severityClass(s: string) {
  return (
    { critical: 'badge-error', warning: 'badge-warning', notice: 'badge-info' }[s] ?? 'badge-ghost'
  )
}

async function loadClassification() {
  try {
    const res = await alertsApi.classification()
    if (res.success && res.data) {
      categories.value = res.data.categories
      // Only the levels the feed can show: info is the ordinary audit trail.
      severities.value = res.data.severities.filter((s) => s !== 'info')
    }
  } catch {
    /* the defaults above still work */
  }
}

async function reload() {
  page.value = 1
  await load()
}
async function go(p: number) {
  page.value = p
  await load()
}

async function load() {
  if (!canView.value) return
  loading.value = true
  errorMessage.value = ''
  try {
    const res = await alertsApi.list({
      page: page.value,
      per_page: perPage,
      min_severity: minSeverity.value,
      category: category.value || undefined,
      acknowledged: ackFilter.value === 'all' ? undefined : ackFilter.value === 'acknowledged',
    })
    if (res.success && res.data) {
      alerts.value = res.data.items
      total.value = res.data.total
      totalPages.value = res.data.total_pages
    } else {
      errorMessage.value = res.error || 'Could not load alerts.'
    }
  } catch (err) {
    const e = err as { message?: string }
    errorMessage.value = e?.message || 'Could not load alerts.'
  } finally {
    loading.value = false
  }
}

async function acknowledge(a: Alert) {
  const note = window.prompt('Note (optional):', '') ?? undefined
  busy.value = true
  errorMessage.value = ''
  try {
    const res = await alertsApi.acknowledge(a.id, note?.trim() || undefined)
    if (res.success) {
      await load()
    } else {
      errorMessage.value = res.error || 'Could not acknowledge.'
    }
  } catch (err) {
    const e = err as { message?: string }
    errorMessage.value = e?.message || 'Could not acknowledge.'
  } finally {
    busy.value = false
  }
}

onMounted(async () => {
  await loadClassification()
  await load()
})
</script>
