<template>
  <div class="container mx-auto px-4 py-8 max-w-2xl">
    <div class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/" class="link">Home</router-link></li>
        <li><router-link :to="`/profile/me`" class="link">Profile</router-link></li>
        <li>Transit Card</li>
      </ul>
    </div>

    <h1 class="text-3xl font-bold mb-2">{{ fieldLabel }}</h1>
    <p class="text-base-content/70 mb-6">
      This is the ID your card presents to door readers and ToolGuard. Scan your card with a phone
      that supports Web NFC, or enter the ID manually if you already know it.
    </p>

    <div v-if="flash" class="alert mb-4" :class="flashOk ? 'alert-success' : 'alert-error'">
      <span>{{ flash }}</span>
      <button class="btn btn-ghost btn-xs" @click="flash = ''">✕</button>
    </div>

    <div v-if="loading" class="text-center py-12">
      <span class="loading loading-spinner loading-lg"></span>
    </div>

    <section v-else class="card bg-base-100 shadow-md">
      <div class="card-body">
        <h2 class="card-title">Scan a card</h2>

        <div v-if="nfcSupported">
          <p class="text-sm text-base-content/70 mb-2">
            Tap "Scan Card", then hold your card against the back of your phone.
          </p>
          <button class="btn btn-primary btn-sm w-fit" :disabled="scanning" @click="scanCard">
            <span v-if="scanning" class="loading loading-spinner loading-xs"></span>
            {{ scanning ? 'Waiting for card…' : 'Scan Card' }}
          </button>
          <button v-if="scanning" class="btn btn-ghost btn-sm w-fit ml-2" @click="stopScan">
            Cancel
          </button>
          <p v-if="scanError" class="text-sm text-error mt-2">{{ scanError }}</p>
        </div>

        <div v-else class="alert alert-info">
          <span>
            Web NFC isn't available on this browser or device — it currently only works in Chrome on
            Android over HTTPS. Enter the card ID manually below instead.
          </span>
        </div>

        <div class="divider">or enter manually</div>

        <label class="label"
          ><span class="label-text">{{ fieldLabel }}</span></label
        >
        <input
          v-model="cardId"
          type="text"
          class="input input-bordered font-mono"
          :placeholder="fieldHelp || 'Card ID'"
        />

        <div class="flex justify-end gap-2 mt-4">
          <button class="btn btn-primary btn-sm" :disabled="saving" @click="save">
            <span v-if="saving" class="loading loading-spinner loading-xs"></span>
            Save
          </button>
        </div>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useConfigStore } from '@/stores/config'
import { profileApi } from '@/utils/api'
import type { ProfileField } from '@/types'

const authStore = useAuthStore()
const configStore = useConfigStore()

const loading = ref(true)
const saving = ref(false)
const scanning = ref(false)
const scanError = ref('')
const flash = ref('')
const flashOk = ref(true)

const cardId = ref('')
const existingProfile = ref<Record<string, any>>({})
const fieldMeta = ref<ProfileField | null>(null)

const fieldKey = computed(() => configStore.cardIdField() || 'card_id')
const fieldLabel = computed(() => fieldMeta.value?.label || 'Transit Card ID')
const fieldHelp = computed(() => fieldMeta.value?.help_text || '')

// Web NFC (NDEFReader) isn't in TypeScript's DOM lib yet, and is only
// implemented by Chrome on Android over a secure context.
const nfcSupported = typeof window !== 'undefined' && 'NDEFReader' in window

let abortController: AbortController | null = null

async function scanCard() {
  scanError.value = ''
  if (!nfcSupported) return

  try {
    scanning.value = true
    abortController = new AbortController()
    const NDEFReaderCtor = (window as any).NDEFReader
    const reader = new NDEFReaderCtor()
    await reader.scan({ signal: abortController.signal })

    reader.onreading = (event: any) => {
      // The tag's hardware serial number is what door/ToolGuard readers key
      // off of — not the NDEF message content, which most access cards
      // don't even carry.
      if (event.serialNumber) {
        cardId.value = event.serialNumber
      }
      scanning.value = false
    }
    reader.onreadingerror = () => {
      scanError.value = 'Could not read that card. Try holding it steady against the phone.'
      scanning.value = false
    }
  } catch (err: any) {
    if (err?.name !== 'AbortError') {
      scanError.value = err?.message || 'Failed to start NFC scan.'
    }
    scanning.value = false
  }
}

function stopScan() {
  abortController?.abort()
  scanning.value = false
}

async function loadProfile() {
  loading.value = true
  try {
    const userId = authStore.user?.id
    if (!userId) return

    const [profileRes, configRes] = await Promise.all([
      profileApi.getUserProfile(userId),
      profileApi.getProfileConfig(),
    ])

    if (profileRes.success && profileRes.data) {
      existingProfile.value = profileRes.data.profile || {}
      cardId.value = existingProfile.value[fieldKey.value] || ''
    }
    if (configRes.success && configRes.data) {
      fieldMeta.value = configRes.data.profile_fields.find((f) => f.key === fieldKey.value) || null
    }
  } catch (err: any) {
    flashOk.value = false
    flash.value = err?.message || 'Failed to load your profile.'
  } finally {
    loading.value = false
  }
}

async function save() {
  const userId = authStore.user?.id
  if (!userId) return

  saving.value = true
  flash.value = ''
  try {
    const updatedProfile = { ...existingProfile.value, [fieldKey.value]: cardId.value }
    const res = await profileApi.updateUserProfile(userId, { profile: updatedProfile })
    if (res.success) {
      existingProfile.value = updatedProfile
      flashOk.value = true
      flash.value = 'Card ID saved.'
    } else {
      flashOk.value = false
      flash.value = res.error || 'Failed to save.'
    }
  } catch (err: any) {
    flashOk.value = false
    flash.value = err?.response?.data?.error || err?.message || 'Failed to save.'
  } finally {
    saving.value = false
  }
}

onMounted(loadProfile)
onUnmounted(() => abortController?.abort())
</script>
