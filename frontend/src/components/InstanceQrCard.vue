<template>
  <div class="bg-base-200 rounded-lg p-4">
    <div class="flex items-start justify-between gap-4 mb-3">
      <div>
        <div class="font-medium">Share this instance</div>
        <div class="text-sm text-base-content/70">
          Scan this from the Cooperative Spaces app's first-launch screen to connect
          another device to {{ payload?.name || 'this instance' }}.
        </div>
      </div>
    </div>

    <div v-if="loading" class="text-sm text-base-content/70">Loading…</div>
    <div v-else-if="error" class="text-sm text-error">{{ error }}</div>
    <div v-else-if="qrDataUrl" class="flex flex-col sm:flex-row gap-4 items-center sm:items-start">
      <img
        :src="qrDataUrl"
        alt="Instance QR code"
        class="rounded-lg bg-white p-2 w-56 h-56 shrink-0"
      />
      <div class="text-sm">
        <div class="font-medium">{{ payload?.name }}</div>
        <div class="font-mono break-all text-base-content/70">{{ payload?.url }}</div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import QRCode from 'qrcode'
import { apiClient } from '@/utils/api'

interface InstanceQrPayload {
  v: number
  url: string
  name: string
}

const payload = ref<InstanceQrPayload | null>(null)
const qrDataUrl = ref<string>('')
const loading = ref(true)
const error = ref<string | null>(null)

async function load() {
  loading.value = true
  error.value = null
  try {
    const resp = await apiClient.get<InstanceQrPayload>('/instance/qr')
    if (resp.success && resp.data) {
      payload.value = resp.data
    } else {
      throw new Error(resp.error || 'Failed to fetch instance QR')
    }
  } catch (e: any) {
    error.value = e?.message || 'Failed to fetch instance QR'
  } finally {
    loading.value = false
  }
}

// Encode the full payload JSON (not just the URL) so client onboarding flows
// can read the display name without a second round-trip.
watch(payload, async (p) => {
  if (!p) { qrDataUrl.value = ''; return }
  try {
    qrDataUrl.value = await QRCode.toDataURL(JSON.stringify(p), { margin: 1, width: 256 })
  } catch (e: any) {
    error.value = e?.message || 'Failed to render QR'
  }
})

onMounted(load)
</script>
