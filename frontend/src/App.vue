<template>
  <div id="app" class="min-h-screen bg-base-100">
    <!-- Navigation -->
    <nav class="navbar bg-base-300 shadow-lg">
      <div class="navbar-start">
        <div class="dropdown">
          <div tabindex="0" role="button" class="btn btn-ghost lg:hidden">
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M4 6h16M4 12h8m-8 6h16"
              />
            </svg>
          </div>
          <ul
            tabindex="0"
            class="menu menu-sm dropdown-content bg-base-100 rounded-box z-[1] mt-3 w-52 p-2 shadow"
          >
            <li><router-link to="/">Home</router-link></li>
            <li><router-link to="/about">About</router-link></li>
            <li><router-link to="/events">Events</router-link></li>
            <li v-if="authStore.isAuthenticated">
              <router-link :to="`/profile/me`">My Profile</router-link>
            </li>
            <li v-if="authStore.isAuthenticated">
              <router-link to="/tools">Tools</router-link>
            </li>
            <li v-if="authStore.isAuthenticated">
              <router-link to="/modules">My Modules</router-link>
            </li>
            <li v-if="showWikiInNav">
              <router-link to="/wiki">Wiki</router-link>
            </li>
            <li v-if="showSiteInNav">
              <router-link to="/page">Pages</router-link>
            </li>
            <li><router-link to="/contact">Contact</router-link></li>
            <li><router-link to="/directions">Directions</router-link></li>
            <li v-if="authStore.isAuthenticated && canAccessStaff">
              <router-link to="/users">Users</router-link>
            </li>
            <li v-if="authStore.isAuthenticated && canAccessAdmin">
              <details>
                <summary>Admin</summary>
                <ul class="p-2">
                  <li><router-link to="/admin">Dashboard</router-link></li>
                  <li><router-link to="/admin/roster">Roster</router-link></li>
                  <li><router-link to="/admin/audit">Audit Log</router-link></li>
                  <li><router-link to="/admin/cmi5">Training Modules</router-link></li>
                </ul>
              </details>
            </li>
          </ul>
        </div>
        <router-link to="/" class="btn btn-ghost">
          <img src="/images/nav_logo.png" alt="Cambridge Hackspace" class="h-10 w-auto" />
        </router-link>
      </div>

      <div class="navbar-center hidden lg:flex">
        <ul class="menu menu-horizontal px-1">
          <li>
            <router-link to="/" :class="{ active: $route.name === 'home' }">Home</router-link>
          </li>
          <li>
            <router-link to="/about" :class="{ active: $route.name === 'about' }"
              >About</router-link
            >
          </li>
          <li>
            <router-link to="/events" :class="{ active: $route.name === 'events' }"
              >Events</router-link
            >
          </li>
          <li v-if="authStore.isAuthenticated">
            <router-link
              :to="`/profile/me`"
              :class="{ active: $route.name === 'profile' && $route.params.userId === 'me' }"
            >
              My Profile
            </router-link>
          </li>
          <li v-if="authStore.isAuthenticated">
            <router-link to="/tools" :class="{ active: $route.name === 'tools' }"
              >Tools</router-link
            >
          </li>
          <li v-if="authStore.isAuthenticated">
            <router-link to="/modules" :class="{ active: $route.name === 'my-modules' }"
              >My Modules</router-link
            >
          </li>
          <li v-if="showWikiInNav">
            <router-link to="/wiki" :class="{ active: $route.path.startsWith('/wiki') }"
              >Wiki</router-link
            >
          </li>
          <li v-if="showSiteInNav">
            <router-link to="/page" :class="{ active: $route.path.startsWith('/page') }"
              >Pages</router-link
            >
          </li>
          <li>
            <router-link to="/contact" :class="{ active: $route.name === 'contact' }"
              >Contact</router-link
            >
          </li>
          <li>
            <router-link to="/directions" :class="{ active: $route.name === 'directions' }"
              >Directions</router-link
            >
          </li>
          <li v-if="authStore.isAuthenticated && canAccessStaff">
            <router-link to="/users" :class="{ active: $route.name === 'users' }"
              >Users</router-link
            >
          </li>
          <li v-if="authStore.isAuthenticated && canAccessAdmin">
            <details>
              <summary>Admin</summary>
              <ul class="p-2 bg-base-100 rounded-box">
                <li><router-link to="/admin">Dashboard</router-link></li>
                <li><router-link to="/admin/roster">Roster</router-link></li>
                <li><router-link to="/admin/audit">Audit Log</router-link></li>
                <li><router-link to="/admin/cmi5">Training Modules</router-link></li>
              </ul>
            </details>
          </li>
        </ul>
      </div>

      <div class="navbar-end">
        <div v-if="!authStore.isAuthenticated" class="flex gap-2">
          <router-link to="/login" class="btn btn-ghost btn-sm">Login</router-link>
          <router-link to="/register" class="btn btn-primary btn-sm">Register</router-link>
        </div>
        <div v-else class="dropdown dropdown-end">
          <div tabindex="0" role="button" class="btn btn-ghost btn-circle avatar">
            <div class="w-10 rounded-full ring ring-primary ring-offset-base-100 ring-offset-2">
              <div class="w-full h-full bg-primary/20 flex items-center justify-center">
                <span class="text-primary font-semibold">
                  {{
                    authStore.user?.full_name?.charAt(0) ||
                    authStore.user?.username?.charAt(0) ||
                    '?'
                  }}
                </span>
              </div>
            </div>
          </div>
          <ul
            tabindex="0"
            class="menu menu-sm dropdown-content bg-base-100 rounded-box z-[1] mt-3 w-52 p-2 shadow"
          >
            <li>
              <router-link :to="`/profile/me`" class="justify-between">
                My Profile
                <span class="badge">{{ authStore.user?.role }}</span>
              </router-link>
            </li>
            <li><router-link :to="`/profile/me`">Settings</router-link></li>
            <li><a @click="logout">Logout</a></li>
          </ul>
        </div>
      </div>
    </nav>

    <!-- Main Content -->
    <main class="min-h-screen">
      <router-view />
    </main>

    <!-- Footer -->
    <footer class="footer footer-center bg-base-300 text-base-content/70 p-6 text-sm">
      <nav class="flex flex-wrap items-center justify-center gap-x-2">
        <router-link to="/" class="link link-hover">Home</router-link>
        <span>|</span>
        <router-link to="/about" class="link link-hover">About</router-link>
        <span>|</span>
        <router-link to="/events" class="link link-hover">Events</router-link>
        <span>|</span>
        <router-link to="/platform" class="link link-hover">Platform</router-link>
        <span>|</span>
        <router-link to="/terms" class="link link-hover">Terms and Conditions</router-link>
        <span>|</span>
        <router-link to="/privacy" class="link link-hover">Privacy Policy</router-link>
        <span>|</span>
        <router-link to="/501c3" class="link link-hover">501(c)(3)</router-link>
        <span>|</span>
        <span>&copy; {{ new Date().getFullYear() }} Cambridge Hackspace</span>
      </nav>
    </footer>

    <!-- Notifications -->
    <div class="toast toast-top toast-end z-50">
      <div
        v-for="notification in notifications"
        :key="notification.id"
        class="alert"
        :class="{
          'alert-success': notification.type === 'success',
          'alert-error': notification.type === 'error',
          'alert-warning': notification.type === 'warning',
          'alert-info': notification.type === 'info',
        }"
      >
        <svg
          v-if="notification.type === 'success'"
          class="w-6 h-6"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M5 13l4 4L19 7"
          />
        </svg>
        <svg
          v-else-if="notification.type === 'error'"
          class="w-6 h-6"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
          />
        </svg>
        <svg
          v-else-if="notification.type === 'warning'"
          class="w-6 h-6"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L3.732 16c-.77.833.192 2.5 1.732 2.5z"
          />
        </svg>
        <svg v-else class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
          />
        </svg>
        <div>
          <h3 class="font-bold">{{ notification.title }}</h3>
          <div class="text-xs">{{ notification.message }}</div>
        </div>
        <button class="btn btn-sm btn-ghost" @click="removeNotification(notification.id)">
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

    <!-- Loading overlay -->
    <div
      v-if="globalLoading"
      class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
    >
      <div class="bg-base-100 p-8 rounded-lg shadow-xl text-center">
        <div class="loading loading-spinner loading-lg mb-4"></div>
        <p class="text-lg">Loading...</p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { useConfigStore } from '@/stores/config'
import type { Notification } from '@/types'
import { UserRole as UserRoleEnum } from '@/types'
import { resolveTheme, onSystemThemeChange } from '@/utils/theme'

const router = useRouter()
const authStore = useAuthStore()
const configStore = useConfigStore()

// Local state
const notifications = ref<Notification[]>([])
const globalLoading = ref(false)

// Computed properties
const canAccessStaff = computed(() => {
  const user = authStore.user
  return user && (user.role === UserRoleEnum.Staff || user.role === UserRoleEnum.Admin)
})

const canAccessAdmin = computed(() => {
  const user = authStore.user
  return user && user.role === UserRoleEnum.Admin
})

// Pages visibility
// Restored after the merge: dev's nav rework dropped the Pages link and its
// computed, but `[pages] site_enabled` still gates a real feature and
// `tests/unit/config-store.spec.ts` asserts the two links are decided
// independently.
const showSiteInNav = computed(() => configStore.shouldShowSiteInNav())

const showWikiInNav = computed(() => {
  const result = configStore.shouldShowWikiInNav()
  console.log('showWikiInNav computed:', result)
  return result
})
// Methods
async function logout() {
  globalLoading.value = true
  try {
    authStore.logout()
    await router.push('/')
  } finally {
    globalLoading.value = false
  }
}

function addNotification(notification: Omit<Notification, 'id'>) {
  const id = Date.now().toString() + Math.random().toString(36).substr(2, 9)
  const newNotification: Notification = { id, ...notification }

  notifications.value.push(newNotification)

  // Auto-remove after duration
  if (notification.duration !== 0) {
    setTimeout(() => {
      removeNotification(id)
    }, notification.duration || 5000)
  }
}

function removeNotification(id: string) {
  const index = notifications.value.findIndex((n) => n.id === id)
  if (index > -1) {
    notifications.value.splice(index, 1)
  }
}

// Apply theme from user meta — resolveTheme treats a missing preference
// (anonymous visitors, or a user who hasn't picked one) the same as an
// explicit "system" choice, following the OS/browser's light/dark setting.
function applyTheme() {
  // `meta` is `Record<string, unknown>`, so the stored theme is `unknown` until
  // it is narrowed. Passing it straight through only type-checks where `meta`
  // is `any`, and a non-string in that slot would reach `setAttribute` as
  // "[object Object]".
  const stored = authStore.user?.meta?.theme
  document.documentElement.setAttribute(
    'data-theme',
    resolveTheme(typeof stored === 'string' ? stored : undefined)
  )
}

// Watch for user changes to apply theme
watch(
  () => authStore.user,
  () => {
    applyTheme()
  },
  { deep: true }
)

// Keep following the OS setting live whenever the effective choice is
// "system" (applyTheme re-reads the stored preference each time, so this is
// a no-op for users with a fixed theme selected).
onSystemThemeChange(applyTheme)

// Lifecycle
onMounted(async () => {
  globalLoading.value = true
  try {
    await Promise.all([authStore.initialize(), configStore.fetchConfig()])
  } catch (error) {
    console.error('Initialization failed:', error)
    addNotification({
      type: 'error',
      title: 'Initialization Error',
      message: 'Failed to initialize application',
      duration: 8000,
    })
  } finally {
    globalLoading.value = false
  }

  // Apply theme after initialization
  applyTheme()
})
</script>

<style scoped>
.router-link-active {
  @apply bg-primary text-primary-content;
}

.toast {
  max-width: 24rem;
}
</style>
