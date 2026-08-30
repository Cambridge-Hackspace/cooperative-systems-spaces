<template>
  <div class="container mx-auto px-4 py-8">
    <!-- Breadcrumbs -->
    <div class="breadcrumbs text-sm mb-6">
      <ul>
        <li>
          <router-link to="/" class="link">Home</router-link>
        </li>
        <li>
          <router-link to="/admin" class="link">Admin</router-link>
        </li>
        <li>Roster Management</li>
      </ul>
    </div>

    <!-- Header -->
    <div class="mb-8">
      <h1 class="text-3xl font-bold mb-2">Roster Management</h1>
      <p class="text-base-content/70">View and manage all user accounts, roles, and permissions.</p>
    </div>

    <!-- Access Control -->
    <div v-if="!canAccessRoster" class="alert alert-error mb-8">
      <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          stroke-width="2"
          d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
        />
      </svg>
      <div>
        <h3 class="font-bold">Access Denied</h3>
        <div class="text-xs">
          You need administrator or staff privileges to access roster management.
        </div>
      </div>
      <router-link to="/admin" class="btn btn-sm"> Back to Admin </router-link>
    </div>

    <!-- Success Toast -->
    <div v-if="successMessage" class="toast toast-top toast-end z-50">
      <div class="alert alert-success">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"
          />
        </svg>
        <span>{{ successMessage }}</span>
      </div>
    </div>

    <!-- Error Toast -->
    <div v-if="errorMessage" class="toast toast-top toast-end z-50">
      <div class="alert alert-error">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
          />
        </svg>
        <span>{{ errorMessage }}</span>
        <button class="btn btn-sm btn-ghost" @click="clearError">
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M6 18L18 6M6 6l12 12"
            />
          </svg>
        </button>
      </div>
    </div>

    <!-- Main Content -->
    <div v-if="canAccessRoster">
      <!-- Quick Actions Card -->
      <div class="card bg-base-100 shadow-xl mb-6">
        <div class="card-body">
          <h2 class="card-title">Quick Actions</h2>
          <div class="flex flex-wrap gap-2">
            <div class="stats shadow">
              <div class="stat">
                <div class="stat-figure text-primary">
                  <svg class="w-8 h-8" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      stroke-width="2"
                      d="M12 4.354a4 4 0 110 5.292M15 21H3v-1a6 6 0 0112 0v1zm0 0h6v-1a6 6 0 00-9-5.197m13.5-9a2.5 2.5 0 11-5 0 2.5 2.5 0 015 0z"
                    />
                  </svg>
                </div>
                <div class="stat-title">Admin Actions</div>
                <div class="stat-desc">Manage user roles and status</div>
              </div>
            </div>

            <div class="flex flex-col gap-2">
              <button
                class="btn btn-primary btn-sm"
                :disabled="isRefreshing"
                @click="refreshRoster"
              >
                <span v-if="isRefreshing" class="loading loading-spinner loading-xs"></span>
                <svg v-else class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="2"
                    d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
                  />
                </svg>
                Refresh
              </button>

              <div class="dropdown">
                <label tabindex="0" class="btn btn-outline btn-sm">
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      stroke-width="2"
                      d="M12 5v.01M12 12v.01M12 19v.01M12 6a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2z"
                    />
                  </svg>
                  More Actions
                </label>
                <ul
                  tabindex="0"
                  class="dropdown-content menu p-2 shadow bg-base-100 rounded-box w-52"
                >
                  <li>
                    <router-link to="/users">
                      <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path
                          stroke-linecap="round"
                          stroke-linejoin="round"
                          stroke-width="2"
                          d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
                        />
                        <path
                          stroke-linecap="round"
                          stroke-linejoin="round"
                          stroke-width="2"
                          d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z"
                        />
                      </svg>
                      User Directory
                    </router-link>
                  </li>
                </ul>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Role Distribution Stats -->
      <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4 mb-6">
        <div class="stat bg-base-100 shadow rounded-lg">
          <div class="stat-figure text-info">
            <svg class="w-8 h-8" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
              />
            </svg>
          </div>
          <div class="stat-title">Newbies</div>
          <div class="stat-value text-info">{{ roleStats.newbie }}</div>
          <div class="stat-desc">New members</div>
        </div>

        <div class="stat bg-base-100 shadow rounded-lg">
          <div class="stat-figure text-success">
            <svg class="w-8 h-8" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"
              />
            </svg>
          </div>
          <div class="stat-title">Members</div>
          <div class="stat-value text-success">{{ roleStats.member }}</div>
          <div class="stat-desc">Active members</div>
        </div>

        <div class="stat bg-base-100 shadow rounded-lg">
          <div class="stat-figure text-warning">
            <svg class="w-8 h-8" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M12 9v3m0 0v3m0-3h3m-3 0H9m12 0a9 9 0 11-18 0 9 9 0 0118 0z"
              />
            </svg>
          </div>
          <div class="stat-title">Staff</div>
          <div class="stat-value text-warning">{{ roleStats.staff }}</div>
          <div class="stat-desc">Staff members</div>
        </div>

        <div class="stat bg-base-100 shadow rounded-lg">
          <div class="stat-figure text-error">
            <svg class="w-8 h-8" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M5.121 17.804A13.937 13.937 0 0112 16c2.5 0 4.847.655 6.879 1.804M15 10a3 3 0 11-6 0 3 3 0 016 0zm6 2a9 9 0 11-18 0 9 9 0 0118 0z"
              />
            </svg>
          </div>
          <div class="stat-title">Admins</div>
          <div class="stat-value text-error">{{ roleStats.admin }}</div>
          <div class="stat-desc">Administrators</div>
        </div>
      </div>

      <!-- Roster Table -->
      <RosterTable ref="rosterTable" @user-updated="handleUserUpdated" @error="handleError" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { userApi } from '@/utils/api'
import RosterTable from '@/components/RosterTable.vue'
import type { User } from '@/types'
import { UserRole } from '@/types'

// Store
const authStore = useAuthStore()

// Reactive state
const successMessage = ref<string | null>(null)
const errorMessage = ref<string | null>(null)
const isRefreshing = ref(false)
const roleStats = ref({
  newbie: 0,
  member: 0,
  staff: 0,
  admin: 0,
})

// Template refs
const rosterTable = ref<InstanceType<typeof RosterTable> | null>(null)

// Computed properties
const canAccessRoster = computed(() => {
  return authStore.isAdmin || authStore.isStaff
})

// Methods
const showSuccess = (message: string) => {
  successMessage.value = message
  setTimeout(() => {
    successMessage.value = null
  }, 5000)
}

const handleError = (message: string) => {
  errorMessage.value = message
  setTimeout(() => {
    errorMessage.value = null
  }, 8000)
}

const clearError = () => {
  errorMessage.value = null
}

const handleUserUpdated = (user: User) => {
  showSuccess(`User ${user.username} has been updated successfully.`)
  void loadRoleStats()
}

const refreshRoster = async () => {
  isRefreshing.value = true
  try {
    // Refresh the roster table
    if (rosterTable.value) {
      await rosterTable.value.fetchUsers()
    }

    // Refresh role stats
    await loadRoleStats()

    showSuccess('Roster data refreshed successfully.')
  } catch (error) {
    // Logged rather than discarded: a swallowed error is indistinguishable
    // from a successful no-op to anyone reading the console.
    console.error(error)
    handleError('Failed to refresh roster data.')
  } finally {
    isRefreshing.value = false
  }
}

const loadRoleStats = async () => {
  try {
    const response = await userApi.getAllUsers() // Get all users for stats

    if (response.success && response.data) {
      const users = response.data.items
      roleStats.value = {
        newbie: users.filter((u: User) => u.role === UserRole.Newbie).length,
        member: users.filter((u: User) => u.role === UserRole.Member).length,
        staff: users.filter((u: User) => u.role === UserRole.Staff).length,
        admin: users.filter((u: User) => u.role === UserRole.Admin).length,
      }
    }
  } catch (error) {
    // Silently fail for stats - not critical
    console.warn('Failed to load role statistics:', error)
  }
}

// Lifecycle
onMounted(() => {
  if (canAccessRoster.value) {
    void loadRoleStats()
  }
})
</script>

<style scoped>
.toast {
  z-index: 1000;
}
</style>
