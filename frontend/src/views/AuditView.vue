<template>
  <div class="container mx-auto px-4 py-8">
    <!-- Breadcrumbs -->
    <div class="breadcrumbs text-sm mb-6">
      <ul>
        <li>
          <router-link to="/" class="link">Home</router-link>
        </li>
        <li>
          <router-link to="/admin" class="link">Admin</router-link>
        </li>
        <li>Audit Logs</li>
      </ul>
    </div>

    <!-- Header -->
    <div class="mb-8">
      <h1 class="text-3xl font-bold mb-2">Audit Logs</h1>
      <p class="text-base-content/70">
        System activity and user action logs for security and compliance monitoring.
      </p>
    </div>

    <!-- Success Toast -->
    <div v-if="successMessage" class="toast toast-top toast-end z-50">
      <div class="alert alert-success">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
                d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"/>
        </svg>
        <span>{{ successMessage }}</span>
      </div>
    </div>

    <!-- Error Toast -->
    <div v-if="errorMessage" class="toast toast-top toast-end z-50">
      <div class="alert alert-error">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
                d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/>
        </svg>
        <span>{{ errorMessage }}</span>
        <button class="btn btn-sm btn-ghost" @click="clearError">
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"/>
          </svg>
        </button>
      </div>
    </div>

    <!-- Audit Log Table Component -->
    <AuditLogTable
      @error="handleError"
      @success="handleSuccess"
    />
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import AuditLogTable from '@/components/AuditLogTable.vue'

// Reactive state
const successMessage = ref<string | null>(null)
const errorMessage = ref<string | null>(null)

// Methods
const handleSuccess = (message: string) => {
  successMessage.value = message
  setTimeout(() => {
    successMessage.value = null
  }, 5000)
}

const handleError = (message: string) => {
  errorMessage.value = message
  setTimeout(() => {
    errorMessage.value = null
  }, 8000)
}

const clearError = () => {
  errorMessage.value = null
}
</script>

<style scoped>
.toast {
  z-index: 1000;
}
</style>