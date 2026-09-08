<template>
  <div class="container mx-auto px-4 py-8 max-w-4xl">
    <div class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/admin" class="link">Admin</router-link></li>
        <li>Tool Billing</li>
      </ul>
    </div>

    <h1 class="text-3xl font-bold mb-6">Tool Billing</h1>

    <div v-if="errorMessage" class="alert alert-error mb-4" role="alert">
      <span>{{ errorMessage }}</span>
    </div>

    <div class="card bg-base-100 shadow-xl mb-6">
      <div class="card-body">
        <h2 class="card-title">Status</h2>
        <div class="stats stats-vertical sm:stats-horizontal">
          <div class="stat">
            <div class="stat-title">Billing mode</div>
            <div class="stat-value text-lg">{{ status?.billing_mode ?? '-' }}</div>
            <div class="stat-desc">Actuation: {{ status?.actuation_mode ?? '-' }}</div>
          </div>
          <div class="stat">
            <div class="stat-title">Membership required</div>
            <div class="stat-value text-lg">{{ status?.require_membership ? 'Yes' : 'No' }}</div>
            <div class="stat-desc">Currency: {{ status?.currency ?? '-' }}</div>
          </div>
        </div>
        <p class="text-base-content/70 mt-2">
          Refunds and corrections are logged as an "adjustment" on the Membership page.
        </p>
      </div>
    </div>

    <div class="card bg-base-100 shadow-xl">
      <div class="card-body">
        <h2 class="card-title">Member tool sessions</h2>
        <div class="form-control gap-2 mt-2">
          <input
            v-model="lookupUserId"
            type="text"
            class="input input-bordered session-user-id"
            placeholder="Member user id (UUID)"
          />
          <button class="btn btn-primary load-sessions" :disabled="loading" @click="loadSessions">
            {{ loading ? 'Loading…' : 'Load sessions' }}
          </button>
        </div>

        <div class="overflow-x-auto mt-4">
          <table class="table table-sm">
            <thead>
              <tr>
                <th>Started</th>
                <th>Ended</th>
                <th>Status</th>
                <th>Held</th>
                <th>Charged</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="s in sessions" :key="s.id">
                <td>{{ formatTime(s.started_at) }}</td>
                <td>{{ s.ended_at ? formatTime(s.ended_at) : '—' }}</td>
                <td>{{ s.status }}</td>
                <td>{{ s.hold_amount }}</td>
                <td>{{ s.charged_amount ?? '—' }}</td>
              </tr>
              <tr v-if="loaded && !sessions.length">
                <td colspan="5" class="text-base-content/60">No sessions for this member.</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { apiClient } from '@/utils/api'

interface ToolBillingStatus {
  enabled: boolean
  billing_mode: string
  actuation_mode: string
  require_membership: boolean
  currency: string
}

interface ToolUsageSession {
  id: string
  tool_id: string
  user_id: string
  started_at: string
  ended_at: string | null
  hold_amount: string
  reported_seconds: string | null
  charged_amount: string | null
  status: string
  ledger_entry_id: string | null
  created_at: string
}

const status = ref<ToolBillingStatus | null>(null)
const errorMessage = ref<string | null>(null)

const lookupUserId = ref('')
const sessions = ref<ToolUsageSession[]>([])
const loading = ref(false)
const loaded = ref(false)

function formatTime(iso: string): string {
  const d = new Date(iso)
  return isNaN(d.getTime()) ? iso : d.toLocaleString()
}

async function fetchStatus() {
  errorMessage.value = null
  try {
    const res = await apiClient.get<ToolBillingStatus>('/admin/tool-billing/status')
    if (res.success && res.data) {
      status.value = res.data
    } else {
      throw new Error(res.error || 'Failed to load status')
    }
  } catch (err: any) {
    errorMessage.value = err.message || 'Failed to load status'
  }
}

async function loadSessions() {
  loading.value = true
  errorMessage.value = null
  try {
    const id = lookupUserId.value.trim()
    const res = await apiClient.get<ToolUsageSession[]>(`/admin/tool-billing/users/${id}/sessions`)
    if (res.success && res.data) {
      sessions.value = res.data
      loaded.value = true
    } else {
      throw new Error(res.error || 'Failed to load sessions')
    }
  } catch (err: any) {
    errorMessage.value = err.message || 'Failed to load sessions'
  } finally {
    loading.value = false
  }
}

onMounted(fetchStatus)
</script>
