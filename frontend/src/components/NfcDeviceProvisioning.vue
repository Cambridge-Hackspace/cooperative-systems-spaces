<template>
  <div class="container mx-auto px-4 py-8 max-w-3xl">
    <h1 class="text-3xl font-bold mb-6">NFC Reader Provisioning</h1>

    <!-- Step 1: Wiring -->
    <section class="card bg-base-100 shadow-md mb-6">
      <div class="card-body">
        <h2 class="card-title">1. Wire the reader</h2>
        <p class="text-sm text-base-content/70 mb-2">
          Both MFRC522 readers share the SPI bus, power, and ground — only chip-select is
          per-reader. Use 3.3V power only; the reader is not 5V tolerant.
        </p>
        <div class="overflow-x-auto">
          <table class="table table-sm">
            <thead>
              <tr>
                <th>MFRC522 pin</th>
                <th>ESP32-C3 GPIO</th>
                <th>Notes</th>
              </tr>
            </thead>
            <tbody>
              <tr><td>SCK</td><td>GPIO 10</td><td>shared by both readers</td></tr>
              <tr><td>MOSI (SDA/MOSI)</td><td>GPIO 5</td><td>shared</td></tr>
              <tr><td>MISO</td><td>GPIO 7</td><td>shared</td></tr>
              <tr><td>SS / SDA (CS) — reader 0</td><td>GPIO 6</td><td>channel 0</td></tr>
              <tr><td>SS / SDA (CS) — reader 1</td><td>GPIO 4</td><td>channel 1 (optional)</td></tr>
              <tr><td>RST</td><td>GPIO 9</td><td>driven high at boot to enable the reader</td></tr>
              <tr><td>3.3V</td><td>3V3</td><td>do not use 5V</td></tr>
              <tr><td>GND</td><td>GND</td><td>shared</td></tr>
            </tbody>
          </table>
        </div>
        <p class="text-sm text-base-content/70 mt-2">
          Single-reader build: wire only reader 0 to the channel-0 CS (GPIO 6); channel 1
          stays idle. The reader's IRQ pin doesn't need to be wired — the firmware polls
          instead of using interrupts.
        </p>
      </div>
    </section>

    <!-- Step 2: Flash -->
    <section class="card bg-base-100 shadow-md mb-6">
      <div class="card-body">
        <h2 class="card-title">2. Flash firmware</h2>
        <p class="text-sm text-base-content/70 mb-2">
          Download the generic reader firmware, then flash it onto the device using
          ESPHome Web (a browser-based flashing tool) over USB.
        </p>
        <ol class="list-decimal list-inside space-y-2 text-sm">
          <li>
            <a :href="firmwareUrl" download class="btn btn-secondary btn-sm">
              ⬇️ Download Firmware
            </a>
          </li>
          <li>
            Open
            <a href="https://web.esphome.io/" target="_blank" rel="noopener" class="link link-primary">
              ESPHome Web
            </a>
            and click <strong>Connect</strong>, then select the device over USB.
          </li>
          <li>
            Choose the option to install a custom firmware file and select the file you
            just downloaded.
          </li>
        </ol>
      </div>
    </section>

    <!-- Step 3: Configure -->
    <section class="card bg-base-100 shadow-md mb-6">
      <div class="card-body">
        <h2 class="card-title">3. Configure settings</h2>
        <p class="text-sm text-base-content/70 mb-4">
          Once flashed, the device is ready to be configured over the same USB
          connection — no need to join its temporary Wi-Fi network.
        </p>

        <div v-if="!serialSupported" class="alert alert-warning text-sm">
          <span>
            This browser doesn't support the Web Serial API. Use Chrome or Edge to
            configure the device from this page — or join the device's own
            <code>scan-setup-&lt;id&gt;</code> Wi-Fi network and use its captive portal
            instead.
          </span>
        </div>

        <template v-else>
          <div v-if="!connected" class="flex flex-col items-start gap-2">
            <button
              @click="connectSerial"
              :disabled="connecting"
              class="btn btn-primary btn-sm"
            >
              <span v-if="connecting" class="loading loading-spinner loading-xs"></span>
              {{ connecting ? 'Connecting…' : 'Connect to Device' }}
            </button>
            <p v-if="connectError" class="text-error text-sm">{{ connectError }}</p>
          </div>

          <div v-else>
            <p class="flex items-center gap-3 mb-4 text-sm">
              Connected to device
              <code class="font-mono bg-base-200 px-2 py-1 rounded">{{ deviceId || 'unknown' }}</code>
              <button
                @click="toggleLed"
                :disabled="togglingLed"
                class="btn btn-secondary btn-xs"
              >
                💡 Toggle LED
              </button>
            </p>
            <p v-if="ledError" class="text-error text-sm mb-4">{{ ledError }}</p>

            <p v-if="existingConfig?.fw_version" class="text-sm mb-4">
              Firmware version:
              <code class="font-mono bg-base-200 px-2 py-1 rounded">{{ existingConfig.fw_version }}</code>
            </p>

            <div v-if="loadingExisting" class="text-sm text-base-content/70 mb-4">
              Checking current settings…
            </div>

            <div v-if="existingConfig?.provisioned" class="alert alert-info text-sm mb-4">
              <span>
                This device is already configured — connected to Wi-Fi network
                <strong>{{ existingConfig.wifi_ssid }}</strong> and MQTT broker
                <strong>{{ existingConfig.mqtt_host }}:{{ existingConfig.mqtt_port }}</strong
                >. Passwords aren't shown or prefilled; leave a password field blank to keep
                it unchanged, or enter a new one to replace it.
              </span>
            </div>

            <div class="form-control mb-3">
              <label class="label" for="wifi_ssid"><span class="label-text">Wi-Fi SSID</span></label>
              <input
                id="wifi_ssid"
                v-model="form.wifi_ssid"
                type="text"
                class="input input-bordered w-full"
              />
            </div>
            <div class="form-control mb-3">
              <label class="label" for="wifi_pass">
                <span class="label-text">Wi-Fi Password</span>
                <span v-if="existingConfig?.provisioned" class="label-text-alt">
                  leave blank to keep current
                </span>
              </label>
              <input
                id="wifi_pass"
                v-model="form.wifi_pass"
                type="password"
                class="input input-bordered w-full"
              />
            </div>
            <div class="form-control mb-3">
              <label class="label" for="mqtt_host"><span class="label-text">MQTT Broker Host</span></label>
              <input
                id="mqtt_host"
                v-model="form.mqtt_host"
                type="text"
                class="input input-bordered w-full"
              />
            </div>
            <div class="form-control mb-3">
              <label class="label" for="mqtt_port"><span class="label-text">MQTT Port</span></label>
              <input
                id="mqtt_port"
                v-model.number="form.mqtt_port"
                type="number"
                class="input input-bordered w-full"
              />
            </div>
            <div class="form-control mb-3">
              <label class="label" for="mqtt_username">
                <span class="label-text">MQTT Username (optional)</span>
              </label>
              <input
                id="mqtt_username"
                v-model="form.mqtt_username"
                type="text"
                class="input input-bordered w-full"
              />
            </div>
            <div class="form-control mb-3">
              <label class="label" for="mqtt_password">
                <span class="label-text">MQTT Password (optional)</span>
                <span v-if="existingConfig?.provisioned" class="label-text-alt">
                  leave blank to keep current
                </span>
              </label>
              <input
                id="mqtt_password"
                v-model="form.mqtt_password"
                type="password"
                class="input input-bordered w-full"
              />
            </div>
            <div class="form-control mb-3">
              <label class="label cursor-pointer justify-start gap-2" for="mqtt_use_tls">
                <input
                  id="mqtt_use_tls"
                  v-model="form.mqtt_use_tls"
                  type="checkbox"
                  class="checkbox checkbox-sm"
                />
                <span class="label-text">Use TLS (mqtts)</span>
              </label>
            </div>
            <div class="form-control mb-3">
              <label class="label" for="topic_root"><span class="label-text">Topic Root</span></label>
              <input
                id="topic_root"
                v-model="form.topic_root"
                type="text"
                class="input input-bordered w-full"
              />
            </div>

            <div class="flex gap-2 mt-4">
              <button
                @click="saveConfig"
                :disabled="saving"
                class="btn btn-primary btn-sm"
              >
                <span v-if="saving" class="loading loading-spinner loading-xs"></span>
                {{ saving ? 'Saving…' : 'Save Configuration' }}
              </button>
            </div>

            <p v-if="saveOk" class="text-success text-sm mt-2">
              Configuration saved. The device will reboot into normal operation.
            </p>
            <p v-if="saveError" class="text-error text-sm mt-2">{{ saveError }}</p>
          </div>
        </template>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, onBeforeUnmount } from 'vue'

const firmwareUrl = '/firmware/nfc-522/0.1.0/merged-firmware.bin'

interface DeviceConfig {
  wifi_ssid: string
  wifi_pass: string
  mqtt_host: string
  mqtt_port: number
  mqtt_username: string
  mqtt_password: string
  mqtt_use_tls: boolean
  topic_root: string
}

// What the device reports for `GET` — passwords are never included (see the
// firmware's `redact()`), so this is a strict subset of DeviceConfig.
interface ExistingConfig {
  provisioned: boolean
  fw_version: string
  wifi_ssid: string
  mqtt_host: string
  mqtt_port: number
  mqtt_username: string | null
  mqtt_use_tls: boolean
  topic_root: string
}

const serialSupported = typeof navigator !== 'undefined' && 'serial' in navigator

const connecting = ref(false)
const connected = ref(false)
const connectError = ref('')
const deviceId = ref<string | null>(null)

const loadingExisting = ref(false)
const existingConfig = ref<ExistingConfig | null>(null)

const togglingLed = ref(false)
const ledError = ref('')

const saving = ref(false)
const saveOk = ref(false)
const saveError = ref('')

const form = reactive<DeviceConfig>({
  wifi_ssid: '',
  wifi_pass: '',
  mqtt_host: '',
  mqtt_port: 1883,
  mqtt_username: '',
  mqtt_password: '',
  mqtt_use_tls: false,
  topic_root: 'neiam',
})

let port: SerialPort | null = null
let reader: ReadableStreamDefaultReader<string> | null = null
let writer: WritableStreamDefaultWriter<string> | null = null

// Lines the device sends are all prefixed `NFC522:`; everything else on the
// wire is ESP-IDF log noise sharing the same UART and is ignored.
const lineListeners: Array<(rest: string) => void> = []

async function readLoop() {
  if (!reader) return
  let buffer = ''
  try {
    // eslint-disable-next-line no-constant-condition
    while (true) {
      const { value, done } = await reader.read()
      if (done) break
      buffer += value
      let idx
      while ((idx = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, idx).trim()
        buffer = buffer.slice(idx + 1)
        if (line.startsWith('NFC522:')) {
          const rest = line.slice('NFC522:'.length)
          for (const listener of [...lineListeners]) listener(rest)
        }
      }
    }
  } catch {
    // Port closed or read error — connection state reflects this already.
  }
}

function waitForLine(
  predicate: (rest: string) => boolean,
  timeoutMs = 5000
): Promise<string> {
  return new Promise((resolve, reject) => {
    const listener = (rest: string) => {
      if (!predicate(rest)) return
      clearTimeout(timer)
      const idx = lineListeners.indexOf(listener)
      if (idx >= 0) lineListeners.splice(idx, 1)
      resolve(rest)
    }
    const timer = setTimeout(() => {
      const idx = lineListeners.indexOf(listener)
      if (idx >= 0) lineListeners.splice(idx, 1)
      reject(new Error('Timed out waiting for a response from the device.'))
    }, timeoutMs)
    lineListeners.push(listener)
  })
}

async function connectSerial() {
  connectError.value = ''
  connecting.value = true
  try {
    port = await navigator.serial.requestPort()
    await port.open({ baudRate: 115200 })

    if (!port.readable || !port.writable) {
      throw new Error('Device port is not readable/writable.')
    }

    const textDecoder = new TextDecoderStream()
    port.readable.pipeTo(textDecoder.writable).catch(() => {})
    reader = textDecoder.readable.getReader()
    readLoop()

    const textEncoder = new TextEncoderStream()
    textEncoder.readable.pipeTo(port.writable).catch(() => {})
    writer = textEncoder.writable.getWriter()

    connected.value = true

    // The device announces itself once with READY on boot; if we connected
    // after that already printed, fall back to a PING/PONG probe.
    const readyPromise = waitForLine((rest) => rest.startsWith('READY '), 3000).catch(
      () => null
    )
    await writer.write('NFC522:PING\n')
    const pongPromise = waitForLine((rest) => rest.startsWith('PONG '), 3000).catch(
      () => null
    )
    const rest = (await readyPromise) ?? (await pongPromise)
    if (rest) {
      deviceId.value = rest.split(' ')[1] || null
    }

    await fetchExistingConfig()
  } catch (err: any) {
    connectError.value = err?.message || 'Failed to connect to device.'
    connected.value = false
  } finally {
    connecting.value = false
  }
}

async function fetchExistingConfig() {
  if (!writer) return
  loadingExisting.value = true
  try {
    const resultPromise = waitForLine((rest) => rest.startsWith('CFG '), 3000)
    await writer.write('NFC522:GET\n')
    const rest = await resultPromise
    const cfg = JSON.parse(rest.slice('CFG '.length)) as ExistingConfig
    existingConfig.value = cfg

    // Prefill everything except passwords — the device never sends those
    // back, and leaving the password fields blank is how the firmware knows
    // to keep the stored password unchanged on save.
    form.wifi_ssid = cfg.wifi_ssid
    form.mqtt_host = cfg.mqtt_host
    form.mqtt_port = cfg.mqtt_port
    form.mqtt_username = cfg.mqtt_username || ''
    form.mqtt_use_tls = cfg.mqtt_use_tls
    form.topic_root = cfg.topic_root
  } catch {
    // Non-fatal — the device may be running older firmware without GET
    // support. The form just starts blank in that case.
  } finally {
    loadingExisting.value = false
  }
}

async function toggleLed() {
  ledError.value = ''
  if (!writer) return

  togglingLed.value = true
  try {
    const resultPromise = waitForLine(
      (rest) => rest.startsWith('LED ') || rest.startsWith('ERR '),
      3000
    )
    await writer.write('NFC522:LED toggle\n')
    const rest = await resultPromise
    if (rest.startsWith('ERR ')) {
      ledError.value = rest.slice('ERR '.length)
    }
  } catch (err: any) {
    ledError.value = err?.message || 'Failed to toggle LED.'
  } finally {
    togglingLed.value = false
  }
}

async function saveConfig() {
  saveError.value = ''
  saveOk.value = false
  if (!writer) return

  saving.value = true
  try {
    const cfg = {
      wifi_ssid: form.wifi_ssid,
      wifi_pass: form.wifi_pass,
      mqtt_host: form.mqtt_host,
      mqtt_port: form.mqtt_port,
      mqtt_username: form.mqtt_username || null,
      mqtt_password: form.mqtt_password || null,
      mqtt_use_tls: form.mqtt_use_tls,
      topic_root: form.topic_root || 'neiam',
    }
    const resultPromise = waitForLine(
      (rest) => rest === 'OK' || rest.startsWith('ERR '),
      5000
    )
    await writer.write(`NFC522:SET ${JSON.stringify(cfg)}\n`)
    const rest = await resultPromise
    if (rest === 'OK') {
      saveOk.value = true
    } else {
      saveError.value = rest.slice('ERR '.length)
    }
  } catch (err: any) {
    saveError.value = err?.message || 'Failed to save configuration.'
  } finally {
    saving.value = false
  }
}

async function disconnectSerial() {
  try {
    await reader?.cancel()
  } catch {
    // ignore
  }
  try {
    writer?.releaseLock()
  } catch {
    // ignore
  }
  try {
    await port?.close()
  } catch {
    // ignore
  }
  reader = null
  writer = null
  port = null
}

onBeforeUnmount(() => {
  disconnectSerial()
})
</script>
