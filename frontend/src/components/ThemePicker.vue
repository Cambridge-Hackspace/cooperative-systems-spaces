<template>
  <div class="card bg-base-100 shadow-xl">
    <div class="card-body">
      <h2 class="card-title text-2xl mb-4">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zm0 0h12a2 2 0 002-2v-4a2 2 0 00-2-2h-2.343M11 7.343l1.657-1.657a2 2 0 012.828 0l2.829 2.829a2 2 0 010 2.828l-8.486 8.485M7 17h.01"
          />
        </svg>
        Theme Preference
      </h2>

      <p class="text-base-content/70 mb-4">
        Choose your preferred color theme. Your selection will be applied across the app.
      </p>

      <!-- Success message -->
      <div v-if="successMessage" class="alert alert-success mb-4">
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

      <!-- Error message -->
      <div v-if="errorMessage" class="alert alert-error mb-4">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
          />
        </svg>
        <span>{{ errorMessage }}</span>
      </div>

      <!-- Theme grid -->
      <div class="space-y-8">
        <div v-for="group in themeGroups" :key="group.name" class="space-y-4">
          <!-- Group header -->
          <h3 class="text-lg font-semibold text-base-content/70"></h3>

          <!-- Group themes -->
          <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-4">
            <button
              v-for="theme in group.themes"
              :key="theme.value"
              :disabled="loading"
              :class="[
                'relative p-4 rounded-lg border-2 transition-all duration-200',
                'hover:scale-105 hover:shadow-lg',
                'disabled:opacity-50 disabled:cursor-not-allowed',
                currentTheme === theme.value
                  ? 'border-primary bg-primary/10'
                  : 'border-base-300 hover:border-primary/50',
              ]"
              :data-theme="theme.value"
              @click="selectTheme(theme.value)"
            >
              <!-- Theme preview colors -->
              <div class="flex flex-col gap-2 mb-2">
                <div class="flex gap-1 h-8">
                  <div class="flex-1 rounded bg-primary"></div>
                  <div class="flex-1 rounded bg-secondary"></div>
                  <div class="flex-1 rounded bg-accent"></div>
                </div>
                <div class="h-4 rounded bg-base-content/10"></div>
              </div>

              <!-- Theme name -->
              <div class="text-sm font-medium text-center capitalize">
                {{ theme.label }}
              </div>

              <!-- Selected indicator -->
              <div
                v-if="currentTheme === theme.value"
                class="absolute top-2 right-2 w-6 h-6 bg-primary rounded-full flex items-center justify-center"
              >
                <svg
                  class="w-4 h-4 text-primary-content"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="3"
                    d="M5 13l4 4L19 7"
                  />
                </svg>
              </div>

              <!-- Loading spinner -->
              <div
                v-if="loading && selectedTheme === theme.value"
                class="absolute inset-0 bg-base-100/80 rounded-lg flex items-center justify-center"
              >
                <span class="loading loading-spinner loading-md text-primary"></span>
              </div>
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { apiClient } from '@/utils/api'
import type { User } from '@/types'

interface Props {
  userId?: string
}

const props = withDefaults(defineProps<Props>(), {
  userId: '',
})

const authStore = useAuthStore()
const loading = ref(false)
const successMessage = ref('')
const errorMessage = ref('')
const selectedTheme = ref<string | null>(null)

// Get the user ID (either from props or current user)
const effectiveUserId = computed(() => props.userId || authStore.user?.id || '')

// Get current theme from user meta
const currentTheme = computed(() => {
  if (!authStore.user?.meta) return 'light'
  return (authStore.user.meta as any).theme || 'light'
})

// Themes from tailwind.config.js (must match the config exactly)
const themes = [
  { value: 'css-light', label: 'CSS Light', group: 'CSS' },
  { value: 'css-dark', label: 'CSS Dark', group: 'CSS' },
  { value: 'afterdark', label: 'After Dark', group: 'NEIAM' },
  { value: 'her', label: 'Her', group: 'NEIAM' },
  { value: 'forest', label: 'Forest', group: 'NEIAM' },
  { value: 'sky', label: 'Sky', group: 'NEIAM' },
  { value: 'clays', label: 'Clays', group: 'NEIAM' },
  { value: 'stones', label: 'Stones', group: 'NEIAM' },
  { value: 'lofi', label: 'Lo-Fi', group: 'DAISY' },
  { value: 'black', label: 'Black', group: 'DAISY' },
  { value: 'light', label: 'Light', group: 'DAISY' },
  { value: 'dark', label: 'Dark', group: 'DAISY' },
  { value: 'cupcake', label: 'Cupcake', group: 'DAISY' },
  { value: 'corporate', label: 'Corporate', group: 'DAISY' },
]

// Group themes by their group property
const themeGroups = computed(() => {
  const groups = new Map<string, { name: string; themes: typeof themes }>()

  themes.forEach((theme) => {
    if (!groups.has(theme.group)) {
      groups.set(theme.group, {
        name: theme.group,
        themes: [],
      })
    }
    groups.get(theme.group).themes.push(theme)
  })

  // Return in a specific order: CSS, NEIAM, DAISY
  const order = ['CSS', 'NEIAM', 'DAISY']
  return order
    .filter((groupName) => groups.has(groupName))
    .map((groupName) => groups.get(groupName))
})

async function selectTheme(theme: string) {
  if (loading.value || theme === currentTheme.value) return

  loading.value = true
  selectedTheme.value = theme
  successMessage.value = ''
  errorMessage.value = ''

  try {
    const response = await apiClient.patch<User>(`/users/${effectiveUserId.value}/theme`, {
      theme,
    })

    if (response.success) {
      // Update the user in the auth store with the new data
      if (response.data) {
        authStore.user = response.data
      }

      // Apply the theme immediately
      document.documentElement.setAttribute('data-theme', theme)

      successMessage.value = `Theme changed to ${theme}`

      // Clear success message after 3 seconds
      setTimeout(() => {
        successMessage.value = ''
      }, 3000)
    }
  } catch (error: any) {
    console.error('Failed to update theme:', error)
    errorMessage.value = error.message || 'Failed to update theme. Please try again.'

    // Clear error message after 5 seconds
    setTimeout(() => {
      errorMessage.value = ''
    }, 5000)
  } finally {
    loading.value = false
    selectedTheme.value = null
  }
}

// Apply current theme on mount
onMounted(() => {
  if (currentTheme.value) {
    document.documentElement.setAttribute('data-theme', currentTheme.value)
  }
})
</script>
