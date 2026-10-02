<template>
  <div class="calendar-month bg-base-300">
    <div class="month-header">
      <button
        class="nav-btn"
        type="button"
        aria-label="Previous month"
        :disabled="loading"
        @click="step(-1)"
      >
        ‹
      </button>
      <h2 class="month-title" data-testid="month-title">{{ title }}</h2>
      <button
        class="nav-btn"
        type="button"
        aria-label="Next month"
        :disabled="loading"
        @click="step(1)"
      >
        ›
      </button>
      <button class="today-btn" type="button" :disabled="loading" @click="goToToday">Today</button>
    </div>

    <p v-if="error" class="error-state" role="alert">
      {{ error }}
      <button class="retry-btn" type="button" @click="() => load()">Try Again</button>
    </p>

    <div class="grid-wrap" :class="{ busy: loading }">
      <div class="weekday-row" aria-hidden="true">
        <div v-for="name in WEEKDAY_NAMES" :key="name" class="weekday">{{ name }}</div>
      </div>

      <div v-for="(week, index) in weeks" :key="index" class="week-row">
        <button
          v-for="day in week"
          :key="day.key"
          type="button"
          class="day-cell"
          :class="{
            outside: !day.inMonth,
            today: day.key === today,
            selected: day.key === selected,
          }"
          :data-day="day.key"
          :aria-label="`${day.key}, ${eventsOn(day.key).length} event(s)`"
          :aria-pressed="day.key === selected"
          @click="selected = day.key"
        >
          <span class="day-number">{{ Number(day.key.slice(8, 10)) }}</span>
          <span
            v-for="event in eventsOn(day.key).slice(0, CHIPS_PER_DAY)"
            :key="event.title + event.start"
            class="chip"
            :style="{ borderLeftColor: event.calendar_color }"
            :title="event.title"
          >
            <span v-if="!event.all_day" class="chip-time">{{ time(event.start) }}</span>
            <span class="chip-title">{{ event.title }}</span>
          </span>
          <span v-if="eventsOn(day.key).length > CHIPS_PER_DAY" class="chip-more">
            +{{ eventsOn(day.key).length - CHIPS_PER_DAY }} more
          </span>
        </button>
      </div>
    </div>

    <section v-if="selected" class="day-detail" data-testid="day-detail">
      <h3 class="detail-title">{{ longDate(selected) }}</h3>
      <p v-if="eventsOn(selected).length === 0" class="detail-empty">Nothing scheduled.</p>
      <article
        v-for="event in eventsOn(selected)"
        :key="event.title + event.start"
        class="detail-event"
        :style="{ borderLeftColor: event.calendar_color }"
      >
        <h4 class="detail-event-title">{{ event.title }}</h4>
        <p class="detail-meta">
          <span v-if="event.all_day">All day</span>
          <span v-else>
            {{ time(event.start) }}<span v-if="event.end"> – {{ time(event.end) }}</span>
            <span class="detail-zone">{{ zone(event.start) }}</span>
          </span>
          <span v-if="event.location"> · {{ event.location }}</span>
        </p>
        <!--
          Rendered server-side from the feed's markup; see the long comment in
          CalendarEvents.vue and server/src/calendar/description.rs. The raw
          `description` is never given to v-html.
        -->
        <!-- eslint-disable vue/no-v-html -- see above; the server renders it -->
        <div
          v-if="event.description_html"
          class="detail-description"
          v-html="event.description_html"
        ></div>
        <!-- eslint-enable vue/no-v-html -->
        <p v-else-if="event.description" class="detail-description">{{ event.description }}</p>
      </article>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import {
  addMonths,
  clockTime,
  daysCovered,
  keyOf,
  lastDayOfMonth,
  monthLabel,
  monthMatrix,
  monthOf,
  todayKey,
  zoneLabel,
  type YearMonth,
} from '@/lib/event-times'

interface CalendarEvent {
  title: string
  description?: string
  description_html?: string
  start: string
  end?: string
  location?: string
  calendar_name: string
  calendar_color: string
  all_day: boolean
}

const props = defineProps<{
  /** IANA zone to read and render event times in; see `lib/event-times`. */
  timezone?: string
}>()

/** Chips before a cell says "+n more". Three is what fits at phone width. */
const CHIPS_PER_DAY = 3
const WEEKDAY_NAMES = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']

const events = ref<CalendarEvent[]>([])
const loading = ref(true)
const error = ref('')
const selected = ref('')
const cursor = ref<YearMonth>(monthOf(new Date().toISOString(), props.timezone))

const today = computed(() => todayKey(props.timezone))
const weeks = computed(() => monthMatrix(cursor.value))
const title = computed(() => monthLabel(cursor.value))

/**
 * Events keyed by the day they appear on, which is every day they touch: a
 * two-day all-day event belongs in two cells, and one that runs past midnight
 * belongs in both.
 */
const byDay = computed(() => {
  const map = new Map<string, CalendarEvent[]>()
  for (const event of events.value) {
    for (const day of daysCovered(event.start, event.end, props.timezone)) {
      const list = map.get(day)
      if (list) list.push(event)
      else map.set(day, [event])
    }
  }
  for (const list of map.values()) {
    // All-day events first, then by start: a chip list that reorders itself
    // as the feed's order changes is hard to read.
    list.sort((a, b) => Number(b.all_day) - Number(a.all_day) || a.start.localeCompare(b.start))
  }
  return map
})

function eventsOn(day: string): CalendarEvent[] {
  return byDay.value.get(day) ?? []
}

function time(iso: string): string {
  return clockTime(iso, props.timezone)
}

function zone(iso: string): string {
  return zoneLabel(iso, props.timezone)
}

function longDate(day: string): string {
  // Formatted from the key itself at UTC noon, not from an instant: the key is
  // already a local date, and re-reading it through a zone could move it.
  const [year, month, date] = day.split('-').map(Number)
  return new Intl.DateTimeFormat('en-US', {
    timeZone: 'UTC',
    weekday: 'long',
    month: 'long',
    day: 'numeric',
    year: 'numeric',
  }).format(new Date(Date.UTC(year ?? 1970, (month ?? 1) - 1, date ?? 1, 12)))
}

function step(delta: number) {
  cursor.value = addMonths(cursor.value, delta)
}

function goToToday() {
  cursor.value = monthOf(new Date().toISOString(), props.timezone)
  selected.value = today.value
}

/**
 * Load the whole grid, not just the month: the first and last rows show days
 * of the neighbouring months, and leaving those cells empty when they have
 * events in them is worse than not drawing them at all.
 */
async function load() {
  loading.value = true
  error.value = ''
  const visible = weeks.value.flat()
  const from = visible[0]?.key ?? keyOf(cursor.value, 1)
  const to = visible[visible.length - 1]?.key ?? keyOf(cursor.value, lastDayOfMonth(cursor.value))
  try {
    const response = await fetch(
      `/api/calendar/events?from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}`
    )
    if (!response.ok) {
      throw new Error(`Failed to fetch events: ${response.statusText}`)
    }
    events.value = await response.json()
  } catch (err) {
    events.value = []
    error.value = err instanceof Error ? err.message : 'Failed to load calendar events'
  } finally {
    loading.value = false
  }
}

watch(cursor, () => {
  void load()
})

onMounted(() => {
  selected.value = today.value
  void load()
})
</script>

<style scoped>
.calendar-month {
  border-radius: 8px;
  padding: 1rem;
}

.month-header {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-bottom: 0.75rem;
}

.month-title {
  flex: 1;
  margin: 0;
  font-size: 1.25rem;
  font-weight: 600;
  text-align: center;
}

.nav-btn,
.today-btn,
.retry-btn {
  border: 1px solid currentColor;
  border-radius: 4px;
  background: none;
  padding: 0.25rem 0.6rem;
  cursor: pointer;
  font-size: 1rem;
  opacity: 0.8;
}

.nav-btn:disabled,
.today-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.today-btn {
  font-size: 0.8rem;
}

.error-state {
  color: #d9534f;
  margin: 0 0 0.75rem;
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.grid-wrap.busy {
  opacity: 0.5;
}

.weekday-row,
.week-row {
  display: grid;
  grid-template-columns: repeat(7, minmax(0, 1fr));
  gap: 2px;
}

.weekday {
  text-align: center;
  font-size: 0.75rem;
  text-transform: uppercase;
  opacity: 0.6;
  padding: 0.25rem 0;
}

.day-cell {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 2px;
  min-height: 5.5rem;
  padding: 0.25rem;
  border: 1px solid rgba(128, 128, 128, 0.25);
  border-radius: 4px;
  cursor: pointer;
  text-align: left;
  overflow: hidden;
}

.day-cell.outside {
  opacity: 0.45;
}

.day-cell.today {
  outline: 2px solid currentColor;
  outline-offset: -2px;
}

.day-cell.selected {
  box-shadow: inset 0 0 0 1px currentColor;
}

.day-number {
  font-size: 0.8rem;
  font-weight: 600;
  opacity: 0.8;
}

.chip {
  display: flex;
  gap: 0.25rem;
  align-items: baseline;
  border-left: 3px solid;
  padding: 0 0.25rem;
  font-size: 0.7rem;
  line-height: 1.3;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  background: rgba(128, 128, 128, 0.12);
  border-radius: 2px;
}

.chip-time {
  opacity: 0.7;
  flex: none;
}

.chip-title {
  overflow: hidden;
  text-overflow: ellipsis;
}

.chip-more {
  font-size: 0.65rem;
  opacity: 0.7;
  padding-left: 0.25rem;
}

.day-detail {
  margin-top: 1rem;
  border-top: 1px solid rgba(128, 128, 128, 0.25);
  padding-top: 0.75rem;
}

.detail-title {
  margin: 0 0 0.5rem;
  font-size: 1rem;
  font-weight: 600;
}

.detail-empty {
  margin: 0;
  opacity: 0.6;
  font-size: 0.875rem;
}

.detail-event {
  border-left: 4px solid;
  padding: 0.25rem 0 0.25rem 0.75rem;
  margin-bottom: 0.75rem;
}

.detail-event-title {
  margin: 0;
  font-size: 0.95rem;
  font-weight: 600;
}

.detail-meta {
  margin: 0.125rem 0 0.25rem;
  font-size: 0.8rem;
  opacity: 0.75;
}

.detail-zone {
  font-size: 0.7rem;
  opacity: 0.8;
  margin-left: 0.25rem;
}

.detail-description {
  font-size: 0.85rem;
  line-height: 1.4;
}

.detail-description :deep(p) {
  margin: 0 0 0.4rem;
}

.detail-description :deep(a) {
  text-decoration: underline;
}

.detail-description :deep(ul),
.detail-description :deep(ol) {
  margin: 0.25rem 0 0.4rem 1.25rem;
  list-style: disc;
}

@media (max-width: 640px) {
  .day-cell {
    min-height: 4rem;
  }

  .chip-time {
    display: none;
  }
}
</style>
