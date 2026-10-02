<template>
  <div class="container mx-auto px-4 py-8">
    <div class="hero min-h-48">
      <div class="hero-content text-center">
        <div class="max-w-3xl">
          <h1 class="text-5xl font-bold">Events</h1>
          <p class="py-4 text-base-content/70">
            Join us for
            <a
              href="https://www.meetup.com/Cambridge-Hackspace/events/"
              target="_blank"
              rel="noopener"
              class="link link-primary"
            >
              Hackspace Project Night
            </a>
            every Tuesday at 6:30pm, or check what else is coming up below.
          </p>
        </div>
      </div>
    </div>

    <div class="flex justify-center mb-4">
      <div class="join" role="group" aria-label="Calendar view">
        <button
          type="button"
          class="btn btn-sm join-item"
          :class="view === 'month' ? 'btn-active' : ''"
          :aria-pressed="view === 'month'"
          @click="view = 'month'"
        >
          Month
        </button>
        <button
          type="button"
          class="btn btn-sm join-item"
          :class="view === 'list' ? 'btn-active' : ''"
          :aria-pressed="view === 'list'"
          @click="view = 'list'"
        >
          List
        </button>
      </div>
    </div>

    <!--
      The month grid is wider than the list, which is why the wrapper's width
      follows the view rather than being fixed: a seven-column grid in a
      `max-w-2xl` column gives each day about 90 pixels.
    -->
    <div :class="view === 'month' ? 'max-w-5xl mx-auto' : 'max-w-2xl mx-auto'">
      <CalendarMonth v-if="view === 'month'" :timezone="siteTimezone" />
      <CalendarEvents v-else :timezone="siteTimezone" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import CalendarEvents from '@/components/CalendarEvents.vue'
import CalendarMonth from '@/components/CalendarMonth.vue'
import { useConfigStore } from '@/stores/config'

const configStore = useConfigStore()
/** Event times belong to the building, not to whoever is reading the page. */
const siteTimezone = computed(() => configStore.siteTimezone())

/**
 * The month grid is the default here: #96 asked for the calendar to be
 * calendar-shaped, and a page called Events is where somebody goes to look at
 * a month. The list stays a click away -- it is the better shape for "what is
 * on next", which is why the home page keeps it.
 */
const view = ref<'month' | 'list'>('month')
</script>
