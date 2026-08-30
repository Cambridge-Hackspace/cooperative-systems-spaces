<template>
  <div class="space-y-4">
    <!-- Search and Filter Controls -->
    <div class="flex flex-col sm:flex-row gap-4 items-start sm:items-center justify-between">
      <div class="form-control w-full sm:w-auto">
        <label class="label">
          <span class="label-text">Filter by event type</span>
        </label>
        <select
          v-model="selectedEventType"
          class="select select-bordered w-full sm:w-64"
          @change="applyFilters"
        >
          <option value="">All Events</option>
          <option value="user_login">User Login</option>
          <option value="user_logout">User Logout</option>
          <option value="user_registration">User Registration</option>
          <option value="user_role_change">Role Change</option>
          <option value="user_activation">User Activation</option>
          <option value="user_deactivation">User Deactivation</option>
          <option value="user_profile_update">Profile Update</option>
          <option value="admin_config_reload">Config Reload</option>
          <option value="profile_config_updated">Profile Config Updated</option>
          <option value="profile_config_rolled_back">Profile Config Rolled Back</option>
          <option value="failed_login_attempt">Failed Login</option>
        </select>
      </div>

      <div class="stats shadow">
        <div class="stat">
          <div class="stat-title">Total Logs</div>
          <div class="stat-value text-primary">{{ totalLogs }}</div>
        </div>
      </div>
    </div>

    <!-- Loading State -->
    <div v-if="isLoading" class="flex justify-center py-8">
      <span class="loading loading-spinner loading-lg"></span>
    </div>

    <!-- Error State -->
    <div v-else-if="error" class="alert alert-error">
      <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          stroke-width="2"
          d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
        />
      </svg>
      <div>
        <h3 class="font-bold">Error loading audit logs</h3>
        <div class="text-xs">{{ error }}</div>
      </div>
      <button class="btn btn-sm" @click="fetchAuditLogs">Retry</button>
    </div>

    <!-- Audit Logs Table -->
    <div v-else-if="auditLogs.length > 0" class="card bg-base-100 shadow-xl">
      <div class="overflow-x-auto">
        <table class="table table-zebra">
          <thead>
            <tr>
              <th>Timestamp</th>
              <th>Event Type</th>
              <th>User</th>
              <th>Actor</th>
              <th>IP Address</th>
              <th>Details</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="log in auditLogs" :key="log.id">
              <!-- Timestamp -->
              <td>
                <div class="flex flex-col">
                  <span class="font-mono text-sm">{{ formatDate(log.created_at) }}</span>
                  <span class="text-xs opacity-70">{{ formatTime(log.created_at) }}</span>
                </div>
              </td>

              <!-- Event Type -->
              <td>
                <span class="badge" :class="getEventBadgeClass(log.event_type)">
                  {{ formatEventType(log.event_type) }}
                </span>
              </td>

              <!-- User -->
              <td>
                <span v-if="log.user_id" class="text-sm">
                  {{ log.user_id }}
                </span>
                <span v-else class="text-sm opacity-50">—</span>
              </td>

              <!-- Actor -->
              <td>
                <span v-if="log.actor_id" class="text-sm">
                  {{ log.actor_id }}
                </span>
                <span v-else class="text-sm opacity-50">System</span>
              </td>

              <!-- IP Address -->
              <td>
                <span v-if="log.ip_address" class="text-sm font-mono">
                  {{ log.ip_address }}
                </span>
                <span v-else class="text-sm opacity-50">—</span>
              </td>

              <!-- Details -->
              <td>
                <div class="flex items-center space-x-2">
                  <button
                    class="btn btn-ghost btn-xs"
                    title="View details"
                    @click="showDetails(log)"
                  >
                    <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                      />
                    </svg>
                  </button>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <!-- Empty State -->
    <div v-else-if="!isLoading" class="text-center py-12">
      <div class="text-6xl mb-4">📋</div>
      <h3 class="text-lg font-medium mb-2">No audit logs found</h3>
      <p class="text-base-content/70">
        {{
          selectedEventType ? 'No logs match your filter criteria.' : 'No audit logs are available.'
        }}
      </p>
    </div>

    <!-- Pagination -->
    <div v-if="totalPages > 1" class="flex justify-center">
      <div class="btn-group">
        <button
          class="btn"
          :class="{ 'btn-disabled': currentPage === 1 }"
          :disabled="currentPage === 1"
          @click="goToPage(currentPage - 1)"
        >
          «
        </button>

        <template v-for="page in visiblePages" :key="page">
          <button v-if="page === '...'" class="btn btn-disabled">...</button>
          <button
            v-else
            class="btn"
            :class="{ 'btn-active': page === currentPage }"
            @click="goToPage(page as number)"
          >
            {{ page }}
          </button>
        </template>

        <button
          class="btn"
          :class="{ 'btn-disabled': currentPage === totalPages }"
          :disabled="currentPage === totalPages"
          @click="goToPage(currentPage + 1)"
        >
          »
        </button>
      </div>
    </div>

    <!-- Details Modal -->
    <div v-if="selectedLog" class="modal modal-open">
      <div class="modal-box max-w-2xl">
        <h3 class="font-bold text-lg mb-4">Audit Log Details</h3>

        <div class="space-y-4">
          <div class="grid grid-cols-2 gap-4">
            <div>
              <label class="label">
                <span class="label-text font-semibold">Event Type</span>
              </label>
              <span class="badge" :class="getEventBadgeClass(selectedLog.event_type)">
                {{ formatEventType(selectedLog.event_type) }}
              </span>
            </div>

            <div>
              <label class="label">
                <span class="label-text font-semibold">Timestamp</span>
              </label>
              <p class="text-sm">
                {{ formatDate(selectedLog.created_at) }} {{ formatTime(selectedLog.created_at) }}
              </p>
            </div>

            <div>
              <label class="label">
                <span class="label-text font-semibold">User ID</span>
              </label>
              <p class="text-sm font-mono">{{ selectedLog.user_id || '—' }}</p>
            </div>

            <div>
              <label class="label">
                <span class="label-text font-semibold">Actor ID</span>
              </label>
              <p class="text-sm font-mono">{{ selectedLog.actor_id || 'System' }}</p>
            </div>

            <div>
              <label class="label">
                <span class="label-text font-semibold">IP Address</span>
              </label>
              <p class="text-sm font-mono">{{ selectedLog.ip_address || '—' }}</p>
            </div>

            <div>
              <label class="label">
                <span class="label-text font-semibold">User Agent</span>
              </label>
              <p class="text-sm">{{ selectedLog.user_agent || '—' }}</p>
            </div>
          </div>

          <div>
            <label class="label">
              <span class="label-text font-semibold">Event Data</span>
            </label>
            <pre class="bg-base-200 p-3 rounded text-sm overflow-auto">{{
              formatEventData(selectedLog.event_data)
            }}</pre>
          </div>
        </div>

        <div class="modal-action">
          <button class="btn" @click="selectedLog = null">Close</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { adminApi } from '@/utils/api'
import type { AuditLog } from '@/types'

// Props and Emits
const emit = defineEmits<{
  error: [message: string]
}>()

// Reactive state
const auditLogs = ref<AuditLog[]>([])
const isLoading = ref(false)
const error = ref<string | null>(null)
const selectedEventType = ref('')
const currentPage = ref(1)
const totalLogs = ref(0)
const totalPages = ref(1)
const perPage = 50
const selectedLog = ref<AuditLog | null>(null)

// Computed properties
const visiblePages = computed(() => {
  const pages: (number | string)[] = []
  const total = totalPages.value
  const current = currentPage.value

  if (total <= 7) {
    for (let i = 1; i <= total; i++) {
      pages.push(i)
    }
  } else {
    pages.push(1)

    if (current > 4) {
      pages.push('...')
    }

    const start = Math.max(2, current - 1)
    const end = Math.min(total - 1, current + 1)

    for (let i = start; i <= end; i++) {
      pages.push(i)
    }

    if (current < total - 3) {
      pages.push('...')
    }

    pages.push(total)
  }

  return pages
})

// Methods
const fetchAuditLogs = async () => {
  isLoading.value = true
  error.value = null

  try {
    const response = await adminApi.getAuditLogs(
      currentPage.value,
      perPage,
      selectedEventType.value || undefined
    )

    if (response.success && response.data) {
      auditLogs.value = Array.isArray(response.data) ? response.data : []
      totalLogs.value = auditLogs.value.length
      totalPages.value = Math.max(1, Math.ceil(totalLogs.value / perPage))
    } else {
      error.value = response.error || 'Failed to load audit logs'
      emit('error', error.value || 'Failed to load audit logs')
    }
  } catch (err: any) {
    error.value = err.response?.data?.error || 'Network error loading audit logs'
    emit('error', error.value || 'Network error loading audit logs')
  } finally {
    isLoading.value = false
  }
}

const applyFilters = () => {
  currentPage.value = 1
  void fetchAuditLogs()
}

const goToPage = (page: number) => {
  if (page >= 1 && page <= totalPages.value) {
    currentPage.value = page
    void fetchAuditLogs()
  }
}

const showDetails = (log: AuditLog) => {
  selectedLog.value = log
}

// Helper methods
const formatDate = (dateString: string): string => {
  return new Date(dateString).toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  })
}

const formatTime = (dateString: string): string => {
  return new Date(dateString).toLocaleTimeString('en-US', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

const formatEventType = (eventType: string): string => {
  return eventType
    .split('_')
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(' ')
}

const getEventBadgeClass = (eventType: string): string => {
  const typeMap: Record<string, string> = {
    user_login: 'badge-success',
    user_logout: 'badge-info',
    user_registration: 'badge-primary',
    user_role_change: 'badge-warning',
    user_activation: 'badge-success',
    user_deactivation: 'badge-error',
    user_profile_update: 'badge-info',
    admin_config_reload: 'badge-secondary',
    profile_config_updated: 'badge-secondary',
    profile_config_rolled_back: 'badge-warning',
    failed_login_attempt: 'badge-error',
  }
  return typeMap[eventType] || 'badge-ghost'
}

const formatEventData = (eventData: any): string => {
  try {
    return JSON.stringify(eventData, null, 2)
  } catch {
    return String(eventData)
  }
}

// Lifecycle
onMounted(() => {
  void fetchAuditLogs()
})
</script>
