<template>
  <div class="nav-item-container">
    <!-- A folder with no page of its own is not a link. Rendering it as one
         gives the reader an anchor to `/wiki/` that 404s; it is a header that
         opens and closes, and it says so by being a button. -->
    <button
      v-if="isGroup"
      type="button"
      class="nav-item nav-group active:bg-primary hover:bg-secondary"
      :class="{ 'nav-item-child': depth > 0 }"
      :aria-expanded="isExpanded"
      @click="$emit('toggle', nodeKey)"
    >
      <span class="nav-title">{{ item.title }}</span>
      <span v-if="hasChildren" class="nav-arrow">{{ isExpanded ? '▼' : '▶' }}</span>
    </button>

    <a
      v-else
      :href="`${baseUrl}/${item.slug}`"
      class="nav-item active:bg-primary hover:bg-secondary"
      :class="{ active: currentSlug === item.slug, 'nav-item-child': depth > 0 }"
      @click.prevent="$emit('select', item.slug)"
    >
      <span class="nav-title">{{ item.title }}</span>
      <span v-if="hasChildren" class="nav-arrow">{{ isExpanded ? '▼' : '▶' }}</span>
    </a>

    <!-- A page that also has children needs both actions: the row navigates,
         this opens. A group has no second action, so it has no second button. -->
    <button
      v-if="hasChildren && !isGroup"
      class="expand-button"
      :aria-label="isExpanded ? 'Collapse' : 'Expand'"
      @click="$emit('toggle', nodeKey)"
    ></button>

    <div v-if="hasChildren && isExpanded" class="nav-children">
      <!-- Recursion, because the wiki nests three deep and the previous
           two-level template could not show the third. -->
      <PageNavItem
        v-for="child in item.children"
        :key="keyOf(child)"
        :item="child"
        :base-url="baseUrl"
        :current-slug="currentSlug"
        :expanded="expanded"
        :depth="depth + 1"
        @select="$emit('select', $event)"
        @toggle="$emit('toggle', $event)"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import type { NavItem } from '@/types/nav'
import { keyOf } from '@/types/nav'

interface Props {
  item: NavItem
  baseUrl: string
  currentSlug?: string
  expanded: Set<string>
  depth?: number
}

const props = withDefaults(defineProps<Props>(), {
  currentSlug: undefined,
  depth: 0,
})

defineEmits<{
  (e: 'select', slug: string): void
  (e: 'toggle', key: string): void
}>()

const isGroup = computed(() => !props.item.slug)
const hasChildren = computed(() => (props.item.children?.length ?? 0) > 0)
const nodeKey = computed(() => keyOf(props.item))
const isExpanded = computed(() => props.expanded.has(nodeKey.value))
</script>

<style scoped>
/* Layout lives in PageNavigation's non-scoped rules; only what is new to a
   group header is here. */
.nav-group {
  width: 100%;
  background: transparent;
  border: none;
  font: inherit;
  text-align: left;
  cursor: pointer;
}
</style>
