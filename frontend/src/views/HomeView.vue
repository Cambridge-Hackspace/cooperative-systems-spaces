<template>
  <div class="container mx-auto px-4 py-8">
    <div class="hero min-h-96">
      <div class="hero-content text-center">
        <div class="max-w-md">
          <h1 class="text-5xl font-bold">{{ siteName }}</h1>
          <div class="py-6">
            &nbsp;
          </div>
          <div class="flex gap-4 justify-center">
            <router-link
              v-if="!authStore.isAuthenticated"
              to="/login"
              class="btn btn-primary"
            >
              Get Started
            </router-link>
            <router-link
              v-if="authStore.isAuthenticated"
              to="/profile/me"
              class="btn btn-primary"
            >
              View My Profile
            </router-link>
            <router-link
              v-if="authStore.isAuthenticated"
              to="/tools"
              class="btn btn-secondary"
            >
              Browse Tools
            </router-link>
            <router-link
              v-if="authStore.isAuthenticated && authStore.isAdmin"
              to="/admin"
              class="btn btn-secondary"
            >
              Admin Panel
            </router-link>
          </div>
        </div>
      </div>
    </div>

    <!-- Calendar Events Section -->
    <div class="mt-16">
      <CalendarEvents />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import CalendarEvents from '@/components/CalendarEvents.vue'
import axios from 'axios'

const authStore = useAuthStore()
const siteName = ref('Loading...')

onMounted(async () => {
  try {
    const response = await axios.get('/api/config/public')
    if (response.data?.data?.site?.site_name) {
      siteName.value = response.data.data.site.site_name
    } else {
      siteName.value = 'Cooperative Systems Spaces'
    }
  } catch (error) {
    console.error('Failed to load site configuration:', error)
    siteName.value = 'Cooperative Systems Spaces'
  }
})
</script>
