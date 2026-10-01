<template>
  <dialog class="modal modal-open" role="dialog" aria-labelledby="merge-title">
    <div class="modal-box max-w-2xl">
      <h3 id="merge-title" class="font-bold text-lg">
        Merge <span class="font-mono">{{ absorbed.username }}</span> into another account
      </h3>
      <p class="text-sm text-base-content/70 mt-1">
        Everything this account holds (cards, balance, roles, training, addresses, history) moves to
        the account you choose, and this account is deleted. The survivor keeps its username,
        password and profile.
      </p>

      <div v-if="errorMessage" class="alert alert-error mt-4" role="alert">
        <span>{{ errorMessage }}</span>
      </div>

      <!-- Step 1: pick the survivor -->
      <div v-if="!plan" class="mt-4 space-y-3">
        <div class="form-control">
          <label class="label" for="merge-survivor-search">
            <span class="label-text">Survivor (the account that stays)</span>
          </label>
          <input
            id="merge-survivor-search"
            v-model="search"
            type="text"
            class="input input-bordered input-sm"
            placeholder="Search by username, name or email"
          />
        </div>
        <ul class="menu bg-base-200 rounded-box max-h-60 overflow-y-auto" data-testid="candidates">
          <li v-for="c in matches" :key="c.id">
            <a
              :class="{ active: survivorId === c.id }"
              :data-candidate-id="c.id"
              @click.prevent="survivorId = c.id"
            >
              <span class="font-mono">{{ c.username }}</span>
              <span class="text-base-content/70">{{ c.full_name }} · {{ c.email }}</span>
            </a>
          </li>
          <li v-if="matches.length === 0" class="disabled"><a>No matching accounts</a></li>
        </ul>
        <div class="modal-action">
          <button class="btn btn-ghost" :disabled="busy" @click="$emit('close')">Cancel</button>
          <button class="btn btn-primary" :disabled="!survivorId || busy" @click="preview">
            <span v-if="busy" class="loading loading-spinner loading-sm"></span>
            Preview merge
          </button>
        </div>
      </div>

      <!-- Step 2: the plan, every warning acknowledged by hand -->
      <div v-else class="mt-4 space-y-4">
        <div class="grid grid-cols-2 gap-3 text-sm">
          <div class="bg-base-200 rounded-lg p-3">
            <div class="text-xs uppercase text-base-content/60">Survivor</div>
            <div class="font-mono">{{ plan.survivor.username }}</div>
            <div>{{ plan.survivor.full_name }}</div>
            <div class="text-base-content/70">{{ plan.survivor.email }}</div>
          </div>
          <div class="bg-base-200 rounded-lg p-3">
            <div class="text-xs uppercase text-base-content/60">Absorbed (deleted)</div>
            <div class="font-mono">{{ plan.absorbed.username }}</div>
            <div>{{ plan.absorbed.full_name }}</div>
            <div class="text-base-content/70">{{ plan.absorbed.email }}</div>
          </div>
        </div>

        <div>
          <div class="font-medium mb-1">What moves</div>
          <table class="table table-xs" data-testid="moves">
            <tbody>
              <tr v-for="(n, key) in plan.moves" :key="key">
                <td class="font-mono">{{ key }}</td>
                <td class="text-right">{{ n }}</td>
              </tr>
              <tr v-if="Object.keys(plan.moves).length === 0">
                <td colspan="2" class="text-base-content/60">Nothing references this account.</td>
              </tr>
            </tbody>
          </table>
        </div>

        <div>
          <div class="font-medium mb-1">
            Warnings
            <span class="text-xs text-base-content/60">(every one must be acknowledged)</span>
          </div>
          <ul class="space-y-2" data-testid="warnings">
            <li
              v-for="w in plan.warnings"
              :key="w.code"
              class="flex items-start gap-2 bg-warning/10 rounded-lg p-2"
              :data-warning-code="w.code"
            >
              <input
                :id="`ack-${w.code}`"
                type="checkbox"
                class="checkbox checkbox-warning checkbox-sm mt-0.5"
                :checked="acknowledged.has(w.code)"
                @change="toggle(w.code)"
              />
              <label :for="`ack-${w.code}`" class="text-sm">
                <span class="font-mono text-xs text-base-content/60">{{ w.code }}</span>
                <div>{{ w.detail }}</div>
              </label>
            </li>
          </ul>
        </div>

        <div class="modal-action">
          <button class="btn btn-ghost" :disabled="busy" @click="plan = null">Back</button>
          <button class="btn btn-ghost" :disabled="busy" @click="$emit('close')">Cancel</button>
          <button class="btn btn-error" :disabled="!allAcknowledged || busy" @click="commit">
            <span v-if="busy" class="loading loading-spinner loading-sm"></span>
            Merge and delete {{ plan.absorbed.username }}
          </button>
        </div>
      </div>
    </div>
  </dialog>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { adminApi } from '@/utils/api'
import type { MergeOutcome, MergeParty, MergePlan } from '@/types'

const props = defineProps<{
  /** The account that will be folded in and deleted. */
  absorbed: MergeParty
  /** Accounts the administrator may pick as the survivor. */
  candidates: MergeParty[]
}>()

const emit = defineEmits<{
  close: []
  merged: [outcome: MergeOutcome]
}>()

const search = ref('')
const survivorId = ref<string | null>(null)
const plan = ref<MergePlan | null>(null)
const acknowledged = ref(new Set<string>())
const busy = ref(false)
const errorMessage = ref('')

const matches = computed(() => {
  const q = search.value.trim().toLowerCase()
  return props.candidates
    .filter((c) => c.id !== props.absorbed.id)
    .filter(
      (c) =>
        !q ||
        c.username.toLowerCase().includes(q) ||
        c.full_name.toLowerCase().includes(q) ||
        c.email.toLowerCase().includes(q)
    )
    .slice(0, 50)
})

const allAcknowledged = computed(
  () => !!plan.value && plan.value.warnings.every((w) => acknowledged.value.has(w.code))
)

function toggle(code: string) {
  const next = new Set(acknowledged.value)
  if (next.has(code)) next.delete(code)
  else next.add(code)
  acknowledged.value = next
}

function fail(err: unknown, fallback: string) {
  const e = err as { response?: { data?: { error?: string } }; message?: string }
  errorMessage.value = e?.response?.data?.error || e?.message || fallback
}

async function preview() {
  if (!survivorId.value) return
  busy.value = true
  errorMessage.value = ''
  try {
    const res = await adminApi.previewMerge(survivorId.value, props.absorbed.id)
    if (res.success && res.data) {
      plan.value = res.data
      acknowledged.value = new Set()
    } else {
      errorMessage.value = res.error || 'The preview was refused.'
    }
  } catch (err) {
    fail(err, 'The preview was refused.')
  } finally {
    busy.value = false
  }
}

async function commit() {
  if (!plan.value || !allAcknowledged.value) return
  busy.value = true
  errorMessage.value = ''
  try {
    const res = await adminApi.mergeUsers(
      plan.value.survivor.id,
      props.absorbed.id,
      plan.value.warnings.map((w) => w.code)
    )
    if (res.success && res.data) {
      emit('merged', res.data)
    } else {
      // A 409 here means the warning set changed since the preview: show the
      // server's current set rather than retrying blind.
      errorMessage.value = res.error || 'The merge was refused.'
      plan.value = null
    }
  } catch (err) {
    fail(err, 'The merge was refused.')
  } finally {
    busy.value = false
  }
}
</script>
