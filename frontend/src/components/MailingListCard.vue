<template>
  <div class="card bg-base-100 shadow-xl">
    <div class="card-body">
      <h2 class="card-title text-2xl mb-4">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z"
          />
        </svg>
        Mailing List
      </h2>

      <p class="text-base-content/70 mb-4">
        Subscribe to the members' email list to get announcements and discussion. You can
        unsubscribe at any time, here or from any list email.
      </p>

      <!-- Subscription only takes effect once the address is verified. -->
      <div v-if="!emailVerified" class="alert alert-info mb-4" role="note">
        <span>
          Your email address is not verified yet. You will be added to the list once it is.
        </span>
      </div>

      <div v-if="errorMessage" class="alert alert-error mb-4" role="alert">
        <span>{{ errorMessage }}</span>
      </div>

      <div class="form-control">
        <label class="label cursor-pointer justify-start gap-4">
          <input
            type="checkbox"
            class="toggle toggle-primary mailing-list-toggle"
            :checked="subscribed"
            :disabled="loading || saving"
            @change="onToggle(($event.target as HTMLInputElement).checked)"
          />
          <span class="label-text">
            {{ subscribed ? 'Subscribed' : 'Not subscribed' }}
          </span>
        </label>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { apiClient } from '@/utils/api'

interface SubscriptionStatus {
  subscribed: boolean
  email_verified: boolean
}

const subscribed = ref(false)
const emailVerified = ref(true)
const loading = ref(false)
const saving = ref(false)
const errorMessage = ref<string | null>(null)

async function fetchStatus() {
  loading.value = true
  errorMessage.value = null
  try {
    const res = await apiClient.get<SubscriptionStatus>('/groupsio/subscription')
    if (res.success && res.data) {
      subscribed.value = res.data.subscribed
      emailVerified.value = res.data.email_verified
    } else {
      throw new Error(res.error || 'Failed to load subscription')
    }
  } catch (err: any) {
    errorMessage.value = err.message || 'Failed to load subscription'
  } finally {
    loading.value = false
  }
}

async function onToggle(next: boolean) {
  // Move the reactive value optimistically so the toggle tracks the click, then
  // revert to the last-confirmed value on failure. Reverting to a *different*
  // value than the optimistic one is what forces Vue to re-sync the checkbox --
  // setting it back to the same value it already held would not repaint the DOM.
  const previous = subscribed.value
  subscribed.value = next
  saving.value = true
  errorMessage.value = null
  try {
    const res = await apiClient.put<SubscriptionStatus>('/groupsio/subscription', {
      subscribed: next,
    })
    if (res.success && res.data) {
      subscribed.value = res.data.subscribed
      emailVerified.value = res.data.email_verified
    } else {
      throw new Error(res.error || 'Failed to update subscription')
    }
  } catch (err: any) {
    errorMessage.value = err.message || 'Failed to update subscription'
    subscribed.value = previous
  } finally {
    saving.value = false
  }
}

onMounted(fetchStatus)
</script>
