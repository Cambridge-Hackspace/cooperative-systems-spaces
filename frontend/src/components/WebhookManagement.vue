<template>
  <div class="container mx-auto px-4 py-8">
    <div class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/" class="link">Home</router-link></li>
        <li><router-link to="/admin" class="link">Admin</router-link></li>
        <li>Webhooks</li>
      </ul>
    </div>

    <div class="flex items-center justify-between mb-6">
      <div>
        <h1 class="text-3xl font-bold mb-1">Webhooks</h1>
        <p class="text-base-content/70">
          Fire outbound HTTP requests when selected audit events occur.
        </p>
      </div>
    </div>

    <!-- Flash message -->
    <div v-if="flash" class="alert mb-4" :class="flashSuccess ? 'alert-success' : 'alert-error'">
      <span>{{ flash }}</span>
      <button class="btn btn-ghost btn-xs" @click="flash = ''">✕</button>
    </div>

    <!-- Tabs -->
    <div role="tablist" class="tabs tabs-boxed mb-6 w-fit">
      <a role="tab" class="tab" :class="{ 'tab-active': tab === 'webhooks' }" @click="tab = 'webhooks'">Webhooks</a>
      <a role="tab" class="tab" :class="{ 'tab-active': tab === 'auth' }" @click="tab = 'auth'">Auth Credentials</a>
      <a role="tab" class="tab" :class="{ 'tab-active': tab === 'deliveries' }" @click="switchToDeliveries">Deliveries</a>
    </div>

    <!-- ===================== WEBHOOKS TAB ===================== -->
    <div v-if="tab === 'webhooks'">
      <div class="flex justify-end mb-4">
        <button class="btn btn-primary btn-sm" @click="openWebhookModal()">+ New Webhook</button>
      </div>

      <div v-if="loading" class="text-center py-8"><span class="loading loading-spinner"></span></div>
      <div v-else-if="webhooks.length === 0" class="text-center py-8 text-base-content/60">
        No webhooks yet. Create one to start receiving audit events.
      </div>
      <div v-else class="overflow-x-auto">
        <table class="table">
          <thead>
            <tr>
              <th>Name</th>
              <th>URL</th>
              <th>Events</th>
              <th>Auth</th>
              <th>Status</th>
              <th class="text-right">Actions</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="wh in webhooks" :key="wh.id">
              <td class="font-medium">{{ wh.name }}</td>
              <td class="max-w-xs truncate font-mono text-xs">{{ wh.url }}</td>
              <td><span class="badge badge-ghost">{{ wh.event_types.length }}</span></td>
              <td><span class="badge badge-ghost">{{ wh.auth_header_ids.length }}</span></td>
              <td>
                <span class="badge" :class="wh.enabled ? 'badge-success' : 'badge-neutral'">
                  {{ wh.enabled ? 'Enabled' : 'Disabled' }}
                </span>
              </td>
              <td class="text-right whitespace-nowrap">
                <button class="btn btn-ghost btn-xs" @click="testWebhook(wh)" :disabled="testingId === wh.id">
                  <span v-if="testingId === wh.id" class="loading loading-spinner loading-xs"></span>
                  <span v-else>Test</span>
                </button>
                <button class="btn btn-ghost btn-xs" @click="openWebhookModal(wh)">Edit</button>
                <button class="btn btn-ghost btn-xs text-error" @click="deleteWebhook(wh)">Delete</button>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <!-- ===================== AUTH CREDENTIALS TAB ===================== -->
    <div v-else-if="tab === 'auth'">
      <div class="flex justify-end mb-4">
        <button class="btn btn-primary btn-sm" @click="openAuthModal()">+ New Credential</button>
      </div>

      <div v-if="authHeaders.length === 0" class="text-center py-8 text-base-content/60">
        No auth credentials yet. Create reusable headers to attach to webhooks.
      </div>
      <div v-else class="overflow-x-auto">
        <table class="table">
          <thead>
            <tr>
              <th>Name</th>
              <th>Header</th>
              <th>Value</th>
              <th class="text-right">Actions</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="h in authHeaders" :key="h.id">
              <td class="font-medium">{{ h.name }}</td>
              <td class="font-mono text-xs">{{ h.header_name }}</td>
              <td class="text-base-content/50 font-mono text-xs">•••••••• (write-only)</td>
              <td class="text-right whitespace-nowrap">
                <button class="btn btn-ghost btn-xs" @click="openAuthModal(h)">Edit</button>
                <button class="btn btn-ghost btn-xs text-error" @click="deleteAuthHeader(h)">Delete</button>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <!-- ===================== DELIVERIES TAB ===================== -->
    <div v-else-if="tab === 'deliveries'">
      <div class="flex justify-end mb-4">
        <button class="btn btn-ghost btn-sm" @click="loadDeliveries">Refresh</button>
      </div>
      <div v-if="deliveries.length === 0" class="text-center py-8 text-base-content/60">
        No delivery attempts recorded yet.
      </div>
      <div v-else class="overflow-x-auto">
        <table class="table table-sm">
          <thead>
            <tr>
              <th>When</th>
              <th>Webhook</th>
              <th>Event</th>
              <th>Attempt</th>
              <th>Status</th>
              <th>Result</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="d in deliveries" :key="d.id">
              <td class="whitespace-nowrap text-xs">{{ formatDate(d.created_at) }}</td>
              <td class="text-xs">{{ webhookName(d.webhook_id) }}</td>
              <td class="font-mono text-xs">{{ d.event_type }}</td>
              <td>{{ d.attempt }}</td>
              <td>
                <span class="badge" :class="d.success ? 'badge-success' : 'badge-error'">
                  {{ d.status_code ?? '—' }}
                </span>
              </td>
              <td class="max-w-xs truncate text-xs" :title="d.error ?? d.response_body ?? ''">
                {{ d.success ? 'OK' : (d.error || 'failed') }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <!-- ===================== WEBHOOK MODAL ===================== -->
    <div v-if="showWebhookModal" class="modal modal-open">
      <div class="modal-box max-w-2xl">
        <h3 class="font-bold text-lg mb-4">{{ editingWebhook ? 'Edit Webhook' : 'New Webhook' }}</h3>

        <div class="form-control mb-3">
          <label class="label"><span class="label-text">Name</span></label>
          <input v-model="whForm.name" type="text" class="input input-bordered" placeholder="My integration" />
        </div>

        <div class="form-control mb-3">
          <label class="label"><span class="label-text">URL</span></label>
          <input v-model="whForm.url" type="text" class="input input-bordered font-mono text-sm" placeholder="https://example.com/hook" />
        </div>

        <div class="form-control mb-3">
          <label class="label cursor-pointer justify-start gap-3">
            <input v-model="whForm.enabled" type="checkbox" class="toggle toggle-primary" />
            <span class="label-text">Enabled</span>
          </label>
        </div>

        <div v-if="editingWebhook" class="form-control mb-3">
          <label class="label"><span class="label-text">Signing secret (HMAC-SHA256)</span></label>
          <input :value="editingWebhook.signing_secret" readonly class="input input-bordered font-mono text-xs" @focus="(e:any)=>e.target.select()" />
          <span class="label-text-alt mt-1 text-base-content/60">Sent as <code>X-Webhook-Signature: sha256=…</code>. Use it to verify payloads.</span>
        </div>

        <div class="form-control mb-3">
          <label class="label">
            <span class="label-text">Audit events ({{ whForm.event_types.length }} of {{ eventTypes.length }} selected)</span>
            <span class="label-text-alt flex gap-3">
              <a class="link link-primary" @click="selectAllEvents">Select all</a>
              <a class="link" @click="clearEvents">Clear</a>
            </span>
          </label>
          <div class="max-h-48 overflow-y-auto border border-base-300 rounded-lg p-2 grid grid-cols-2 gap-1">
            <label v-for="et in eventTypes" :key="et.value" class="label cursor-pointer justify-start gap-2 py-1">
              <input type="checkbox" class="checkbox checkbox-sm" :value="et.value" v-model="whForm.event_types" />
              <span class="label-text text-xs">{{ et.label }}</span>
            </label>
          </div>
        </div>

        <div class="form-control mb-3">
          <label class="label"><span class="label-text">Auth credentials</span></label>
          <div v-if="authHeaders.length === 0" class="text-xs text-base-content/60">
            No credentials defined. Add some in the Auth Credentials tab.
          </div>
          <div v-else class="border border-base-300 rounded-lg p-2 grid grid-cols-2 gap-1">
            <label v-for="h in authHeaders" :key="h.id" class="label cursor-pointer justify-start gap-2 py-1">
              <input type="checkbox" class="checkbox checkbox-sm" :value="h.id" v-model="whForm.auth_header_ids" />
              <span class="label-text text-xs">{{ h.name }} <span class="text-base-content/50">({{ h.header_name }})</span></span>
            </label>
          </div>
        </div>

        <div class="modal-action">
          <button class="btn btn-ghost" @click="showWebhookModal = false">Cancel</button>
          <button class="btn btn-primary" @click="saveWebhook" :disabled="saving">
            <span v-if="saving" class="loading loading-spinner loading-sm"></span>
            <span v-else>{{ editingWebhook ? 'Save' : 'Create' }}</span>
          </button>
        </div>
      </div>
    </div>

    <!-- ===================== AUTH HEADER MODAL ===================== -->
    <div v-if="showAuthModal" class="modal modal-open">
      <div class="modal-box">
        <h3 class="font-bold text-lg mb-4">{{ editingAuth ? 'Edit Credential' : 'New Credential' }}</h3>

        <div class="form-control mb-3">
          <label class="label"><span class="label-text">Name</span></label>
          <input v-model="authForm.name" type="text" class="input input-bordered" placeholder="Bearer token for X" />
        </div>

        <div class="form-control mb-3">
          <label class="label"><span class="label-text">Header name</span></label>
          <input v-model="authForm.header_name" type="text" class="input input-bordered font-mono text-sm" placeholder="Authorization" />
        </div>

        <div class="form-control mb-3">
          <label class="label">
            <span class="label-text">Header value (secret)</span>
          </label>
          <input v-model="authForm.header_value" type="password" autocomplete="new-password" class="input input-bordered font-mono text-sm"
                 :placeholder="editingAuth ? 'Leave blank to keep current value' : 'Bearer abc123…'" />
          <span class="label-text-alt mt-1 text-base-content/60">Write-only. The stored value is never shown again.</span>
        </div>

        <div class="modal-action">
          <button class="btn btn-ghost" @click="showAuthModal = false">Cancel</button>
          <button class="btn btn-primary" @click="saveAuthHeader" :disabled="saving">
            <span v-if="saving" class="loading loading-spinner loading-sm"></span>
            <span v-else>{{ editingAuth ? 'Save' : 'Create' }}</span>
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { webhooksApi } from '@/utils/api'
import type {
  Webhook, WebhookAuthHeader, WebhookEventType, WebhookDelivery,
} from '@/types'

const tab = ref<'webhooks' | 'auth' | 'deliveries'>('webhooks')
const loading = ref(false)
const saving = ref(false)
const testingId = ref<string | null>(null)
const flash = ref('')
const flashSuccess = ref(true)

const webhooks = ref<Webhook[]>([])
const authHeaders = ref<WebhookAuthHeader[]>([])
const eventTypes = ref<WebhookEventType[]>([])
const deliveries = ref<WebhookDelivery[]>([])

function notify(message: string, success = true) {
  flash.value = message
  flashSuccess.value = success
  setTimeout(() => { if (flash.value === message) flash.value = '' }, 5000)
}

function formatDate(iso: string) {
  return new Date(iso).toLocaleString()
}
function webhookName(id: string) {
  return webhooks.value.find(w => w.id === id)?.name ?? id.slice(0, 8)
}

async function loadAll() {
  loading.value = true
  const [whRes, authRes, etRes] = await Promise.all([
    webhooksApi.listWebhooks(),
    webhooksApi.listAuthHeaders(),
    webhooksApi.listEventTypes(),
  ])
  if (whRes.success && whRes.data) webhooks.value = whRes.data
  if (authRes.success && authRes.data) authHeaders.value = authRes.data
  if (etRes.success && etRes.data) eventTypes.value = etRes.data
  loading.value = false
}

async function loadDeliveries() {
  const res = await webhooksApi.listDeliveries({ limit: 100 })
  if (res.success && res.data) deliveries.value = res.data
}
function switchToDeliveries() {
  tab.value = 'deliveries'
  loadDeliveries()
}

// ----- Webhook modal -----
const showWebhookModal = ref(false)
const editingWebhook = ref<Webhook | null>(null)
const whForm = ref({
  name: '', url: '', enabled: true,
  event_types: [] as string[], auth_header_ids: [] as string[],
})

function selectAllEvents() {
  whForm.value.event_types = eventTypes.value.map(e => e.value)
}
function clearEvents() {
  whForm.value.event_types = []
}

function openWebhookModal(wh?: Webhook) {
  editingWebhook.value = wh ?? null
  whForm.value = wh
    ? { name: wh.name, url: wh.url, enabled: wh.enabled, event_types: [...wh.event_types], auth_header_ids: [...wh.auth_header_ids] }
    : { name: '', url: '', enabled: true, event_types: [], auth_header_ids: [] }
  showWebhookModal.value = true
}

async function saveWebhook() {
  if (!whForm.value.name.trim() || !whForm.value.url.trim()) {
    notify('Name and URL are required', false)
    return
  }
  saving.value = true
  const res = editingWebhook.value
    ? await webhooksApi.updateWebhook(editingWebhook.value.id, whForm.value)
    : await webhooksApi.createWebhook(whForm.value)
  saving.value = false
  if (res.success) {
    showWebhookModal.value = false
    notify(editingWebhook.value ? 'Webhook updated' : 'Webhook created')
    await loadAll()
  } else {
    notify(res.error || 'Failed to save webhook', false)
  }
}

async function deleteWebhook(wh: Webhook) {
  if (!confirm(`Delete webhook "${wh.name}"? This also removes its delivery history.`)) return
  const res = await webhooksApi.deleteWebhook(wh.id)
  if (res.success) { notify('Webhook deleted'); await loadAll() }
  else notify(res.error || 'Failed to delete', false)
}

async function testWebhook(wh: Webhook) {
  testingId.value = wh.id
  const res = await webhooksApi.testWebhook(wh.id)
  testingId.value = null
  if (res.success && res.data?.delivered) notify(`Test delivered to "${wh.name}"`)
  else notify(res.message || res.data?.error || 'Test delivery failed', false)
}

// ----- Auth header modal -----
const showAuthModal = ref(false)
const editingAuth = ref<WebhookAuthHeader | null>(null)
const authForm = ref({ name: '', header_name: '', header_value: '' })

function openAuthModal(h?: WebhookAuthHeader) {
  editingAuth.value = h ?? null
  authForm.value = h
    ? { name: h.name, header_name: h.header_name, header_value: '' }
    : { name: '', header_name: '', header_value: '' }
  showAuthModal.value = true
}

async function saveAuthHeader() {
  if (!authForm.value.name.trim() || !authForm.value.header_name.trim()) {
    notify('Name and header name are required', false)
    return
  }
  saving.value = true
  let res
  if (editingAuth.value) {
    const payload: any = { name: authForm.value.name, header_name: authForm.value.header_name }
    if (authForm.value.header_value) payload.header_value = authForm.value.header_value
    res = await webhooksApi.updateAuthHeader(editingAuth.value.id, payload)
  } else {
    if (!authForm.value.header_value) {
      saving.value = false
      notify('A header value is required', false)
      return
    }
    res = await webhooksApi.createAuthHeader(authForm.value)
  }
  saving.value = false
  if (res.success) {
    showAuthModal.value = false
    notify(editingAuth.value ? 'Credential updated' : 'Credential created')
    await loadAll()
  } else {
    notify(res.error || 'Failed to save credential', false)
  }
}

async function deleteAuthHeader(h: WebhookAuthHeader) {
  if (!confirm(`Delete credential "${h.name}"?`)) return
  const res = await webhooksApi.deleteAuthHeader(h.id)
  if (res.success) { notify('Credential deleted'); await loadAll() }
  else notify(res.error || 'Failed to delete (it may still be attached to a webhook)', false)
}

onMounted(loadAll)
</script>
