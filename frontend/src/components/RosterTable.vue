<template>
  <div class="space-y-4">
    <!-- Search and Filter Controls -->
    <div class="flex flex-col sm:flex-row gap-4 items-start sm:items-center justify-between">
      <div class="form-control w-full sm:w-auto">
        <label class="label">
          <span class="label-text">Search users</span>
        </label>
        <div class="input-group">
          <input
            v-model="searchQuery"
            type="text"
            placeholder="Search by name, username, or email..."
            class="input input-bordered w-full sm:w-80"
            @input="debouncedSearch"
          />
          <button v-if="searchQuery" class="btn btn-square" @click="clearSearch">
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M6 18L18 6M6 6l12 12"
              />
            </svg>
          </button>
          <button v-else class="btn btn-square">
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
              />
            </svg>
          </button>
        </div>
      </div>

      <div class="stats shadow">
        <div class="stat">
          <div class="stat-title">Total Users</div>
          <div class="stat-value text-primary">{{ totalUsers }}</div>
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
        <h3 class="font-bold">Error loading roster</h3>
        <div class="text-xs">{{ error }}</div>
      </div>
      <button class="btn btn-sm" @click="fetchUsers">Retry</button>
    </div>

    <!-- Users Table -->
    <div v-else-if="users.length > 0" class="card bg-base-100 shadow-xl">
      <div class="overflow-x-auto">
        <table class="table table-zebra">
          <thead>
            <tr>
              <th>User</th>
              <th>Email</th>
              <th>Role</th>
              <th>Status</th>
              <th>MFA</th>
              <th>Member Since</th>
              <th class="text-center">Actions</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="user in users" :key="user.id">
              <!-- User Info -->
              <td>
                <div class="flex items-center space-x-3">
                  <div class="avatar placeholder">
                    <div class="bg-neutral-focus text-neutral-content rounded-full w-12">
                      <span class="text-xl">{{ getUserInitials(user) }}</span>
                    </div>
                  </div>
                  <div>
                    <div class="font-bold">{{ user.full_name }}</div>
                    <div class="text-sm opacity-50">@{{ user.username }}</div>
                  </div>
                </div>
              </td>

              <!-- Email -->
              <td>
                <span class="text-sm">{{ user.email }}</span>
              </td>

              <!-- Role -->
              <td>
                <div v-if="editingUser === user.id" class="form-control w-32">
                  <select
                    v-model="editingRole"
                    class="select select-bordered select-sm"
                    :disabled="isUpdatingRole"
                    @change="updateUserRole(user.id, editingRole)"
                  >
                    <option v-for="role in availableRoles" :key="role.value" :value="role.value">
                      {{ role.label }}
                    </option>
                  </select>
                </div>
                <div v-else class="flex items-center space-x-2">
                  <span class="badge" :class="getRoleBadgeClass(user.role)">
                    {{ getRoleLabel(user.role) }}
                  </span>
                  <button
                    v-if="canEditRoles && user.id !== authStore.user?.id"
                    class="btn btn-ghost btn-xs"
                    title="Edit role"
                    @click.stop="startEditingRole(user)"
                  >
                    <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z"
                      />
                    </svg>
                  </button>
                </div>
              </td>

              <!-- Status -->
              <td>
                <div class="flex items-center space-x-2">
                  <span class="badge" :class="user.is_active ? 'badge-success' : 'badge-error'">
                    {{ user.is_active ? 'Active' : 'Inactive' }}
                  </span>
                  <button
                    v-if="canToggleStatus(user)"
                    class="btn btn-ghost btn-xs"
                    :disabled="isUpdatingStatus"
                    :title="user.is_active ? 'Deactivate user' : 'Activate user'"
                    @click="toggleUserStatus(user)"
                  >
                    <svg
                      v-if="user.is_active"
                      class="w-4 h-4"
                      fill="none"
                      stroke="currentColor"
                      viewBox="0 0 24 24"
                    >
                      <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M18.364 18.364A9 9 0 005.636 5.636m12.728 12.728L5.636 5.636m12.728 12.728L18.364 5.636M5.636 18.364l12.728-12.728"
                      />
                    </svg>
                    <svg
                      v-else
                      class="w-4 h-4"
                      fill="none"
                      stroke="currentColor"
                      viewBox="0 0 24 24"
                    >
                      <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"
                      />
                    </svg>
                  </button>
                </div>
              </td>

              <!-- MFA -->
              <td>
                <div class="flex items-center gap-2">
                  <span
                    class="badge badge-sm"
                    :class="user.mfa_enrolled_at ? 'badge-success' : 'badge-ghost'"
                  >
                    {{ user.mfa_enrolled_at ? 'Enrolled' : '—' }}
                  </span>
                  <button
                    v-if="canResetMfa && user.mfa_enrolled_at"
                    class="btn btn-ghost btn-xs"
                    :disabled="resettingMfaFor === user.id"
                    title="Reset MFA (lockout recovery)"
                    @click="resetMfa(user)"
                  >
                    <span
                      v-if="resettingMfaFor === user.id"
                      class="loading loading-spinner loading-xs"
                    ></span>
                    <span v-else>Reset</span>
                  </button>
                </div>
              </td>

              <!-- Member Since -->
              <td>
                <span class="text-sm">{{ formatDate(user.created_at) }}</span>
              </td>

              <!-- Actions -->
              <td class="text-center">
                <div class="dropdown dropdown-end">
                  <label tabindex="0" class="btn btn-ghost btn-xs">
                    <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M12 5v.01M12 12v.01M12 19v.01M12 6a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2z"
                      />
                    </svg>
                  </label>
                  <ul
                    tabindex="0"
                    class="dropdown-content menu p-2 shadow bg-base-100 rounded-box w-52"
                  >
                    <li>
                      <router-link :to="`/users/${user.id}`">
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
                        View Profile
                      </router-link>
                    </li>
                  </ul>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <!-- Empty State -->
    <div v-else-if="!isLoading" class="text-center py-12">
      <div class="text-6xl mb-4">👥</div>
      <h3 class="text-lg font-medium mb-2">No users found</h3>
      <p class="text-base-content/70">
        {{ searchQuery ? 'No users match your search criteria.' : 'No users are registered yet.' }}
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
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { adminApi, userApi } from '@/utils/api'
import type { User, UserRole } from '@/types'
import { UserRole as UserRoleEnum } from '@/types'

// Props and Emits
const emit = defineEmits<{
  userUpdated: [user: User]
  error: [message: string]
}>()

// Store
const authStore = useAuthStore()

// Reactive state
const users = ref<User[]>([])
const isLoading = ref(false)
const error = ref<string | null>(null)
const searchQuery = ref('')
const currentPage = ref(1)
const totalUsers = ref(0)
const totalPages = ref(1)

// Role editing state
const editingUser = ref<string | null>(null)
const editingRole = ref<UserRole | null>(null)
const isUpdatingRole = ref(false)
const isUpdatingStatus = ref(false)

// Available roles for dropdown
const availableRoles = computed(() => [
  { value: UserRoleEnum.Newbie, label: 'Newbie' },
  { value: UserRoleEnum.Member, label: 'Member' },
  { value: UserRoleEnum.Staff, label: 'Staff' },
  { value: UserRoleEnum.Admin, label: 'Admin' },
])

// Computed properties
const canEditRoles = computed(() => authStore.isAdmin)
const canToggleUserStatus = computed(() => authStore.isAdmin)
const canResetMfa = computed(() => authStore.isAdmin)

// MFA reset state
const resettingMfaFor = ref<string | null>(null)

async function resetMfa(user: User) {
  if (
    !confirm(
      `Reset MFA for ${user.full_name} (@${user.username})? ` +
        `Their authenticator app, security keys, and recovery codes will all be removed. ` +
        `They will be able to sign in with just their password until they re-enroll.`
    )
  )
    return
  resettingMfaFor.value = user.id
  try {
    const resp = await adminApi.resetUserMfa(user.id)
    if (resp.success) {
      // Optimistically clear locally so the badge updates without a refetch.
      user.mfa_enrolled_at = null
      emit('userUpdated', user)
    } else {
      emit('error', resp.error || 'Failed to reset MFA')
    }
  } catch (e: any) {
    emit('error', e?.response?.data?.error || 'Network error resetting MFA')
  } finally {
    resettingMfaFor.value = null
  }
}

// Pagination helpers
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
const fetchUsers = async () => {
  isLoading.value = true
  error.value = null

  try {
    const response = await userApi.getAllUsers()

    if (response.success && response.data) {
      users.value = response.data.items
      totalUsers.value = response.data.total
      totalPages.value = response.data.total_pages
    } else {
      error.value = response.error || 'Failed to load users'
      emit('error', error.value || 'Failed to load users')
    }
  } catch (err: any) {
    error.value = err.response?.data?.error || 'Network error loading users'
    emit('error', error.value || 'Network error loading users')
  } finally {
    isLoading.value = false
  }
}

const debouncedSearch = (() => {
  let timeout: ReturnType<typeof setTimeout>
  return () => {
    clearTimeout(timeout)
    timeout = setTimeout(() => {
      currentPage.value = 1
      void fetchUsers()
    }, 300)
  }
})()

const clearSearch = () => {
  searchQuery.value = ''
  currentPage.value = 1
  void fetchUsers()
}

const goToPage = (page: number) => {
  if (page >= 1 && page <= totalPages.value) {
    currentPage.value = page
    void fetchUsers()
  }
}

const canToggleStatus = (user: User): boolean => {
  if (!canToggleUserStatus.value) return false
  if (String(user.id) === String(authStore.user?.id)) return false // Can't deactivate self
  return true
}

const startEditingRole = (user: User) => {
  editingUser.value = user.id
  editingRole.value = user.role
}

const cancelEditingRole = () => {
  editingUser.value = null
  editingRole.value = null
}

const updateUserRole = async (userId: string, newRole: UserRole | null) => {
  if (!newRole) return

  isUpdatingRole.value = true
  try {
    const response = await userApi.updateUserRole(userId, newRole)

    if (response.success && response.data) {
      const userIndex = users.value.findIndex((u) => u.id === userId)
      if (userIndex !== -1) {
        users.value[userIndex] = response.data
        emit('userUpdated', response.data)
      }
      cancelEditingRole()
    } else {
      // Emitted, not assigned to `error`. That ref is the *load* error state and
      // the template renders it in place of the table -- so writing an action
      // failure into it replaced the whole roster with a banner, losing every
      // row because one update was refused. The parent already receives this
      // and puts it somewhere the user is looking.
      emit('error', response.error || 'Failed to update user role')
    }
  } catch (err: any) {
    emit('error', err.response?.data?.error || 'Network error updating user role')
  } finally {
    isUpdatingRole.value = false
  }
}

const toggleUserStatus = async (user: User) => {
  isUpdatingStatus.value = true
  try {
    const response = user.is_active
      ? await userApi.deactivateUser(user.id)
      : await userApi.activateUser(user.id)

    if (response.success && response.data) {
      const userIndex = users.value.findIndex((u) => u.id === user.id)
      if (userIndex !== -1) {
        users.value[userIndex] = response.data
        emit('userUpdated', response.data)
      }
    } else {
      // See updateUserRole: `error` is the load channel and replaces the table.
      emit('error', response.error || 'Failed to update user status')
    }
  } catch (err: any) {
    emit('error', err.response?.data?.error || 'Network error updating user status')
  } finally {
    isUpdatingStatus.value = false
  }
}

// Helper methods
const getUserInitials = (user: User): string => {
  const names = user.full_name.trim().split(' ')
  if (names.length === 1) {
    return names[0].charAt(0).toUpperCase()
  }
  return (names[0].charAt(0) + names[names.length - 1].charAt(0)).toUpperCase()
}

const getRoleLabel = (role: UserRole): string => {
  const roleMap: Record<UserRole, string> = {
    [UserRoleEnum.Unknown]: 'Unknown',
    [UserRoleEnum.Newbie]: 'Newbie',
    [UserRoleEnum.Member]: 'Member',
    [UserRoleEnum.Staff]: 'Staff',
    [UserRoleEnum.Admin]: 'Admin',
  }
  return roleMap[role] || 'Unknown'
}

const getRoleBadgeClass = (role: UserRole): string => {
  const classMap: Record<UserRole, string> = {
    [UserRoleEnum.Unknown]: 'badge-ghost',
    [UserRoleEnum.Newbie]: 'badge-info',
    [UserRoleEnum.Member]: 'badge-success',
    [UserRoleEnum.Staff]: 'badge-warning',
    [UserRoleEnum.Admin]: 'badge-error',
  }
  return classMap[role] || 'badge-ghost'
}

const formatDate = (dateString: string): string => {
  return new Date(dateString).toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  })
}

// Lifecycle
onMounted(() => {
  void fetchUsers()
})

// Handle clicking outside to cancel editing
const handleClickOutside = (event: Event) => {
  if (editingUser.value && !(event.target as Element).closest('.form-control')) {
    cancelEditingRole()
  }
}

onMounted(() => {
  document.addEventListener('click', handleClickOutside)
})

// Watch for search query changes
watch(searchQuery, () => {
  if (!searchQuery.value) {
    currentPage.value = 1
    void fetchUsers()
  }
})

// `<script setup>` exposes nothing to a template ref by default, so
// RosterView's `rosterTable.value.fetchUsers()` was `undefined` at runtime --
// its refresh button called a function that did not exist. The call site had
// an `as any` cast on it, which silenced the type error and left the runtime
// failure to be swallowed by the surrounding try/catch.
//
// Found when the lint upgrade's no-unnecessary-type-assertion rule removed
// that cast and the type-check finally said what had always been true.
defineExpose({ fetchUsers })
</script>
