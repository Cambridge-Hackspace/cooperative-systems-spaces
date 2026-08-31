<template>
  <div class="min-h-screen flex items-center justify-center bg-base-200">
    <div class="card w-full max-w-sm shadow-2xl bg-base-100">
      <div class="card-body">
        <h2 class="card-title justify-center text-2xl mb-6">
          {{ authStore.pendingMfa ? 'Two-factor verification' : 'Login' }}
        </h2>

        <div v-if="authStore.error" class="alert alert-error mb-4">
          <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
            />
          </svg>
          <span>{{ authStore.error }}</span>
        </div>

        <!-- ===== Password step ===== -->
        <form v-if="!authStore.pendingMfa" @submit.prevent="handleLogin">
          <!--
            `for` / `id`, not a bare label beside an input.
            A <label> with no `for` whose input is a *sibling* rather than a
            child is not associated with anything: a screen reader announces an
            unlabeled text field, and clicking the label does not focus it.
            Both fields on the only form every user must complete were like
            this.
            Found because the browser tier addresses these fields by their
            label, which is the same thing assistive technology does -- so the
            spec's selector is the accessibility assertion rather than a
            separate one somebody has to remember to write.
          -->
          <div class="form-control">
            <label class="label" for="login-username">
              <span class="label-text">Username or Email</span>
            </label>
            <input
              id="login-username"
              v-model="credentials.username_or_email"
              type="text"
              placeholder="Enter username or email"
              class="input input-bordered"
              :disabled="authStore.isLoading"
              required
            />
          </div>

          <div class="form-control">
            <label class="label" for="login-password">
              <span class="label-text">Password</span>
            </label>
            <input
              id="login-password"
              v-model="credentials.password"
              type="password"
              placeholder="Enter password"
              class="input input-bordered"
              :disabled="authStore.isLoading"
              required
            />
            <label class="label">
              <a href="#" class="label-text-alt link link-hover">Forgot password?</a>
            </label>
          </div>

          <div class="form-control mt-6">
            <button type="submit" class="btn btn-primary" :disabled="authStore.isLoading">
              <span v-if="authStore.isLoading" class="loading loading-spinner loading-sm"></span>
              <span v-else>Login</span>
            </button>
          </div>
        </form>

        <!-- ===== MFA challenge step ===== -->
        <div v-else class="space-y-4">
          <div role="tablist" class="tabs tabs-boxed">
            <a
              v-if="methodAvailable('totp')"
              role="tab"
              class="tab"
              :class="{ 'tab-active': method === 'totp' }"
              @click="method = 'totp'"
              >Code</a
            >
            <a
              v-if="methodAvailable('webauthn')"
              role="tab"
              class="tab"
              :class="{ 'tab-active': method === 'webauthn' }"
              @click="method = 'webauthn'"
              >Security key</a
            >
            <a
              v-if="methodAvailable('recovery')"
              role="tab"
              class="tab"
              :class="{ 'tab-active': method === 'recovery' }"
              @click="method = 'recovery'"
              >Recovery</a
            >
          </div>

          <div v-if="method === 'totp'">
            <label class="label" for="mfa-totp-code"
              ><span class="label-text">6-digit code from your authenticator</span></label
            >
            <input
              id="mfa-totp-code"
              v-model="totpCode"
              type="text"
              inputmode="numeric"
              autocomplete="one-time-code"
              maxlength="6"
              class="input input-bordered w-full font-mono tracking-widest text-center text-xl"
              placeholder="123 456"
              :disabled="mfaBusy"
              @keydown.enter="submitTotp"
            />
            <button
              class="btn btn-primary w-full mt-3"
              :disabled="mfaBusy || totpCode.length < 6"
              @click="submitTotp"
            >
              <span v-if="mfaBusy" class="loading loading-spinner loading-sm"></span>
              <span v-else>Verify</span>
            </button>
          </div>

          <div v-else-if="method === 'webauthn'">
            <p class="text-sm text-base-content/70">
              Insert and tap your security key (or use Touch ID / Windows Hello).
            </p>
            <button class="btn btn-primary w-full mt-3" :disabled="mfaBusy" @click="submitWebauthn">
              <span v-if="mfaBusy" class="loading loading-spinner loading-sm"></span>
              <span v-else>Use security key</span>
            </button>
          </div>

          <div v-else-if="method === 'recovery'">
            <label class="label" for="mfa-recovery-code"
              ><span class="label-text">One of your recovery codes</span></label
            >
            <input
              id="mfa-recovery-code"
              v-model="recoveryCode"
              type="text"
              autocomplete="one-time-code"
              class="input input-bordered w-full font-mono"
              placeholder="XXXX-XXXX-XXXX"
              :disabled="mfaBusy"
              @keydown.enter="submitRecovery"
            />
            <button
              class="btn btn-primary w-full mt-3"
              :disabled="mfaBusy || !recoveryCode.trim()"
              @click="submitRecovery"
            >
              <span v-if="mfaBusy" class="loading loading-spinner loading-sm"></span>
              <span v-else>Verify</span>
            </button>
          </div>

          <button class="btn btn-ghost btn-sm w-full" @click="authStore.cancelMfa()">
            Use a different account
          </button>
        </div>

        <div v-if="!authStore.pendingMfa" class="divider">OR</div>
        <div v-if="!authStore.pendingMfa" class="text-center">
          <p class="text-sm">
            Don't have an account?
            <router-link to="/register" class="link link-primary">Register here</router-link>
          </p>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { mfaApi } from '@/utils/api'
import { get as webauthnGet } from '@github/webauthn-json'
import type { LoginRequest } from '@/types'

const router = useRouter()
const authStore = useAuthStore()

const credentials = ref<LoginRequest>({ username_or_email: '', password: '' })

// MFA challenge state
const method = ref<'totp' | 'webauthn' | 'recovery'>('totp')
const totpCode = ref('')
const recoveryCode = ref('')
const mfaBusy = ref(false)

function methodAvailable(m: 'totp' | 'webauthn' | 'recovery') {
  return authStore.pendingMfa?.methods.includes(m) ?? false
}

// When a challenge arrives, pick the first available method as the default tab.
watch(
  () => authStore.pendingMfa,
  (c) => {
    if (!c) return
    method.value =
      (c.methods.find((m) => m === 'webauthn') as any) || (c.methods[0] as any) || 'totp'
    totpCode.value = ''
    recoveryCode.value = ''
  }
)

async function handleLogin() {
  authStore.clearError()
  const result = await authStore.login(credentials.value)
  if (result === 'ok') {
    await afterAuthenticated()
  }
  // 'mfa' → template re-renders into challenge step automatically.
  // 'error' → error banner shows.
}

async function submitTotp() {
  await verify({ method: 'totp', code: totpCode.value.trim() })
}

async function submitRecovery() {
  await verify({ method: 'recovery', code: recoveryCode.value.trim() })
}

async function submitWebauthn() {
  if (!authStore.pendingMfa?.webauthn_options) return
  mfaBusy.value = true
  authStore.clearError()
  try {
    const credential = await webauthnGet(authStore.pendingMfa.webauthn_options)
    await verify({ method: 'webauthn', response: credential })
  } catch (e: any) {
    authStore.$patch({ error: e?.message || 'Security key request failed' })
  } finally {
    mfaBusy.value = false
  }
}

async function verify(body: {
  method: 'totp' | 'webauthn' | 'recovery'
  code?: string
  response?: unknown
}) {
  if (!authStore.pendingMfa) return
  mfaBusy.value = true
  authStore.clearError()
  try {
    const resp = await mfaApi.verify({
      challenge_token: authStore.pendingMfa.challenge_token,
      ...body,
    })
    if (resp.success && resp.data) {
      authStore.completeMfa(resp.data)
      await afterAuthenticated()
    } else {
      authStore.$patch({ error: resp.error || 'Verification failed' })
    }
  } catch (e: any) {
    authStore.$patch({ error: e?.response?.data?.error || 'Network error' })
  } finally {
    mfaBusy.value = false
  }
}

async function afterAuthenticated() {
  // Awaited. A navigation that a guard aborts leaves the user sitting on the
  // login form having just authenticated successfully, with nothing said --
  // and unawaited it was an unhandled rejection as well.
  if (authStore.mustEnrollMfa) {
    await router.push('/profile/mfa')
    return
  }
  const redirect = router.currentRoute.value.query.redirect as string
  await router.push(redirect || '/')
}
</script>
