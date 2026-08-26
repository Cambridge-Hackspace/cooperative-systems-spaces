<template>
  <div class="container mx-auto px-4 py-8">
    <div class="hero">
      <div class="hero-content text-center">
        <div class="max-w-5xl">
          <h1 class="text-5xl font-bold">{{ siteName }}</h1>
          <div class="py-6">&nbsp;</div>
          <div class="flex gap-4 justify-center flex-wrap">
            <router-link v-if="!authStore.isAuthenticated" to="/login" class="btn btn-primary">
              Get Started
            </router-link>
            <router-link
              v-if="authStore.isAuthenticated && homepageLinks.view_my_profile"
              to="/profile/me"
              class="btn btn-primary"
            >
              View My Profile
            </router-link>
            <router-link
              v-if="authStore.isAuthenticated && homepageLinks.browse_tools"
              to="/tools"
              class="btn btn-secondary"
            >
              Browse Tools
            </router-link>
            <router-link
              v-if="authStore.isAuthenticated && authStore.isAdmin && homepageLinks.admin_panel"
              to="/admin"
              class="btn btn-secondary"
            >
              Admin Panel
            </router-link>
            <router-link
              v-if="pagesConfig.wiki_enabled && shouldShowWikiLink && homepageLinks.wiki"
              to="/wiki"
              class="btn btn-accent"
            >
              📚 Wiki
            </router-link>
          </div>
        </div>
      </div>
    </div>

    <!-- Hours today + Links — shown side by side when both have content. -->
    <section
      v-if="publicSchedules.length || homeLinks.length"
      class="mt-16 grid gap-8 max-w-5xl mx-auto md:grid-cols-2"
    >
      <!-- Hours today -->
      <div v-if="publicSchedules.length">
        <h2 class="text-2xl font-semibold mb-2">Hours today</h2>
        <p class="text-sm text-base-content/70 mb-4">
          Posted hours for {{ todayLabel }}. Times are local to the space.
        </p>
        <div class="grid gap-3">
          <div v-for="row in scheduleRowsToday" :key="row.id" class="card bg-base-200 shadow-sm">
            <div class="card-body py-4">
              <div class="flex items-center justify-between">
                <h3 class="font-semibold">{{ row.name }}</h3>
                <span class="badge badge-sm" :class="row.openNow ? 'badge-success' : 'badge-ghost'">
                  {{ row.openNow ? 'Open now' : 'Closed' }}
                </span>
              </div>
              <p v-if="row.description" class="text-xs text-base-content/70">
                {{ row.description }}
              </p>
              <div v-if="row.windows.length" class="font-mono text-sm">
                {{ row.windows.map((w) => `${w.start}–${w.end}`).join(', ') }}
              </div>
              <div v-else class="text-sm text-base-content/60 italic">Closed today</div>
            </div>
          </div>
        </div>
      </div>

      <!-- Links (audience-gated by the server). -->
      <div v-if="homeLinks.length">
        <h2 class="text-2xl font-semibold mb-2">Links</h2>
        <p class="text-sm text-base-content/70 mb-4">Useful destinations curated by your admins.</p>
        <div class="grid gap-3">
          <a
            v-for="l in homeLinks"
            :key="l.id"
            :href="l.url"
            target="_blank"
            rel="noopener"
            class="card bg-base-200 shadow-sm hover:bg-base-300 transition-colors"
          >
            <div class="card-body py-4">
              <div class="flex items-start justify-between gap-2">
                <h3 class="font-semibold">
                  <span v-if="l.icon" class="mr-1">{{ l.icon }}</span
                  >{{ l.label }}
                </h3>
                <span
                  v-if="l.audience !== 'everyone'"
                  class="badge badge-sm whitespace-nowrap"
                  :class="audienceBadgeClass(l.audience)"
                  :title="audienceTooltip(l.audience)"
                >
                  {{ audienceLabel(l.audience) }}
                </span>
              </div>
              <p v-if="l.description" class="text-xs text-base-content/70">{{ l.description }}</p>
            </div>
          </a>
        </div>
      </div>
    </section>

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
import { schedulesApi, homeLinksApi } from '@/utils/api'
import type { DayOfWeek, HomeLink, HomeLinkAudience, Schedule } from '@/types'
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

interface HomepageLinksFlags {
  view_my_profile: boolean
  browse_tools: boolean
  admin_panel: boolean
  wiki: boolean
}

// Defaults match the server's `HomepageLinksConfig::default()` — all true,
// so behavior matches pre-config-flag installs while the public config
// payload is in flight (or on older servers that don't surface the field).
const homepageLinks = ref<HomepageLinksFlags>({
  view_my_profile: true,
  browse_tools: true,
  admin_panel: true,
  wiki: true,
})

const shouldShowWikiLink = computed(() => {
  return pagesConfig.value.wiki_link === 'HomePage' || pagesConfig.value.wiki_link === 'Both'
})

/** Public schedules drive the "Hours today" panel. */
const publicSchedules = ref<Schedule[]>([])

const dowMap: Record<number, DayOfWeek> = {
  0: 'sun',
  1: 'mon',
  2: 'tue',
  3: 'wed',
  4: 'thu',
  5: 'fri',
  6: 'sat',
}
const dowLabel: Record<DayOfWeek, string> = {
  mon: 'Monday',
  tue: 'Tuesday',
  wed: 'Wednesday',
  thu: 'Thursday',
  fri: 'Friday',
  sat: 'Saturday',
  sun: 'Sunday',
}
const today: DayOfWeek = dowMap[new Date().getDay()]
const todayLabel = dowLabel[today]

const scheduleRowsToday = computed(() =>
  publicSchedules.value.map((s) => {
    const windows = s.intervals
      .filter((iv) => iv.day === today)
      .sort((a, b) => a.start.localeCompare(b.start))
    return {
      id: s.id,
      name: s.name,
      description: s.description,
      windows,
      openNow: windows.some((w) => isNowBetween(w.start, w.end)),
    }
  })
)

function isNowBetween(start: string, end: string): boolean {
  const d = new Date()
  const nowMin = d.getHours() * 60 + d.getMinutes()
  return toMin(start) <= nowMin && nowMin < toMin(end)
}
function toMin(hhmm: string): number {
  const [h, m] = hhmm.split(':').map((n) => parseInt(n, 10))
  return (h || 0) * 60 + (m || 0)
}

async function loadPublicSchedules() {
  try {
    const r = await schedulesApi.publicList()
    if (r.success && r.data) publicSchedules.value = r.data
  } catch {
    publicSchedules.value = []
  }
}

/** Admin-curated home links; server filters by audience using any bearer token. */
const homeLinks = ref<HomeLink[]>([])

async function loadHomeLinks() {
  try {
    const r = await homeLinksApi.publicList()
    if (r.success && r.data) homeLinks.value = r.data
  } catch {
    homeLinks.value = []
  }
}

function audienceLabel(a: HomeLinkAudience): string {
  return {
    everyone: 'Everyone',
    anonymous: 'Anonymous',
    logged_in: 'Members & guests',
    member: 'Members+',
    staff: 'Staff+',
  }[a]
}
function audienceBadgeClass(a: HomeLinkAudience): string {
  return {
    everyone: 'badge-ghost',
    anonymous: 'badge-warning',
    logged_in: 'badge-info',
    member: 'badge-success',
    staff: 'badge-secondary',
  }[a]
}
function audienceTooltip(a: HomeLinkAudience): string {
  return {
    everyone: 'Visible to everyone',
    anonymous: 'Visible only to signed-out visitors',
    logged_in: 'Visible to any authenticated user',
    member: 'Visible to Members or higher',
    staff: 'Visible to Staff or higher',
  }[a]
}

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

    // Homepage button toggles. Missing fields default to true (preserves
    // pre-flag behavior).
    const hl = response.data?.data?.site?.homepage_links
    if (hl) {
      homepageLinks.value = {
        view_my_profile: hl.view_my_profile !== false,
        browse_tools: hl.browse_tools !== false,
        admin_panel: hl.admin_panel !== false,
        wiki: hl.wiki !== false,
      }
    }
  } catch (error) {
    console.error('Failed to load site configuration:', error)
    siteName.value = 'Cooperative Systems Spaces'
  }
  // Awaited together rather than fired and forgotten: neither handles its own
  // errors, so an unawaited rejection here was an unhandled promise rejection
  // and the section it feeds simply stayed empty with no indication why.
  await Promise.allSettled([loadPublicSchedules(), loadHomeLinks()])
})
</script>
