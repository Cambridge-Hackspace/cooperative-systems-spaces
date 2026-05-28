<template>
  <div class="container mx-auto px-4 py-8 max-w-2xl">
    <div class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/" class="link">Home</router-link></li>
        <li><router-link to="/profile" class="link">Profile</router-link></li>
        <li>Two-factor</li>
      </ul>
    </div>

    <h1 class="text-3xl font-bold mb-2">Two-factor authentication</h1>
    <p class="text-base-content/70 mb-6">
      Add a second factor so a stolen password isn't enough to sign in.
    </p>

    <div v-if="flash" class="alert mb-4" :class="flashOk ? 'alert-success' : 'alert-error'">
      <span>{{ flash }}</span>
      <button class="btn btn-ghost btn-xs" @click="flash = ''">✕</button>
    </div>

    <div v-if="!status" class="text-center py-12">
      <span class="loading loading-spinner loading-lg"></span>
    </div>

    <template v-else>
      <div v-if="!status.enabled" class="alert alert-warning mb-6">
        <span>MFA is disabled in server configuration. Ask an admin to enable it in <code>[auth.mfa]</code>.</span>
      </div>

      <div v-if="status.must_enroll" class="alert alert-info mb-6">
        <span>Your role requires MFA — enroll at least one method below.</span>
      </div>

      <!-- ===== TOTP ===== -->
      <section class="card bg-base-100 shadow-md mb-6">
        <div class="card-body">
          <h2 class="card-title">Authenticator app (TOTP)</h2>
          <p class="text-sm text-base-content/70">
            Use Aegis, 1Password, Google Authenticator, etc.
          </p>

          <div v-if="status.totp_enrolled && !totpSetup" class="flex items-center gap-3">
            <span class="badge badge-success">Enrolled</span>
            <button class="btn btn-error btn-sm btn-outline" @click="disableTotp">Disable</button>
          </div>

          <div v-else>
            <button v-if="!totpSetup" class="btn btn-primary btn-sm w-fit" :disabled="!status.enabled || busy" @click="beginTotp">
              Set up authenticator
            </button>

            <div v-else class="mt-2 space-y-3">
              <div class="flex flex-col items-center gap-2">
                <img v-if="qrDataUrl" :src="qrDataUrl" alt="otpauth QR" class="border border-base-300 rounded p-2 bg-white" />
                <code class="text-xs break-all bg-base-200 p-2 rounded">{{ totpSetup.secret_base32 }}</code>
                <p class="text-xs text-base-content/60">
                  Scan the QR code, or enter the secret manually if your app needs it.
                </p>
              </div>

              <label class="label"><span class="label-text">Enter the current 6-digit code to confirm</span></label>
              <input
                v-model="totpConfirmCode"
                type="text"
                inputmode="numeric"
                maxlength="6"
                class="input input-bordered w-full font-mono text-center tracking-widest text-xl"
                placeholder="123 456"
                @keydown.enter="confirmTotp"
              />
              <div class="flex justify-end gap-2">
                <button class="btn btn-ghost btn-sm" @click="cancelTotp">Cancel</button>
                <button class="btn btn-primary btn-sm" :disabled="busy || totpConfirmCode.length < 6" @click="confirmTotp">
                  Confirm
                </button>
              </div>
            </div>
          </div>
        </div>
      </section>

      <!-- ===== WebAuthn ===== -->
      <section class="card bg-base-100 shadow-md mb-6">
        <div class="card-body">
          <h2 class="card-title">Security keys</h2>
          <p class="text-sm text-base-content/70">
            Yubikey, Touch ID, Windows Hello, or any FIDO2 / passkey authenticator.
          </p>

          <table v-if="webauthn.length" class="table table-sm mt-2">
            <thead>
              <tr><th>Label</th><th>Added</th><th>Last used</th><th></th></tr>
            </thead>
            <tbody>
              <tr v-for="c in webauthn" :key="c.id">
                <td class="font-medium">{{ c.label }}</td>
                <td class="text-xs">{{ fmt(c.created_at) }}</td>
                <td class="text-xs">{{ c.last_used_at ? fmt(c.last_used_at) : '—' }}</td>
                <td class="text-right">
                  <button class="btn btn-ghost btn-xs text-error" @click="removeWebauthn(c.id)">Remove</button>
                </td>
              </tr>
            </tbody>
          </table>

          <div class="flex items-end gap-2 mt-3">
            <div class="form-control flex-1">
              <label class="label py-1"><span class="label-text">Label for this key</span></label>
              <input v-model="newKeyLabel" type="text" class="input input-bordered input-sm" placeholder="Yubikey 5C" />
            </div>
            <button class="btn btn-primary btn-sm" :disabled="!status.enabled || busy || !newKeyLabel.trim()" @click="addWebauthn">
              Add security key
            </button>
          </div>
        </div>
      </section>

      <!-- ===== Recovery codes ===== -->
      <section class="card bg-base-100 shadow-md">
        <div class="card-body">
          <h2 class="card-title">Recovery codes</h2>
          <p class="text-sm text-base-content/70">
            One-time codes for when you lose access to your other factors. Each can be used once.
          </p>
          <p class="text-sm">Unused codes remaining: <strong>{{ status.recovery_codes_remaining }}</strong></p>

          <div v-if="freshRecovery.length" class="mt-3">
            <div class="alert alert-warning mb-2">
              <span>Save these somewhere safe. They will not be shown again.</span>
            </div>
            <ul class="grid grid-cols-2 gap-1 font-mono text-sm bg-base-200 p-3 rounded">
              <li v-for="code in freshRecovery" :key="code">{{ code }}</li>
            </ul>
          </div>

          <button class="btn btn-outline btn-sm w-fit mt-3" :disabled="!status.enabled || busy" @click="regenRecovery">
            {{ status.recovery_codes_remaining > 0 ? 'Regenerate recovery codes' : 'Generate recovery codes' }}
          </button>
        </div>
      </section>
    </template>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import QRCode from 'qrcode'
import { create as webauthnCreate } from '@github/webauthn-json'
import { mfaApi } from '@/utils/api'
import type { MfaStatus, MfaTotpSetup, MfaWebauthnCredential } from '@/types'

const status = ref<MfaStatus | null>(null)
const webauthn = ref<MfaWebauthnCredential[]>([])
const busy = ref(false)
const flash = ref('')
const flashOk = ref(true)

const totpSetup = ref<MfaTotpSetup | null>(null)
const totpConfirmCode = ref('')
const qrDataUrl = ref<string>('')

const newKeyLabel = ref('')
const freshRecovery = ref<string[]>([])

function notify(message: string, ok = true) {
  flash.value = message
  flashOk.value = ok
  setTimeout(() => { if (flash.value === message) flash.value = '' }, 5000)
}
function fmt(iso: string) { return new Date(iso).toLocaleString() }

async function loadAll() {
  const [s, w] = await Promise.all([mfaApi.status(), mfaApi.listWebauthn()])
  if (s.success && s.data) status.value = s.data
  if (w.success && w.data) webauthn.value = w.data
}

// Regenerate the QR data URL whenever a new TOTP setup arrives.
watch(totpSetup, async (v) => {
  if (!v) { qrDataUrl.value = ''; return }
  try {
    qrDataUrl.value = await QRCode.toDataURL(v.otpauth_uri, { margin: 1, width: 220 })
  } catch (e: any) {
    notify('Failed to render QR: ' + (e?.message || ''), false)
  }
})

async function beginTotp() {
  busy.value = true
  const r = await mfaApi.totpSetup()
  busy.value = false
  if (r.success && r.data) {
    totpSetup.value = r.data
    totpConfirmCode.value = ''
  } else notify(r.error || 'Failed to start TOTP setup', false)
}

async function confirmTotp() {
  busy.value = true
  const r = await mfaApi.totpConfirm(totpConfirmCode.value.trim())
  busy.value = false
  if (r.success && r.data) {
    freshRecovery.value = r.data.recovery_codes
    totpSetup.value = null
    totpConfirmCode.value = ''
    notify('TOTP enabled.')
    await loadAll()
  } else notify(r.error || 'Invalid code', false)
}

function cancelTotp() {
  totpSetup.value = null
  totpConfirmCode.value = ''
}

async function disableTotp() {
  if (!confirm('Disable authenticator-app login?')) return
  const r = await mfaApi.totpDisable()
  if (r.success) { notify('TOTP disabled.'); await loadAll() }
  else notify(r.error || 'Failed', false)
}

async function addWebauthn() {
  busy.value = true
  try {
    const begin = await mfaApi.webauthnRegisterBegin(newKeyLabel.value.trim())
    if (!begin.success || !begin.data) {
      notify(begin.error || 'Failed to start key registration', false); return
    }
    const credential = await webauthnCreate(begin.data.options as any)
    const finish = await mfaApi.webauthnRegisterFinish(begin.data.challenge_token, credential)
    if (finish.success) {
      newKeyLabel.value = ''
      notify('Security key added.')
      await loadAll()
    } else notify(finish.error || 'Registration rejected', false)
  } catch (e: any) {
    notify(e?.message || 'WebAuthn ceremony failed', false)
  } finally {
    busy.value = false
  }
}

async function removeWebauthn(id: string) {
  if (!confirm('Remove this security key?')) return
  const r = await mfaApi.webauthnRemove(id)
  if (r.success) { notify('Removed.'); await loadAll() }
  else notify(r.error || 'Failed', false)
}

async function regenRecovery() {
  if (!confirm('Generate a fresh set of recovery codes? Any existing unused codes will be invalidated.')) return
  busy.value = true
  const r = await mfaApi.regenerateRecoveryCodes()
  busy.value = false
  if (r.success && r.data) {
    freshRecovery.value = r.data.recovery_codes
    notify('New recovery codes generated.')
    await loadAll()
  } else notify(r.error || 'Failed', false)
}

onMounted(loadAll)
</script>
