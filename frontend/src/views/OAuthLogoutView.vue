<template>
  <div class="min-h-screen flex items-center justify-center bg-base-200">
    <div class="card w-full max-w-sm shadow-2xl bg-base-100">
      <div class="card-body items-center">
        <span v-if="!error" class="loading loading-spinner loading-lg" data-test="working"></span>
        <div v-else class="alert alert-error" role="alert" data-test="error">
          <span>{{ error }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
// #225: an app asked CS:S to sign the member out. The request arrived
// cross-site, without the session, so this page -- same-site, with the
// session -- ends it and goes where the server says. The server only honors a
// redirect registered to the app the ID token names; anything else is home.
import { onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import { oauthApi } from '@/utils/api'
import { leaveFor } from '@/utils/navigate'
import { useAuthStore } from '@/stores/auth'

const route = useRoute()
const error = ref<string | null>(null)

const param = (k: string): string | undefined => {
  const v = route.query[k]
  return typeof v === 'string' ? v : undefined
}

onMounted(async () => {
  const r = await oauthApi.endSession({
    id_token_hint: param('id_token_hint'),
    post_logout_redirect_uri: param('post_logout_redirect_uri'),
    state: param('state'),
  })
  if (!r.success || !r.data) {
    error.value = r.error || 'Signing out did not complete.'
    return
  }
  useAuthStore().clearSession()
  leaveFor(r.data.redirect)
})
</script>
