<template>
  <div class="card bg-base-100 shadow-xl">
    <div class="card-body">
      <div class="flex items-center justify-between gap-2">
        <h2 class="card-title">Possible duplicates</h2>
        <button class="btn btn-ghost btn-sm" :disabled="loading" @click="load">
          <span v-if="loading" class="loading loading-spinner loading-xs"></span>
          Refresh
        </button>
      </div>
      <p class="text-sm text-base-content/70">
        Pairs of accounts that may be the same person, with the reason each was proposed. Nothing
        here is decided: merging opens the preview, which lists what would move and what to
        acknowledge.
      </p>

      <div v-if="errorMessage" class="alert alert-error mt-2" role="alert">
        <span>{{ errorMessage }}</span>
      </div>

      <p v-if="!loading && !errorMessage && candidates.length === 0" class="text-sm mt-2">
        No likely duplicates found.
      </p>

      <table v-if="candidates.length" class="table table-sm mt-2" data-testid="duplicates">
        <thead>
          <tr>
            <th>Older account</th>
            <th>Newer account</th>
            <th>Why</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="c in candidates"
            :key="`${c.a.id}:${c.b.id}`"
            :data-pair="`${c.a.id}:${c.b.id}`"
          >
            <td>
              <div class="font-mono">{{ c.a.username }}</div>
              <div class="text-xs">{{ c.a.full_name }} · {{ c.a.email }}</div>
            </td>
            <td>
              <div class="font-mono">{{ c.b.username }}</div>
              <div class="text-xs">{{ c.b.full_name }} · {{ c.b.email }}</div>
            </td>
            <td>
              <span
                v-for="r in c.reasons"
                :key="r"
                class="badge badge-outline badge-sm mr-1"
                :data-reason="r"
              >
                {{ reasonLabel(r) }}
              </span>
            </td>
            <td class="text-right whitespace-nowrap">
              <button class="btn btn-xs btn-outline" @click="open(c.b, c.a)">
                Merge newer into older
              </button>
              <button class="btn btn-xs btn-ghost ml-1" @click="open(c.a, c.b)">
                Merge older into newer
              </button>
            </td>
          </tr>
        </tbody>
      </table>

      <UserMergeModal
        v-if="pending"
        :absorbed="pending.absorbed"
        :candidates="[pending.survivor]"
        @close="pending = null"
        @merged="onMerged"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { adminApi } from '@/utils/api'
import UserMergeModal from '@/components/UserMergeModal.vue'
import type { DuplicateCandidate, MergeOutcome, MergeParty } from '@/types'

const emit = defineEmits<{
  merged: [message: string]
  error: [message: string]
}>()

const candidates = ref<DuplicateCandidate[]>([])
const loading = ref(false)
const errorMessage = ref('')
const pending = ref<{ absorbed: MergeParty; survivor: MergeParty } | null>(null)

const LABELS: Record<string, string> = {
  same_name: 'same name',
  email_alias: 'same mailbox',
  shared_card: 'shared card',
}
function reasonLabel(r: string) {
  return LABELS[r] ?? r
}

async function load() {
  loading.value = true
  errorMessage.value = ''
  try {
    const res = await adminApi.listDuplicateCandidates()
    if (res.success && res.data) {
      candidates.value = res.data
    } else {
      errorMessage.value = res.error || 'Could not load the duplicates report.'
      emit('error', errorMessage.value)
    }
  } catch (err) {
    const e = err as { message?: string }
    errorMessage.value = e?.message || 'Could not load the duplicates report.'
    emit('error', errorMessage.value)
  } finally {
    loading.value = false
  }
}

function open(absorbed: MergeParty, survivor: MergeParty) {
  pending.value = { absorbed, survivor }
}

async function onMerged(outcome: MergeOutcome) {
  const absorbed = pending.value?.absorbed.username ?? outcome.absorbed_id
  const survivor = pending.value?.survivor.username ?? outcome.survivor_id
  pending.value = null
  emit('merged', `${absorbed} merged into ${survivor}.`)
  await load()
}

onMounted(load)
</script>
