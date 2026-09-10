<template>
  <div :class="embedded ? '' : 'container mx-auto px-4 py-8'">
    <div v-if="!embedded" class="mb-6">
      <h1 class="text-3xl font-bold mb-1">Power</h1>
      <p class="text-base-content/70">
        Map circuits, outlets, and receptacles, and plug tools into them.
      </p>
    </div>

    <div v-if="error" class="alert alert-error mb-4">
      <span>{{ error }}</span>
      <button class="btn btn-sm btn-ghost" @click="error = ''">Dismiss</button>
    </div>

    <div role="tablist" class="tabs tabs-boxed w-fit mb-6">
      <a
        v-for="t in TABS"
        :key="t.key"
        role="tab"
        class="tab"
        :class="{ 'tab-active': section === t.key }"
        @click="section = t.key"
        >{{ t.label }}</a
      >
    </div>

    <div v-if="loading" class="flex justify-center py-10">
      <span class="loading loading-spinner loading-lg" />
    </div>

    <!-- ===== Circuits ===== -->
    <section v-else-if="section === 'circuits'">
      <form class="grid gap-2 sm:grid-cols-5 items-end mb-4" @submit.prevent="submitCircuit">
        <label class="form-control">
          <span class="label-text">Breaker label</span>
          <input
            v-model="circuitForm.breaker_label"
            class="input input-bordered input-sm"
            required
          />
        </label>
        <label class="form-control">
          <span class="label-text">Voltage</span>
          <input
            v-model.number="circuitForm.voltage_rating"
            type="number"
            min="1"
            class="input input-bordered input-sm"
            required
          />
        </label>
        <label class="form-control">
          <span class="label-text">Amperage limit</span>
          <input
            v-model="circuitForm.amperage_limit"
            class="input input-bordered input-sm"
            required
          />
        </label>
        <label class="form-control">
          <span class="label-text">Upstream trunk</span>
          <select v-model="circuitForm.parent_circuit_id" class="select select-bordered select-sm">
            <option :value="null">— None (top-level) —</option>
            <option
              v-for="c in circuits"
              :key="c.id"
              :value="c.id"
              :disabled="c.id === editingCircuitId"
            >
              {{ c.breaker_label }}
            </option>
          </select>
        </label>
        <div class="flex gap-2">
          <button type="submit" class="btn btn-primary btn-sm">
            {{ editingCircuitId ? 'Save' : 'Add' }}
          </button>
          <button
            v-if="editingCircuitId"
            type="button"
            class="btn btn-ghost btn-sm"
            @click="resetCircuitForm"
          >
            Cancel
          </button>
        </div>
      </form>

      <div class="overflow-x-auto">
        <table class="table table-sm">
          <thead>
            <tr>
              <th>Breaker</th>
              <th>Voltage</th>
              <th>Amp limit</th>
              <th>Draw</th>
              <th>Upstream</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="c in circuits" :key="c.id">
              <td>{{ c.breaker_label }}</td>
              <td>{{ c.voltage_rating }} V</td>
              <td>{{ c.amperage_limit }} A</td>
              <td :class="circuitOverLimit(c) ? 'text-error font-semibold' : ''">
                {{ circuitDraw(c.id) }} A
              </td>
              <td>{{ circuitLabel(c.parent_circuit_id) }}</td>
              <td class="text-right">
                <button class="btn btn-ghost btn-xs" @click="editCircuit(c)">Edit</button>
                <button class="btn btn-ghost btn-xs text-error" @click="removeCircuit(c.id)">
                  Delete
                </button>
              </td>
            </tr>
            <tr v-if="!circuits.length">
              <td colspan="6" class="text-center text-base-content/50">No circuits yet.</td>
            </tr>
          </tbody>
        </table>
      </div>
    </section>

    <!-- ===== Outlets ===== -->
    <section v-else-if="section === 'outlets'">
      <form class="grid gap-2 sm:grid-cols-5 items-end mb-4" @submit.prevent="submitOutlet">
        <label class="form-control">
          <span class="label-text">Circuit</span>
          <select v-model="outletForm.circuit_id" class="select select-bordered select-sm" required>
            <option value="" disabled>— Choose a circuit —</option>
            <option v-for="c in circuits" :key="c.id" :value="c.id">{{ c.breaker_label }}</option>
          </select>
        </label>
        <label class="form-control">
          <span class="label-text">Room</span>
          <PlacePicker v-model="outletForm.place_id" :places="places" hide-null />
        </label>
        <label class="form-control">
          <span class="label-text">Label</span>
          <input v-model="outletForm.label" class="input input-bordered input-sm" required />
        </label>
        <label class="form-control">
          <span class="label-text">Location (optional)</span>
          <input v-model="outletForm.location" class="input input-bordered input-sm" />
        </label>
        <div class="flex gap-2">
          <button type="submit" class="btn btn-primary btn-sm">
            {{ editingOutletId ? 'Save' : 'Add' }}
          </button>
          <button
            v-if="editingOutletId"
            type="button"
            class="btn btn-ghost btn-sm"
            @click="resetOutletForm"
          >
            Cancel
          </button>
        </div>
      </form>

      <div class="overflow-x-auto">
        <table class="table table-sm">
          <thead>
            <tr>
              <th>Label</th>
              <th>Circuit</th>
              <th>Room</th>
              <th>Location</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="o in outlets" :key="o.id">
              <td>{{ o.label }}</td>
              <td>{{ circuitLabel(o.circuit_id) }}</td>
              <td>{{ placeLabel(o.place_id) }}</td>
              <td>{{ o.location || '—' }}</td>
              <td class="text-right">
                <button class="btn btn-ghost btn-xs" @click="editOutlet(o)">Edit</button>
                <button class="btn btn-ghost btn-xs text-error" @click="removeOutlet(o.id)">
                  Delete
                </button>
              </td>
            </tr>
            <tr v-if="!outlets.length">
              <td colspan="5" class="text-center text-base-content/50">No outlets yet.</td>
            </tr>
          </tbody>
        </table>
      </div>
    </section>

    <!-- ===== Receptacles ===== -->
    <section v-else-if="section === 'receptacles'">
      <form class="grid gap-2 sm:grid-cols-3 items-end mb-4" @submit.prevent="submitReceptacle">
        <label class="form-control">
          <span class="label-text">Outlet</span>
          <select
            v-model="receptacleForm.outlet_id"
            class="select select-bordered select-sm"
            required
          >
            <option value="" disabled>— Choose an outlet —</option>
            <option v-for="o in outlets" :key="o.id" :value="o.id">{{ outletLabel(o.id) }}</option>
          </select>
        </label>
        <label class="form-control">
          <span class="label-text">Label</span>
          <input v-model="receptacleForm.label" class="input input-bordered input-sm" required />
        </label>
        <div class="flex gap-2">
          <button type="submit" class="btn btn-primary btn-sm">
            {{ editingReceptacleId ? 'Save' : 'Add' }}
          </button>
          <button
            v-if="editingReceptacleId"
            type="button"
            class="btn btn-ghost btn-sm"
            @click="resetReceptacleForm"
          >
            Cancel
          </button>
        </div>
      </form>

      <div class="overflow-x-auto">
        <table class="table table-sm">
          <thead>
            <tr>
              <th>Label</th>
              <th>Outlet</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="r in receptacles" :key="r.id">
              <td>{{ r.label }}</td>
              <td>{{ outletLabel(r.outlet_id) }}</td>
              <td class="text-right">
                <button class="btn btn-ghost btn-xs" @click="editReceptacle(r)">Edit</button>
                <button class="btn btn-ghost btn-xs text-error" @click="removeReceptacle(r.id)">
                  Delete
                </button>
              </td>
            </tr>
            <tr v-if="!receptacles.length">
              <td colspan="3" class="text-center text-base-content/50">No receptacles yet.</td>
            </tr>
          </tbody>
        </table>
      </div>
    </section>

    <!-- ===== Assignments ===== -->
    <section v-else-if="section === 'assignments'">
      <p class="text-sm text-base-content/70 mb-3">
        Plug each tool into the receptacle it draws from. Battery-powered or unmapped tools stay
        unassigned.
      </p>
      <div class="overflow-x-auto">
        <table class="table table-sm">
          <thead>
            <tr>
              <th>Tool</th>
              <th>Receptacle</th>
              <th>Last draw</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="t in tools" :key="t.id">
              <td>{{ t.name }}</td>
              <td>
                <select
                  class="select select-bordered select-sm w-full max-w-md"
                  :value="t.receptacle_id ?? ''"
                  @change="assign(t, ($event.target as HTMLSelectElement).value)"
                >
                  <option value="">— Unassigned —</option>
                  <option v-for="r in receptacles" :key="r.id" :value="r.id">
                    {{ receptacleLabel(r.id) }}
                  </option>
                </select>
              </td>
              <td>{{ toolDraw(t.id) !== null ? `${toolDraw(t.id)} A` : '—' }}</td>
            </tr>
            <tr v-if="!tools.length">
              <td colspan="3" class="text-center text-base-content/50">No tools.</td>
            </tr>
          </tbody>
        </table>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { reactive, ref } from 'vue'
import PlacePicker from './PlacePicker.vue'
import { useReloadOnReactivate } from '@/composables/useReloadOnReactivate'
import { powerApi, placesApi, toolsApi } from '@/utils/api'
import type { Place, PowerCircuit, PowerOutlet, PowerReceptacle, PowerTelemetry } from '@/types'
import type { Tool } from '@/types/tools'

defineProps<{ embedded?: boolean }>()

type Section = 'circuits' | 'outlets' | 'receptacles' | 'assignments'
const TABS: { key: Section; label: string }[] = [
  { key: 'circuits', label: 'Circuits' },
  { key: 'outlets', label: 'Outlets' },
  { key: 'receptacles', label: 'Receptacles' },
  { key: 'assignments', label: 'Assignments' },
]

const section = ref<Section>('circuits')
const loading = ref(true)
const error = ref('')

const circuits = ref<PowerCircuit[]>([])
const outlets = ref<PowerOutlet[]>([])
const receptacles = ref<PowerReceptacle[]>([])
const places = ref<Place[]>([])
const tools = ref<Tool[]>([])
const telemetry = ref<PowerTelemetry | null>(null)

// ---- forms ----
const editingCircuitId = ref<string | null>(null)
const circuitForm = reactive({
  breaker_label: '',
  voltage_rating: 120,
  amperage_limit: '',
  parent_circuit_id: null,
})
const editingOutletId = ref<string | null>(null)
const outletForm = reactive({ circuit_id: '', place_id: null, label: '', location: '' })
const editingReceptacleId = ref<string | null>(null)
const receptacleForm = reactive({ outlet_id: '', label: '' })

// ---- label helpers ----
function circuitLabel(id: string | null): string {
  if (!id) return '—'
  return circuits.value.find((c) => c.id === id)?.breaker_label ?? '(unknown)'
}
function placeLabel(id: string): string {
  return places.value.find((p) => p.id === id)?.name ?? '(unknown)'
}
function outletLabel(id: string): string {
  const o = outlets.value.find((x) => x.id === id)
  if (!o) return '(unknown)'
  return `${circuitLabel(o.circuit_id)} / ${o.label}`
}
function receptacleLabel(id: string): string {
  const r = receptacles.value.find((x) => x.id === id)
  if (!r) return '(unknown)'
  return `${outletLabel(r.outlet_id)} / ${r.label}`
}

// Live-draw helpers (#43). Returns '0' for a circuit that has reported nothing.
function circuitDraw(id: string): string {
  return telemetry.value?.circuits.find((c) => c.circuit_id === id)?.total_draw_amps ?? '0'
}
function circuitOverLimit(c: PowerCircuit): boolean {
  return Number(circuitDraw(c.id)) > Number(c.amperage_limit)
}
function toolDraw(toolId: string): string | null {
  return telemetry.value?.tools.find((s) => s.tool_id === toolId)?.last_draw_amps ?? null
}

// ---- loaders ----
async function loadAll() {
  loading.value = true
  const [c, o, r, p, t, tel] = await Promise.all([
    powerApi.listCircuits(),
    powerApi.listOutlets(),
    powerApi.listReceptacles(),
    placesApi.list(),
    toolsApi.getTools(),
    powerApi.telemetry(),
  ])
  if (c.success && c.data) circuits.value = c.data
  else error.value = c.error || 'Failed to load circuits'
  if (o.success && o.data) outlets.value = o.data
  if (r.success && r.data) receptacles.value = r.data
  if (p.success && p.data) places.value = p.data
  if (t.success && t.data) tools.value = t.data
  if (tel.success && tel.data) telemetry.value = tel.data
  loading.value = false
}
useReloadOnReactivate(loadAll)

// ---- circuits ----
function resetCircuitForm() {
  editingCircuitId.value = null
  circuitForm.breaker_label = ''
  circuitForm.voltage_rating = 120
  circuitForm.amperage_limit = ''
  circuitForm.parent_circuit_id = null
}
function editCircuit(c: PowerCircuit) {
  editingCircuitId.value = c.id
  circuitForm.breaker_label = c.breaker_label
  circuitForm.voltage_rating = c.voltage_rating
  circuitForm.amperage_limit = c.amperage_limit
  circuitForm.parent_circuit_id = c.parent_circuit_id
}
async function submitCircuit() {
  const body = {
    breaker_label: circuitForm.breaker_label,
    voltage_rating: circuitForm.voltage_rating,
    amperage_limit: String(circuitForm.amperage_limit),
    parent_circuit_id: circuitForm.parent_circuit_id,
  }
  const res = editingCircuitId.value
    ? await powerApi.updateCircuit(editingCircuitId.value, body)
    : await powerApi.createCircuit(body)
  if (!res.success) {
    error.value = res.error || 'Failed to save circuit'
    return
  }
  resetCircuitForm()
  await loadAll()
}
async function removeCircuit(id: string) {
  const res = await powerApi.removeCircuit(id)
  if (!res.success) {
    error.value = res.error || 'Failed to delete circuit'
    return
  }
  await loadAll()
}

// ---- outlets ----
function resetOutletForm() {
  editingOutletId.value = null
  outletForm.circuit_id = ''
  outletForm.place_id = null
  outletForm.label = ''
  outletForm.location = ''
}
function editOutlet(o: PowerOutlet) {
  editingOutletId.value = o.id
  outletForm.circuit_id = o.circuit_id
  outletForm.place_id = o.place_id
  outletForm.label = o.label
  outletForm.location = o.location ?? ''
}
async function submitOutlet() {
  if (!outletForm.place_id) {
    error.value = 'An outlet needs a room'
    return
  }
  const body = {
    circuit_id: outletForm.circuit_id,
    place_id: outletForm.place_id,
    label: outletForm.label,
    location: outletForm.location || null,
  }
  const res = editingOutletId.value
    ? await powerApi.updateOutlet(editingOutletId.value, body)
    : await powerApi.createOutlet(body)
  if (!res.success) {
    error.value = res.error || 'Failed to save outlet'
    return
  }
  resetOutletForm()
  await loadAll()
}
async function removeOutlet(id: string) {
  const res = await powerApi.removeOutlet(id)
  if (!res.success) {
    error.value = res.error || 'Failed to delete outlet'
    return
  }
  await loadAll()
}

// ---- receptacles ----
function resetReceptacleForm() {
  editingReceptacleId.value = null
  receptacleForm.outlet_id = ''
  receptacleForm.label = ''
}
function editReceptacle(r: PowerReceptacle) {
  editingReceptacleId.value = r.id
  receptacleForm.outlet_id = r.outlet_id
  receptacleForm.label = r.label
}
async function submitReceptacle() {
  const body = { outlet_id: receptacleForm.outlet_id, label: receptacleForm.label }
  const res = editingReceptacleId.value
    ? await powerApi.updateReceptacle(editingReceptacleId.value, body)
    : await powerApi.createReceptacle(body)
  if (!res.success) {
    error.value = res.error || 'Failed to save receptacle'
    return
  }
  resetReceptacleForm()
  await loadAll()
}
async function removeReceptacle(id: string) {
  const res = await powerApi.removeReceptacle(id)
  if (!res.success) {
    error.value = res.error || 'Failed to delete receptacle'
    return
  }
  await loadAll()
}

// ---- assignments ----
async function assign(tool: Tool, receptacleId: string) {
  const res = await powerApi.assignToolReceptacle(tool.id, receptacleId || null)
  if (!res.success) {
    error.value = res.error || 'Failed to assign receptacle'
    // Reload so the dropdown snaps back to the true state on a conflict.
    await loadAll()
    return
  }
  tool.receptacle_id = receptacleId || null
}
</script>
