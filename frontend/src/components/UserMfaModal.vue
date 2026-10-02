<template>
  <dialog class="modal modal-open" role="dialog" aria-labelledby="mfa-admin-title">
    <div class="modal-box max-w-xl">
      <h3 id="mfa-admin-title" class="font-bold text-lg">
        Second factors for <span class="font-mono">{{ username }}</span>
      </h3>
      <p class="text-sm text-base-content/70 mt-1">
        Remove one factor when a member lost one device; reset everything only for a full lockout.
        Removing is recorded with the factor's label.
      </p>

      <div v-if="errorMessage" class="alert alert-error mt-4" role="alert">
        <span>{{ errorMessage }}</span>
      </div>

      <div v-if="loading" class="flex justify-center py-6">
        <span class="loading loading-spinner"></span>
      </div>

      <template v-else-if="view">
        <h4 class="font-medium mt-4">Authenticator apps</h4>
        <p v-if="view.totp.length === 0" class="text-sm text-base-content/60">None.</p>
        <ul v-else class="divide-y divide-base-200" data-testid="totp-list">
          <li
            v-for="t in view.totp"
            :key="t.id"
            class="py-2 flex items-center justify-between gap-2"
            :data-totp-id="t.id"
          >
            <div>
              <div>{{ t.label }}</div>
              <div class="text-xs text-base-content/60">added {{ fmt(t.created_at) }}</div>
            </div>
            <button
              class="btn btn-xs btn-outline btn-error"
              :disabled="busy"
              @click="removeTotp(t)"
            >
              Remove
            </button>
          </li>
        </ul>

        <h4 class="font-medium mt-4">Security keys</h4>
        <p v-if="view.webauthn.length === 0" class="text-sm text-base-content/60">None.</p>
        <ul v-else class="divide-y divide-base-200" data-testid="webauthn-list">
          <li
            v-for="k in view.webauthn"
            :key="k.id"
            class="py-2 flex items-center justify-between gap-2"
            :data-webauthn-id="k.id"
          >
            <div>
              <div>{{ k.label }}</div>
              <div class="text-xs text-base-content/60">
                added {{ fmt(k.created_at) }}
                <span v-if="k.last_used_at">· last used {{ fmt(k.last_used_at) }}</span>
              </div>
            </div>
            <button
              class="btn btn-xs btn-outline btn-error"
              :disabled="busy"
              @click="removeWebauthn(k)"
            >
              Remove
            </button>
          </li>
        </ul>

        <p class="text-sm mt-4">
          Unused recovery codes: <strong>{{ view.recovery_codes_remaining }}</strong>
          <span class="text-base-content/60">
            · enrolled: {{ view.mfa_enrolled_at ? 'yes' : 'no' }}</span
          >
        </p>
      </template>

      <div class="modal-action">
        <button class="btn btn-ghost" :disabled="busy" @click="$emit('close')">Close</button>
        <button class="btn btn-error btn-outline" :disabled="busy || !view" @click="resetAll">
          Reset everything
        </button>
      </div>
    </div>
  </dialog>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { adminApi } from '@/utils/api'
import type { AdminMfaView, AdminTotpFactor, AdminWebauthnFactor } from '@/types'

const props = defineProps<{ userId: string; username: string }>()
const emit = defineEmits<{
  close: []
  /** The user's enrolment state after a change, so the roster badge can follow. */
  changed: [enrolled: boolean]
}>()

const view = ref<AdminMfaView | null>(null)
const loading = ref(false)
const busy = ref(false)
const errorMessage = ref('')

function fmt(iso: string) {
  return new Date(iso).toLocaleDateString()
}

function apply(res: { success: boolean; error?: string; data?: AdminMfaView }, fallback: string) {
  if (res.success && res.data) {
    view.value = res.data
    emit('changed', !!res.data.mfa_enrolled_at)
  } else {
    errorMessage.value = res.error || fallback
  }
}

async function load() {
  loading.value = true
  errorMessage.value = ''
  try {
    const res = await adminApi.listUserMfa(props.userId)
    if (res.success && res.data) view.value = res.data
    else errorMessage.value = res.error || 'Could not load second factors.'
  } catch (err) {
    const e = err as { message?: string }
    errorMessage.value = e?.message || 'Could not load second factors.'
  } finally {
    loading.value = false
  }
}

async function removeTotp(t: AdminTotpFactor) {
  if (!window.confirm(`Remove the authenticator "${t.label}"?`)) return
  busy.value = true
  errorMessage.value = ''
  try {
    apply(await adminApi.removeUserTotp(props.userId, t.id), 'Could not remove the authenticator.')
  } finally {
    busy.value = false
  }
}

async function removeWebauthn(k: AdminWebauthnFactor) {
  if (!window.confirm(`Remove the security key "${k.label}"?`)) return
  busy.value = true
  errorMessage.value = ''
  try {
    apply(await adminApi.removeUserWebauthn(props.userId, k.id), 'Could not remove the key.')
  } finally {
    busy.value = false
  }
}

async function resetAll() {
  if (
    !window.confirm(
      `Reset MFA for @${props.username}? Every authenticator, security key and recovery code is removed; they sign in with just their password until they re-enrol.`
    )
  )
    return
  busy.value = true
  errorMessage.value = ''
  try {
    const res = await adminApi.resetUserMfa(props.userId)
    if (res.success) {
      emit('changed', false)
      await load()
    } else {
      errorMessage.value = res.error || 'Could not reset MFA.'
    }
  } finally {
    busy.value = false
  }
}

onMounted(load)
</script>
