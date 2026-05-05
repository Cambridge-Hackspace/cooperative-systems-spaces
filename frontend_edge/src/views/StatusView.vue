<template>
  <div>
    <!-- Header -->
    <div>
        <div class="text-center mb-8">
          <div class="text-6xl mb-4">⚙️</div>
          <h1 class="text-3xl font-bold text-accent">{{ deviceName }}</h1>
          <p class="text-sm text-gray-500">Edge Management</p>
        </div>

        <!-- Loading State -->
        <div v-if="loading" class="flex flex-col items-center py-10">
          <span class="loading loading-spinner loading-lg text-primary"></span>
          <p class="mt-4 text-gray-600">Loading status...</p>
        </div>

        <!-- Main Content -->
        <div v-else>
          <!-- Status Badge -->
          <div class="flex justify-center mb-6">
            <div 
              class="badge badge-lg"
              :class="{
                'badge-success': status.auth_status === 'approved',
                'badge-warning': status.auth_status === 'pending',
                'badge-error': status.auth_status === 'denied' || status.auth_status === 'unauthenticated'
              }"
            >
              {{ statusText }}
            </div>
          </div>

          <!-- System Information -->
          <div class="grid grid-cols-1 md:grid-cols-2 gap-4 mb-6">
            <div class="stat bg-base-200 rounded-lg">
              <div class="stat-title">Status</div>
              <div class="stat-value text-2xl">{{ statusText }}</div>
            </div>
            <div class="stat bg-base-200 rounded-lg">
              <div class="stat-title">Platform</div>
              <div class="stat-value text-2xl">{{ status.system_info?.platform || 'Unknown' }}</div>
            </div>
            <div class="stat bg-base-200 rounded-lg">
              <div class="stat-title">MAC Address</div>
              <div class="stat-value text-lg">{{ status.system_info?.mac_address || 'N/A' }}</div>
            </div>
            <div v-if="status.system_info?.software_version" class="stat bg-base-200 rounded-lg">
              <div class="stat-title">Software Version</div>
              <div class="stat-value text-lg">{{ status.system_info.software_version }}</div>
            </div>
            <div v-if="status.system_info?.ipv4_address" class="stat bg-base-200 rounded-lg">
              <div class="stat-title">IPv4 Address</div>
              <div class="stat-value text-lg">{{ status.system_info.ipv4_address }}</div>
            </div>
          </div>

          <!-- Registration Form (only if not registered) -->
          <div v-if="status.auth_status === 'unauthenticated'" class="mt-6">
            <div role="tablist" class="tabs tabs-boxed mb-6">
              <a 
                role="tab" 
                class="tab" 
                :class="{ 'tab-active': activeTab === 'web' }"
                @click="activeTab = 'web'"
              >
                🌐 Web Registration
              </a>
              <a 
                role="tab" 
                class="tab"
                :class="{ 'tab-active': activeTab === 'cli' }"
                @click="activeTab = 'cli'"
              >
                💻 CLI Registration
              </a>
            </div>

            <!-- Web Registration Tab -->
            <div v-if="activeTab === 'web'">
              <div v-if="registrationError" class="alert alert-error mb-4">
                <span>{{ registrationError }}</span>
              </div>

              <div v-if="registrationSuccess" class="alert alert-success mb-4">
                <span>{{ registrationSuccess }}</span>
              </div>

              <form @submit.prevent="registerDevice" class="space-y-4">
                <div class="form-control">
                  <label class="label">
                    <span class="label-text">CS: Spaces Server URL</span>
                  </label>
                  <input
                    v-model="registrationForm.instance_url"
                    type="url"
                    placeholder="https://your-server.com"
                    class="input input-bordered"
                    required
                    :disabled="registering"
                  />
                  <label class="label">
                    <span class="label-text-alt">Enter the full URL of your CS: Spaces Server</span>
                  </label>
                </div>

                <div class="form-control">
                  <label class="label">
                    <span class="label-text">Device Invite Code</span>
                  </label>
                  <input
                    v-model="registrationForm.device_code"
                    type="text"
                    placeholder="8 emoji characters"
                    class="input input-bordered"
                    required
                    :disabled="registering"
                  />
                  <label class="label">
                    <span class="label-text-alt">Enter the 8-emoji invite code from your administrator</span>
                  </label>
                </div>

                <button 
                  type="submit" 
                  class="btn btn-primary w-full"
                  :disabled="registering"
                >
                  <span v-if="registering" class="loading loading-spinner"></span>
                  {{ registering ? 'Registering...' : 'Register Device' }}
                </button>
              </form>
            </div>

            <!-- CLI Instructions Tab -->
            <div v-if="activeTab === 'cli'">
              <div class="alert alert-info mb-4">
                <span>To register this device using the command line:</span>
              </div>
              <div class="mockup-code">
                <pre><code>edge register --instance-url https://your-server.com --code &lt;8-emoji-code&gt;</code></pre>
              </div>
              <p class="text-sm text-gray-500 mt-4">
                Obtain the invite code from your CS: Spaces Server administrator, then run the command above in your terminal.
              </p>
            </div>
          </div>

          <!-- Already Registered Info -->
          <div v-if="status.auth_status === 'approved'" class="alert alert-success">
            <div>
              <strong>✅ Device is registered!</strong>
              <p class="text-sm mt-1">This device is connected and communicating with the CS: Spaces Server.</p>
            </div>
          </div>

          <div v-if="status.auth_status === 'pending'" class="alert alert-warning">
            <div>
              <strong>⏳ Registration pending</strong>
              <p class="text-sm mt-1">Your device registration is awaiting approval from an administrator.</p>
            </div>
          </div>

          <div v-if="status.auth_status === 'denied'" class="alert alert-error">
            <div>
              <strong>❌ Registration denied</strong>
              <p class="text-sm mt-1">Your device registration was denied. Please contact your administrator.</p>
            </div>
          </div>
        </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { deviceApi } from '@/api/device'
import type { DeviceStatus } from '@/types'

const loading = ref(true)
const status = ref<DeviceStatus>({
  device_name: 'Edge',
  is_registered: false,
  auth_status: 'unauthenticated'
})

const registrationForm = ref({
  instance_url: '',
  device_code: ''
})

const registering = ref(false)
const registrationError = ref<string | null>(null)
const registrationSuccess = ref<string | null>(null)
const activeTab = ref<'web' | 'cli'>('web')

let pollInterval: number | null = null

const deviceName = computed(() => status.value.device_name || 'Edge Device')

const statusText = computed(() => {
  const statusMap = {
    approved: 'Registered',
    pending: 'Registration Pending',
    unauthenticated: 'Not Registered',
    denied: 'Registration Denied'
  }
  return statusMap[status.value.auth_status] || 'Unknown'
})

async function loadStatus() {
  try {
    const result = await deviceApi.getStatus()
    if (result.success && result.data) {
      status.value = result.data
    }
  } catch (error) {
    console.error('Error loading status:', error)
  } finally {
    loading.value = false
  }
}

async function registerDevice() {
  registering.value = true
  registrationError.value = null
  registrationSuccess.value = null

  try {
    const result = await deviceApi.register(registrationForm.value)

    if (result.success) {
      registrationSuccess.value = result.data || 'Registration successful!'
      // Reload status after a short delay
      setTimeout(() => {
        loadStatus()
      }, 2000)
    } else {
      registrationError.value = result.error || 'Registration failed'
    }
  } catch (error: any) {
    registrationError.value = error.response?.data?.error || 'Network error: ' + error.message
  } finally {
    registering.value = false
  }
}

onMounted(() => {
  loadStatus()
  // Poll for status updates every 30 seconds
  pollInterval = window.setInterval(() => {
    loadStatus()
  }, 30000)
})

onUnmounted(() => {
  if (pollInterval) {
    clearInterval(pollInterval)
  }
})
</script>
