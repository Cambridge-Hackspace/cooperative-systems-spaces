<template>
  <div class="min-h-screen flex items-center justify-center bg-base-200">
    <div class="card w-full max-w-sm shadow-2xl bg-base-100">
      <div class="card-body">
        <h2 class="card-title justify-center text-2xl mb-6">Choose a new password</h2>

        <div v-if="done" data-test="done" class="alert alert-success mb-4">
          <span>Your password has been changed. You can sign in with it now.</span>
        </div>

        <template v-else>
          <div v-if="error" data-test="error" class="alert alert-error mb-4">
            <span>{{ error }}</span>
          </div>

          <div v-if="!token" data-test="no-token" class="alert alert-warning mb-4">
            <span>This link is missing its token. Request a new reset link.</span>
          </div>

          <form v-else @submit.prevent="submit">
            <div class="form-control">
              <label class="label" for="reset-password">
                <span class="label-text">New password</span>
              </label>
              <input
                id="reset-password"
                v-model="password"
                type="password"
                autocomplete="new-password"
                class="input input-bordered"
                :disabled="busy"
                required
              />
            </div>

            <div class="form-control">
              <label class="label" for="reset-confirm">
                <span class="label-text">Confirm new password</span>
              </label>
              <input
                id="reset-confirm"
                v-model="confirmation"
                type="password"
                autocomplete="new-password"
                class="input input-bordered"
                :disabled="busy"
                required
              />
              <label v-if="mismatch" class="label">
                <span class="label-text-alt text-error" data-test="mismatch">
                  The two passwords do not match.
                </span>
              </label>
            </div>

            <div class="form-control mt-6">
              <button
                type="submit"
                class="btn btn-primary"
                :disabled="busy || mismatch"
                data-test="submit"
              >
                <span v-if="busy" class="loading loading-spinner loading-sm"></span>
                <span v-else>Set new password</span>
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
import { computed, ref } from 'vue'
import { useRoute } from 'vue-router'
import { accountApi } from '@/utils/api'

const route = useRoute()

// From the query rather than a path parameter, so the link in the mail is the
// same shape whether or not a token is present, and a truncated one lands on a
// page that can say so.
const token = computed(() => {
  const raw = route.query.token
  return typeof raw === 'string' ? raw : ''
})

const password = ref('')
const confirmation = ref('')
const busy = ref(false)
const done = ref(false)
const error = ref<string | null>(null)

const mismatch = computed(
  () => confirmation.value.length > 0 && password.value !== confirmation.value
)

async function submit() {
  if (mismatch.value) return
  busy.value = true
  error.value = null
  try {
    const response = await accountApi.consumePasswordReset(token.value, password.value)
    if (response.success) {
      done.value = true
    } else {
      // The server answers 400 for an unknown, expired or already-used token,
      // never 401 -- deliberately, because the API client signs the user out on
      // any 401 and a stale link would present as a mysterious session expiry.
      error.value = response.error ?? 'This reset link is invalid or has expired.'
    }
  } finally {
    busy.value = false
  }
}
</script>
