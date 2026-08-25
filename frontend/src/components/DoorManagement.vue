<template>
  <div :class="embedded ? '' : 'container mx-auto px-4 py-8'">
    <div v-if="!embedded" class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/" class="link">Home</router-link></li>
        <li><router-link to="/admin" class="link">Admin</router-link></li>
        <li>Doors</li>
      </ul>
    </div>

    <div class="flex items-center justify-between mb-6">
      <div>
        <h1 v-if="!embedded" class="text-3xl font-bold mb-1">Doors</h1>
        <p class="text-base-content/70">
          Manage physical doors backed by edge devices, their access rules, and view unlock events.
        </p>
      </div>
      <button class="btn btn-primary btn-sm" @click="openNew">+ New door</button>
    </div>

    <div v-if="flash" class="alert mb-4" :class="flashOk ? 'alert-success' : 'alert-error'">
      <span>{{ flash }}</span>
      <button class="btn btn-ghost btn-xs" @click="flash = ''">✕</button>
    </div>

    <div v-if="loading" class="text-center py-12"><span class="loading loading-spinner loading-lg"></span></div>
    <div v-else-if="doors.length === 0" class="text-center py-8 text-base-content/60">
      No doors yet. Create one to get started.
    </div>
    <div v-else class="overflow-x-auto">
      <table class="table">
        <thead>
          <tr>
            <th>Name</th>
            <th>Location</th>
            <th>Edge device</th>
            <th>Status</th>
            <th class="text-right">Actions</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="d in doors" :key="d.id">
            <td class="font-medium">{{ d.name }}</td>
            <td class="text-sm text-base-content/70">{{ d.location || '—' }}</td>
            <td class="font-mono text-xs">{{ deviceLabel(d.edge_device_id) }}</td>
            <td>
              <span class="badge" :class="d.enabled ? 'badge-success' : 'badge-neutral'">
                {{ d.enabled ? 'Enabled' : 'Disabled' }}
              </span>
            </td>
            <td class="text-right whitespace-nowrap">
              <button class="btn btn-ghost btn-xs" @click="openDetail(d)">Manage</button>
              <button class="btn btn-ghost btn-xs" :disabled="!d.edge_device_id || unlockingId === d.id" @click="adminUnlock(d)">
                <span v-if="unlockingId === d.id" class="loading loading-spinner loading-xs"></span>
                <span v-else>Unlock</span>
              </button>
              <button class="btn btn-ghost btn-xs text-error" @click="deleteDoor(d)">Delete</button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- =============== Create / Edit door modal =============== -->
    <div v-if="showForm" class="modal modal-open">
      <div class="modal-box max-w-2xl">
        <h3 class="font-bold text-lg mb-4">{{ editing ? 'Edit door' : 'New door' }}</h3>

        <div class="form-control mb-3">
          <label class="label"><span class="label-text">Name</span></label>
          <input v-model="form.name" type="text" class="input input-bordered" placeholder="Front door" />
        </div>

        <div class="form-control mb-3">
          <label class="label"><span class="label-text">Location</span></label>
          <input v-model.lazy="formLocation" type="text" class="input input-bordered" placeholder="Lobby" />
        </div>

        <div class="form-control mb-3">
          <label class="label"><span class="label-text">Edge device</span></label>
          <select v-model="form.edge_device_id" class="select select-bordered">
            <option :value="null">— None —</option>
            <option v-for="dev in devices" :key="dev.id" :value="dev.id">
              {{ dev.name }} ({{ shortId(dev.id) }})
            </option>
          </select>
          <span class="label-text-alt mt-1 text-base-content/60">
            Bind this door to the edge device that owns its RFID reader + relay.
          </span>
        </div>

        <div class="form-control mb-3">
          <label class="label"><span class="label-text">Unlock duration (ms)</span></label>
          <input v-model.number="form.unlock_duration_ms" type="number" min="500" max="60000" class="input input-bordered" />
        </div>

        <div class="form-control mb-3">
          <label class="label cursor-pointer justify-start gap-3">
            <input v-model="form.enabled" type="checkbox" class="toggle toggle-primary" />
            <span class="label-text">Enabled</span>
          </label>
        </div>

        <div v-if="places.length" class="grid grid-cols-2 gap-3 mb-3">
          <div class="form-control">
            <label class="label py-1"><span class="label-text">From</span></label>
            <PlacePicker v-model="form.place_id_from" :places="places" hide-null />
          </div>
          <div class="form-control">
            <label class="label py-1"><span class="label-text">To</span></label>
            <PlacePicker v-model="form.place_id_to" :places="places" hide-null />
          </div>
        </div>
        <div v-if="!places.length" class="alert alert-warning mb-3 text-sm">
          You need at least one place to create a door. Add a place (or a
          special place like
          <span class="badge badge-warning badge-sm">Outside</span>)
          on the Places tab first.
        </div>

        <div class="modal-action">
          <button class="btn btn-ghost" @click="showForm = false">Cancel</button>
          <button class="btn btn-primary" :disabled="saving" @click="saveDoor">
            <span v-if="saving" class="loading loading-spinner loading-sm"></span>
            <span v-else>{{ editing ? 'Save' : 'Create' }}</span>
          </button>
        </div>
      </div>
    </div>

    <!-- =============== Detail modal: rules / events / QR =============== -->
    <div v-if="detail" class="modal modal-open">
      <div class="modal-box max-w-3xl">
        <h3 class="font-bold text-lg mb-2">{{ detail.name }}</h3>
        <p class="text-sm text-base-content/60 mb-4">{{ detail.location || '' }}</p>

        <div role="tablist" class="tabs tabs-boxed mb-4">
          <a role="tab" class="tab" :class="{ 'tab-active': detailTab === 'settings' }" @click="detailTab = 'settings'">Settings</a>
          <a role="tab" class="tab" :class="{ 'tab-active': detailTab === 'rules' }" @click="detailTab = 'rules'">Access rules</a>
          <a role="tab" class="tab" :class="{ 'tab-active': detailTab === 'events' }" @click="switchToEvents">Events</a>
          <a role="tab" class="tab" :class="{ 'tab-active': detailTab === 'qr' }" @click="switchToQr">QR / signage</a>
        </div>

        <!-- Settings tab: read-only summary OR inline edit form. -->
        <div v-if="detailTab === 'settings'">
          <!-- Read-only summary -->
          <div v-if="!editingInline" class="space-y-2 text-sm">
            <div><strong>Edge device:</strong> {{ deviceLabel(detail.edge_device_id) }}</div>
            <div><strong>Unlock duration:</strong> {{ detail.unlock_duration_ms }} ms</div>
            <div><strong>Status:</strong> {{ detail.enabled ? 'Enabled' : 'Disabled' }}</div>
            <div>
              <strong>From:</strong>
              {{ placeLabel(detail.place_id_from) }}
              <span class="text-base-content/50">→</span>
              <strong>To:</strong>
              {{ placeLabel(detail.place_id_to) }}
            </div>
            <div class="flex gap-2 mt-3">
              <button class="btn btn-primary btn-sm" @click="beginInlineEdit">Edit settings</button>
              <button class="btn btn-sm" @click="republish(detail)">Republish state to edge</button>
            </div>
          </div>

          <!-- Inline edit form: same fields as the create modal, no second modal. -->
          <div v-else class="space-y-3">
            <div class="form-control">
              <label class="label py-1"><span class="label-text">Name</span></label>
              <input v-model="form.name" type="text" class="input input-bordered input-sm" />
            </div>

            <div class="form-control">
              <label class="label py-1"><span class="label-text">Location</span></label>
              <input v-model.lazy="formLocation" type="text" class="input input-bordered input-sm" />
            </div>

            <div class="form-control">
              <label class="label py-1"><span class="label-text">Edge device</span></label>
              <select v-model="form.edge_device_id" class="select select-bordered select-sm">
                <option :value="null">— None —</option>
                <option v-for="dev in devices" :key="dev.id" :value="dev.id">
                  {{ dev.name }} ({{ shortId(dev.id) }})
                </option>
              </select>
            </div>

            <div class="grid grid-cols-2 gap-3">
              <div class="form-control">
                <label class="label py-1"><span class="label-text">Unlock duration (ms)</span></label>
                <input v-model.number="form.unlock_duration_ms" type="number" min="500" max="60000" class="input input-bordered input-sm" />
              </div>
              <div class="form-control">
                <label class="label cursor-pointer justify-start gap-3 mt-7">
                  <input v-model="form.enabled" type="checkbox" class="toggle toggle-primary toggle-sm" />
                  <span class="label-text">Enabled</span>
                </label>
              </div>
            </div>

            <div v-if="places.length" class="grid grid-cols-2 gap-3">
              <div class="form-control">
                <label class="label py-1"><span class="label-text">From</span></label>
                <PlacePicker v-model="form.place_id_from" :places="places" hide-null />
              </div>
              <div class="form-control">
                <label class="label py-1"><span class="label-text">To</span></label>
                <PlacePicker v-model="form.place_id_to" :places="places" hide-null />
              </div>
            </div>

            <div class="flex justify-end gap-2 pt-1">
              <button class="btn btn-ghost btn-sm" @click="cancelInlineEdit">Cancel</button>
              <button class="btn btn-primary btn-sm" :disabled="saving" @click="saveInline">
                <span v-if="saving" class="loading loading-spinner loading-sm"></span>
                <span v-else>Save</span>
              </button>
            </div>
          </div>
        </div>

        <!-- Rules tab -->
        <div v-else-if="detailTab === 'rules'">
          <div v-if="!detail.rules.length" class="text-sm text-base-content/60 mb-3">
            No rules yet. Add one below.
          </div>
          <table v-else class="table table-sm">
            <thead><tr><th>Kind</th><th>Value</th><th>Effect</th><th>Schedule</th><th></th></tr></thead>
            <tbody>
              <tr v-for="r in detail.rules" :key="r.id">
                <td>{{ r.kind }}</td>
                <td class="font-mono text-xs">{{ ruleValueLabel(r) }}</td>
                <td>
                  <span class="badge" :class="r.effect === 'deny' ? 'badge-error' : 'badge-success'">{{ r.effect }}</span>
                </td>
                <td class="text-xs">{{ scheduleLabel(r.schedule_id) }}</td>
                <td class="text-right">
                  <button class="btn btn-ghost btn-xs text-error" @click="removeRule(r)">Remove</button>
                </td>
              </tr>
            </tbody>
          </table>

          <div class="border-t border-base-300 mt-4 pt-3 grid grid-cols-1 md:grid-cols-5 gap-2 items-end">
            <div class="form-control">
              <label class="label py-1"><span class="label-text">Kind</span></label>
              <select v-model="newRule.kind" class="select select-bordered select-sm">
                <option value="role">Role (≥)</option>
                <option value="user">User</option>
                <option value="card">Card ID</option>
              </select>
            </div>
            <div class="form-control md:col-span-2">
              <label class="label py-1"><span class="label-text">Value</span></label>
              <select v-if="newRule.kind === 'role'" v-model="newRule.value" class="select select-bordered select-sm">
                <option value="Member">Member</option>
                <option value="Staff">Staff</option>
                <option value="Admin">Admin</option>
              </select>
              <select v-else-if="newRule.kind === 'user'" v-model="newRule.value" class="select select-bordered select-sm">
                <option value="">— pick a user —</option>
                <option v-for="u in users" :key="u.id" :value="u.id">{{ u.full_name }} (@{{ u.username }})</option>
              </select>
              <input v-else v-model="newRule.value" type="text" class="input input-bordered input-sm" placeholder="card ID" />
            </div>
            <div class="form-control">
              <label class="label py-1"><span class="label-text">Effect</span></label>
              <select v-model="newRule.effect" class="select select-bordered select-sm">
                <option value="allow">Allow</option>
                <option value="deny">Deny</option>
              </select>
            </div>
            <div class="form-control">
              <label class="label py-1"><span class="label-text">Schedule</span></label>
              <SchedulePicker v-model="newRule.schedule_id" :schedules="schedules" />
            </div>
            <button class="btn btn-primary btn-sm md:col-span-5" :disabled="!newRule.value.trim()" @click="addRule">
              Add rule
            </button>
          </div>
        </div>

        <!-- Events tab -->
        <div v-else-if="detailTab === 'events'">
          <div v-if="!events.length" class="text-sm text-base-content/60">No events recorded yet.</div>
          <table v-else class="table table-sm">
            <thead><tr><th>When</th><th>Method</th><th>User / card</th><th>Result</th></tr></thead>
            <tbody>
              <tr v-for="e in events" :key="e.id">
                <td class="text-xs whitespace-nowrap">{{ fmt(e.occurred_at) }}</td>
                <td>{{ e.method }}</td>
                <td class="text-xs">
                  <span v-if="e.user_id">{{ userLabel(e.user_id) }}</span>
                  <span v-if="e.card_id_attempted" class="font-mono text-base-content/60">{{ e.card_id_attempted }}</span>
                </td>
                <td>
                  <span class="badge" :class="e.granted ? 'badge-success' : 'badge-error'">
                    {{ e.granted ? 'Granted' : 'Denied' }}
                  </span>
                  <span v-if="e.reason" class="text-xs text-base-content/60 ml-1">{{ e.reason }}</span>
                </td>
              </tr>
            </tbody>
          </table>
        </div>

        <!-- QR tab -->
        <div v-else-if="detailTab === 'qr'" class="text-center">
          <p class="text-sm text-base-content/70 mb-2">
            Print this and post it at the door. Members scan, sign in, and tap <strong>I'm here</strong>.
          </p>
          <img v-if="qrDataUrl" :src="qrDataUrl" alt="Door QR" class="mx-auto border border-base-300 rounded p-2 bg-white" />
          <code class="block mt-2 break-all text-xs bg-base-200 p-2 rounded">{{ qrUrl }}</code>
        </div>

        <div class="modal-action">
          <button class="btn" @click="closeDetail">Close</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import QRCode from 'qrcode'
import { doorsApi, placesApi, schedulesApi, apiClient } from '@/utils/api'
import type {
  Door, DoorAccessEvent, DoorAccessRule, DoorDetail, DoorRuleEffect, DoorRuleKind, Place, Schedule, User,
} from '@/types'
import PlacePicker from './PlacePicker.vue'
import SchedulePicker from './SchedulePicker.vue'

withDefaults(
  defineProps<{
    /** Hide the breadcrumb + outer container so the component can be
        dropped into a parent (e.g. the combined Facility page) without
        duplicating page chrome. */
    embedded?: boolean
  }>(),
  { embedded: false },
)

const loading = ref(false)
const saving = ref(false)
const flash = ref('')
const flashOk = ref(true)

const doors = ref<Door[]>([])
const devices = ref<Array<{ id: string; name: string }>>([])
const users = ref<User[]>([])
const places = ref<Place[]>([])

const showForm = ref(false)
const editing = ref<Door | null>(null)
const form = ref<{
  name: string
  location: string | null
  edge_device_id: string | null
  unlock_duration_ms: number
  enabled: boolean
  /** Required — `''` means "not picked yet", which fails validation on save. */
  place_id_from: string
  place_id_to: string
}>({
  name: '', location: null, edge_device_id: null, unlock_duration_ms: 5000, enabled: true,
  place_id_from: '', place_id_to: '',
})

// Bridge between input v-model (empty string) and the nullable Location column.
const formLocation = ref('')

const detail = ref<DoorDetail | null>(null)
const detailTab = ref<'settings' | 'rules' | 'events' | 'qr'>('rules')
/** When true, the Settings tab renders the form inline instead of the
    read-only summary. Reuses the same `form` ref the create modal uses. */
const editingInline = ref(false)
const events = ref<DoorAccessEvent[]>([])
const qrUrl = ref('')
const qrDataUrl = ref('')
const unlockingId = ref<string | null>(null)

const schedules = ref<Schedule[]>([])
const newRule = ref<{
  kind: DoorRuleKind
  value: string
  effect: DoorRuleEffect
  schedule_id: string | null
}>({
  kind: 'role', value: 'Member', effect: 'allow', schedule_id: null,
})

function notify(msg: string, ok = true) {
  flash.value = msg
  flashOk.value = ok
  setTimeout(() => { if (flash.value === msg) flash.value = '' }, 5000)
}
function fmt(iso: string) { return new Date(iso).toLocaleString() }
function shortId(id: string) { return id.slice(0, 8) }
function deviceLabel(id: string | null) {
  if (!id) return '—'
  const d = devices.value.find(x => x.id === id)
  return d ? `${d.name} (${shortId(id)})` : shortId(id)
}
function placeLabel(id: string | null | undefined) {
  if (!id) return '— (unset) —'
  const p = places.value.find(x => x.id === id)
  if (!p) return id.slice(0, 8)
  return p.is_special ? `★ ${p.name}` : p.name
}
function scheduleLabel(id: string | null | undefined) {
  if (!id) return 'Always'
  const s = schedules.value.find(x => x.id === id)
  return s?.name ?? id.slice(0, 8)
}
function userLabel(id: string) {
  const u = users.value.find(x => x.id === id)
  return u ? `${u.full_name} (@${u.username})` : id.slice(0, 8)
}
function ruleValueLabel(r: DoorAccessRule) {
  if (r.kind === 'user') return userLabel(r.value)
  return r.value
}

async function loadDoors() {
  loading.value = true
  const r = await doorsApi.list()
  loading.value = false
  if (r.success && r.data) doors.value = r.data
}

async function loadDevices() {
  // Reuses the existing admin devices endpoint exposed by DeviceManagement.
  try {
    const resp = await apiClient.raw.get('/admin/devices')
    devices.value = (resp.data?.data || resp.data || []) as Array<{ id: string; name: string }>
  } catch {
    devices.value = []
  }
}

async function loadUsers() {
  try {
    const resp = await apiClient.get<User[]>('/admin/roster')
    if (resp.success && resp.data) users.value = resp.data
  } catch {
    users.value = []
  }
}

function openNew() {
  editing.value = null
  // Pre-pick the first available place on both sides so a fresh form is
  // valid out of the gate; the user can change either picker.
  const first = places.value[0]?.id ?? ''
  form.value = {
    name: '',
    location: null,
    edge_device_id: null,
    unlock_duration_ms: 5000,
    enabled: true,
    place_id_from: first,
    place_id_to: first,
  }
  formLocation.value = ''
  showForm.value = true
}

async function loadPlaces() {
  try {
    const r = await placesApi.list()
    if (r.success && r.data) places.value = r.data
  } catch {
    places.value = []
  }
}

async function saveDoor() {
  if (!form.value.name.trim()) { notify('Name is required', false); return }
  if (!form.value.place_id_from || !form.value.place_id_to) {
    notify('Pick a From and a To place', false)
    return
  }
  form.value.location = formLocation.value.trim() || null
  saving.value = true
  const r = await doorsApi.create(form.value)
  saving.value = false
  if (r.success) {
    notify('Door created')
    showForm.value = false
    await loadDoors()
  } else notify(r.error || 'Failed', false)
}

async function deleteDoor(d: Door) {
  if (!confirm(`Delete door "${d.name}"? Its rules and events will be removed.`)) return
  const r = await doorsApi.remove(d.id)
  if (r.success) { notify('Door deleted'); await loadDoors() }
  else notify(r.error || 'Failed', false)
}

async function adminUnlock(d: Door) {
  unlockingId.value = d.id
  const r = await doorsApi.unlock(d.id)
  unlockingId.value = null
  if (r.success && r.data?.unlocked) notify(`Unlocked "${d.name}"`)
  else notify(r.error || 'Unlock failed', false)
}

async function republish(d: Door) {
  const r = await doorsApi.republish(d.id)
  if (r.success) notify('State republished to edge')
  else notify(r.error || 'Failed', false)
}

async function openDetail(d: Door) {
  const r = await doorsApi.get(d.id)
  if (r.success && r.data) {
    detail.value = r.data
    detailTab.value = 'rules'
    events.value = []
    qrUrl.value = ''
    qrDataUrl.value = ''
    editingInline.value = false
  } else notify(r.error || 'Failed to load door detail', false)
}

function closeDetail() {
  detail.value = null
  editingInline.value = false
}

/** Flip Settings tab into edit mode, hydrating `form` from `detail`. */
function beginInlineEdit() {
  if (!detail.value) return
  editing.value = detail.value as Door
  form.value = {
    name: detail.value.name,
    location: detail.value.location,
    edge_device_id: detail.value.edge_device_id,
    unlock_duration_ms: detail.value.unlock_duration_ms,
    enabled: detail.value.enabled,
    place_id_from: detail.value.place_id_from ?? '',
    place_id_to: detail.value.place_id_to ?? '',
  }
  formLocation.value = detail.value.location || ''
  editingInline.value = true
}

function cancelInlineEdit() {
  editingInline.value = false
}

async function saveInline() {
  if (!detail.value) return
  if (!form.value.name.trim()) { notify('Name is required', false); return }
  if (!form.value.place_id_from || !form.value.place_id_to) {
    notify('Pick a From and a To place', false)
    return
  }
  form.value.location = formLocation.value.trim() || null
  saving.value = true
  const r = await doorsApi.update(detail.value.id, form.value)
  saving.value = false
  if (r.success) {
    notify('Door saved')
    editingInline.value = false
    await loadDoors()
    await openDetail({ id: detail.value.id } as Door)
  } else notify(r.error || 'Failed', false)
}

async function switchToEvents() {
  if (!detail.value) return
  detailTab.value = 'events'
  const r = await doorsApi.events(detail.value.id, { limit: 100 })
  if (r.success && r.data) events.value = r.data
}

async function switchToQr() {
  if (!detail.value) return
  detailTab.value = 'qr'
  const r = await doorsApi.qrUrl(detail.value.id)
  if (r.success && r.data) {
    qrUrl.value = r.data.url
    try {
      qrDataUrl.value = await QRCode.toDataURL(r.data.url, { margin: 1, width: 256 })
    } catch (e: any) {
      notify('Failed to render QR: ' + (e?.message || ''), false)
    }
  }
}

async function addRule() {
  if (!detail.value || !newRule.value.value.trim()) return
  try {
    const r = await doorsApi.addRule(detail.value.id, { ...newRule.value })
    if (r.success) {
      notify('Rule added')
      newRule.value.value = newRule.value.kind === 'role' ? 'Member' : ''
      newRule.value.schedule_id = null
      await openDetail(detail.value)
    } else notify(r.error || 'Failed', false)
  } catch (e: any) {
    notify(e?.response?.data?.error || 'Failed to add rule', false)
  }
}

async function loadSchedules() {
  try {
    const r = await schedulesApi.list()
    if (r.success && r.data) schedules.value = r.data
  } catch {
    schedules.value = []
  }
}

async function removeRule(rule: DoorAccessRule) {
  if (!detail.value) return
  if (!confirm('Remove this rule?')) return
  try {
    const r = await doorsApi.removeRule(detail.value.id, rule.id)
    if (r.success) { notify('Rule removed'); await openDetail(detail.value) }
    else notify(r.error || 'Failed', false)
  } catch (e: any) {
    notify(e?.response?.data?.error || 'Failed to remove rule', false)
  }
}

onMounted(async () => {
  await Promise.all([loadDoors(), loadDevices(), loadUsers(), loadPlaces(), loadSchedules()])
})
</script>
