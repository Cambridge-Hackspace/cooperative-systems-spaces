<template>
  <div v-if="content" class="site-index-content bg-base-300 text-base-content">
    <div class="markdown-content" v-html="content.html_content"></div>
  </div>
  <div v-else-if="loading" class="loading-state">
    <div class="spinner"></div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'

interface SiteIndex {
  title: string
  html_content: string
  slug: string
  relative_path: string
}

const content = ref<SiteIndex | null>(null)
const loading = ref(false)

onMounted(() => {
  fetchSiteIndex()
})

async function fetchSiteIndex() {
  loading.value = true

  try {
    const response = await fetch('/api/pages/page/index')
    
    if (response.ok) {
      content.value = await response.json()
    }
  } catch (err) {
    console.error('Error fetching site index:', err)
  } finally {
    loading.value = false
  }
}
</script>

<style scoped>
.site-index-content {
  border-radius: 8px;
  padding: 2rem;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
}

.loading-state {
  text-align: center;
  padding: 2rem;
}

.spinner {
  border: 3px solid #f3f3f3;
  border-top: 3px solid #3788d8;
  border-radius: 50%;
  width: 40px;
  height: 40px;
  animation: spin 1s linear infinite;
  margin: 0 auto;
}

@keyframes spin {
  0% { transform: rotate(0deg); }
  100% { transform: rotate(360deg); }
}

/* Markdown content styling */
.markdown-content :deep(h1),
.markdown-content :deep(h2),
.markdown-content :deep(h3),
.markdown-content :deep(h4),
.markdown-content :deep(h5),
.markdown-content :deep(h6) {
  margin-top: 1.5rem;
  margin-bottom: 0.75rem;
  font-weight: 600;
  line-height: 1.3;
}

.markdown-content :deep(h1) {
  font-size: 1.75rem;
}

.markdown-content :deep(h2) {
  font-size: 1.5rem;
}

.markdown-content :deep(h3) {
  font-size: 1.25rem;
}

.markdown-content :deep(p) {
  margin-bottom: 1rem;
  line-height: 1.7;
}

.markdown-content :deep(a) {
  border-bottom: 1px solid;
  transition: border-color 0.2s;
}

.markdown-content :deep(a:hover) {
  border-bottom-color: #3788d8;
}

.markdown-content :deep(ul),
.markdown-content :deep(ol) {
  margin-bottom: 1rem;
  padding-left: 2rem;
}

.markdown-content :deep(li) {
  margin-bottom: 0.5rem;
}

.markdown-content :deep(code) {
  background: #f5f5f5;
  padding: 0.2rem 0.4rem;
  border-radius: 3px;
  font-family: 'Courier New', Courier, monospace;
  font-size: 0.9em;
  color: #d63384;
}

.markdown-content :deep(pre) {
  background: #f8f9fa;
  padding: 1rem;
  border-radius: 6px;
  overflow-x: auto;
  margin-bottom: 1rem;
  border: 1px solid #e0e0e0;
}

.markdown-content :deep(pre code) {
  background: none;
  padding: 0;
  color: #333;
  font-size: 0.875rem;
}

.markdown-content :deep(blockquote) {
  border-left: 4px solid #3788d8;
  padding-left: 1rem;
  margin: 1rem 0;
  color: #666;
  font-style: italic;
}

.markdown-content :deep(img) {
  max-width: 100%;
  height: auto;
  border-radius: 6px;
  margin: 1rem 0;
}

.markdown-content :deep(table) {
  width: 100%;
  border-collapse: collapse;
  margin: 1rem 0;
}

.markdown-content :deep(table th),
.markdown-content :deep(table td) {
  border: 1px solid #e0e0e0;
  padding: 0.75rem;
  text-align: left;
}

.markdown-content :deep(table th) {
  background: #f8f9fa;
  font-weight: 600;
}

.markdown-content :deep(table tr:hover) {
  background: #f8f9fa;
}

.markdown-content :deep(hr) {
  border: none;
  border-top: 2px solid #e0e0e0;
  margin: 2rem 0;
}

/* Responsive design */
@media (max-width: 768px) {
  .site-index-content {
    padding: 1.5rem;
  }

  .markdown-content :deep(h1) {
    font-size: 1.5rem;
  }

  .markdown-content :deep(h2) {
    font-size: 1.25rem;
  }
}
</style>
