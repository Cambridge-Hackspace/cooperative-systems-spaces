<template>
  <div :class="embedded ? '' : 'container mx-auto px-4 py-8'">
    <div v-if="!embedded" class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/" class="link">Home</router-link></li>
        <li><router-link to="/admin" class="link">Admin</router-link></li>
        <li>Schedules</li>
      </ul>
    </div>

    <div class="flex items-start justify-between mb-6 gap-4">
      <div>
        <h1 v-if="!embedded" class="text-3xl font-bold mb-1">Schedules</h1>
        <p class="text-base-content/70">
          Reusable weekly windows you can attach to a door's access rules.
          Times are evaluated in the configured site time zone.
        </p>
      </div>
      <button class="btn btn-primary btn-sm whitespace-nowrap" @click="openNew">
        + New schedule
      </button>
    </div>

    <div v-if="flash" class="alert mb-4" :class="flashOk ? 'alert-success' : 'alert-error'">
      <span>{{ flash }}</span>
      <button class="btn btn-ghost btn-xs" @click="flash = ''">✕</button>
    </div>

    <div v-if="loading" class="text-center py-12">
      <span class="loading loading-spinner loading-lg"></span>
    </div>
    <div v-else-if="!schedules.length" class="text-center py-8 text-base-content/60">
      No schedules yet. Click <strong>+ New schedule</strong> to create one
      from a template (24/7, 9–5 weekdays, …) and tweak it from there.
    </div>
    <div v-else class="overflow-x-auto">
      <table class="table">
        <thead>
          <tr>
            <th>Name</th>
            <th>Windows</th>
            <th>Public?</th>
            <th class="text-right">Actions</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="s in schedules" :key="s.id">
            <td>
              <div class="font-medium">{{ s.name }}</div>
              <div v-if="s.description" class="text-xs text-base-content/60">{{ s.description }}</div>
            </td>
            <td class="text-xs">
              <div v-for="(line, i) in summarize(s)" :key="i">{{ line }}</div>
              <div v-if="!s.intervals.length" class="text-base-content/60 italic">never</div>
            </td>
            <td>
              <span
                v-if="s.is_public"
                class="badge badge-success badge-sm"
                title="Surfaced on the public home page"
              >Public</span>
              <span v-else class="badge badge-ghost badge-sm">Internal</span>
            </td>
            <td class="text-right whitespace-nowrap">
              <button class="btn btn-ghost btn-xs" @click="openEdit(s)">Edit</button>
              <button class="btn btn-ghost btn-xs text-error" @click="onDelete(s)">Delete</button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- =============== Create / Edit modal =============== -->
    <div v-if="showForm" class="modal modal-open">
      <div class="modal-box max-w-3xl">
        <h3 class="font-bold text-lg mb-3">
          {{ editing ? 'Edit schedule' : 'New schedule' }}
        </h3>

        <div class="grid grid-cols-2 gap-3 mb-3">
          <div class="form-control">
            <label class="label py-1"><span class="label-text">Name</span></label>
            <input v-model="form.name" type="text" class="input input-bordered" placeholder="Member Hours" />
          </div>
          <div class="form-control">
            <label class="label py-1"><span class="label-text">Apply template</span></label>
            <select
              class="select select-bordered"
              :value="''"
              @change="onTemplate(($event.target as HTMLSelectElement).value)"
            >
              <option :value="''">— Choose a preset —</option>
              <option v-for="t in templates" :key="t.id" :value="t.id">
                {{ t.label }}<span v-if="t.description"> ({{ t.description }})</span>
              </option>
            </select>
          </div>
        </div>

        <div class="form-control mb-3">
          <label class="label py-1"><span class="label-text">Description</span></label>
          <textarea v-model="form.description" class="textarea textarea-bordered" rows="2" />
        </div>

        <div class="form-control mb-4">
          <label class="label cursor-pointer justify-start gap-3">
            <input v-model="form.is_public" type="checkbox" class="toggle toggle-success" />
            <span class="label-text">
              Public — show on the home page "Hours today" card
            </span>
          </label>
        </div>

        <div class="card bg-base-200 mb-3">
          <div class="card-body py-4">
            <h4 class="font-semibold text-sm uppercase tracking-wide text-base-content/70 mb-2">
              Weekly windows
            </h4>
            <div v-for="day in allDays" :key="day" class="flex items-start gap-2 py-1.5">
              <div class="w-12 text-xs uppercase font-medium mt-2">{{ day }}</div>
              <div class="flex-1 space-y-1">
                <div
                  v-for="(iv, idx) in intervalsByDay[day]"
                  :key="idx"
                  class="flex items-center gap-2"
                >
                  <input
                    :value="iv.start"
                    type="time"
                    class="input input-bordered input-sm w-32"
                    @change="updateInterval(day, idx, 'start', ($event.target as HTMLInputElement).value)"
                  />
                  <span class="text-base-content/60">–</span>
                  <input
                    :value="iv.end"
                    type="time"
                    class="input input-bordered input-sm w-32"
                    @change="updateInterval(day, idx, 'end', ($event.target as HTMLInputElement).value)"
                  />
                  <button class="btn btn-ghost btn-xs text-error" @click="removeInterval(day, idx)">×</button>
                </div>
                <button class="btn btn-ghost btn-xs" @click="addInterval(day)">+ Add window</button>
              </div>
            </div>
          </div>
        </div>

        <div class="modal-action">
          <button class="btn btn-ghost" @click="showForm = false">Cancel</button>
          <button class="btn btn-primary" :disabled="saving || !form.name.trim()" @click="save">
            <span v-if="saving" class="loading loading-spinner loading-sm"></span>
            <span v-else>{{ editing ? 'Save' : 'Create' }}</span>
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { schedulesApi } from '@/utils/api'
import type { DayOfWeek, Schedule, ScheduleInterval } from '@/types'
import { ALL_DAYS, SCHEDULE_TEMPLATES } from './schedule_templates'

withDefaults(defineProps<{ embedded?: boolean }>(), { embedded: false })

const loading = ref(false)
const saving = ref(false)
const flash = ref('')
const flashOk = ref(true)

const schedules = ref<Schedule[]>([])
const allDays: DayOfWeek[] = ALL_DAYS
const templates = SCHEDULE_TEMPLATES

const showForm = ref(false)
const editing = ref<Schedule | null>(null)
const form = ref<{
  name: string
  description: string | null
  intervals: ScheduleInterval[]
  is_public: boolean
}>({
  name: '',
  description: null,
  intervals: [],
  is_public: false,
})

/** Group the working list by day so the editor can render a row per day. */
const intervalsByDay = computed<Record<DayOfWeek, ScheduleInterval[]>>(() => {
  const out: Record<DayOfWeek, ScheduleInterval[]> = {
    mon: [], tue: [], wed: [], thu: [], fri: [], sat: [], sun: [],
  }
  for (const iv of form.value.intervals) out[iv.day].push(iv)
  for (const list of Object.values(out)) list.sort((a, b) => a.start.localeCompare(b.start))
  return out
})

function notify(msg: string, ok = true) {
  flash.value = msg
  flashOk.value = ok
  setTimeout(() => { if (flash.value === msg) flash.value = '' }, 5000)
}

/** Human-friendly summary lines for the table: groups identical windows
    across days, so "Mon–Fri 09:00–17:00" reads as one line, not five. */
function summarize(s: Schedule): string[] {
  if (!s.intervals.length) return []
  type Key = string
  const groups = new Map<Key, DayOfWeek[]>()
  for (const iv of s.intervals) {
    const k = `${iv.start}-${iv.end}`
    const arr = groups.get(k) ?? []
    arr.push(iv.day)
    groups.set(k, arr)
  }
  const dayOrder: Record<DayOfWeek, number> = { mon:0,tue:1,wed:2,thu:3,fri:4,sat:5,sun:6 }
  const out: string[] = []
  for (const [windowKey, days] of groups) {
    days.sort((a, b) => dayOrder[a] - dayOrder[b])
    const range = compactDayRange(days)
    const [start, end] = windowKey.split('-')
    out.push(`${range} ${start}–${end}`)
  }
  return out
}

/** ["mon","tue","wed","fri"] → "Mon–Wed, Fri" */
function compactDayRange(days: DayOfWeek[]): string {
  const labels: Record<DayOfWeek, string> = {
    mon:'Mon', tue:'Tue', wed:'Wed', thu:'Thu', fri:'Fri', sat:'Sat', sun:'Sun',
  }
  const dayOrder: Record<DayOfWeek, number> = { mon:0,tue:1,wed:2,thu:3,fri:4,sat:5,sun:6 }
  const sorted = [...days].sort((a, b) => dayOrder[a] - dayOrder[b])
  const ranges: string[] = []
  let i = 0
  while (i < sorted.length) {
    let j = i
    while (j + 1 < sorted.length && dayOrder[sorted[j + 1]] === dayOrder[sorted[j]] + 1) j++
    ranges.push(
      j === i ? labels[sorted[i]] : `${labels[sorted[i]]}–${labels[sorted[j]]}`,
    )
    i = j + 1
  }
  return ranges.join(', ')
}

function openNew() {
  editing.value = null
  // Default to "9–5 weekdays" so first-time use is a single Save click away
  // from something useful; the user can still pick a different template.
  form.value = {
    name: '',
    description: null,
    intervals: SCHEDULE_TEMPLATES.find(t => t.id === 'weekday-9-5')!.build(),
    is_public: false,
  }
  showForm.value = true
}

function openEdit(s: Schedule) {
  editing.value = s
  form.value = {
    name: s.name,
    description: s.description,
    // Clone to avoid mutating the table row.
    intervals: s.intervals.map(iv => ({ ...iv })),
    is_public: s.is_public,
  }
  showForm.value = true
}

function onTemplate(id: string) {
  if (!id) return
  const t = SCHEDULE_TEMPLATES.find(t => t.id === id)
  if (!t) return
  if (form.value.intervals.length && !confirm('Replace the current windows with this template?')) return
  form.value.intervals = t.build()
}

function addInterval(day: DayOfWeek) {
  form.value.intervals.push({ day, start: '09:00', end: '17:00' })
}

function removeInterval(day: DayOfWeek, idxWithinDay: number) {
  // Map "idx-within-day" back to the array index since the renderer groups.
  let seen = 0
  for (let i = 0; i < form.value.intervals.length; i++) {
    if (form.value.intervals[i].day !== day) continue
    if (seen === idxWithinDay) { form.value.intervals.splice(i, 1); return }
    seen++
  }
}

function updateInterval(day: DayOfWeek, idxWithinDay: number, key: 'start' | 'end', value: string) {
  let seen = 0
  for (let i = 0; i < form.value.intervals.length; i++) {
    if (form.value.intervals[i].day !== day) continue
    if (seen === idxWithinDay) {
      form.value.intervals[i] = { ...form.value.intervals[i], [key]: value }
      return
    }
    seen++
  }
}

async function load() {
  loading.value = true
  const r = await schedulesApi.list()
  loading.value = false
  if (r.success && r.data) schedules.value = r.data
}

async function save() {
  if (!form.value.name.trim()) { notify('Name is required', false); return }
  for (const iv of form.value.intervals) {
    if (iv.end <= iv.start) {
      notify(`Interval ${iv.day} ${iv.start}–${iv.end} must end after it starts`, false)
      return
    }
  }
  saving.value = true
  const body = {
    name: form.value.name.trim(),
    description: form.value.description,
    intervals: form.value.intervals,
    is_public: form.value.is_public,
  }
  const res = editing.value
    ? await schedulesApi.update(editing.value.id, body)
    : await schedulesApi.create(body)
  saving.value = false
  if (res.success) {
    notify(editing.value ? 'Schedule saved' : 'Schedule created')
    showForm.value = false
    await load()
  } else notify(res.error || 'Failed to save', false)
}

async function onDelete(s: Schedule) {
  if (!confirm(`Delete "${s.name}"? Rules referencing it will revert to "always".`)) return
  const r = await schedulesApi.remove(s.id)
  if (r.success) { notify('Schedule deleted'); await load() }
  else notify(r.error || 'Failed to delete', false)
}

onMounted(load)
</script>
