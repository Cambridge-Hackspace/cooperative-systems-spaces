<template>
  <div class="min-h-screen flex items-center justify-center bg-base-200">
    <div class="card w-full max-w-md shadow-2xl bg-base-100">
      <div class="card-body">
        <div v-if="busy" data-test="working" class="flex justify-center py-4">
          <span class="loading loading-spinner loading-lg"></span>
        </div>

        <div v-else-if="error" data-test="error" class="alert alert-error" role="alert">
          <span>{{ error }}</span>
        </div>

        <!--
          #225: an app asked CS:S to sign this member in. Shown only when there
          is a question to ask: an app the space trusts, or one the member has
          already agreed to, is answered without showing anything.
        -->
        <template v-else-if="request">
          <h2 class="card-title text-2xl" data-test="app">{{ request.app }}</h2>
          <p class="text-base-content/70">
            wants to sign you in with your CS:S account. It will be able to see:
          </p>
          <ul class="list-disc pl-6 space-y-1" data-test="scopes">
            <li v-for="s in request.scopes" :key="s.scope">{{ s.description }}</li>
          </ul>
          <p class="text-sm text-base-content/60" data-test="returns-to">
            You'll go back to {{ request.returns_to }} afterwards.
          </p>
          <div class="card-actions justify-end mt-4">
            <button
              class="btn btn-ghost"
              data-test="deny"
              :disabled="answering"
              @click="answer(false)"
            >
              Don't allow
            </button>
            <button
              class="btn btn-primary"
              data-test="allow"
              :disabled="answering"
              @click="answer(true)"
            >
              Allow
            </button>
          </div>
        </template>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import { oauthApi } from '@/utils/api'
import { leaveFor } from '@/utils/navigate'
import type { OAuthRequestView } from '@/types'

const route = useRoute()
const busy = ref(true)
const answering = ref(false)
const error = ref<string | null>(null)
const request = ref<OAuthRequestView | null>(null)

const requestId = (): string => String(route.params.id ?? '')

async function answer(approve: boolean) {
  answering.value = true
  const r = await oauthApi.decide(requestId(), approve)
  if (r.success && r.data) {
    // Back to the app, with a code or with access_denied.
    leaveFor(r.data.redirect)
    return
  }
  answering.value = false
  error.value = r.error || 'The sign-in could not be completed. Start again from the app.'
}

onMounted(async () => {
  const r = await oauthApi.request(requestId())
  if (!r.success || !r.data) {
    error.value = r.error || 'This sign-in request has expired. Start again from the app.'
    busy.value = false
    return
  }
  if (r.data.automatic) {
    await answer(true)
    busy.value = false
    return
  }
  request.value = r.data
  busy.value = false
})
</script>
