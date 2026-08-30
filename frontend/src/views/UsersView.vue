<template>
  <div class="container mx-auto px-4 py-8">
    <div class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/" class="link">Home</router-link></li>
        <li>Users</li>
      </ul>
    </div>

    <div class="mb-8">
      <h1 class="text-3xl font-bold mb-2">User Management</h1>
      <p class="text-base-content/70">View and manage user accounts.</p>
    </div>

    <!-- Access Control -->
    <div v-if="!canAccessUsers" class="alert alert-error mb-8">
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
        <div class="text-xs">You need staff or administrator privileges to access this page.</div>
      </div>
    </div>

    <!-- Users List -->
    <div v-if="canAccessUsers" class="card bg-base-100 shadow-xl">
      <div class="card-body">
        <h2 class="card-title mb-4">Users</h2>

        <!-- Loading State -->
        <div v-if="loading" class="flex items-center justify-center py-12">
          <div class="loading loading-spinner loading-lg"></div>
          <span class="ml-3">Loading users...</span>
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
            <h3 class="font-bold">Error loading users</h3>
            <div class="text-xs">{{ error }}</div>
          </div>
          <button class="btn btn-sm" @click="loadUsers">Retry</button>
        </div>

        <!-- Users Table -->
        <div v-else-if="users.length > 0" class="overflow-x-auto">
          <table class="table table-zebra">
            <thead>
              <tr>
                <th>User</th>
                <th>Role</th>
                <th>Status</th>
                <th>Created</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="user in users" :key="user.id">
                <td>
                  <div class="flex items-center space-x-3">
                    <div class="avatar placeholder">
                      <div class="bg-neutral text-neutral-content rounded-full w-12 h-12">
                        <span class="text-xs">{{ user.full_name.charAt(0) }}</span>
                      </div>
                    </div>
                    <div>
                      <div class="font-bold">{{ user.full_name }}</div>
                      <div class="text-sm opacity-50">{{ user.username }}</div>
                      <div class="text-sm opacity-50">{{ user.email }}</div>
                    </div>
                  </div>
                </td>
                <td>
                  <div class="badge" :class="getRoleBadgeClass(user.role)">
                    {{ user.role }}
                  </div>
                </td>
                <td>
                  <div class="badge" :class="user.is_active ? 'badge-success' : 'badge-error'">
                    {{ user.is_active ? 'Active' : 'Inactive' }}
                  </div>
                </td>
                <td>{{ formatDate(user.created_at) }}</td>
                <td>
                  <div class="flex gap-2">
                    <router-link :to="`/profile/${user.id}`" class="btn btn-ghost btn-xs">
                      View Profile
                    </router-link>
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </div>

        <!-- No Users State -->
        <div v-else class="text-center py-12">
          <svg
            class="w-16 h-16 mx-auto text-base-content/30 mb-4"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M12 4.354a4 4 0 110 5.292M15 21H3v-1a6 6 0 0112 0v1zm0 0h6v-1a6 6 0 00-9-5.197m13.5-9a2.5 2.5 0 11-5 0 2.5 2.5 0z"
            />
          </svg>
          <h3 class="text-lg font-medium text-base-content/70 mb-2">No Users Found</h3>
          <p class="text-base-content/50">No users are currently registered.</p>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { apiClient } from '@/utils/api'
import { UserRole } from '@/types'
import type { User, PaginatedResponse } from '@/types'

const authStore = useAuthStore()

// Local state
const users = ref<User[]>([])
const loading = ref(false)
const error = ref<string | null>(null)

// Computed properties
const canAccessUsers = computed(() => {
  const user = authStore.user
  return user && (user.role === UserRole.Staff || user.role === UserRole.Admin)
})

// Methods
async function loadUsers() {
  if (!canAccessUsers.value) return

  loading.value = true
  error.value = null

  try {
    const response = await apiClient.get<PaginatedResponse<User>>('/users', {
      page: 1,
      per_page: 50,
    })

    if (response.success && response.data) {
      users.value = response.data.items
    } else {
      throw new Error(response.error || 'Failed to load users')
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to load users'
  } finally {
    loading.value = false
  }
}

function getRoleBadgeClass(role: string) {
  switch (role) {
    case 'admin':
      return 'badge-error'
    case 'staff':
      return 'badge-warning'
    case 'member':
      return 'badge-info'
    case 'newbie':
      return 'badge-success'
    default:
      return 'badge-ghost'
  }
}

function formatDate(dateString: string) {
  return new Date(dateString).toLocaleDateString()
}

// Lifecycle
onMounted(async () => {
  if (canAccessUsers.value) {
    await loadUsers()
  }
})
</script>
