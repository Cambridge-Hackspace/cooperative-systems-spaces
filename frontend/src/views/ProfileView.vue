<template>
  <div class="container mx-auto px-4 py-8 max-w-4xl">
    <div class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/" class="link">Home</router-link></li>
        <li>
          <span v-if="isOwnProfile">My Profile</span>
          <span v-else>{{ user?.full_name || 'User' }}'s Profile</span>
        </li>
      </ul>
    </div>

    <UserProfile 
      :user-id="userId" 
      :user="user || undefined"
      class="mb-8"
    />

    <!-- Two-factor link (only when viewing own profile AND MFA is enabled server-side) -->
    <div v-if="isOwnProfile && mfaAvailable" class="mb-6 flex items-center justify-between bg-base-200 rounded-lg p-4">
      <div>
        <div class="font-medium">Two-factor authentication</div>
        <div class="text-sm text-base-content/70">
          Add an authenticator app or security key to your account.
        </div>
      </div>
      <router-link to="/profile/mfa" class="btn btn-primary btn-sm">Manage</router-link>
    </div>

    <!-- Theme Picker (only shown for own profile) -->
    <ThemePicker
      v-if="isOwnProfile"
      :user-id="userId"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useRoute } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import UserProfile from '@/components/UserProfile.vue'
import ThemePicker from '@/components/ThemePicker.vue'
import { apiClient, mfaApi } from '@/utils/api'
import type { User } from '@/types'

const route = useRoute()
const authStore = useAuthStore()

// Local state
const user = ref<User | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
/** True only after we've confirmed the server has MFA enabled. */
const mfaAvailable = ref(false)

// Computed properties
const userId = computed(() => {
  const routeUserId = route.params.userId as string
  return routeUserId === 'me' || !routeUserId ? authStore.user?.id || '' : routeUserId
})

const isOwnProfile = computed(() => userId.value === authStore.user?.id)

// Methods
async function fetchUser() {
  if (isOwnProfile.value) {
    user.value = authStore.user
    return
  }

  if (!userId.value) return

  loading.value = true
  error.value = null

  try {
    const response = await apiClient.get<User>(`/users/${userId.value}`)
    if (response.success && response.data) {
      user.value = response.data
    } else {
      throw new Error(response.error || 'Failed to fetch user')
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to fetch user'
  } finally {
    loading.value = false
  }
}

// Lifecycle
onMounted(async () => {
  await fetchUser()
  // Decide whether to surface the two-factor link without giving away whether
  // *this* user is enrolled — we just need the server-wide enabled flag.
  if (isOwnProfile.value) {
    try {
      const s = await mfaApi.status()
      mfaAvailable.value = !!(s.success && s.data?.enabled)
    } catch {
      mfaAvailable.value = false
    }
  }
})

// Watch for route changes
watch(() => userId.value, async () => {
  await fetchUser()
})

// Watch for auth changes
watch(() => authStore.user, () => {
  if (isOwnProfile.value) {
    user.value = authStore.user
  }
})
</script>
