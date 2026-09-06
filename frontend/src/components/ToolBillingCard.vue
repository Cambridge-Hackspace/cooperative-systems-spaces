<template>
  <div class="card bg-base-100 shadow-xl">
    <div class="card-body">
      <h2 class="card-title text-2xl mb-4">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z"
          />
        </svg>
        Tool credit
      </h2>

      <p class="text-base-content/70 mb-4">
        Metered tools draw on your credit. Held funds are reserved by a tool you have running; you
        can only start a tool your available credit covers.
      </p>

      <div v-if="errorMessage" class="alert alert-error mb-4" role="alert">
        <span>{{ errorMessage }}</span>
      </div>

      <div v-if="loading" class="text-base-content/70">Loading credit…</div>

      <div v-else class="space-y-1">
        <p>
          <span class="font-semibold">Available:</span>
          <span class="ml-2 tool-billing-available">{{ currency }} {{ available }}</span>
        </p>
        <p>
          <span class="font-semibold">Held:</span>
          <span class="ml-2 tool-billing-held">{{ currency }} {{ held }}</span>
        </p>
        <p>
          <span class="font-semibold">Balance:</span>
          <span class="ml-2 tool-billing-balance">{{ currency }} {{ balance }}</span>
        </p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { apiClient } from '@/utils/api'

interface ToolBillingView {
  balance: string
  held: string
  available: string
  currency: string
}

const balance = ref('0.00')
const held = ref('0.00')
const available = ref('0.00')
const currency = ref('USD')
const loading = ref(false)
const errorMessage = ref<string | null>(null)

async function fetchStatus() {
  loading.value = true
  errorMessage.value = null
  try {
    const res = await apiClient.get<ToolBillingView>('/tool-billing')
    if (res.success && res.data) {
      balance.value = res.data.balance
      held.value = res.data.held
      available.value = res.data.available
      currency.value = res.data.currency
    } else {
      throw new Error(res.error || 'Failed to load tool credit')
    }
  } catch (err: any) {
    errorMessage.value = err.message || 'Failed to load tool credit'
  } finally {
    loading.value = false
  }
}

onMounted(fetchStatus)
</script>
