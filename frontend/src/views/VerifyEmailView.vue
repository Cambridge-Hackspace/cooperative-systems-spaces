<template>
  <div class="min-h-screen flex items-center justify-center bg-base-200">
    <div class="card w-full max-w-sm shadow-2xl bg-base-100">
      <div class="card-body">
        <h2 class="card-title justify-center text-2xl mb-6">Confirm your email</h2>

        <div v-if="busy" data-test="working" class="flex justify-center py-4">
          <span class="loading loading-spinner loading-lg"></span>
        </div>

        <div v-else-if="verified" data-test="verified" class="alert alert-success mb-4">
          <span>Your email address is confirmed. You can sign in now.</span>
        </div>

        <template v-else>
          <div v-if="error" data-test="error" class="alert alert-error mb-4">
            <span>{{ error }}</span>
          </div>

          <!--
            A resend form rather than a dead end. Without one, a confirmation
            mail that never arrived leaves an account nobody can sign into and
            nobody can fix without an administrator editing the database.
          -->
          <div v-if="resent" data-test="resent" class="alert alert-success mb-4">
            <span>If that address needs confirming, a new link has been sent.</span>
          </div>
          <form v-else @submit.prevent="resend">
            <div class="form-control">
              <label class="label" for="verify-email">
                <span class="label-text">Send a new confirmation link</span>
              </label>
              <input
                id="verify-email"
                v-model="email"
                type="email"
                autocomplete="email"
                placeholder="you@example.com"
                class="input input-bordered"
                :disabled="resending"
                required
              />
            </div>
            <div class="form-control mt-4">
              <button
                type="submit"
                class="btn btn-primary"
                :disabled="resending"
                data-test="resend"
              >
                <span v-if="resending" class="loading loading-spinner loading-sm"></span>
                <span v-else>Resend</span>
              </button>
            </div>
          </form>
        </template>

        <div class="text-center mt-4">
          <router-link to="/login" class="link link-hover text-sm">Back to sign in</router-link>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import { accountApi } from '@/utils/api'

const route = useRoute()

const busy = ref(false)
const verified = ref(false)
const error = ref<string | null>(null)
const email = ref('')
const resending = ref(false)
const resent = ref(false)

onMounted(async () => {
  const raw = route.query.token
  const token = typeof raw === 'string' ? raw : ''
  if (!token) {
    error.value = 'This link is missing its token. Ask for a new confirmation email below.'
    return
  }

  busy.value = true
  try {
    const response = await accountApi.verifyEmail(token)
    if (response.success) {
      verified.value = true
    } else {
      error.value = response.error ?? 'This confirmation link is invalid or has expired.'
    }
  } finally {
    busy.value = false
  }
})

async function resend() {
  resending.value = true
  try {
    const response = await accountApi.resendVerification(email.value)
    if (response.success) {
      resent.value = true
    } else {
      error.value = response.error ?? 'Could not send a new link. Try again shortly.'
    }
  } finally {
    resending.value = false
  }
}
</script>
