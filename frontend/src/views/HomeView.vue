<template>
  <div class="container mx-auto px-4 py-8">
    <!-- Announcement banners -->
    <div class="alert mb-4 text-center justify-center" style="background-color:#FFD966; color:#3a2f00;">
      <span class="text-lg font-semibold">
        We are operating and accepting new members!<br />
        Tuesday evening project nights are
        <a href="https://www.meetup.com/Cambridge-Hackspace/" target="_blank" rel="noopener" class="underline">in person</a>.
      </span>
    </div>
    <div class="alert alert-info mb-8 text-center justify-center">
      <span>
        Want to use our new Laser Cutter + CNCs and 3D Printers?
        <router-link to="/register" class="underline font-semibold">Checkout our memberships</router-link>,
        and see what we have to offer!
      </span>
    </div>

    <div class="hero">
      <div class="hero-content text-center">
        <div class="max-w-5xl">
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

    <!-- Title bar: what / why / who / where -->
    <div class="my-10">
      <div class="flex items-center gap-4 max-w-3xl mx-auto mb-10">
        <div class="flex-1 border-t-2 border-dotted border-base-content/40"></div>
        <div class="text-3xl md:text-4xl font-bold text-center px-2">Cambridge Hackspace</div>
        <div class="flex-1 border-t-2 border-dotted border-base-content/40"></div>
      </div>

      <div class="grid gap-8 md:grid-cols-4 text-center max-w-6xl mx-auto">
        <div>
          <div class="w-20 h-20 rounded-full mx-auto mb-4 flex items-center justify-center" style="background-color:#c1544f;">
            <svg class="w-9 h-9 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 3l7.07 16.97 2.51-7.39 7.39-2.51L3 3z"/>
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 11l6 6"/>
            </svg>
          </div>
          <h3 class="text-xl font-bold mb-2">what</h3>
          <p class="text-sm text-base-content/70">
            We are a 501(c)3 makerspace in the center of Somerville, MA. We have a 1200sqft workshop catering to enthusiastic makers,
            and hold weekly meetings where we get together and make things.
            We have a Laser cutter, CNCs, 3D printers, and a range of other tools for our members to use.
          </p>
        </div>

        <div>
          <div class="w-20 h-20 rounded-full mx-auto mb-4 flex items-center justify-center" style="background-color:#3f83a8;">
            <svg class="w-9 h-9 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 9l-4 3 4 3m8-6l4 3-4 3"/>
            </svg>
          </div>
          <h3 class="text-xl font-bold mb-2">why</h3>
          <p class="text-sm text-base-content/70">
            We've started the Cambridge Hackspace to provide a physical space where people can gather and work on their projects,
            have access to larger or expensive tools (like the 3D printers, and laser cutter),
            and provide a place where the community can share their knowledge.
          </p>
        </div>

        <div>
          <div class="w-20 h-20 rounded-full mx-auto mb-4 flex items-center justify-center" style="background-color:#e0a010;">
            <svg class="w-9 h-9 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4.354a4 4 0 110 5.292M15 21H3v-1a6 6 0 0112 0v1zm0 0h6v-1a6 6 0 00-9-5.197m13.5-9a2.5 2.5 0 11-5 0 2.5 2.5 0 015 0z"/>
            </svg>
          </div>
          <h3 class="text-xl font-bold mb-2">who</h3>
          <p class="text-sm text-base-content/70">
            Anyone and everyone is welcome. Whether your thing is software, electronics, woodwork, or knitting,
            we provide a space where you can meet fellow makers, to learn, and be inspired.
            We hold workshops every month so you can learn new skills.
          </p>
        </div>

        <div>
          <div class="w-20 h-20 rounded-full mx-auto mb-4 flex items-center justify-center" style="background-color:#7cb342;">
            <svg class="w-9 h-9 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17.657 16.657L13.414 20.9a1.998 1.998 0 01-2.827 0l-4.244-4.243a8 8 0 1111.314 0z"/>
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 11a3 3 0 11-6 0 3 3 0 016 0z"/>
            </svg>
          </div>
          <h3 class="text-xl font-bold mb-2">where</h3>
          <p class="text-sm text-base-content/70">
            We are located in Union Square, Somerville
            (<router-link to="/about" class="underline">address</router-link>).
            We hold events every week, our next meeting is on Tuesday at 6.30pm — check our events page for future events.
            You can also like our
            <a href="https://www.facebook.com/CambridgeHackspace" target="_blank" rel="noopener" class="underline">Facebook page</a>
            and follow us on Twitter
            (<a href="https://twitter.com/hackcambridge" target="_blank" rel="noopener" class="underline">@HackCambridge</a>).
          </p>
        </div>
      </div>
    </div>

    <div class="border-t-2 border-dotted border-base-content/30 max-w-3xl mx-auto my-10"></div>

    <!-- Join / Next event / Donate + photo grid -->
    <div class="grid gap-10 md:grid-cols-2 max-w-6xl mx-auto mb-16">
      <div class="text-center md:text-left space-y-8">
        <div>
          <template v-if="authStore.isAuthenticated">
            <h3 class="text-2xl font-bold mb-3">Welcome back!</h3>
            <router-link to="/profile/me" class="btn btn-primary btn-lg">Visit your dashboard</router-link>
          </template>
          <template v-else>
            <h2 class="text-3xl font-bold mb-3">Join Cambridge Hackspace</h2>
            <p class="text-base-content/70 mb-4">
              Become a Cambridge Hackspace member, join us and help support our efforts to get bigger and better.
            </p>
            <router-link to="/register" class="btn btn-primary btn-lg">Become a Member</router-link>
          </template>
        </div>

        <div class="border-t-2 border-dotted border-base-content/30"></div>

        <div>
          <h3 class="text-2xl font-bold mb-1">Next Event</h3>
          <h4 class="text-lg">Hackspace Project Night - Open to Everyone!</h4>
          <p class="text-base-content/70 mb-3">Tuesday @ 6.30pm</p>
          <a href="https://www.meetup.com/Cambridge-Hackspace/events/" target="_blank" rel="noopener" class="btn btn-info btn-lg">
            Meetup Events
          </a>
        </div>

        <div class="border-t-2 border-dotted border-base-content/30"></div>

        <div>
          <h3 class="text-2xl font-bold mb-1">Donate</h3>
          <p class="text-base-content/70">Support our mission of building a community that shares knowledge about making things.</p>
          <p class="text-base-content/70 mb-3">The donate button goes to our payment processor, Stripe, to keep your payment secure.</p>
          <a href="https://donate.stripe.com/3csg1z40seWLbcccMM" target="_blank" rel="noopener" class="btn btn-info btn-lg">
            Donate
          </a>
        </div>
      </div>

      <div class="grid grid-cols-3 gap-2 content-start">
        <a href="https://c1.staticflickr.com/9/8805/27578742453_492441e76a_b.jpg" target="_blank" rel="noopener">
          <img src="https://c1.staticflickr.com/9/8805/27578742453_492441e76a_q.jpg" class="rounded w-full" />
        </a>
        <a href="https://c2.staticflickr.com/8/7569/27578790623_a7e34a5c2f_b.jpg" target="_blank" rel="noopener">
          <img src="https://c2.staticflickr.com/8/7569/27578790623_a7e34a5c2f_q.jpg" class="rounded w-full" />
        </a>
        <a href="https://c1.staticflickr.com/9/8893/27913231840_c7eef84f2d_b.jpg" target="_blank" rel="noopener">
          <img src="https://c1.staticflickr.com/9/8893/27913231840_c7eef84f2d_q.jpg" class="rounded w-full" />
        </a>
        <a href="https://c1.staticflickr.com/9/8844/28194194325_02fbcaa297_b.jpg" target="_blank" rel="noopener">
          <img src="https://c1.staticflickr.com/9/8844/28194194325_02fbcaa297_q.jpg" class="rounded w-full" />
        </a>
        <a href="https://c1.staticflickr.com/9/8562/28116304091_850a86a4d7_b.jpg" target="_blank" rel="noopener">
          <img src="https://c1.staticflickr.com/9/8562/28116304091_850a86a4d7_q.jpg" class="rounded w-full" />
        </a>
        <a href="https://c1.staticflickr.com/1/766/22779288984_5cf810bd5e_b.jpg" target="_blank" rel="noopener">
          <img src="https://c1.staticflickr.com/1/766/22779288984_5cf810bd5e_q.jpg" class="rounded w-full" />
        </a>
        <a href="https://c1.staticflickr.com/9/8837/27578445104_8a301fa3b8_b.jpg" target="_blank" rel="noopener">
          <img src="https://c1.staticflickr.com/9/8837/27578445104_8a301fa3b8_q.jpg" class="rounded w-full" />
        </a>
        <a href="https://c2.staticflickr.com/8/7287/28194207805_79a8375c63_b.jpg" target="_blank" rel="noopener">
          <img src="https://c2.staticflickr.com/8/7287/28194207805_79a8375c63_q.jpg" class="rounded w-full" />
        </a>
        <a href="https://c2.staticflickr.com/6/5640/23111885530_e73ec6fd5c_b.jpg" target="_blank" rel="noopener">
          <img src="https://c2.staticflickr.com/6/5640/23111885530_e73ec6fd5c_q.jpg" class="rounded w-full" />
        </a>
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
          <div
            v-for="row in scheduleRowsToday"
            :key="row.id"
            class="card bg-base-200 shadow-sm"
          >
            <div class="card-body py-4">
              <div class="flex items-center justify-between">
                <h3 class="font-semibold">{{ row.name }}</h3>
                <span
                  class="badge badge-sm"
                  :class="row.openNow ? 'badge-success' : 'badge-ghost'"
                >
                  {{ row.openNow ? 'Open now' : 'Closed' }}
                </span>
              </div>
              <p v-if="row.description" class="text-xs text-base-content/70">{{ row.description }}</p>
              <div v-if="row.windows.length" class="font-mono text-sm">
                {{ row.windows.map(w => `${w.start}–${w.end}`).join(', ') }}
              </div>
              <div v-else class="text-sm text-base-content/60 italic">Closed today</div>
            </div>
          </div>
        </div>
      </div>

      <!-- Links (audience-gated by the server). -->
      <div v-if="homeLinks.length">
        <h2 class="text-2xl font-semibold mb-2">Links</h2>
        <p class="text-sm text-base-content/70 mb-4">
          Useful destinations curated by your admins.
        </p>
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
                  <span v-if="l.icon" class="mr-1">{{ l.icon }}</span>{{ l.label }}
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

    <!-- Recent Events -->
    <div class="my-10">
      <div class="flex items-center gap-4 max-w-3xl mx-auto mb-8">
        <div class="flex-1 border-t-2 border-dotted border-base-content/40"></div>
        <div class="text-3xl md:text-4xl font-bold text-center px-2">Recent Events</div>
        <div class="flex-1 border-t-2 border-dotted border-base-content/40"></div>
      </div>
      <div class="flex justify-center">
        <iframe
          src="https://player.vimeo.com/video/145580978?byline=0&portrait=0"
          width="700"
          height="394"
          frameborder="0"
          class="max-w-full"
          webkitallowfullscreen
          mozallowfullscreen
          allowfullscreen
        ></iframe>
      </div>
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
  0: 'sun', 1: 'mon', 2: 'tue', 3: 'wed', 4: 'thu', 5: 'fri', 6: 'sat',
}
const dowLabel: Record<DayOfWeek, string> = {
  mon: 'Monday', tue: 'Tuesday', wed: 'Wednesday', thu: 'Thursday',
  fri: 'Friday', sat: 'Saturday', sun: 'Sunday',
}
const today: DayOfWeek = dowMap[new Date().getDay()]
const todayLabel = dowLabel[today]

const scheduleRowsToday = computed(() =>
  publicSchedules.value.map(s => {
    const windows = s.intervals
      .filter(iv => iv.day === today)
      .sort((a, b) => a.start.localeCompare(b.start))
    return {
      id: s.id,
      name: s.name,
      description: s.description,
      windows,
      openNow: windows.some(w => isNowBetween(w.start, w.end)),
    }
  }),
)

function isNowBetween(start: string, end: string): boolean {
  const d = new Date()
  const nowMin = d.getHours() * 60 + d.getMinutes()
  return toMin(start) <= nowMin && nowMin < toMin(end)
}
function toMin(hhmm: string): number {
  const [h, m] = hhmm.split(':').map(n => parseInt(n, 10))
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
        browse_tools:    hl.browse_tools    !== false,
        admin_panel:     hl.admin_panel     !== false,
        wiki:            hl.wiki            !== false,
      }
    }
  } catch (error) {
    console.error('Failed to load site configuration:', error)
    siteName.value = 'Cooperative Systems Spaces'
  }
  loadPublicSchedules()
  loadHomeLinks()
})
</script>
