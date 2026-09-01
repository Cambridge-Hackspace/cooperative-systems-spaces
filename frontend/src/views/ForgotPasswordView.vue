<template>
  <div class="min-h-screen flex items-center justify-center bg-base-200">
    <div class="card w-full max-w-sm shadow-2xl bg-base-100">
      <div class="card-body">
        <h2 class="card-title justify-center text-2xl mb-6">Reset your password</h2>

        <!--
          One message, shown whatever happened.

          The server answers identically whether or not the address has an
          account, so this view must not infer anything either. A "no account
          with that address" state here would rebuild, in the client, exactly
          the enumeration oracle the endpoint was written to avoid.
        -->
        <div v-if="sent" data-test="sent" class="alert alert-success mb-4">
          <span>{{ SENT_MESSAGE }}</span>
        </div>

        <template v-else>
          <div v-if="error" data-test="error" class="alert alert-error mb-4">
            <span>{{ error }}</span>
          </div>

          <p class="text-sm opacity-70 mb-4">
            Enter the address you signed up with and we will send you a link to choose a new
            password.
          </p>

          <form @submit.prevent="submit">
            <div class="form-control">
              <label class="label" for="forgot-email">
                <span class="label-text">Email</span>
              </label>
              <input
                id="forgot-email"
                v-model="email"
                type="email"
                autocomplete="email"
                placeholder="you@example.com"
                class="input input-bordered"
                :disabled="busy"
                required
              />
            </div>

            <div class="form-control mt-6">
              <button type="submit" class="btn btn-primary" :disabled="busy" data-test="submit">
                <span v-if="busy" class="loading loading-spinner loading-sm"></span>
                <span v-else>Send reset link</span>
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
import { ref } from 'vue'
import { accountApi } from '@/utils/api'

const SENT_MESSAGE =
  'If an account exists for that address, a password reset link has been sent. ' +
  'The link expires in an hour.'

const email = ref('')
const busy = ref(false)
const sent = ref(false)
const error = ref<string | null>(null)

async function submit() {
  busy.value = true
  error.value = null
  try {
    const response = await accountApi.requestPasswordReset(email.value)
    // A failure here is a deployment problem -- recovery switched off, or the
    // request throttled -- and not a statement about the address. Both are
    // worth showing; neither reveals whether an account exists.
    if (response.success) {
      sent.value = true
    } else {
      error.value = response.error ?? 'Could not send a reset link. Try again shortly.'
    }
  } finally {
    busy.value = false
  }
}
</script>
