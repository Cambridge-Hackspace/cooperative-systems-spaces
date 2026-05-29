<template>
  <div class="container mx-auto px-4 py-8">
    <div class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/" class="link">Home</router-link></li>
        <li><router-link to="/admin" class="link">Admin</router-link></li>
        <li>Facility</li>
      </ul>
    </div>

    <div class="mb-6">
      <h1 class="text-3xl font-bold mb-1">Facility</h1>
      <p class="text-base-content/70">
        Define the physical layout of your space and the doors that connect it.
      </p>
    </div>

    <div role="tablist" class="tabs tabs-boxed w-fit mb-6">
      <a
        role="tab"
        class="tab"
        :class="{ 'tab-active': tab === 'places' }"
        @click="setTab('places')"
      >
        Places
      </a>
      <a
        role="tab"
        class="tab"
        :class="{ 'tab-active': tab === 'doors' }"
        @click="setTab('doors')"
      >
        Doors
      </a>
      <a
        role="tab"
        class="tab"
        :class="{ 'tab-active': tab === 'schedules' }"
        @click="setTab('schedules')"
      >
        Schedules
      </a>
      <a
        role="tab"
        class="tab"
        :class="{ 'tab-active': tab === 'graph' }"
        @click="setTab('graph')"
      >
        Graph
      </a>
    </div>

    <KeepAlive>
      <PlaceManagement v-if="tab === 'places'" embedded />
      <DoorManagement v-else-if="tab === 'doors'" embedded />
      <ScheduleManagement v-else-if="tab === 'schedules'" embedded />
      <FacilityGraph v-else />
    </KeepAlive>
  </div>
</template>

<script setup lang="ts">
import { defineAsyncComponent, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import PlaceManagement from './PlaceManagement.vue'
import DoorManagement from './DoorManagement.vue'
import ScheduleManagement from './ScheduleManagement.vue'
// Lazy-load the graph view — cytoscape is ~150 KB gzipped and only matters
// when the user actually opens the Graph tab.
const FacilityGraph = defineAsyncComponent(() => import('./FacilityGraph.vue'))

type Tab = 'places' | 'doors' | 'schedules' | 'graph'

const route = useRoute()
const router = useRouter()

/** Tab is reflected in the URL via `?tab=places|doors` so deep-links and
    refresh land you back on the right view. */
const tab = ref<Tab>(((route.query.tab as Tab) || 'places') as Tab)

function setTab(next: Tab) {
  if (tab.value === next) return
  tab.value = next
  router.replace({ query: { ...route.query, tab: next } })
}

// React to a manual URL change too (back/forward, paste).
watch(
  () => route.query.tab,
  (v) => {
    const next = (v as Tab) || 'places'
    if (next !== tab.value) tab.value = next
  },
)
</script>
