<template>
  <div class="min-h-screen flex items-center justify-center bg-base-200 px-4 py-8">
    <div class="card w-full max-w-sm shadow-2xl bg-base-100">
      <div class="card-body items-center text-center">
        <!-- Loading -->
        <div v-if="loading" class="py-12">
          <span class="loading loading-spinner loading-lg"></span>
        </div>

        <!-- Loaded -->
        <template v-else-if="info">
          <div class="text-5xl mb-2">🚪</div>
          <h1 class="card-title text-2xl">{{ info.name }}</h1>
          <p v-if="info.location" class="text-base-content/70 text-sm">{{ info.location }}</p>

          <div v-if="!info.enabled" class="alert alert-warning mt-3">
            <span>This door is currently disabled.</span>
          </div>

          <div v-else-if="!info.you_are_authorized" class="alert alert-error mt-3 text-left">
            <div>
              <div class="font-bold">Not authorized</div>
              <div class="text-xs">{{ info.reason || 'No matching access rule.' }}</div>
            </div>
          </div>

          <div v-else class="alert alert-success mt-3">
            <span>You are authorized.</span>
          </div>

          <div v-if="result" class="alert mt-3 text-left" :class="result.unlocked ? 'alert-success' : 'alert-error'">
            <div>
              <div class="font-bold">{{ result.unlocked ? 'Door unlocked' : 'Did not unlock' }}</div>
              <div v-if="result.reason" class="text-xs">{{ result.reason }}</div>
            </div>
          </div>

          <button
            class="btn btn-primary btn-lg w-full mt-4"
            :disabled="!canUnlock || busy"
            @click="checkin"
          >
            <span v-if="busy" class="loading loading-spinner"></span>
            <span v-else>I'm here — unlock</span>
          </button>

          <router-link to="/" class="btn btn-ghost btn-sm w-full mt-2">Back to home</router-link>
        </template>

        <!-- Error -->
        <div v-else-if="error" class="alert alert-error">
          <span>{{ error }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import { doorsApi } from '@/utils/api'
import type { DoorInfo, DoorCheckinResult } from '@/types'

const route = useRoute()
const doorId = computed(() => route.params.id as string)

const info = ref<DoorInfo | null>(null)
const loading = ref(true)
const error = ref<string | null>(null)
const busy = ref(false)
const result = ref<DoorCheckinResult | null>(null)
// Local debounce so a member can't spam the relay from one device.
const lastAttempt = ref(0)

const canUnlock = computed(() => !!info.value?.enabled && !!info.value?.you_are_authorized)

async function load() {
  loading.value = true
  error.value = null
  try {
    const r = await doorsApi.info(doorId.value)
    if (r.success && r.data) info.value = r.data
    else error.value = r.error || 'Failed to load door'
  } catch (e: any) {
    error.value = e?.response?.data?.error || 'Failed to load door'
  } finally {
    loading.value = false
  }
}

async function checkin() {
  if (!canUnlock.value || busy.value) return
  const now = Date.now()
  if (now - lastAttempt.value < 10_000) {
    result.value = { unlocked: false, reason: 'Please wait a few seconds before trying again.' }
    return
  }
  lastAttempt.value = now
  busy.value = true
  result.value = null
  try {
    const r = await doorsApi.checkin(doorId.value)
    if (r.success && r.data) {
      result.value = r.data
    } else {
      result.value = { unlocked: false, reason: r.error || 'Check-in failed' }
    }
  } catch (e: any) {
    result.value = { unlocked: false, reason: e?.response?.data?.error || 'Check-in failed' }
  } finally {
    busy.value = false
  }
}

onMounted(load)
</script>
