<template>
  <div class="card bg-base-100 shadow-xl">
    <div class="card-body">
      <h2 class="card-title text-2xl mb-2">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z"
          />
        </svg>
        Email Addresses
      </h2>
      <p class="text-sm text-base-content/70 mb-4">
        Any of these addresses can sign in. The primary address receives account mail and is the one
        on the mailing list. An address must be confirmed before it can become the primary.
      </p>

      <div v-if="successMessage" class="alert alert-success mb-4" role="status">
        <span>{{ successMessage }}</span>
      </div>
      <div v-if="errorMessage" class="alert alert-error mb-4" role="alert">
        <span>{{ errorMessage }}</span>
      </div>

      <div v-if="loading" class="flex justify-center py-4">
        <span class="loading loading-spinner"></span>
      </div>

      <ul v-else class="divide-y divide-base-200 mb-4" data-testid="email-list">
        <li
          v-for="row in emails"
          :key="row.id"
          class="py-3 flex flex-wrap items-center justify-between gap-2"
          :data-email-id="row.id"
        >
          <div class="min-w-0">
            <div class="font-mono break-all">{{ row.email }}</div>
            <div class="text-xs flex gap-2 mt-1">
              <span v-if="row.is_primary" class="badge badge-primary badge-sm">Primary</span>
              <span v-if="row.verified_at" class="badge badge-success badge-sm">Confirmed</span>
              <span v-else class="badge badge-warning badge-sm">Unconfirmed</span>
            </div>
          </div>
          <div class="flex gap-2">
            <button
              v-if="!row.verified_at"
              class="btn btn-ghost btn-xs"
              :disabled="busy"
              @click="resend(row)"
            >
              Resend link
            </button>
            <button
              v-if="!row.is_primary && row.verified_at"
              class="btn btn-outline btn-xs"
              :disabled="busy"
              @click="makePrimary(row)"
            >
              Make primary
            </button>
            <button
              v-if="!row.is_primary"
              class="btn btn-error btn-outline btn-xs"
              :disabled="busy"
              @click="remove(row)"
            >
              Remove
            </button>
          </div>
        </li>
      </ul>

      <form class="space-y-3 max-w-sm" @submit.prevent="add">
        <div class="form-control">
          <label class="label"><span class="label-text">Add an address</span></label>
          <input
            v-model="newEmail"
            type="email"
            class="input input-bordered"
            autocomplete="email"
            required
          />
        </div>
        <div v-if="self" class="form-control">
          <label class="label"><span class="label-text">Current password</span></label>
          <input
            v-model="currentPassword"
            type="password"
            class="input input-bordered"
            autocomplete="current-password"
            required
          />
          <label class="label">
            <span class="label-text-alt text-base-content/60">
              Adding or promoting an address changes where account mail goes, so it needs your
              password.
            </span>
          </label>
        </div>
        <button
          type="submit"
          class="btn btn-primary"
          :disabled="busy || !newEmail || (self && !currentPassword)"
        >
          <span v-if="busy" class="loading loading-spinner loading-sm"></span>
          Add address
        </button>
      </form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, watch } from 'vue'
import { apiClient } from '@/utils/api'
import type { UserEmail } from '@/types'

const props = defineProps<{
  /** Whose addresses. */
  userId: string
  /**
   * True when the viewer is managing their OWN addresses. A self-service add
   * or promotion is a credential change and the server requires the current
   * password (#120/#2); a manager acting on someone else supplies none.
   */
  self: boolean
}>()

const emails = ref<UserEmail[]>([])
const loading = ref(false)
const busy = ref(false)
const newEmail = ref('')
const currentPassword = ref('')
const successMessage = ref('')
const errorMessage = ref('')

function base() {
  return `/users/${props.userId}/emails`
}

function fail(err: unknown, fallback: string) {
  const e = err as { response?: { data?: { error?: string } }; message?: string }
  errorMessage.value = e?.response?.data?.error || e?.message || fallback
}

async function load() {
  loading.value = true
  errorMessage.value = ''
  try {
    const res = await apiClient.get<UserEmail[]>(base())
    if (res.success && res.data) {
      emails.value = res.data
    } else {
      errorMessage.value = res.error || 'Failed to load email addresses.'
    }
  } catch (err) {
    fail(err, 'Failed to load email addresses.')
  } finally {
    loading.value = false
  }
}

async function run(
  action: () => Promise<{ success: boolean; error?: string; message?: string }>,
  done: string
) {
  busy.value = true
  successMessage.value = ''
  errorMessage.value = ''
  try {
    const res = await action()
    if (res.success) {
      successMessage.value = res.message || done
      await load()
      return true
    }
    errorMessage.value = res.error || 'The request was refused.'
  } catch (err) {
    fail(err, 'The request was refused.')
  } finally {
    busy.value = false
  }
  return false
}

async function add() {
  const body: Record<string, string> = { email: newEmail.value.trim() }
  if (props.self) body.current_password = currentPassword.value
  const added = await run(() => apiClient.post(base(), body), 'Address added.')
  if (added) {
    newEmail.value = ''
    currentPassword.value = ''
  }
}

function makePrimary(row: UserEmail) {
  const body: Record<string, string> = {}
  if (props.self) body.current_password = currentPassword.value
  return run(() => apiClient.put(`${base()}/${row.id}/primary`, body), 'Primary address updated.')
}

function remove(row: UserEmail) {
  return run(() => apiClient.delete(`${base()}/${row.id}`), 'Address removed.')
}

function resend(row: UserEmail) {
  return run(() => apiClient.post(`${base()}/${row.id}/resend`), 'Confirmation link sent.')
}

onMounted(load)
watch(() => props.userId, load)
</script>
