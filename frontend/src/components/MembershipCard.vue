<template>
  <div class="card bg-base-100 shadow-xl">
    <div class="card-body">
      <h2 class="card-title text-2xl mb-4">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M3 10h18M7 15h1m4 0h1m-7 4h12a3 3 0 003-3V8a3 3 0 00-3-3H6a3 3 0 00-3 3v8a3 3 0 003 3z"
          />
        </svg>
        {{ planName }}
      </h2>

      <div v-if="errorMessage" class="alert alert-error mb-4" role="alert">
        <span>{{ errorMessage }}</span>
      </div>

      <div v-if="loading" class="text-base-content/70">Loading membership…</div>

      <template v-else>
        <div class="mb-4 space-y-1">
          <p>
            <span class="font-semibold">Status:</span>
            <span
              class="badge ml-2 membership-status"
              :class="isMember ? 'badge-success' : 'badge-ghost'"
            >
              {{ isMember ? 'Member' : 'Not a current member' }}
            </span>
          </p>
          <p>
            <span class="font-semibold">Balance:</span>
            <span class="ml-2 membership-balance">{{ currency }} {{ balance }}</span>
          </p>
          <p v-if="nextDueAt">
            <span class="font-semibold">Next dues due:</span>
            <span class="ml-2">{{ formatDate(nextDueAt) }}</span>
          </p>
        </div>

        <div v-if="stripeEnabled" class="card-actions">
          <button
            class="btn btn-primary membership-start"
            :disabled="busy"
            @click="startCheckout('subscription')"
          >
            {{ enrolled ? 'Renew / add a subscription' : 'Start membership' }}
          </button>
          <button
            class="btn btn-outline membership-oneshot"
            :disabled="busy"
            @click="startCheckout('one_shot')"
          >
            Make a one-time payment
          </button>
          <button
            v-if="hasSubscription"
            class="btn btn-ghost membership-manage"
            :disabled="busy"
            @click="openPortal"
          >
            Manage / cancel
          </button>
        </div>
        <div v-else class="alert alert-info" role="note">
          <span>Online payment is not enabled. To pay your dues, please see an admin.</span>
        </div>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { apiClient } from '@/utils/api'
import { useConfigStore } from '@/stores/config'

interface MembershipView {
  enrolled: boolean
  is_member: boolean
  balance: string
  currency: string
  next_due_at: string | null
  has_subscription: boolean
  plan_name: string
}

interface RedirectResponse {
  url: string
}

const configStore = useConfigStore()

const enrolled = ref(false)
const isMember = ref(false)
const balance = ref('0.00')
const currency = ref('USD')
const nextDueAt = ref<string | null>(null)
const hasSubscription = ref(false)
const planName = ref('Membership')

const loading = ref(false)
const busy = ref(false)
const errorMessage = ref<string | null>(null)

const stripeEnabled = computed(() => configStore.stripeEnabled())

function formatDate(iso: string): string {
  const d = new Date(iso)
  return isNaN(d.getTime()) ? iso : d.toLocaleDateString()
}

async function fetchStatus() {
  loading.value = true
  errorMessage.value = null
  try {
    const res = await apiClient.get<MembershipView>('/membership')
    if (res.success && res.data) {
      enrolled.value = res.data.enrolled
      isMember.value = res.data.is_member
      balance.value = res.data.balance
      currency.value = res.data.currency
      nextDueAt.value = res.data.next_due_at
      hasSubscription.value = res.data.has_subscription
      planName.value = res.data.plan_name
    } else {
      throw new Error(res.error || 'Failed to load membership')
    }
  } catch (err: any) {
    errorMessage.value = err.message || 'Failed to load membership'
  } finally {
    loading.value = false
  }
}

async function startCheckout(mode: 'subscription' | 'one_shot') {
  busy.value = true
  errorMessage.value = null
  try {
    const res = await apiClient.post<RedirectResponse>('/stripe/checkout', { mode })
    if (res.success && res.data?.url) {
      window.location.href = res.data.url
    } else {
      throw new Error(res.error || 'Failed to start checkout')
    }
  } catch (err: any) {
    errorMessage.value = err.message || 'Failed to start checkout'
    busy.value = false
  }
}

async function openPortal() {
  busy.value = true
  errorMessage.value = null
  try {
    const res = await apiClient.post<RedirectResponse>('/stripe/portal', {})
    if (res.success && res.data?.url) {
      window.location.href = res.data.url
    } else {
      throw new Error(res.error || 'Failed to open billing portal')
    }
  } catch (err: any) {
    errorMessage.value = err.message || 'Failed to open billing portal'
    busy.value = false
  }
}

onMounted(fetchStatus)
</script>
