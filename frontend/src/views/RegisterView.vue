<template>
  <div class="min-h-screen flex items-center justify-center bg-base-200">
    <div class="card w-full max-w-md shadow-2xl bg-base-100">
      <div class="card-body">
        <h2 class="card-title justify-center text-2xl mb-6">Register</h2>

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

        <div v-if="registrationSuccess" class="alert alert-success mb-4">
          <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M5 13l4 4L19 7"
            />
          </svg>
          <div>
            <h3 class="font-bold">Registration Successful!</h3>
            <div class="text-xs">Please login with your new account.</div>
          </div>
        </div>

        <form v-if="!registrationSuccess" @submit.prevent="handleRegister">
          <div class="form-control">
            <label class="label">
              <span class="label-text">Full Name</span>
            </label>
            <input
              v-model="userData.full_name"
              type="text"
              placeholder="Enter your full name"
              class="input input-bordered"
              :disabled="authStore.isLoading"
              required
            />
          </div>

          <div class="form-control">
            <label class="label">
              <span class="label-text">Username</span>
            </label>
            <input
              v-model="userData.username"
              type="text"
              placeholder="Choose a username"
              class="input input-bordered"
              :disabled="authStore.isLoading"
              required
            />
          </div>

          <div class="form-control">
            <label class="label">
              <span class="label-text">Email</span>
            </label>
            <input
              v-model="userData.email"
              type="email"
              placeholder="Enter your email"
              class="input input-bordered"
              :disabled="authStore.isLoading"
              required
            />
          </div>

          <div class="form-control">
            <label class="label">
              <span class="label-text">Password</span>
            </label>
            <input
              v-model="userData.password"
              type="password"
              placeholder="Choose a password"
              class="input input-bordered"
              :disabled="authStore.isLoading"
              minlength="8"
              required
            />
            <label class="label">
              <span class="label-text-alt">Password must be at least 8 characters</span>
            </label>
          </div>

          <div v-if="challengeConfig?.enabled" class="form-control">
            <label class="label">
              <span class="label-text">Registration Phrase</span>
            </label>
            <input
              v-model="userData.challenge_phrase"
              type="text"
              :placeholder="challengeConfig?.hint || 'Enter the registration phrase'"
              class="input input-bordered"
              :disabled="authStore.isLoading"
              required
            />
            <label class="label">
              <span class="label-text-alt">{{ challengeConfig?.hint }}</span>
            </label>
          </div>

          <!-- Terms of Service checkbox -->
          <div v-if="challengeConfig?.terms_of_service_checkbox" class="form-control">
            <label class="cursor-pointer label justify-start">
              <input
                v-model="userData.terms_of_service_accepted"
                type="checkbox"
                class="checkbox checkbox-primary"
                :disabled="authStore.isLoading"
                required
              />
              <!-- Deliberate and narrow: this is the terms-of-service text from config.toml, which only a server
       administrator can set -- the same person who can already change anything
       else the server serves. NOTE: the field is named _md and holds markdown,
       but nothing converts it, so markdown syntax renders literally. That is a
       separate rendering bug, recorded rather than fixed here. -->
              <!-- eslint-disable-next-line vue/no-v-html -->
              <span class="label-text ml-3" v-html="challengeConfig?.terms_of_service_md"></span>
            </label>
          </div>

          <!-- reCAPTCHA placeholder (would need reCAPTCHA library integration) -->
          <div v-if="challengeConfig?.recaptcha_enabled" class="form-control">
            <div class="alert alert-info">
              <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                />
              </svg>
              <span>reCAPTCHA verification would appear here</span>
            </div>
            <!-- This would be replaced with actual reCAPTCHA component -->
            <div class="text-sm text-base-content/70 mt-2">
              Site key: {{ challengeConfig?.recaptcha_site_key || 'Not configured' }}
            </div>
          </div>

          <div class="form-control mt-6">
            <button type="submit" class="btn btn-primary" :disabled="authStore.isLoading">
              <span v-if="authStore.isLoading" class="loading loading-spinner loading-sm"></span>
              <span v-else>Register</span>
            </button>
          </div>
        </form>

        <div class="text-center mt-4">
          <p class="text-sm">
            Already have an account?
            <router-link to="/login" class="link link-primary">Login here</router-link>
          </p>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import type { RegisterRequest } from '@/types'

const authStore = useAuthStore()

const userData = ref<RegisterRequest>({
  username: '',
  email: '',
  password: '',
  full_name: '',
  challenge_phrase: '',
  terms_of_service_accepted: false,
  recaptcha_token: undefined,
})

const registrationSuccess = ref(false)
const challengeConfig = ref<{
  enabled: boolean
  hint: string
  throttle_enabled: boolean
  terms_of_service_checkbox: boolean
  terms_of_service_md: string
  recaptcha_enabled: boolean
  recaptcha_site_key: string
} | null>(null)

// Fetch registration challenge configuration on mount
onMounted(async () => {
  try {
    // We'll need to create an endpoint to get this configuration
    // For now, assume it's available from a config endpoint
    const response = await fetch('/api/config/registration')
    if (response.ok) {
      const config = await response.json()
      challengeConfig.value = config.data.registration_challenge
    }
  } catch (error) {
    // If we can't fetch config, assume no challenge is needed
    console.warn('Could not fetch registration challenge config:', error)
  }
})

async function handleRegister() {
  authStore.clearError()

  const success = await authStore.register(userData.value)

  if (success) {
    registrationSuccess.value = true
  }
}
</script>
