<template>
  <div class="card bg-base-100 shadow-xl">
    <div class="card-body">
      <h2 class="card-title text-2xl mb-2">Stripe customers</h2>
      <p class="text-sm text-base-content/70 mb-4">
        Every customer id Stripe knows this member by. A payment on any of them reaches the ledger;
        checkout and the Billing Portal use the one marked current. Unlinking happens on Stripe's
        side.
      </p>

      <div v-if="errorMessage" class="alert alert-error mb-4" role="alert">
        <span>{{ errorMessage }}</span>
      </div>

      <div v-if="loading" class="flex justify-center py-4">
        <span class="loading loading-spinner"></span>
      </div>

      <p v-else-if="!errorMessage && customers.length === 0" class="text-sm">
        No Stripe customer yet.
      </p>

      <table v-else-if="customers.length" class="table table-sm" data-testid="stripe-customers">
        <thead>
          <tr>
            <th>Customer</th>
            <th>Subscription</th>
            <th>Linked</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="c in customers" :key="c.id" :data-customer="c.customer_id">
            <td>
              <span class="font-mono">{{ c.customer_id }}</span>
              <span v-if="c.current" class="badge badge-primary badge-sm ml-2">Current</span>
            </td>
            <td>
              <template v-if="c.subscription_id">
                <span class="font-mono text-xs">{{ c.subscription_id }}</span>
                <span class="badge badge-sm ml-2" :class="statusClass(c.subscription_status)">
                  {{ c.subscription_status ?? 'unknown' }}
                </span>
              </template>
              <span v-else class="text-base-content/50">
                {{ c.subscription_status ? c.subscription_status : '—' }}
              </span>
            </td>
            <td class="text-xs">{{ fmt(c.created_at) }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { userApi } from '@/utils/api'
import type { StripeCustomerLink } from '@/types'

const props = defineProps<{ userId: string }>()

const customers = ref<StripeCustomerLink[]>([])
const loading = ref(false)
const errorMessage = ref('')

function fmt(iso: string) {
  return new Date(iso).toLocaleDateString()
}
function statusClass(status: string | null) {
  return status === 'active' || status === 'trialing' ? 'badge-success' : 'badge-ghost'
}

async function load() {
  loading.value = true
  errorMessage.value = ''
  try {
    const res = await userApi.listStripeCustomers(props.userId)
    if (res.success && res.data) {
      customers.value = res.data
    } else {
      errorMessage.value = res.error || 'Could not load Stripe customers.'
    }
  } catch (err) {
    const e = err as { message?: string }
    errorMessage.value = e?.message || 'Could not load Stripe customers.'
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch(() => props.userId, load)
</script>
