<template>
  <div class="site-view">
    <!--
    <div class="site-header">
      <h1>📄 Site Pages</h1>
      <p class="site-description">Browse site content and documentation</p>
    </div>
    -->

    <div class="site-container">
      <aside class="site-sidebar">
        <PageNavigation
          type="site"
          title="Pages"
          :current-slug="currentSlug"
          base-url="/page"
          @select="handlePageSelect"
        />
      </aside>

      <main class="site-main">
        <PageViewer
          type="site"
          :slug="currentSlug"
          @back="currentSlug = undefined"
        />
      </main>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import PageNavigation from '@/components/PageNavigation.vue'
import PageViewer from '@/components/PageViewer.vue'

const route = useRoute()
const router = useRouter()

const currentSlug = ref<string | undefined>()

onMounted(() => {
  // Load slug from URL params if available
  // Handle both string and array (for nested paths)
  if (route.params.slug) {
    if (Array.isArray(route.params.slug)) {
      currentSlug.value = route.params.slug.join('/')
    } else if (typeof route.params.slug === 'string') {
      currentSlug.value = route.params.slug
    }
  }
})

function handlePageSelect(slug: string) {
  currentSlug.value = slug
  // Update URL without navigation
  router.push({ name: 'site', params: { slug } })
}
</script>

<style scoped>
.site-view {
  max-width: 1400px;
  margin: 0 auto;
  padding: 2rem;
}

.site-header {
  margin-bottom: 2rem;
  text-align: center;
}

.site-header h1 {
  font-size: 2.5rem;
  margin: 0 0 0.5rem 0;
  color: #333;
}

.site-description {
  color: #666;
  font-size: 1.125rem;
  margin: 0;
}

.site-container {
  display: grid;
  grid-template-columns: 300px 1fr;
  gap: 2rem;
  align-items: start;
}

.site-sidebar {
  position: sticky;
  top: 2rem;
  max-height: calc(100vh - 4rem);
  overflow-y: auto;
}

.site-main {
  min-height: 500px;
}

/* Responsive design */
@media (max-width: 1024px) {
  .site-container {
    grid-template-columns: 250px 1fr;
    gap: 1.5rem;
  }
}

@media (max-width: 768px) {
  .site-view {
    padding: 1rem;
  }

  .site-header h1 {
    font-size: 2rem;
  }

  .site-container {
    grid-template-columns: 1fr;
    gap: 1rem;
  }

  .site-sidebar {
    position: static;
    max-height: none;
  }
}
</style>
