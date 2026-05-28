<template>
  <div class="container mx-auto px-4 py-8">
    <div class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/" class="link">Home</router-link></li>
        <li>Admin</li>
      </ul>
    </div>

    <div class="mb-8">
      <h1 class="text-3xl font-bold mb-2">Admin Dashboard</h1>
      <p class="text-base-content/70">
        Manage system settings and configurations.
      </p>
    </div>

    <!-- Access Control -->
    <div v-if="!canAccessAdmin" class="alert alert-error mb-8">
      <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/>
      </svg>
      <div>
        <h3 class="font-bold">Access Denied</h3>
        <div class="text-xs">You need administrator privileges to access this page.</div>
      </div>
    </div>

    <!-- Admin Content -->
    <div v-if="canAccessAdmin" class="grid gap-6 md:grid-cols-2 lg:grid-cols-3">

      <!-- User Management -->
      <div class="card bg-base-100 shadow-xl">
        <div class="card-body">
          <h2 class="card-title">
            <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4.354a4 4 0 110 5.292M15 21H3v-1a6 6 0 0112 0v1zm0 0h6v-1a6 6 0 00-9-5.197m13.5-9a2.5 2.5 0 11-5 0 2.5 2.5 0 015 0z"/>
            </svg>
            Roster Management
          </h2>
          <p>Manage user accounts, roles, and permissions.</p>
          <div class="card-actions justify-end">
            <router-link to="/admin/roster" class="btn btn-primary btn-sm">
              Manage Roster
            </router-link>
          </div>
        </div>
      </div>

      <!-- User Directory -->
      <div class="card bg-base-100 shadow-xl">
        <div class="card-body">
          <h2 class="card-title">
            <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z"/>
            </svg>
            User Directory
          </h2>
          <p>Browse and search user profiles and information.</p>
          <div class="card-actions justify-end">
            <router-link to="/users" class="btn btn-primary btn-sm">
              View Directory
            </router-link>
          </div>
        </div>
      </div>

      <!-- Audit Logs -->
      <div class="card bg-base-100 shadow-xl">
        <div class="card-body">
          <h2 class="card-title">
            <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"/>
            </svg>
            Audit Logs
          </h2>
          <p>View system activity and user action logs.</p>
          <div class="card-actions justify-end">
            <router-link to="/admin/audit" class="btn btn-primary btn-sm">
              View Logs
            </router-link>
          </div>
        </div>
      </div>

      <!-- Device Management -->
      <div class="card bg-base-100 shadow-xl">
        <div class="card-body">
          <h2 class="card-title">
            <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 18h.01M8 21h8a2 2 0 002-2V5a2 2 0 00-2-2H8a2 2 0 00-2 2v14a2 2 0 002 2z"/>
            </svg>
            Device Management
          </h2>
          <p>Manage edge apparatuss, generate invite codes, and monitor device status.</p>
          <div class="card-actions justify-end">
            <router-link to="/admin/devices" class="btn btn-primary btn-sm">
              Manage Devices
            </router-link>
          </div>
        </div>
      </div>

      <!-- Webhooks -->
      <div class="card bg-base-100 shadow-xl">
        <div class="card-body">
          <h2 class="card-title">
            <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1"/>
            </svg>
            Webhooks
          </h2>
          <p>Define outbound webhooks on audit events and manage reusable auth credentials.</p>
          <div class="card-actions justify-end">
            <router-link to="/admin/webhooks" class="btn btn-primary btn-sm">
              Manage Webhooks
            </router-link>
          </div>
        </div>
      </div>

      <!-- System Settings -->
      <div class="card bg-base-100 shadow-xl">
        <div class="card-body">
          <h2 class="card-title">
            <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"/>
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/>
            </svg>
            System Settings
          </h2>
          <p>Configure system-wide settings and preferences.</p>
          <div class="card-actions justify-end">
            <button class="btn btn-primary btn-sm" disabled>
              Coming Soon
            </button>
          </div>
        </div>
      </div>

      <!-- Configuration Management -->
      <div class="card bg-base-100 shadow-xl">
        <div class="card-body">
          <h2 class="card-title">
            <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"/>
            </svg>
            Configuration
          </h2>
          <p>Reload configuration from disk without restarting the server.</p>
          <div class="card-actions justify-end">
            <button
              @click="reloadConfig"
              class="btn btn-secondary btn-sm"
              :disabled="isReloading"
            >
              <span v-if="isReloading" class="loading loading-spinner loading-sm"></span>
              <span v-else>Reload Config</span>
            </button>
          </div>
        </div>
      </div>

      <!-- Wiki Pages Management -->
      <div class="card bg-base-100 shadow-xl">
        <div class="card-body">
          <h2 class="card-title">
            <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253"/>
            </svg>
            Wiki Pages
          </h2>
          <p>Refresh wiki pages from the Git repository.</p>
          <div class="card-actions justify-end">
            <button
              @click="refreshWikiPages"
              class="btn btn-secondary btn-sm"
              :disabled="isRefreshingWiki"
            >
              <span v-if="isRefreshingWiki" class="loading loading-spinner loading-sm"></span>
              <span v-else>Refresh Wiki</span>
            </button>
          </div>
        </div>
      </div>

      <!-- Site Pages Management -->
      <div class="card bg-base-100 shadow-xl">
        <div class="card-body">
          <h2 class="card-title">
            <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 21h10a2 2 0 002-2V9.414a1 1 0 00-.293-.707l-5.414-5.414A1 1 0 0012.586 3H7a2 2 0 00-2 2v14a2 2 0 002 2z"/>
            </svg>
            Site Pages
          </h2>
          <p>Refresh site pages from the Git repository.</p>
          <div class="card-actions justify-end">
            <button
              @click="refreshSitePages"
              class="btn btn-secondary btn-sm"
              :disabled="isRefreshingSite"
            >
              <span v-if="isRefreshingSite" class="loading loading-spinner loading-sm"></span>
              <span v-else>Refresh Site</span>
            </button>
          </div>
        </div>
      </div>

    </div>

    <!-- Success/Error Messages -->
    <div v-if="reloadMessage" class="alert mt-6" :class="reloadSuccess ? 'alert-success' : 'alert-error'">
      <svg v-if="reloadSuccess" class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"/>
      </svg>
      <svg v-else class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/>
      </svg>
      <div>
        <h3 class="font-bold">{{ reloadSuccess ? 'Configuration Reloaded' : 'Reload Failed' }}</h3>
        <div class="text-xs">{{ reloadMessage }}</div>
      </div>
      <button @click="clearMessage" class="btn btn-ghost btn-xs">
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"/>
        </svg>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { UserRole } from '@/types'

const authStore = useAuthStore()

const canAccessAdmin = computed(() => {
  return authStore.isAdmin || authStore.hasRole(UserRole.Admin)
})

// Configuration reload state
const isReloading = ref(false)
const reloadMessage = ref('')
const reloadSuccess = ref(false)

// Pages refresh state
const isRefreshingWiki = ref(false)
const isRefreshingSite = ref(false)

const reloadConfig = async () => {
  if (!canAccessAdmin.value) return

  isReloading.value = true
  reloadMessage.value = ''

  try {
    const token = authStore.token
    if (!token) {
      throw new Error('Authentication token not found')
    }

    const response = await fetch('/api/admin/reload-config', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${token}`
      }
    })

    const result = await response.json()

    if (response.ok && result.success) {
      reloadSuccess.value = true
      reloadMessage.value = result.message || 'Configuration reloaded successfully'

      // Show some config info if available
      if (result.data) {
        const configInfo = []
        if (result.data.site_name) configInfo.push(`Site: ${result.data.site_name}`)
        if (result.data.debug_mode !== undefined) configInfo.push(`Debug: ${result.data.debug_mode ? 'enabled' : 'disabled'}`)
        if (result.data.auth_config?.allow_registration !== undefined) {
          configInfo.push(`Registration: ${result.data.auth_config.allow_registration ? 'enabled' : 'disabled'}`)
        }

        if (configInfo.length > 0) {
          reloadMessage.value += `. ${configInfo.join(', ')}`
        }
      }
    } else {
      reloadSuccess.value = false
      reloadMessage.value = result.error || result.message || 'Failed to reload configuration'
    }
  } catch (error) {
    console.error('Config reload error:', error)
    reloadSuccess.value = false
    reloadMessage.value = error instanceof Error ? error.message : 'Network error occurred'
  } finally {
    isReloading.value = false

    // Auto-clear the message after 5 seconds
    setTimeout(() => {
      clearMessage()
    }, 5000)
  }
}

const clearMessage = () => {
  reloadMessage.value = ''
  reloadSuccess.value = false
}

const refreshWikiPages = async () => {
  if (!canAccessAdmin.value) return

  isRefreshingWiki.value = true
  reloadMessage.value = ''

  try {
    const token = authStore.token
    if (!token) {
      throw new Error('Authentication token not found')
    }

    const response = await fetch('/api/admin/pages/wiki/refresh', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${token}`
      }
    })

    const result = await response.json()

    if (response.ok && result.success) {
      reloadSuccess.value = true
      reloadMessage.value = result.message || 'Wiki pages refreshed successfully'
    } else {
      reloadSuccess.value = false
      reloadMessage.value = result.error || result.message || 'Failed to refresh wiki pages'
    }
  } catch (error) {
    console.error('Wiki refresh error:', error)
    reloadSuccess.value = false
    reloadMessage.value = error instanceof Error ? error.message : 'Network error occurred'
  } finally {
    isRefreshingWiki.value = false

    // Auto-clear the message after 5 seconds
    setTimeout(() => {
      clearMessage()
    }, 5000)
  }
}

const refreshSitePages = async () => {
  if (!canAccessAdmin.value) return

  isRefreshingSite.value = true
  reloadMessage.value = ''

  try {
    const token = authStore.token
    if (!token) {
      throw new Error('Authentication token not found')
    }

    const response = await fetch('/api/admin/pages/site/refresh', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${token}`
      }
    })

    const result = await response.json()

    if (response.ok && result.success) {
      reloadSuccess.value = true
      reloadMessage.value = result.message || 'Site pages refreshed successfully'
    } else {
      reloadSuccess.value = false
      reloadMessage.value = result.error || result.message || 'Failed to refresh site pages'
    }
  } catch (error) {
    console.error('Site refresh error:', error)
    reloadSuccess.value = false
    reloadMessage.value = error instanceof Error ? error.message : 'Network error occurred'
  } finally {
    isRefreshingSite.value = false

    // Auto-clear the message after 5 seconds
    setTimeout(() => {
      clearMessage()
    }, 5000)
  }
}

</script>