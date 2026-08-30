<template>
  <div class="page-viewer bg-base-300">
    <div v-if="loading" class="loading-state">
      <div class="spinner"></div>
      <p>Loading page...</p>
    </div>

    <div v-else-if="error" class="error-state">
      <h2>😕 Page Not Found</h2>
      <p>{{ error }}</p>
      <button class="back-btn" @click="$emit('back')">← Back to List</button>
    </div>

    <article v-else-if="page" class="page-content">
      <header class="page-header">
        <h1>{{ page.title }}</h1>
        <div class="page-meta">
          <span class="page-path bg-secondary text-secondary-content"
            >📄 {{ page.relative_path }}</span
          >
        </div>
      </header>

      <!-- Deliberate and narrow: markdown already rendered to HTML server-side by comrak with Options::default()
       (server/src/pages.rs:398), whose render.unsafe_ is false -- raw HTML in the
       source is escaped before it is ever sent. -->
      <!-- eslint-disable-next-line vue/no-v-html -->
      <div class="page-body markdown-content text-base-content" v-html="page.html_content"></div>

      <!-- Edit link in bottom right -->
      <div v-if="editUrl" class="edit-link-container">
        <a
          :href="editUrl"
          target="_blank"
          rel="noopener noreferrer"
          class="edit-link bg-primary text-primary-content"
        >
          ✏️ Edit on {{ platformName }}
        </a>
      </div>
    </article>

    <div v-else class="empty-state">
      <p>Select a page from the navigation</p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch, computed } from 'vue'

interface Page {
  title: string
  html_content: string
  slug: string
  relative_path: string
  repo_url?: string
  default_branch?: string
}

interface Props {
  type: 'wiki' | 'site'
  slug?: string
}

const props = defineProps<Props>()

defineEmits<{
  (e: 'back'): void
}>()

const page = ref<Page | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)

// Compute the edit URL based on repo_url and relative_path
const editUrl = computed(() => {
  if (!page.value || !page.value.repo_url || !page.value.relative_path) {
    return null
  }

  const repoUrl = page.value.repo_url
  const path = page.value.relative_path
  const branch = page.value.default_branch || 'main'

  // Remove .git suffix if present
  const cleanUrl = repoUrl.replace(/\.git$/, '')

  // GitHub: https://github.com/user/repo/edit/{branch}/path
  if (repoUrl.includes('github.com')) {
    return `${cleanUrl}/edit/${branch}/${path}`
  }

  // GitLab: https://gitlab.com/user/repo/-/edit/{branch}/path
  if (repoUrl.includes('gitlab.com') || repoUrl.includes('gitlab.')) {
    return `${cleanUrl}/-/edit/${branch}/${path}`
  }

  // Gitea: https://gitea.com/user/repo/_edit/{branch}/path
  if (repoUrl.includes('gitea.com') || repoUrl.includes('gitea.')) {
    return `${cleanUrl}/_edit/${branch}/${path}`
  }

  // Forgejo: https://codeberg.org/user/repo/_edit/{branch}/path (uses Gitea-style URLs)
  if (repoUrl.includes('codeberg.org') || repoUrl.includes('forgejo.')) {
    return `${cleanUrl}/_edit/${branch}/${path}`
  }

  // For unknown git hosts, just link to the repo
  return cleanUrl
})

// Compute the platform name for the edit link text
const platformName = computed(() => {
  if (!page.value || !page.value.repo_url) {
    return 'Git'
  }

  const repoUrl = page.value.repo_url

  if (repoUrl.includes('github.com')) return 'GitHub'
  if (repoUrl.includes('gitlab.com') || repoUrl.includes('gitlab.')) return 'GitLab'
  if (repoUrl.includes('gitea.com') || repoUrl.includes('gitea.')) return 'Gitea'
  if (repoUrl.includes('codeberg.org')) return 'Codeberg'
  if (repoUrl.includes('forgejo.')) return 'Forgejo'

  return 'Git'
})

// Watch for slug changes and load the page
watch(
  () => props.slug,
  (newSlug) => {
    if (newSlug) {
      void fetchPage(newSlug)
    } else {
      page.value = null
    }
  },
  { immediate: true }
)

async function fetchPage(slug: string) {
  loading.value = true
  error.value = null
  page.value = null

  try {
    const endpoint = props.type === 'wiki' ? `/api/pages/wiki/${slug}` : `/api/pages/page/${slug}`

    const response = await fetch(endpoint)

    if (!response.ok) {
      if (response.status === 404) {
        throw new Error('Page not found')
      }
      throw new Error(`Failed to fetch page: ${response.statusText}`)
    }

    page.value = await response.json()
  } catch (err) {
    console.error('Error fetching page:', err)
    error.value = err instanceof Error ? err.message : 'Failed to load page'
  } finally {
    loading.value = false
  }
}
</script>

<style scoped>
.page-viewer {
  border-radius: 8px;
  padding: 2rem;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
  min-height: 400px;
}

.loading-state,
.error-state,
.empty-state {
  text-align: center;
  padding: 4rem 2rem;
}

.spinner {
  border: 4px solid #f3f3f3;
  border-top: 4px solid #3788d8;
  border-radius: 50%;
  width: 50px;
  height: 50px;
  animation: spin 1s linear infinite;
  margin: 0 auto 1rem;
}

@keyframes spin {
  0% {
    transform: rotate(0deg);
  }
  100% {
    transform: rotate(360deg);
  }
}

.error-state {
  color: #d9534f;
}

.error-state h2 {
  font-size: 2rem;
  margin-bottom: 1rem;
}

.back-btn {
  margin-top: 1.5rem;
  padding: 0.75rem 1.5rem;
  background: #3788d8;
  color: white;
  border: none;
  border-radius: 6px;
  cursor: pointer;
  font-size: 1rem;
  transition: background 0.2s;
}

.back-btn:hover {
  background: #2a6ab8;
}

.page-content {
  max-width: 900px;
  margin: 0 auto;
}

.page-header {
  margin-bottom: 2rem;
  padding-bottom: 1rem;
  border-bottom: 2px solid #e0e0e0;
}

.page-header h1 {
  margin: 0 0 0.5rem 0;
  font-size: 2.5rem;
  font-weight: 700;
  line-height: 1.2;
}

.page-meta {
  display: flex;
  gap: 1rem;
  font-size: 0.875rem;
}

.page-path {
  font-family: monospace;
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
}

.page-body {
  line-height: 1.7;
}

/* Edit link styling */
.edit-link-container {
  display: flex;
  justify-content: flex-end;
  margin-top: 3rem;
  padding-top: 2rem;
  border-top: 1px solid #e0e0e0;
}

.edit-link {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.5rem 1rem;
  text-decoration: none;
  border-radius: 6px;
  font-size: 0.875rem;
  transition: all 0.2s;
  border: 1px solid #e0e0e0;
}

.edit-link:hover {
  background: #e9ecef;
  color: #333;
  border-color: #ccc;
}

/* Markdown content styling */
.markdown-content :deep(h1),
.markdown-content :deep(h2),
.markdown-content :deep(h3),
.markdown-content :deep(h4),
.markdown-content :deep(h5),
.markdown-content :deep(h6) {
  margin-top: 2rem;
  margin-bottom: 1rem;
  font-weight: 600;
  line-height: 1.3;
}

.markdown-content :deep(h1) {
  font-size: 2rem;
  padding-bottom: 0.5rem;
}

.markdown-content :deep(h2) {
  font-size: 1.75rem;
  padding-bottom: 0.4rem;
}

.markdown-content :deep(h3) {
  font-size: 1.5rem;
}

.markdown-content :deep(h4) {
  font-size: 1.25rem;
}

.markdown-content :deep(p) {
  margin-bottom: 1rem;
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
  padding: 1rem;
  border-radius: 6px;
  overflow-x: auto;
  margin-bottom: 1rem;
  border: 1px solid #e0e0e0;
}

.markdown-content :deep(pre code) {
  background: none;
  padding: 0;
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
  .page-viewer {
    padding: 1.5rem;
  }

  .page-header h1 {
    font-size: 2rem;
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
}

@media (max-width: 640px) {
  .page-viewer {
    padding: 1rem;
  }

  .markdown-content :deep(pre) {
    padding: 0.75rem;
    font-size: 0.8rem;
  }

  .markdown-content :deep(table) {
    font-size: 0.875rem;
  }
}
</style>
