<template>
  <div class="container mx-auto px-4 py-8">
    <!-- Announcement banners -->
    <div class="alert mb-8 flex justify-center text-center" style="background-color:#d9edf7; color:#31708f;">
      <span>
        Interested in joining our community?
        <router-link to="/join" class="underline font-semibold">Checkout our memberships</router-link>,
        and see what we have to offer!
      </span>
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
          <img src="/images/index_red.png" alt="" class="w-20 h-20 rounded-full mx-auto mb-4" />
          <h3 class="text-xl font-bold mb-2">what</h3>
          <p class="text-sm text-base-content/70">
            We are a 501(c)(3) makerspace in the center of Somerville, MA. We have a 1200sqft workshop catering to enthusiastic makers,
            and hold weekly meetings where we get together and make things.
            We have a Laser cutter, CNCs, 3D printers, and a range of other tools for our members to use.
          </p>
        </div>

        <div>
          <img src="/images/index_blue.png" alt="" class="w-20 h-20 rounded-full mx-auto mb-4" />
          <h3 class="text-xl font-bold mb-2">why</h3>
          <p class="text-sm text-base-content/70">
            We've started the Cambridge Hackspace to provide a physical space where people can gather and work on their projects,
            have access to larger or expensive tools (like the 3D printers, and laser cutter),
            and provide a place where the community can share their knowledge.
          </p>
        </div>

        <div>
          <img src="/images/index_yellow.png" alt="" class="w-20 h-20 rounded-full mx-auto mb-4" />
          <h3 class="text-xl font-bold mb-2">who</h3>
          <p class="text-sm text-base-content/70">
            Anyone and everyone is welcome. Whether your thing is software, electronics, woodwork, or knitting,
            we provide a space where you can meet fellow makers, to learn, and be inspired.
            We hold workshops every month so you can learn new skills.
          </p>
        </div>

        <div>
          <img src="/images/index_green.png" alt="" class="w-20 h-20 rounded-full mx-auto mb-4" />
          <h3 class="text-xl font-bold mb-2">where</h3>
          <p class="text-sm text-base-content/70">
            We are located in Union Square, Somerville
            (<router-link to="/contact" class="underline">address</router-link>).
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
            <router-link to="/join" class="btn btn-primary btn-lg">Become a Member</router-link>
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

      <div class="flex items-center justify-center md:justify-start">
        <div class="text-center md:text-left">
          <h3 class="text-2xl font-bold mb-1">See what we've been up to</h3>
          <p class="text-base-content/70 mb-3">Photos from project nights, builds, and events.</p>
          <a href="https://www.instagram.com/hackcambridge/" target="_blank" rel="noopener" class="btn btn-secondary btn-lg">
            @hackcambridge on Instagram
          </a>
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
import { schedulesApi, homeLinksApi } from '@/utils/api'
import type { DayOfWeek, HomeLink, HomeLinkAudience, Schedule } from '@/types'

const authStore = useAuthStore()

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

onMounted(() => {
  loadPublicSchedules()
  loadHomeLinks()
})
</script>
