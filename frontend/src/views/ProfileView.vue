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

    <UserProfile :user-id="userId" :user="user || undefined" class="mb-8" />

    <!-- Two-factor link (only when viewing own profile AND MFA is enabled server-side) -->
    <div
      v-if="isOwnProfile && mfaAvailable"
      class="mb-6 flex items-center justify-between bg-base-200 rounded-lg p-4"
    >
      <div>
        <div class="font-medium">Two-factor authentication</div>
        <div class="text-sm text-base-content/70">
          Add an authenticator app or security key to your account.
        </div>
      </div>
      <router-link to="/profile/mfa" class="btn btn-primary btn-sm">Manage</router-link>
    </div>

    <!-- Change password link (only shown for own profile) -->
    <div
      v-if="isOwnProfile"
      class="mb-6 flex items-center justify-between bg-base-200 rounded-lg p-4"
    >
      <div>
        <div class="font-medium">Password</div>
        <div class="text-sm text-base-content/70">Change your account password.</div>
      </div>
      <router-link to="/profile/password" class="btn btn-primary btn-sm">Change</router-link>
    </div>

    <!-- Transit card link (only when viewing own profile AND a card field is configured) -->
    <div
      v-if="isOwnProfile && cardFieldConfigured"
      class="mb-6 flex items-center justify-between bg-base-200 rounded-lg p-4"
    >
      <div>
        <div class="font-medium">Transit Card</div>
        <div class="text-sm text-base-content/70">
          Set your RFID/NFC card ID for door and tool access.
        </div>
      </div>
      <router-link to="/profile/card" class="btn btn-primary btn-sm">Manage</router-link>
    </div>

    <!-- Mailing List subscription (own profile only, when the module is enabled) -->
    <MembershipCard v-if="isOwnProfile && membershipEnabled" class="mb-6" />

    <MailingListCard v-if="isOwnProfile && groupsioEnabled" class="mb-6" />

    <!-- Theme Picker (only shown for own profile) -->
    <ThemePicker v-if="isOwnProfile" :user-id="userId" />

    <!-- Instance QR (own profile only). Drives cross-device handoff into the
         Cooperative Spaces app — the JSON payload mirrors what the Android
         onboarding scanner expects. -->
    <InstanceQrCard v-if="isOwnProfile" class="mt-6" />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useRoute } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { useConfigStore } from '@/stores/config'
import UserProfile from '@/components/UserProfile.vue'
import ThemePicker from '@/components/ThemePicker.vue'
import MailingListCard from '@/components/MailingListCard.vue'
import MembershipCard from '@/components/MembershipCard.vue'
import InstanceQrCard from '@/components/InstanceQrCard.vue'
import { apiClient, mfaApi } from '@/utils/api'
import type { User } from '@/types'

const route = useRoute()
const authStore = useAuthStore()
const configStore = useConfigStore()

// Local state
const user = ref<User | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
/** True only after we've confirmed the server has MFA enabled. */
const mfaAvailable = ref(false)
/** True when the server has a profile field configured to hold a card id. */
const cardFieldConfigured = computed(() => !!configStore.cardIdField())
/** True when the Groups.io mailing-list module is enabled server-side. */
const groupsioEnabled = computed(() => configStore.groupsioEnabled())
const membershipEnabled = computed(() => configStore.membershipEnabled())

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
watch(
  () => userId.value,
  async () => {
    await fetchUser()
  }
)

// Watch for auth changes
watch(
  () => authStore.user,
  () => {
    if (isOwnProfile.value) {
      user.value = authStore.user
    }
  }
)
</script>
