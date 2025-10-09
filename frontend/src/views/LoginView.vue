<template>
  <div class="min-h-screen flex items-center justify-center bg-base-200">
    <div class="card w-full max-w-sm shadow-2xl bg-base-100">
      <div class="card-body">
        <h2 class="card-title justify-center text-2xl mb-6">Login</h2>
        
        <div v-if="authStore.error" class="alert alert-error mb-4">
          <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/>
          </svg>
          <span>{{ authStore.error }}</span>
        </div>

        <form @submit.prevent="handleLogin">
          <div class="form-control">
            <label class="label">
              <span class="label-text">Username or Email</span>
            </label>
            <input
              v-model="credentials.username_or_email"
              type="text"
              placeholder="Enter username or email"
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
              v-model="credentials.password"
              type="password"
              placeholder="Enter password"
              class="input input-bordered"
              :disabled="authStore.isLoading"
              required
            />
            <label class="label">
              <a href="#" class="label-text-alt link link-hover">Forgot password?</a>
            </label>
          </div>
          
          <div class="form-control mt-6">
            <button 
              type="submit" 
              class="btn btn-primary"
              :disabled="authStore.isLoading"
            >
              <span v-if="authStore.isLoading" class="loading loading-spinner loading-sm"></span>
              <span v-else>Login</span>
            </button>
          </div>
        </form>

        <div class="divider">OR</div>
        
        <div class="text-center">
          <p class="text-sm">
            Don't have an account?
            <router-link to="/register" class="link link-primary">Register here</router-link>
          </p>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import type { LoginRequest } from '@/types'

const router = useRouter()
const authStore = useAuthStore()

const credentials = ref<LoginRequest>({
  username_or_email: '',
  password: ''
})

async function handleLogin() {
  authStore.clearError()
  
  const success = await authStore.login(credentials.value)
  
  if (success) {
    const redirect = router.currentRoute.value.query.redirect as string
    router.push(redirect || '/')
  }
}
</script>