<template>
  <div class="container mx-auto p-4 max-w-5xl">
    <div class="text-sm breadcrumbs mb-2">
      <ul>
        <li><router-link to="/admin">Admin</router-link></li>
        <li>Apps</li>
      </ul>
    </div>
    <h1 class="text-3xl font-bold mb-1">Apps</h1>
    <p class="text-base-content/70 mb-6">
      Other programs that sign members in with their CS:S account. An app only ever sees what its
      scopes allow, and members are asked first unless you mark the app as the space's own.
    </p>

    <div v-if="error" class="alert alert-error mb-4" role="alert" data-test="error">
      <span>{{ error }}</span>
    </div>

    <!-- A secret is shown once, when it is made, and never again. -->
    <div v-if="shownSecret" class="alert alert-warning mb-4" data-test="secret">
      <div>
        <p class="font-semibold">Copy this secret now. CS:S keeps only its hash.</p>
        <p class="text-sm">
          {{ shownSecret.app }}: client id <code>{{ shownSecret.clientId }}</code>
        </p>
        <code class="block break-all mt-1" data-test="secret-value">{{ shownSecret.secret }}</code>
      </div>
      <button class="btn btn-sm" @click="shownSecret = null">Done</button>
    </div>

    <div class="card bg-base-100 shadow mb-6">
      <form class="card-body" data-test="create-form" @submit.prevent="create">
        <h2 class="card-title">Register an app</h2>
        <div class="form-control">
          <label class="label" for="app-name"><span class="label-text">Name</span></label>
          <input id="app-name" v-model="form.name" class="input input-bordered" required />
        </div>
        <div class="form-control">
          <label class="label" for="app-redirects">
            <span class="label-text">Redirect URIs, one per line</span>
          </label>
          <textarea
            id="app-redirects"
            v-model="form.redirects"
            class="textarea textarea-bordered"
            rows="2"
            placeholder="https://app.example.org/auth/callback"
          ></textarea>
        </div>
        <div class="form-control">
          <label class="label" for="app-logout">
            <span class="label-text">After sign-out, one per line (optional)</span>
          </label>
          <textarea
            id="app-logout"
            v-model="form.logoutRedirects"
            class="textarea textarea-bordered"
            rows="1"
          ></textarea>
        </div>
        <fieldset class="form-control">
          <legend class="label-text mb-1">What the app may see or do</legend>
          <label
            v-for="s in SCOPES"
            :key="s.scope"
            class="label cursor-pointer justify-start gap-2"
          >
            <input
              v-model="form.scopes"
              type="checkbox"
              class="checkbox checkbox-sm"
              :value="s.scope"
              :disabled="s.scope === 'openid'"
              :data-test="`scope-${s.scope}`"
            />
            <span class="label-text">{{ s.label }}</span>
          </label>
        </fieldset>
        <label class="label cursor-pointer justify-start gap-2">
          <input v-model="form.confidential" type="checkbox" class="checkbox checkbox-sm" />
          <span class="label-text">
            Has a secret (a server, not an app running in a browser or on a phone)
          </span>
        </label>
        <label class="label cursor-pointer justify-start gap-2">
          <input v-model="form.skipConsent" type="checkbox" class="checkbox checkbox-sm" />
          <span class="label-text">The space's own app: don't ask members to agree first</span>
        </label>
        <div class="card-actions justify-end">
          <button class="btn btn-primary" type="submit" :disabled="saving">Register</button>
        </div>
      </form>
    </div>

    <div v-if="loading" class="flex justify-center py-6">
      <span class="loading loading-spinner loading-lg"></span>
    </div>
    <p v-else-if="apps.length === 0" class="text-base-content/60" data-test="empty">
      No apps registered yet.
    </p>
    <div v-else class="space-y-4">
      <div
        v-for="app in apps"
        :key="app.id"
        class="card bg-base-100 shadow"
        :data-test="`app-${app.client_id}`"
      >
        <div class="card-body">
          <div class="flex flex-wrap items-center gap-2">
            <h3 class="card-title">{{ app.name }}</h3>
            <span v-if="app.disabled" class="badge badge-error">disabled</span>
            <span v-if="app.skip_consent" class="badge badge-info">the space's own</span>
            <span class="badge badge-ghost">{{
              app.confidential ? 'has a secret' : 'public'
            }}</span>
          </div>
          <p class="text-sm">
            Client id <code>{{ app.client_id }}</code>
          </p>
          <p class="text-sm">Scopes: {{ app.allowed_scopes.join(', ') }}</p>
          <p class="text-sm break-all">Redirects: {{ app.redirect_uris.join(', ') || 'none' }}</p>
          <div class="card-actions justify-end">
            <button
              class="btn btn-sm"
              :data-test="`toggle-${app.client_id}`"
              @click="setDisabled(app, !app.disabled)"
            >
              {{ app.disabled ? 'Enable' : 'Disable' }}
            </button>
            <button
              v-if="app.confidential"
              class="btn btn-sm"
              :data-test="`rotate-${app.client_id}`"
              @click="rotate(app)"
            >
              New secret
            </button>
            <button
              v-if="confirmingDelete !== app.id"
              class="btn btn-sm btn-error btn-outline"
              :data-test="`delete-${app.client_id}`"
              @click="confirmingDelete = app.id"
            >
              Remove
            </button>
            <template v-else>
              <span class="text-sm self-center">
                Remove it? Everything it was issued stops working.
              </span>
              <button
                class="btn btn-sm btn-error"
                :data-test="`confirm-delete-${app.client_id}`"
                @click="remove(app)"
              >
                Remove
              </button>
              <button class="btn btn-sm" @click="confirmingDelete = null">Keep</button>
            </template>
          </div>
        </div>
      </div>
    </div>

    <div class="mt-8 text-sm text-base-content/70">
      <p>
        Signing keys rotate on their own. Rotate now if one may have leaked: tokens already issued
        keep working until they expire.
      </p>
      <button class="btn btn-sm mt-2" data-test="rotate-keys" @click="rotateKeys">
        Rotate the signing key
      </button>
      <span v-if="rotatedKey" class="ml-2" data-test="rotated-key">New key {{ rotatedKey }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { oauthAdminApi } from '@/utils/api'
import type { OAuthClient } from '@/types'

// What an administrator can grant, in the words the consent page uses.
// `notify:members` is the app acting as itself (#226), never for a member.
const SCOPES = [
  { scope: 'openid', label: 'Confirm who the member is (always)' },
  { scope: 'profile', label: 'Their name and username' },
  { scope: 'email', label: 'Their email address' },
  { scope: 'roles', label: 'Their roles in CS:S' },
  { scope: 'training:read', label: 'Which tools they are trained on' },
  { scope: 'notify:members', label: 'Send members notifications (needs a secret)' },
]

const apps = ref<OAuthClient[]>([])
const loading = ref(true)
const saving = ref(false)
const error = ref('')
const confirmingDelete = ref<string | null>(null)
const rotatedKey = ref('')
const shownSecret = ref<{ app: string; clientId: string; secret: string } | null>(null)

const blank = () => ({
  name: '',
  redirects: '',
  logoutRedirects: '',
  scopes: ['openid'] as string[],
  confidential: true,
  skipConsent: false,
})
const form = reactive(blank())

const lines = (s: string) =>
  s
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)

async function load() {
  loading.value = true
  const r = await oauthAdminApi.list()
  if (r.success && r.data) {
    apps.value = r.data
    error.value = ''
  } else {
    error.value = r.error || 'The apps could not be loaded'
  }
  loading.value = false
}

async function create() {
  saving.value = true
  const r = await oauthAdminApi.create({
    name: form.name,
    redirect_uris: lines(form.redirects),
    post_logout_redirect_uris: lines(form.logoutRedirects),
    allowed_scopes: form.scopes,
    confidential: form.confidential,
    skip_consent: form.skipConsent,
  })
  saving.value = false
  if (r.success && r.data) {
    apps.value = [...apps.value, r.data.client]
    if (r.data.client_secret) {
      shownSecret.value = {
        app: r.data.client.name,
        clientId: r.data.client.client_id,
        secret: r.data.client_secret,
      }
    }
    Object.assign(form, blank())
    error.value = ''
  } else {
    error.value = r.error || 'The app was not registered'
  }
}

function replace(updated: OAuthClient) {
  apps.value = apps.value.map((a) => (a.id === updated.id ? updated : a))
}

async function setDisabled(app: OAuthClient, disabled: boolean) {
  const r = await oauthAdminApi.update(app.id, { disabled })
  if (r.success && r.data) {
    replace(r.data)
    error.value = ''
  } else {
    error.value = r.error || 'The app was not changed'
  }
}

async function rotate(app: OAuthClient) {
  const r = await oauthAdminApi.rotateSecret(app.id)
  if (r.success && r.data?.client_secret) {
    replace(r.data.client)
    shownSecret.value = {
      app: r.data.client.name,
      clientId: r.data.client.client_id,
      secret: r.data.client_secret,
    }
    error.value = ''
  } else {
    error.value = r.error || 'No new secret was made'
  }
}

async function remove(app: OAuthClient) {
  const r = await oauthAdminApi.remove(app.id)
  confirmingDelete.value = null
  if (r.success) {
    apps.value = apps.value.filter((a) => a.id !== app.id)
    error.value = ''
  } else {
    error.value = r.error || 'The app was not removed'
  }
}

async function rotateKeys() {
  const r = await oauthAdminApi.rotateKeys()
  if (r.success && r.data) {
    rotatedKey.value = r.data.kid
    error.value = ''
  } else {
    error.value = r.error || 'The signing key was not rotated'
  }
}

onMounted(load)
</script>
