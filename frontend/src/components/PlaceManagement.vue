<template>
  <div :class="embedded ? '' : 'container mx-auto px-4 py-8'">
    <div v-if="!embedded" class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/" class="link">Home</router-link></li>
        <li><router-link to="/admin" class="link">Admin</router-link></li>
        <li>Places</li>
      </ul>
    </div>

    <div class="flex items-start justify-between mb-6 gap-4">
      <div>
        <h1 v-if="!embedded" class="text-3xl font-bold mb-1">Places</h1>
        <p class="text-base-content/70">
          A self-referential hierarchy of physical places. The level vocabulary (<span
            class="font-mono text-sm"
            >{{ config?.types?.join(' › ') || '…' }}</span
          >) comes from <code class="text-xs bg-base-300 px-1 rounded">[place].types</code> in your
          server config.
        </p>
      </div>
      <div class="flex gap-2">
        <button
          class="btn btn-ghost btn-sm whitespace-nowrap"
          :disabled="!config?.enabled"
          @click="openCreateSpecial"
        >
          + Special place
        </button>
        <button
          class="btn btn-primary btn-sm whitespace-nowrap"
          :disabled="!config?.enabled || !config?.types?.length"
          @click="openCreate(null)"
        >
          + New root place
        </button>
      </div>
    </div>

    <div v-if="config && !config.enabled" class="alert alert-warning mb-6">
      <span
        >The places module is disabled in server configuration (<code>[place].enabled</code>).</span
      >
    </div>

    <div v-if="flash" class="alert mb-4" :class="flashOk ? 'alert-success' : 'alert-error'">
      <span>{{ flash }}</span>
      <button class="btn btn-ghost btn-xs" @click="flash = ''">✕</button>
    </div>

    <div v-if="loading" class="text-center py-12">
      <span class="loading loading-spinner loading-lg"></span>
    </div>
    <div v-else-if="!places.length" class="text-center py-8 text-base-content/60">
      No places yet. Click <strong>+ New root place</strong> to start.
    </div>
    <div v-else class="space-y-4">
      <!-- Special places (Outside, Common Area, Parking Lot, …). -->
      <div v-if="specialPlaces.length" class="card bg-base-200 shadow-sm">
        <div class="card-body py-4">
          <h3 class="font-semibold text-sm uppercase tracking-wide text-base-content/70 mb-2">
            Special places
          </h3>
          <PlaceNode
            v-for="sp in specialPlaces"
            :key="sp.id"
            :place="sp"
            :children-map="childrenMap"
            :config="config"
            @add-child="openCreate"
            @edit="openEdit"
            @delete="onDelete"
          />
        </div>
      </div>

      <!-- Regular hierarchy. -->
      <div v-if="hierarchyRoots.length" class="card bg-base-100 shadow-md">
        <div class="card-body">
          <h3
            v-if="specialPlaces.length"
            class="font-semibold text-sm uppercase tracking-wide text-base-content/70 mb-2"
          >
            Locations
          </h3>
          <PlaceNode
            v-for="root in hierarchyRoots"
            :key="root.id"
            :place="root"
            :children-map="childrenMap"
            :config="config"
            @add-child="openCreate"
            @edit="openEdit"
            @delete="onDelete"
          />
        </div>
      </div>
    </div>

    <!-- ===== Create / Edit modal ===== -->
    <div v-if="showForm" class="modal modal-open">
      <div class="modal-box max-w-lg">
        <h3 class="font-bold text-lg mb-1">
          {{ editing ? 'Edit place' : form.is_special ? 'New special place' : 'New place' }}
        </h3>
        <p v-if="formParent && !form.is_special" class="text-sm text-base-content/60 mb-3">
          inside <strong>{{ formParent.name }}</strong> ({{ formParent.place_type }})
        </p>

        <div class="form-control mb-3">
          <label class="label cursor-pointer justify-start gap-3">
            <input
              v-model="form.is_special"
              type="checkbox"
              class="toggle toggle-warning"
              @change="onSpecialToggle"
            />
            <span class="label-text"> Special place (Outside, Common Area, Parking Lot, …) </span>
          </label>
          <span class="label-text-alt mt-1 text-base-content/60">
            Special places sit outside the normal hierarchy: free-form type, no parent, no level
            rules.
          </span>
        </div>

        <div class="form-control mb-3">
          <label class="label"><span class="label-text">Name</span></label>
          <input
            v-model="form.name"
            type="text"
            class="input input-bordered"
            :placeholder="form.is_special ? 'Outside' : 'Room 5'"
          />
        </div>

        <div class="form-control mb-3">
          <label class="label"><span class="label-text">Type</span></label>
          <input
            v-if="form.is_special"
            v-model="form.place_type"
            type="text"
            class="input input-bordered"
            placeholder="Outside"
          />
          <select v-else v-model="form.place_type" class="select select-bordered">
            <option v-for="t in allowedTypes" :key="t" :value="t">{{ t }}</option>
          </select>
          <span class="label-text-alt mt-1 text-base-content/60">
            <template v-if="form.is_special">
              Free-form label; pick anything that reads well in pickers.
            </template>
            <template v-else> Children must use a level deeper than their parent. </template>
          </span>
        </div>

        <div v-if="editing && !form.is_special" class="form-control mb-3">
          <label class="label"><span class="label-text">Move to (parent)</span></label>
          <PlacePicker
            v-model="form.parent_id"
            :places="movableParentOptions"
            null-label="— Root —"
          />
        </div>

        <div class="form-control mb-3">
          <label class="label"><span class="label-text">Description</span></label>
          <textarea v-model="form.description" class="textarea textarea-bordered" rows="2" />
        </div>

        <div class="form-control mb-3">
          <label class="label"><span class="label-text">External ID (optional)</span></label>
          <input
            v-model="form.external_id"
            type="text"
            class="input input-bordered input-sm font-mono"
          />
        </div>

        <div class="modal-action">
          <button class="btn btn-ghost" @click="showForm = false">Cancel</button>
          <button
            class="btn btn-primary"
            :disabled="saving || !form.name.trim() || !form.place_type"
            @click="save"
          >
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
import { placesApi } from '@/utils/api'
import type { Place, PlaceConfig } from '@/types'
import PlaceNode from './PlaceNode.vue'
import PlacePicker from './PlacePicker.vue'

withDefaults(
  defineProps<{
    /** Hide the breadcrumb + outer container so the component can be
        dropped into a parent (e.g. the combined Facility page) without
        duplicating page chrome. */
    embedded?: boolean
  }>(),
  { embedded: false }
)

const loading = ref(false)
const saving = ref(false)
const flash = ref('')
const flashOk = ref(true)

const places = ref<Place[]>([])
const config = ref<PlaceConfig | null>(null)

const showForm = ref(false)
const editing = ref<Place | null>(null)
const formParent = ref<Place | null>(null)
const form = ref<{
  name: string
  place_type: string
  parent_id: string | null
  description: string | null
  external_id: string | null
  is_special: boolean
}>({
  name: '',
  place_type: '',
  parent_id: null,
  description: null,
  external_id: null,
  is_special: false,
})

function notify(msg: string, ok = true) {
  flash.value = msg
  flashOk.value = ok
  setTimeout(() => {
    if (flash.value === msg) flash.value = ''
  }, 5000)
}

const childrenMap = computed(() => {
  const m = new Map<string | null, Place[]>()
  for (const p of places.value) {
    const k = p.parent_id
    const arr = m.get(k) ?? []
    arr.push(p)
    m.set(k, arr)
  }
  for (const list of m.values()) list.sort((a, b) => a.name.localeCompare(b.name))
  return m
})
const roots = computed(() => childrenMap.value.get(null) ?? [])
const specialPlaces = computed(() =>
  places.value.filter((p) => p.is_special).sort((a, b) => a.name.localeCompare(b.name))
)
const hierarchyRoots = computed(() => roots.value.filter((p) => !p.is_special))

/** Types that are strictly deeper than the (prospective) parent's type. */
const allowedTypes = computed(() => {
  if (!config.value?.types?.length) return []
  if (!formParent.value) return config.value.types.slice()
  const i = config.value.types.indexOf(formParent.value.place_type)
  if (i < 0) return config.value.types.slice()
  return config.value.types.slice(i + 1)
})

/** Places that are NOT in the editing place's subtree (avoid creating a cycle). */
const movableParentOptions = computed(() => {
  if (!editing.value) return places.value
  const banned = new Set<string>([editing.value.id])
  // BFS to enumerate descendants of `editing.value`.
  const stack = [editing.value.id]
  while (stack.length) {
    const id = stack.pop()
    for (const c of childrenMap.value.get(id) ?? []) {
      banned.add(c.id)
      stack.push(c.id)
    }
  }
  return places.value.filter((p) => !banned.has(p.id))
})

async function loadAll() {
  loading.value = true
  try {
    const [cfg, list] = await Promise.all([placesApi.config(), placesApi.list()])
    // A refused config used to leave `config` null, which is exactly what the
    // module being switched off looks like: no warning, the level vocabulary
    // rendered as "…", and both create buttons dead with nothing to say why.
    if (cfg.success && cfg.data) config.value = cfg.data
    else notify(cfg.error || 'Could not load the places configuration', false)
    if (list.success && list.data) places.value = list.data
    else notify(list.error || 'Could not load the places', false)
  } catch (e) {
    notify(e instanceof Error ? e.message : 'Could not load the places', false)
  } finally {
    loading.value = false
  }
}

function openCreate(parent: Place | null) {
  editing.value = null
  formParent.value = parent
  const types = allowedTypesFor(parent)
  form.value = {
    name: '',
    place_type: types[0] ?? '',
    parent_id: parent?.id ?? null,
    description: null,
    external_id: null,
    is_special: false,
  }
  showForm.value = true
}

function openCreateSpecial() {
  editing.value = null
  formParent.value = null
  form.value = {
    name: '',
    place_type: 'Outside',
    parent_id: null,
    description: null,
    external_id: null,
    is_special: true,
  }
  showForm.value = true
}

function onSpecialToggle() {
  // Flipping the toggle changes which fields apply: clear conflicting state
  // so the user doesn't accidentally submit a parent on a special place.
  if (form.value.is_special) {
    form.value.parent_id = null
    formParent.value = null
  } else {
    const types = allowedTypesFor(null)
    if (!types.includes(form.value.place_type)) {
      form.value.place_type = types[0] ?? ''
    }
  }
}

function openEdit(p: Place) {
  editing.value = p
  formParent.value = p.parent_id ? (places.value.find((x) => x.id === p.parent_id) ?? null) : null
  form.value = {
    name: p.name,
    place_type: p.place_type,
    parent_id: p.parent_id,
    description: p.description,
    external_id: p.external_id,
    is_special: p.is_special,
  }
  showForm.value = true
}

function allowedTypesFor(parent: Place | null): string[] {
  if (!config.value?.types?.length) return []
  if (!parent) return config.value.types.slice()
  const i = config.value.types.indexOf(parent.place_type)
  if (i < 0) return config.value.types.slice()
  return config.value.types.slice(i + 1)
}

async function save() {
  saving.value = true
  let res
  try {
    if (editing.value) {
      res = await placesApi.update(editing.value.id, {
        name: form.value.name.trim(),
        place_type: form.value.place_type,
        parent_id: form.value.parent_id,
        description: form.value.description,
        external_id: form.value.external_id,
        is_special: form.value.is_special,
      })
    } else {
      res = await placesApi.create({
        name: form.value.name.trim(),
        place_type: form.value.place_type,
        parent_id: form.value.parent_id,
        description: form.value.description,
        external_id: form.value.external_id,
        is_special: form.value.is_special,
      })
    }
    if (res.success) {
      notify(editing.value ? 'Place saved' : 'Place created')
      showForm.value = false
      await loadAll()
    } else {
      notify(res.error || 'Failed to save', false)
    }
  } catch (e) {
    notify(e instanceof Error ? e.message : 'Failed to save', false)
  } finally {
    saving.value = false
  }
}

async function onDelete(p: Place) {
  if (!confirm(`Delete "${p.name}"? Attached doors/tools/devices will be detached but kept.`))
    return
  const r = await placesApi.remove(p.id)
  if (r.success) {
    notify('Place deleted')
    await loadAll()
  } else notify(r.error || 'Failed to delete (move/delete children first?)', false)
}

onMounted(loadAll)
</script>
