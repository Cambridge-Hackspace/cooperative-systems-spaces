<template>
  <div class="container mx-auto px-4 py-8 max-w-4xl">
    <div class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/admin" class="link">Admin</router-link></li>
        <li>Membership</li>
      </ul>
    </div>

    <h1 class="text-3xl font-bold mb-6">Membership Billing</h1>

    <div v-if="errorMessage" class="alert alert-error mb-4" role="alert">
      <span>{{ errorMessage }}</span>
    </div>

    <div class="card bg-base-100 shadow-xl mb-6">
      <div class="card-body">
        <h2 class="card-title">Status</h2>
        <div class="stats stats-vertical sm:stats-horizontal">
          <div class="stat">
            <div class="stat-title">Enrolled members</div>
            <div class="stat-value text-primary">{{ status?.enrolled_count ?? '-' }}</div>
            <div class="stat-desc">Have an active dues clock</div>
          </div>
          <div class="stat">
            <div class="stat-title">Online payment</div>
            <div class="stat-value text-lg">
              {{ status?.stripe_enabled ? 'Stripe' : 'Cash only' }}
            </div>
            <div class="stat-desc">{{ lastRunWhen }}</div>
          </div>
        </div>
        <div class="card-actions justify-end mt-4">
          <button class="btn btn-primary reconcile-now" :disabled="reconciling" @click="reconcile">
            <span v-if="reconciling" class="loading loading-spinner loading-sm"></span>
            {{ reconciling ? 'Running…' : 'Reconcile now' }}
          </button>
        </div>
      </div>
    </div>

    <div class="card bg-base-100 shadow-xl mb-6">
      <div class="card-body">
        <h2 class="card-title">Log a cash payment</h2>
        <p class="text-base-content/70">
          Record an off-Stripe payment (or an adjustment) as a ledger credit. This is the
          accountability record for money taken outside Stripe; no card data is stored.
        </p>
        <div v-if="paymentMessage" class="alert alert-success" role="status">
          <span>{{ paymentMessage }}</span>
        </div>
        <div class="form-control gap-2 mt-2">
          <input
            v-model="payUserId"
            type="text"
            class="input input-bordered pay-user-id"
            placeholder="Member user id (UUID)"
          />
          <input
            v-model="payAmount"
            type="text"
            class="input input-bordered pay-amount"
            placeholder="Amount, e.g. 25.00"
          />
          <select v-model="payType" class="select select-bordered pay-type">
            <option value="cash_payment">Cash payment</option>
            <option value="adjustment">Adjustment</option>
          </select>
          <input
            v-model="payDescription"
            type="text"
            class="input input-bordered pay-description"
            placeholder="Note (optional)"
          />
          <button class="btn btn-secondary log-payment" :disabled="logging" @click="logPayment">
            {{ logging ? 'Recording…' : 'Record payment' }}
          </button>
        </div>
      </div>
    </div>

    <div class="card bg-base-100 shadow-xl">
      <div class="card-body">
        <h2 class="card-title">Recent renewal runs</h2>
        <div class="overflow-x-auto">
          <table class="table table-sm">
            <thead>
              <tr>
                <th>Started</th>
                <th>Result</th>
                <th>Checked</th>
                <th>Dues</th>
                <th>Lapsed</th>
                <th>Errors</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="run in status?.recent_runs || []" :key="run.id">
                <td>{{ formatTime(run.started_at) }}</td>
                <td>
                  <span v-if="run.ok" class="badge badge-success">ok</span>
                  <span v-else class="badge badge-error" :title="run.error || ''">failed</span>
                </td>
                <td>{{ run.users_checked }}</td>
                <td>{{ run.dues_charged }}</td>
                <td>{{ run.lapsed }}</td>
                <td>{{ run.errors }}</td>
              </tr>
              <tr v-if="!status?.recent_runs?.length">
                <td colspan="6" class="text-base-content/60">No runs recorded yet.</td>
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
  users_checked: number
  dues_charged: number
  lapsed: number
  errors: number
  ok: boolean
  error: string | null
}

interface MembershipAdminStatus {
  enabled: boolean
  stripe_enabled: boolean
  enrolled_count: number
  recent_runs: SyncRun[]
}

interface CycleOutcome {
  ok: boolean
  error: string | null
}

interface LogPaymentResponse {
  posted: boolean
  balance: string
}

const status = ref<MembershipAdminStatus | null>(null)
const reconciling = ref(false)
const errorMessage = ref<string | null>(null)

const payUserId = ref('')
const payAmount = ref('')
const payType = ref('cash_payment')
const payDescription = ref('')
const logging = ref(false)
const paymentMessage = ref<string | null>(null)

const lastRun = computed<SyncRun | null>(() => status.value?.recent_runs?.[0] ?? null)
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
    const res = await apiClient.get<MembershipAdminStatus>('/admin/membership/status')
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
    const res = await apiClient.post<CycleOutcome>('/admin/membership/reconcile')
    if (!res.success || !res.data) {
      throw new Error(res.error || 'Reconcile failed')
    }
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

async function logPayment() {
  logging.value = true
  errorMessage.value = null
  paymentMessage.value = null
  try {
    const res = await apiClient.post<LogPaymentResponse>('/admin/membership/payments', {
      user_id: payUserId.value.trim(),
      amount: payAmount.value.trim(),
      entry_type: payType.value,
      description: payDescription.value.trim() || null,
    })
    if (!res.success || !res.data) {
      throw new Error(res.error || 'Failed to record payment')
    }
    paymentMessage.value = `Recorded. New balance: ${res.data.balance}`
    payAmount.value = ''
    payDescription.value = ''
    await fetchStatus()
  } catch (err: any) {
    errorMessage.value = err.message || 'Failed to record payment'
  } finally {
    logging.value = false
  }
}

onMounted(fetchStatus)
</script>
