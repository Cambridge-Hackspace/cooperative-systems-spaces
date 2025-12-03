<template>
  <div class="wiki-view">
    <!--
    <div class="wiki-header">
      <h1>📚 Wiki</h1>
      <p class="wiki-description">Browse our knowledge base and documentation</p>
    </div>
    -->

    <div class="wiki-container">
      <aside class="wiki-sidebar">
        <PageNavigation
          type="wiki"
          title="Wiki"
          :current-slug="currentSlug"
          base-url="/wiki"
          @select="handlePageSelect"
        />
      </aside>

      <main class="wiki-main">
        <PageViewer
          type="wiki"
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
  router.push({ name: 'wiki', params: { slug } })
}
</script>

<style scoped>
.wiki-view {
  max-width: 1400px;
  margin: 0 auto;
  padding: 2rem;
}

.wiki-header {
  margin-bottom: 2rem;
  text-align: center;
}

.wiki-header h1 {
  font-size: 2.5rem;
  margin: 0 0 0.5rem 0;
  color: #333;
}

.wiki-description {
  color: #666;
  font-size: 1.125rem;
  margin: 0;
}

.wiki-container {
  display: grid;
  grid-template-columns: 300px 1fr;
  gap: 2rem;
  align-items: start;
}

.wiki-sidebar {
  position: sticky;
  top: 2rem;
  max-height: calc(100vh - 4rem);
  overflow-y: auto;
}

.wiki-main {
  min-height: 500px;
}

/* Responsive design */
@media (max-width: 1024px) {
  .wiki-container {
    grid-template-columns: 250px 1fr;
    gap: 1.5rem;
  }
}

@media (max-width: 768px) {
  .wiki-view {
    padding: 1rem;
  }

  .wiki-header h1 {
    font-size: 2rem;
  }

  .wiki-container {
    grid-template-columns: 1fr;
    gap: 1rem;
  }

  .wiki-sidebar {
    position: static;
    max-height: none;
  }
}
</style>
