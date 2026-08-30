<template>
  <div class="container mx-auto px-4 py-8">
    <div class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/" class="link">Home</router-link></li>
        <li><router-link to="/admin" class="link">Admin</router-link></li>
        <li>Homepage links</li>
      </ul>
    </div>

    <div class="flex items-start justify-between mb-6 gap-4">
      <div>
        <h1 class="text-3xl font-bold mb-1">Homepage links</h1>
        <p class="text-base-content/70">
          Curated links shown on the public home page. Each link picks an audience &mdash;
          <code class="text-xs bg-base-300 px-1 rounded">everyone</code>,
          <code class="text-xs bg-base-300 px-1 rounded">anonymous</code> (signed-out only),
          <code class="text-xs bg-base-300 px-1 rounded">logged-in</code>,
          <code class="text-xs bg-base-300 px-1 rounded">member</code>, or
          <code class="text-xs bg-base-300 px-1 rounded">staff</code>.
        </p>
      </div>
      <button class="btn btn-primary btn-sm whitespace-nowrap" @click="openNew">+ New link</button>
    </div>

    <div v-if="flash" class="alert mb-4" :class="flashOk ? 'alert-success' : 'alert-error'">
      <span>{{ flash }}</span>
      <button class="btn btn-ghost btn-xs" @click="flash = ''">✕</button>
    </div>

    <div v-if="loading" class="text-center py-12">
      <span class="loading loading-spinner loading-lg"></span>
    </div>
    <div v-else-if="!links.length" class="text-center py-8 text-base-content/60">
      No homepage links yet. Click <strong>+ New link</strong> to add one.
    </div>
    <div v-else class="overflow-x-auto">
      <table class="table">
        <thead>
          <tr>
            <th>Order</th>
            <th>Label</th>
            <th>URL</th>
            <th>Audience</th>
            <th>Status</th>
            <th>Expires</th>
            <th class="text-right">Actions</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="l in links" :key="l.id">
            <td class="font-mono text-xs">{{ l.sort_order }}</td>
            <td>
              <div class="font-medium">
                <span v-if="l.icon" class="mr-1">{{ l.icon }}</span
                >{{ l.label }}
              </div>
              <div v-if="l.description" class="text-xs text-base-content/60">
                {{ l.description }}
              </div>
            </td>
            <td class="font-mono text-xs max-w-xs truncate">{{ l.url }}</td>
            <td>
              <span class="badge" :class="audienceBadgeClass(l.audience)">
                {{ audienceLabel(l.audience) }}
              </span>
            </td>
            <td>
              <span class="badge badge-sm" :class="l.enabled ? 'badge-success' : 'badge-ghost'">
                {{ l.enabled ? 'Enabled' : 'Disabled' }}
              </span>
            </td>
            <td class="text-xs">
              <span v-if="!l.expires_at" class="text-base-content/50">—</span>
              <span v-else-if="isExpired(l.expires_at)" class="badge badge-error badge-sm"
                >Expired</span
              >
              <span v-else class="text-base-content/80" :title="formatAbs(l.expires_at)">
                {{ formatRel(l.expires_at) }}
              </span>
            </td>
            <td class="text-right whitespace-nowrap">
              <button class="btn btn-ghost btn-xs" @click="openEdit(l)">Edit</button>
              <button class="btn btn-ghost btn-xs text-error" @click="onDelete(l)">Delete</button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- ===== Create / Edit modal ===== -->
    <div v-if="showForm" class="modal modal-open">
      <div class="modal-box max-w-xl">
        <h3 class="font-bold text-lg mb-3">
          {{ editing ? 'Edit homepage link' : 'New homepage link' }}
        </h3>

        <div class="form-control mb-3">
          <label class="label py-1"><span class="label-text">Label</span></label>
          <input v-model="form.label" type="text" class="input input-bordered" placeholder="Wiki" />
        </div>

        <div class="form-control mb-3">
          <label class="label py-1"><span class="label-text">URL</span></label>
          <input
            v-model="form.url"
            type="text"
            class="input input-bordered font-mono text-sm"
            placeholder="https://wiki.example.org"
          />
        </div>

        <div class="grid grid-cols-2 gap-3 mb-3">
          <div class="form-control">
            <label class="label py-1"><span class="label-text">Icon (emoji or text)</span></label>
            <input
              v-model="form.icon"
              type="text"
              class="input input-bordered"
              placeholder="📚"
              maxlength="60"
            />
          </div>
          <div class="form-control">
            <label class="label py-1"><span class="label-text">Sort order</span></label>
            <input v-model.number="form.sort_order" type="number" class="input input-bordered" />
          </div>
        </div>

        <div class="form-control mb-3">
          <label class="label py-1"><span class="label-text">Description (optional)</span></label>
          <input v-model="form.description" type="text" class="input input-bordered" />
        </div>

        <div class="form-control mb-3">
          <label class="label py-1"><span class="label-text">Audience</span></label>
          <select v-model="form.audience" class="select select-bordered">
            <option value="everyone">Everyone (signed-in or out)</option>
            <option value="anonymous">Anonymous (signed-out only)</option>
            <option value="logged_in">Logged-in (any authenticated)</option>
            <option value="member">Members (Member or higher)</option>
            <option value="staff">Staff (Staff or higher)</option>
          </select>
        </div>

        <div class="form-control mb-3">
          <label class="label py-1">
            <span class="label-text">Hide after (optional)</span>
          </label>
          <div class="flex items-center gap-2">
            <input
              v-model="form.expires_at_local"
              type="datetime-local"
              class="input input-bordered flex-1"
            />
            <button
              v-if="form.expires_at_local"
              type="button"
              class="btn btn-ghost btn-sm"
              title="Clear expiry"
              @click="form.expires_at_local = ''"
            >
              Clear
            </button>
          </div>
          <span class="label-text-alt mt-1 text-base-content/60">
            Useful for announcements. Once this moment passes the link disappears from the public
            home page; admins still see it (marked Expired) until they delete it.
          </span>
        </div>

        <div class="form-control mb-3">
          <label class="label cursor-pointer justify-start gap-3">
            <input v-model="form.enabled" type="checkbox" class="toggle toggle-success" />
            <span class="label-text">Enabled (shown on the home page)</span>
          </label>
        </div>

        <div class="modal-action">
          <button class="btn btn-ghost" @click="showForm = false">Cancel</button>
          <button class="btn btn-primary" :disabled="saving || !canSave" @click="save">
            <span v-if="saving" class="loading loading-spinner loading-sm"></span>
            <span v-else>{{ editing ? 'Save' : 'Create' }}</span>
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { homeLinksApi } from '@/utils/api'
import { ALLOWED_LINK_SCHEMES, isSafeLinkUrl } from '@/lib/urls'
import type { HomeLink, HomeLinkAudience } from '@/types'

const loading = ref(false)
const saving = ref(false)
const flash = ref('')
const flashOk = ref(true)

const links = ref<HomeLink[]>([])

const showForm = ref(false)
const editing = ref<HomeLink | null>(null)
const form = ref<{
  label: string
  url: string
  description: string | null
  icon: string | null
  audience: HomeLinkAudience
  sort_order: number
  enabled: boolean
  /** Stored as the `<input type="datetime-local">` string in the user's
      browser time zone. Converted to RFC-3339 on save. Empty = no expiry. */
  expires_at_local: string
}>({
  label: '',
  url: '',
  description: null,
  icon: null,
  audience: 'everyone',
  sort_order: 0,
  enabled: true,
  expires_at_local: '',
})

const canSave = computed(() => form.value.label.trim() && form.value.url.trim())

function notify(msg: string, ok = true) {
  flash.value = msg
  flashOk.value = ok
  setTimeout(() => {
    if (flash.value === msg) flash.value = ''
  }, 5000)
}

function audienceLabel(a: HomeLinkAudience): string {
  return {
    everyone: 'Everyone',
    anonymous: 'Anonymous',
    logged_in: 'Logged-in',
    member: 'Member+',
    staff: 'Staff+',
  }[a]
}
function audienceBadgeClass(a: HomeLinkAudience): string {
  return {
    everyone: 'badge-primary',
    anonymous: 'badge-warning',
    logged_in: 'badge-info',
    member: 'badge-success',
    staff: 'badge-secondary',
  }[a]
}

function openNew() {
  editing.value = null
  form.value = {
    label: '',
    url: '',
    description: null,
    icon: null,
    audience: 'everyone',
    sort_order: links.value.length ? Math.max(...links.value.map((l) => l.sort_order)) + 10 : 0,
    enabled: true,
    expires_at_local: '',
  }
  showForm.value = true
}

function openEdit(l: HomeLink) {
  editing.value = l
  form.value = {
    label: l.label,
    url: l.url,
    description: l.description,
    icon: l.icon,
    audience: l.audience,
    sort_order: l.sort_order,
    enabled: l.enabled,
    expires_at_local: l.expires_at ? isoToLocalInput(l.expires_at) : '',
  }
  showForm.value = true
}

/** RFC-3339 → "YYYY-MM-DDTHH:MM" in the browser's local time zone. */
function isoToLocalInput(iso: string): string {
  const d = new Date(iso)
  // Pad fields and produce the format expected by `datetime-local`.
  const pad = (n: number) => n.toString().padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`
}

/** "YYYY-MM-DDTHH:MM" (local) → RFC-3339 in UTC, or null when empty. */
function localInputToIso(local: string): string | null {
  if (!local) return null
  // `new Date(localStr)` interprets in the browser tz; toISOString gives UTC.
  const d = new Date(local)
  return isNaN(d.getTime()) ? null : d.toISOString()
}

function isExpired(iso: string): boolean {
  return new Date(iso).getTime() <= Date.now()
}

function formatAbs(iso: string): string {
  return new Date(iso).toLocaleString()
}

/** Compact relative label like `in 3d`, `in 5h`, `in 12m`. */
function formatRel(iso: string): string {
  const diffMs = new Date(iso).getTime() - Date.now()
  if (diffMs <= 0) return 'just now'
  const m = Math.round(diffMs / 60000)
  if (m < 60) return `in ${m}m`
  const h = Math.round(m / 60)
  if (h < 48) return `in ${h}h`
  const d = Math.round(h / 24)
  return `in ${d}d`
}

async function load() {
  loading.value = true
  try {
    const r = await homeLinksApi.list()
    if (r.success && r.data) links.value = r.data
    else notify(r.error || 'Could not load the homepage links', false)
  } catch (e) {
    // Without this the rejection escaped to `app.config.errorHandler`, which
    // `src/main.ts` never sets -- so it reached the browser console and nowhere
    // else, and the spinner never cleared because the line that cleared it was
    // after the await.
    notify(e instanceof Error ? e.message : 'Could not load the homepage links', false)
  } finally {
    loading.value = false
  }
}

async function save() {
  if (!canSave.value) return

  // The URL ends up in `:href` on the public home page, and Vue does not
  // sanitise an href binding. See lib/urls.ts.
  if (!isSafeLinkUrl(form.value.url)) {
    notify(
      `That URL cannot be used as a link. Permitted: ${ALLOWED_LINK_SCHEMES.join(', ')} or a path on this site.`,
      false
    )
    return
  }
  saving.value = true
  // `null` clears an existing expiry on update; on create it just means "no expiry".
  const body = {
    label: form.value.label.trim(),
    url: form.value.url.trim(),
    description: form.value.description?.trim() || null,
    icon: form.value.icon?.trim() || null,
    audience: form.value.audience,
    sort_order: form.value.sort_order,
    enabled: form.value.enabled,
    expires_at: localInputToIso(form.value.expires_at_local),
  }
  try {
    const res = editing.value
      ? await homeLinksApi.update(editing.value.id, body)
      : await homeLinksApi.create(body)
    if (res.success) {
      notify(editing.value ? 'Link saved' : 'Link created')
      showForm.value = false
      await load()
    } else notify(res.error || 'Failed to save', false)
  } catch (e) {
    notify(e instanceof Error ? e.message : 'Failed to save', false)
  } finally {
    // In a `finally`, so a rejection cannot strand the button disabled and the
    // modal open with no way to retry.
    saving.value = false
  }
}

async function onDelete(l: HomeLink) {
  if (!confirm(`Delete "${l.label}"?`)) return
  const r = await homeLinksApi.remove(l.id)
  if (r.success) {
    notify('Link deleted')
    await load()
  } else notify(r.error || 'Failed to delete', false)
}

onMounted(load)
</script>
