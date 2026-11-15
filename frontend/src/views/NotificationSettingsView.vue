<template>
  <div class="container mx-auto px-4 py-8 max-w-3xl">
    <div class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/profile" class="link">My Profile</router-link></li>
        <li>Notifications from apps</li>
      </ul>
    </div>
    <h1 class="text-3xl font-bold mb-1">Notifications from apps</h1>
    <p class="text-base-content/70 mb-6">
      Other apps can send you messages through CS:S. They never see your email address. Until you
      choose, only messages an app marks as urgent reach you.
    </p>

    <div v-if="error" class="alert alert-error mb-4" role="alert" data-test="error">
      <span>{{ error }}</span>
    </div>

    <div v-if="loading" class="flex justify-center py-6">
      <span class="loading loading-spinner loading-lg"></span>
    </div>
    <p v-else-if="apps.length === 0" class="text-base-content/60" data-test="empty">
      No app sends messages yet.
    </p>
    <div v-else class="space-y-4">
      <div
        v-for="app in apps"
        :key="app.client"
        class="card bg-base-100 shadow"
        :data-test="`app-${app.client}`"
      >
        <div class="card-body">
          <label class="flex items-center justify-between gap-4">
            <span class="card-title">{{ app.app }}</span>
            <span class="flex items-center gap-2 text-sm">
              Anything at all
              <input
                type="checkbox"
                class="toggle"
                :checked="app.enabled"
                :data-test="`enabled-${app.client}`"
                @change="setEnabled(app, ($event.target as HTMLInputElement).checked)"
              />
            </span>
          </label>
          <ul class="space-y-2 mt-2">
            <li
              v-for="k in app.kinds"
              :key="k.kind"
              class="flex items-center justify-between gap-4"
            >
              <span :class="{ 'opacity-50': !app.enabled }">{{ k.label }}</span>
              <label class="flex items-center gap-2 text-sm">
                Email
                <input
                  type="checkbox"
                  class="checkbox checkbox-sm"
                  :checked="k.email"
                  :disabled="!app.enabled"
                  :data-test="`email-${app.client}-${k.kind}`"
                  @change="setKind(app, k.kind, ($event.target as HTMLInputElement).checked)"
                />
              </label>
            </li>
          </ul>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { notifyApi } from '@/utils/api'
import type { NotifyAppChoices } from '@/types'

const apps = ref<NotifyAppChoices[]>([])
const loading = ref(true)
const error = ref('')

async function load() {
  const r = await notifyApi.preferences()
  if (r.success && r.data) {
    apps.value = r.data
    error.value = ''
  } else {
    error.value = r.error || 'Your notification settings could not be loaded'
  }
  loading.value = false
}

async function setEnabled(app: NotifyAppChoices, enabled: boolean) {
  const r = await notifyApi.setPreferences(app.client, { enabled })
  if (r.success) {
    app.enabled = enabled
    error.value = ''
  } else {
    error.value = r.error || 'That was not saved'
  }
}

async function setKind(app: NotifyAppChoices, kind: string, email: boolean) {
  const r = await notifyApi.setPreferences(app.client, { kinds: { [kind]: email } })
  const k = app.kinds.find((x) => x.kind === kind)
  if (r.success && k) {
    k.email = email
    k.default = false
    error.value = ''
  } else {
    error.value = r.error || 'That was not saved'
  }
}

onMounted(load)
</script>
