<template>
  <div class="min-h-screen flex items-center justify-center bg-base-200">
    <div class="card w-full max-w-md shadow-2xl bg-base-100">
      <div class="card-body">
        <h2 class="card-title justify-center text-2xl mb-6">Register</h2>
        
        <div v-if="authStore.error" class="alert alert-error mb-4">
          <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/>
          </svg>
          <span>{{ authStore.error }}</span>
        </div>

        <div v-if="registrationSuccess" class="alert alert-success mb-4">
          <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"/>
          </svg>
          <div>
            <h3 class="font-bold">Registration Successful!</h3>
            <div class="text-xs">Please login with your new account.</div>
          </div>
        </div>

        <form @submit.prevent="handleRegister" v-if="!registrationSuccess">
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
          
          <div class="form-control mt-6">
            <button 
              type="submit" 
              class="btn btn-primary"
              :disabled="authStore.isLoading"
            >
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
import { ref } from 'vue'
import { useAuthStore } from '@/stores/auth'
import type { RegisterRequest } from '@/types'

const authStore = useAuthStore()

const userData = ref<RegisterRequest>({
  username: '',
  email: '',
  password: '',
  full_name: ''
})

const registrationSuccess = ref(false)

async function handleRegister() {
  authStore.clearError()
  
  const success = await authStore.register(userData.value)
  
  if (success) {
    registrationSuccess.value = true
  }
}
</script>