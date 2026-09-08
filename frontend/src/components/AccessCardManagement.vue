<template>
  <div class="space-y-4">
    <div v-if="error" class="alert alert-error text-sm">
      <span>{{ error }}</span>
    </div>

    <!-- Issue a new card -->
    <form class="flex gap-2 items-end" @submit.prevent="issue">
      <div class="form-control grow">
        <label class="label py-1"><span class="label-text">New card code</span></label>
        <input
          v-model="newCode"
          type="text"
          placeholder="e.g. 2A-9E-7B-92"
          class="input input-bordered input-sm w-full"
          :disabled="busy"
        />
      </div>
      <button type="submit" class="btn btn-primary btn-sm" :disabled="busy || !newCode.trim()">
        Issue card
      </button>
    </form>

    <div v-if="loading" class="flex items-center gap-2 text-sm">
      <span class="loading loading-spinner loading-sm"></span> Loading cards…
    </div>

    <p v-else-if="cards.length === 0" class="text-sm text-base-content/60">
      This member holds no cards.
    </p>

    <div v-else class="overflow-x-auto">
      <table class="table table-sm">
        <thead>
          <tr>
            <th>Code</th>
            <th>Status</th>
            <th>Last used</th>
            <th class="text-right">Actions</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="card in cards" :key="card.id">
            <td class="font-mono">{{ card.code }}</td>
            <td>
              <span class="badge badge-sm" :class="statusClass(card.status)">{{
                card.status
              }}</span>
              <span
                v-if="card.disabled_reason && card.status === CardStatus.Disabled"
                class="text-xs text-base-content/50 ml-1"
                >{{ card.disabled_reason }}</span
              >
            </td>
            <td class="text-sm">{{ formatDate(card.last_used_at) }}</td>
            <td class="text-right whitespace-nowrap">
              <template v-if="card.status === CardStatus.Active">
                <button class="btn btn-ghost btn-xs" :disabled="busy" @click="disable(card)">
                  Disable
                </button>
                <button class="btn btn-ghost btn-xs" :disabled="busy" @click="release(card)">
                  Release
                </button>
              </template>
              <button
                v-else-if="card.status === CardStatus.Disabled"
                class="btn btn-ghost btn-xs"
                :disabled="busy"
                @click="release(card)"
              >
                Release
              </button>
              <span v-else class="text-xs text-base-content/40">—</span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { apiClient } from '@/utils/api'
import { CardStatus } from '@/types'
import type { Card } from '@/types'

const props = defineProps<{ userId: string }>()

const cards = ref<Card[]>([])
const loading = ref(false)
const busy = ref(false)
const error = ref<string | null>(null)
const newCode = ref('')

async function load() {
  loading.value = true
  error.value = null
  try {
    const res = await apiClient.get<Card[]>(`/cards/user/${props.userId}`)
    if (res.success && res.data) {
      cards.value = res.data
    } else {
      throw new Error(res.error || 'Failed to load cards')
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to load cards'
  } finally {
    loading.value = false
  }
}

async function issue() {
  const code = newCode.value.trim()
  if (!code) return
  busy.value = true
  error.value = null
  try {
    const res = await apiClient.post<Card>(`/cards/user/${props.userId}`, { code })
    if (!res.success) throw new Error(res.error || 'Failed to issue card')
    newCode.value = ''
    await load()
  } catch (err: any) {
    error.value = err.message || 'Failed to issue card'
  } finally {
    busy.value = false
  }
}

async function disable(card: Card) {
  const reason = window.prompt(`Disable card ${card.code}? Optional reason:`, '') ?? undefined
  await act(
    () => apiClient.post<Card>(`/cards/${card.id}/disable`, { reason }),
    'Failed to disable card'
  )
}

async function release(card: Card) {
  if (!window.confirm(`Release card ${card.code} back to the pool? The member loses access.`))
    return
  await act(() => apiClient.post<Card>(`/cards/${card.id}/release`, {}), 'Failed to release card')
}

async function act(call: () => Promise<{ success: boolean; error?: string }>, fallback: string) {
  busy.value = true
  error.value = null
  try {
    const res = await call()
    if (!res.success) throw new Error(res.error || fallback)
    await load()
  } catch (err: any) {
    error.value = err.message || fallback
  } finally {
    busy.value = false
  }
}

function statusClass(status: CardStatus) {
  switch (status) {
    case CardStatus.Active:
      return 'badge-success'
    case CardStatus.Disabled:
      return 'badge-warning'
    case CardStatus.Released:
      return 'badge-ghost'
    default:
      return 'badge-ghost'
  }
}

function formatDate(value?: string | null) {
  if (!value) return 'never'
  return new Date(value).toLocaleString()
}

onMounted(load)
watch(
  () => props.userId,
  () => load()
)
</script>
