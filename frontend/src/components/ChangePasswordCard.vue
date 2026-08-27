<template>
  <div class="card bg-base-100 shadow-xl">
    <div class="card-body">
      <h2 class="card-title text-2xl mb-4">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
        </svg>
        Change Password
      </h2>

      <div v-if="successMessage" class="alert alert-success mb-4">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"/>
        </svg>
        <span>{{ successMessage }}</span>
      </div>

      <div v-if="errorMessage" class="alert alert-error mb-4">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/>
        </svg>
        <span>{{ errorMessage }}</span>
      </div>

      <form @submit.prevent="submit" class="space-y-4 max-w-sm">
        <div class="form-control">
          <label class="label"><span class="label-text">Current password</span></label>
          <input
            v-model="currentPassword"
            type="password"
            class="input input-bordered"
            autocomplete="current-password"
            required
          />
        </div>

        <div class="form-control">
          <label class="label"><span class="label-text">New password</span></label>
          <input
            v-model="newPassword"
            type="password"
            class="input input-bordered"
            autocomplete="new-password"
            minlength="8"
            required
          />
        </div>

        <div class="form-control">
          <label class="label"><span class="label-text">Confirm new password</span></label>
          <input
            v-model="confirmPassword"
            type="password"
            class="input input-bordered"
            autocomplete="new-password"
            required
          />
          <label v-if="confirmPassword && confirmPassword !== newPassword" class="label">
            <span class="label-text-alt text-error">Passwords don't match</span>
          </label>
        </div>

        <button
          type="submit"
          class="btn btn-primary"
          :disabled="submitting || !currentPassword || !newPassword || newPassword !== confirmPassword"
        >
          <span v-if="submitting" class="loading loading-spinner loading-sm"></span>
          Update password
        </button>
      </form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { userApi } from '@/utils/api'

const currentPassword = ref('')
const newPassword = ref('')
const confirmPassword = ref('')
const submitting = ref(false)
const successMessage = ref('')
const errorMessage = ref('')

async function submit() {
  submitting.value = true
  successMessage.value = ''
  errorMessage.value = ''
  try {
    const res = await userApi.changePassword(currentPassword.value, newPassword.value)
    if (res.success) {
      successMessage.value = 'Password updated successfully.'
      currentPassword.value = ''
      newPassword.value = ''
      confirmPassword.value = ''
    } else {
      errorMessage.value = res.error || 'Failed to update password.'
    }
  } catch (err: any) {
    errorMessage.value = err?.response?.data?.error || err?.message || 'Failed to update password.'
  } finally {
    submitting.value = false
  }
}
</script>
