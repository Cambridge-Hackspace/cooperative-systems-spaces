<template>
  <div class="container mx-auto px-4 py-8">
    <div class="hero min-h-96">
      <div class="hero-content text-center">
        <div class="max-w-md">
          <h1 class="text-5xl font-bold">{{ siteName }}</h1>
          <div class="py-6">
            &nbsp;
          </div>
          <div class="flex gap-4 justify-center flex-wrap">
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
            <router-link
              v-if="pagesConfig.wiki_enabled && shouldShowWikiLink"
              to="/wiki"
              class="btn btn-accent"
            >
              📚 Wiki
            </router-link>
          </div>
        </div>
      </div>
    </div>

    <!-- Site Index Content -->
    <div v-if="pagesConfig.site_enabled" class="mt-16">
      <SiteIndexContent />
    </div>

    <!-- Calendar Events Section -->
    <div class="mt-16">
      <CalendarEvents />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import CalendarEvents from '@/components/CalendarEvents.vue'
import SiteIndexContent from '@/components/SiteIndexContent.vue'
import axios from 'axios'

const authStore = useAuthStore()
const siteName = ref('Loading...')

interface PagesConfig {
  wiki_enabled: boolean
  wiki_link: 'Navigation' | 'HomePage' | 'Both'
  site_enabled: boolean
  site_link: 'Navigation' | 'HomePage' | 'Both'
}

const pagesConfig = ref<PagesConfig>({
  wiki_enabled: false,
  wiki_link: 'Navigation',
  site_enabled: false,
  site_link: 'Navigation',
})

const shouldShowWikiLink = computed(() => {
  return pagesConfig.value.wiki_link === 'HomePage' || pagesConfig.value.wiki_link === 'Both'
})

onMounted(async () => {
  try {
    const response = await axios.get('/api/config/public')
    if (response.data?.data?.site?.site_name) {
      siteName.value = response.data.data.site.site_name
    } else {
      siteName.value = 'Cooperative Systems Spaces'
    }

    // Load pages configuration
    if (response.data?.data?.pages) {
      pagesConfig.value = response.data.data.pages
    }
  } catch (error) {
    console.error('Failed to load site configuration:', error)
    siteName.value = 'Cooperative Systems Spaces'
  }
})
</script>
