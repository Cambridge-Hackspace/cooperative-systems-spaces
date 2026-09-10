<template>
  <div class="rate-tiers mt-4">
    <h4 class="font-semibold mb-1">Rate tiers</h4>
    <p class="text-sm text-base-content/70 mb-2">
      Named alternatives to this tool's default rate. A member assigned a tier is charged that
      tier's rate; unassigned members pay the default above.
    </p>

    <div v-if="error" class="alert alert-error alert-sm mb-2">
      <span>{{ error }}</span>
      <button type="button" class="btn btn-xs btn-ghost" @click="error = ''">Dismiss</button>
    </div>

    <!-- Tiers -->
    <div class="overflow-x-auto mb-3">
      <table class="table table-xs">
        <thead>
          <tr>
            <th>Name</th>
            <th>Flat</th>
            <th>Per min</th>
            <th>Max min</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="t in tiers" :key="t.id">
            <td>{{ t.name }}</td>
            <td>{{ t.flat_fee ?? '—' }}</td>
            <td>{{ t.rate_per_min ?? '—' }}</td>
            <td>{{ t.max_session_minutes ?? '—' }}</td>
            <td class="text-right">
              <button
                type="button"
                class="btn btn-ghost btn-xs text-error"
                @click="removeTier(t.id)"
              >
                Delete
              </button>
            </td>
          </tr>
          <tr v-if="!tiers.length">
            <td colspan="5" class="text-center text-base-content/50">No tiers yet.</td>
          </tr>
        </tbody>
      </table>
    </div>

    <form class="grid gap-2 sm:grid-cols-5 items-end mb-4" @submit.prevent="addTier">
      <label class="form-control">
        <span class="label-text text-xs">Tier name</span>
        <input v-model="tierForm.name" class="input input-bordered input-xs" required />
      </label>
      <label class="form-control">
        <span class="label-text text-xs">Flat fee</span>
        <input v-model="tierForm.flat_fee" class="input input-bordered input-xs" placeholder="—" />
      </label>
      <label class="form-control">
        <span class="label-text text-xs">Rate/min</span>
        <input
          v-model="tierForm.rate_per_min"
          class="input input-bordered input-xs"
          placeholder="—"
        />
      </label>
      <label class="form-control">
        <span class="label-text text-xs">Max min</span>
        <input
          v-model.number="tierForm.max_session_minutes"
          type="number"
          class="input input-bordered input-xs"
        />
      </label>
      <button type="submit" class="btn btn-primary btn-xs">Add tier</button>
    </form>

    <!-- Assignments -->
    <h4 class="font-semibold mb-1">Member assignments</h4>
    <div class="overflow-x-auto mb-2">
      <table class="table table-xs">
        <thead>
          <tr>
            <th>Member</th>
            <th>Tier</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="a in assignments" :key="a.id">
            <td>{{ userName(a.user_id) }}</td>
            <td>{{ tierName(a.tier_id) }}</td>
            <td class="text-right">
              <button
                type="button"
                class="btn btn-ghost btn-xs text-error"
                @click="clear(a.user_id)"
              >
                Remove
              </button>
            </td>
          </tr>
          <tr v-if="!assignments.length">
            <td colspan="3" class="text-center text-base-content/50">
              No members on a non-default tier.
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <form class="grid gap-2 sm:grid-cols-3 items-end" @submit.prevent="assign">
      <label class="form-control">
        <span class="label-text text-xs">Member</span>
        <select v-model="assignForm.user_id" class="select select-bordered select-xs" required>
          <option value="" disabled>— Choose a member —</option>
          <option v-for="u in users" :key="u.id" :value="u.id">{{ u.label }}</option>
        </select>
      </label>
      <label class="form-control">
        <span class="label-text text-xs">Tier</span>
        <select v-model="assignForm.tier_id" class="select select-bordered select-xs" required>
          <option value="" disabled>— Choose a tier —</option>
          <option v-for="t in tiers" :key="t.id" :value="t.id">{{ t.name }}</option>
        </select>
      </label>
      <button type="submit" class="btn btn-primary btn-xs" :disabled="!tiers.length">Assign</button>
    </form>
  </div>
</template>

<script setup lang="ts">
import { reactive, ref } from 'vue'
import { useReloadOnReactivate } from '@/composables/useReloadOnReactivate'
import { tiersApi, userApi } from '@/utils/api'
import type { ToolRateTier, ToolTierAssignment } from '@/types'

const props = defineProps<{ toolId: string }>()

const error = ref('')
const tiers = ref<ToolRateTier[]>([])
const assignments = ref<ToolTierAssignment[]>([])
const users = ref<{ id: string; label: string }[]>([])

const tierForm = reactive({
  name: '',
  flat_fee: '',
  rate_per_min: '',
  max_session_minutes: undefined,
})
const assignForm = reactive({ user_id: '', tier_id: '' })

function tierName(id: string): string {
  return tiers.value.find((t) => t.id === id)?.name ?? '(unknown)'
}
function userName(id: string): string {
  return users.value.find((u) => u.id === id)?.label ?? id
}

async function loadAll() {
  const [t, a, r] = await Promise.all([
    tiersApi.listTiers(props.toolId),
    tiersApi.listAssignments(props.toolId),
    userApi.getAllUsers(),
  ])
  if (t.success && t.data) tiers.value = t.data
  else error.value = t.error || 'Failed to load tiers'
  if (a.success && a.data) assignments.value = a.data
  // The roster shape is loose; extract id + a display label defensively.
  const raw: unknown = r.success ? r.data : []
  const list = Array.isArray(raw)
    ? raw
    : ((raw as { items?: unknown[]; users?: unknown[] })?.items ??
      (raw as { users?: unknown[] })?.users ??
      [])
  users.value = (list as Record<string, unknown>[])
    .filter((u) => typeof u.id === 'string')
    .map((u) => ({
      id: u.id as string,
      label: (u.full_name as string) || (u.username as string) || (u.id as string),
    }))
}
useReloadOnReactivate(loadAll)

async function addTier() {
  const res = await tiersApi.createTier(props.toolId, {
    name: tierForm.name,
    flat_fee: tierForm.flat_fee || null,
    rate_per_min: tierForm.rate_per_min || null,
    max_session_minutes: tierForm.max_session_minutes ?? null,
  })
  if (!res.success) {
    error.value = res.error || 'Failed to create tier'
    return
  }
  tierForm.name = ''
  tierForm.flat_fee = ''
  tierForm.rate_per_min = ''
  tierForm.max_session_minutes = undefined
  await loadAll()
}
async function removeTier(id: string) {
  const res = await tiersApi.removeTier(id)
  if (!res.success) {
    error.value = res.error || 'Failed to delete tier'
    return
  }
  await loadAll()
}
async function assign() {
  const res = await tiersApi.assignTier(assignForm.user_id, props.toolId, assignForm.tier_id)
  if (!res.success) {
    error.value = res.error || 'Failed to assign tier'
    return
  }
  assignForm.user_id = ''
  assignForm.tier_id = ''
  await loadAll()
}
async function clear(userId: string) {
  const res = await tiersApi.clearTier(userId, props.toolId)
  if (!res.success) {
    error.value = res.error || 'Failed to clear assignment'
    return
  }
  await loadAll()
}
</script>
