<template>
  <div class="page-navigation bg-base-300">
    <div class="nav-header">
      <h3>{{ title }}</h3>
    </div>

    <div v-if="loading" class="loading-state">
      <div class="spinner"></div>
      <p>Loading navigation...</p>
    </div>

    <div v-else-if="error" class="error-state">
      <p>{{ error }}</p>
    </div>

    <nav v-else class="nav-list">
      <div v-if="navItems.length === 0" class="empty-state">
        <p>No pages available</p>
      </div>
      <div v-else>
        <div
          v-for="item in navItems"
          :key="item.slug"
          class="nav-item-container"
        >
          <a
            :href="`${baseUrl}/${item.slug}`"
            @click.prevent="$emit('select', item.slug)"
            class="nav-item active:bg-primary hover:bg-secondary"
            :class="{ active: currentSlug === item.slug }"
          >
            <span class="nav-title">{{ item.title }}</span>
            <span v-if="item.children && item.children.length > 0" class="nav-arrow">
              {{ expandedItems.has(item.slug) ? '▼' : '▶' }}
            </span>
          </a>
          
          <!-- Toggle button for items with children -->
          <button
            v-if="item.children && item.children.length > 0"
            @click="toggleExpanded(item.slug)"
            class="expand-button"
            :aria-label="expandedItems.has(item.slug) ? 'Collapse' : 'Expand'"
          >
          </button>
          
          <!-- Child items -->
          <div v-if="item.children && item.children.length > 0 && expandedItems.has(item.slug)" class="nav-children">
            <a
              v-for="child in item.children"
              :key="child.slug"
              :href="`${baseUrl}/${child.slug}`"
              @click.prevent="$emit('select', child.slug)"
              class="nav-item nav-item-child"
              :class="{ active: currentSlug === child.slug }"
            >
              <span class="nav-title">{{ child.title }}</span>
            </a>
          </div>
        </div>
      </div>
    </nav>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'

interface NavItem {
  title: string
  slug: string
  path: string
  children: NavItem[]
}

interface Props {
  type: 'wiki' | 'site'
  title?: string
  currentSlug?: string
  baseUrl?: string
}

const props = withDefaults(defineProps<Props>(), {
  title: 'Pages',
  baseUrl: '/pages'
})

const emit = defineEmits<{
  (e: 'select', slug: string): void
}>()

const navItems = ref<NavItem[]>([])
const loading = ref(true)
const error = ref<string | null>(null)
const expandedItems = ref<Set<string>>(new Set())

onMounted(() => {
  fetchNavigation()
})

async function fetchNavigation() {
  loading.value = true
  error.value = null

  try {
    const response = await fetch('/api/pages/navigation')
    
    if (!response.ok) {
      throw new Error(`Failed to fetch navigation: ${response.statusText}`)
    }

    const data = await response.json()
    navItems.value = props.type === 'wiki' ? data.wiki_nav : data.site_nav
  } catch (err) {
    console.error('Error fetching navigation:', err)
    error.value = err instanceof Error ? err.message : 'Failed to load navigation'
  } finally {
    loading.value = false
  }
}

function toggleExpanded(slug: string) {
  if (expandedItems.value.has(slug)) {
    expandedItems.value.delete(slug)
  } else {
    expandedItems.value.add(slug)
  }
  // Trigger reactivity
  expandedItems.value = new Set(expandedItems.value)
}
</script>

<style scoped>
.page-navigation {
  border-radius: 8px;
  padding: 1rem;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
  height: 100%;
}

.nav-header {
  margin-bottom: 1rem;
  padding-bottom: 0.75rem;
  border-bottom: 2px solid #e0e0e0;
}

.nav-header h3 {
  margin: 0;
  font-size: 1.25rem;
  font-weight: 600;
}

.loading-state,
.error-state,
.empty-state {
  text-align: center;
  padding: 2rem 1rem;
}

.spinner {
  border: 3px solid #f3f3f3;
  border-top: 3px solid #3788d8;
  border-radius: 50%;
  width: 30px;
  height: 30px;
  animation: spin 1s linear infinite;
  margin: 0 auto 1rem;
}

@keyframes spin {
  0% { transform: rotate(0deg); }
  100% { transform: rotate(360deg); }
}

.error-state {
  color: #d9534f;
}

.nav-list {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.nav-item-container {
  position: relative;
}

.nav-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.75rem 1rem;
  text-decoration: none;
  border-radius: 6px;
  transition: all 0.2s;
  cursor: pointer;
}

.nav-item:hover {
  transform: translateX(4px);
}

.nav-item.active {
  font-weight: 600;
  border-left: 3px solid #3788d8;
}

.nav-title {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.nav-arrow {
  font-size: 0.875rem;
  opacity: 0.6;
  margin-left: 0.5rem;
  transition: transform 0.2s;
}

.expand-button {
  position: absolute;
  right: 0;
  top: 0;
  bottom: 0;
  width: 40px;
  background: transparent;
  border: none;
  cursor: pointer;
  opacity: 0;
  z-index: 1;
}

.nav-item-container:hover .expand-button {
  opacity: 0.3;
}

.nav-children {
  padding-left: 1.5rem;
  margin-top: 0.25rem;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.nav-item-child {
  padding: 0.6rem 0.75rem;
  font-size: 0.9375rem;
  border-left: 2px solid #e0e0e0;
  margin-left: 0.5rem;
}

.nav-item-child:hover {
  border-left-color: #3788d8;
}

.nav-item-child.active {
  border-left-color: #3788d8;
}

/* Responsive design */
@media (max-width: 768px) {
  .page-navigation {
    padding: 0.75rem;
  }

  .nav-item {
    padding: 0.6rem 0.75rem;
  }
}
</style>
