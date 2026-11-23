<template>
  <div :class="embedded ? '' : 'container mx-auto px-4 py-8'">
    <div v-if="!embedded" class="mb-6">
      <h1 class="text-3xl font-bold mb-1">Resource Wiring</h1>
      <p class="text-base-content/70">
        Which devices read, switch, latch and watch each resource -- a door, a tool, a room -- and
        the interlocks that gate them.
      </p>
    </div>

    <div v-if="error" class="alert alert-error mb-4" role="alert">{{ error }}</div>

    <div v-if="loading" class="flex justify-center py-8">
      <span class="loading loading-spinner loading-lg" aria-label="Loading"></span>
    </div>

    <!-- Guarded on `!error`: a failed load leaves the list empty, and showing
         the empty state next to the error would report a server failure as a
         fact about the space. -->
    <div v-else-if="wired.length === 0 && !error" class="alert">
      <span>
        No resource has a device bound yet. Bind a reader, a power controller, a strike or a sensor
        to a tool or a door to start.
      </span>
    </div>

    <div v-else class="space-y-6">
      <div v-for="tool in wired" :key="tool.tool_id" class="card bg-base-200 shadow">
        <div class="card-body">
          <div class="flex items-center justify-between gap-4 flex-wrap">
            <h2 class="card-title">
              {{ resourceName(tool) }}
              <span class="badge badge-ghost" :data-testid="`kind-${tool.tool_id}`">{{
                tool.kind
              }}</span>
            </h2>
            <!--
              The fail-safe verdict is stated plainly rather than hidden behind a
              tooltip. A tool whose plug cannot de-energize itself is one where
              the coordinator's cut is a mitigation, not an interlock, and whoever
              is authoring rules for it needs to know that while they do it.
            -->
            <span
              v-if="hasPower(tool)"
              class="badge"
              :class="tool.power_fails_safe ? 'badge-success' : 'badge-warning'"
              :data-testid="`fail-safe-${tool.tool_id}`"
            >
              {{ tool.power_fails_safe ? 'Power fails safe' : 'Power cannot fail safe' }}
            </span>
          </div>

          <!--
            #182: how a session on this resource ends is the resource's to say,
            whatever its kind. A door set to "until stopped" keeps its power
            bindings leased until somebody stops it: the room.
          -->
          <label class="flex items-center gap-2 text-sm">
            <span class="text-base-content/70">Session</span>
            <select
              class="select select-bordered select-xs"
              :value="profileOf(tool)"
              :data-testid="`profile-${tool.tool_id}`"
              @change="setProfile(tool, ($event.target as HTMLSelectElement).value)"
            >
              <option value="momentary">momentary (ends with the strike pulse)</option>
              <option value="until_stopped">until stopped</option>
            </select>
          </label>

          <!--
            #228: a tool can name the member who tapped on its yes, for a device
            that has to know who it is serving, such as a print tower. Doors
            can't: the edge decides them from card digests and has nobody to name.
          -->
          <label v-if="tool.kind !== 'door'" class="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              class="toggle toggle-xs"
              :checked="identifiesMember(tool)"
              :data-testid="`identify-${tool.tool_id}`"
              @change="setIdentify(tool, $event.target as HTMLInputElement)"
            />
            <span>Identify the member on a scan</span>
          </label>

          <!--
            #249: every message about this resource on the local broker must be
            signed. Off until the firmware of the devices bound here signs: on,
            the edge refuses anything unsigned, so an older device stops working.
            Any kind -- a door's strike is what a forged reply would open.
          -->
          <label class="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              class="toggle toggle-xs"
              :checked="requiresSigned(tool)"
              :data-testid="`require-signed-${tool.tool_id}`"
              @change="setRequireSigned(tool, $event.target as HTMLInputElement)"
            />
            <span>Require signed messages</span>
            <span class="text-base-content/60">
              (turn on once every device here has firmware that signs; others stop working)
            </span>
          </label>

          <!--
            #241: a yes can also sign the member in to one of the space's own
            apps, so a card tap at its station is a sign-in. Only apps CS:S may
            sign in without asking -- the space's own, with a secret, enabled --
            are offered; the server holds the same rule and is the authority.
            It names its member, so it shows only while identifying.
          -->
          <div
            v-if="tool.kind !== 'door' && identifiesMember(tool)"
            class="flex items-center gap-4 flex-wrap text-sm"
          >
            <label class="flex items-center gap-2">
              <span>A tap signs in to</span>
              <select
                class="select select-bordered select-xs"
                :value="signInAppOf(tool) ?? ''"
                :data-testid="`sign-in-app-${tool.tool_id}`"
                @change="setSignInApp(tool, ($event.target as HTMLSelectElement).value)"
              >
                <option value="">nothing</option>
                <option v-if="unlistedApp(tool)" :value="unlistedApp(tool)">
                  an app this list can't show ({{ unlistedApp(tool) }})
                </option>
                <option v-for="a in appsFor(tool)" :key="a.id" :value="a.id">
                  {{ a.name }}{{ signInEligible(a) ? '' : ' (no longer eligible)' }}
                </option>
              </select>
            </label>
            <label v-if="signInAppOf(tool)" class="flex items-center gap-2">
              <span>Also ask for an authenticator code from</span>
              <select
                class="select select-bordered select-xs"
                :value="secondFactorOf(tool)"
                :data-testid="`second-factor-${tool.tool_id}`"
                @change="setSecondFactor(tool, ($event.target as HTMLSelectElement).value)"
              >
                <option value="off">nobody</option>
                <option value="admins">admins</option>
                <option value="everyone">everyone</option>
              </select>
            </label>
          </div>

          <!--
            #190: the session as the platform knows it -- a room in use is a
            fact here now, not only on its coordinator -- and the one action an
            administrator has on it.
          -->
          <div
            v-if="sessionOf(tool)"
            class="flex items-center gap-3 flex-wrap text-sm"
            :data-testid="`session-${tool.tool_id}`"
          >
            <span class="badge badge-info">In use</span>
            <span>
              by {{ sessionOf(tool)?.full_name || sessionOf(tool)?.username || 'someone' }} since
              {{ new Date(sessionOf(tool)?.opened_at ?? '').toLocaleString() }}
            </span>
            <button
              class="btn btn-xs btn-outline"
              :data-testid="`end-session-${tool.tool_id}`"
              @click="endSession(tool)"
            >
              End session
            </button>
          </div>

          <p v-if="hasPower(tool) && !tool.power_fails_safe" class="text-sm text-base-content/70">
            No power module bound to this tool reaches a safe state on its own when it loses
            contact. Cuts still work while the link is up, but treat them as a mitigation rather
            than an interlock.
          </p>

          <h3 class="font-semibold mt-4">Modules</h3>
          <div v-if="tool.modules.length === 0" class="text-sm text-base-content/70">
            None bound.
          </div>
          <table v-else class="table table-sm">
            <thead>
              <tr>
                <th>Role</th>
                <th>Name</th>
                <th>On disconnect</th>
                <th>Start</th>
                <th>Run-on</th>
                <th>Pulse</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="m in tool.modules" :key="m.id">
                <td>
                  <span class="badge badge-outline">{{ m.role }}</span>
                </td>
                <td>{{ m.name }}</td>
                <td>
                  <span :class="m.on_disconnect === 'fail_off' ? 'text-success' : ''">
                    {{ m.on_disconnect }}
                  </span>
                </td>
                <td class="whitespace-nowrap">
                  <template v-if="m.role === 'power'">
                    stage {{ m.start_stage ?? 0
                    }}<span v-if="m.start_delay_ms">, then {{ seconds(m.start_delay_ms) }}</span>
                  </template>
                  <span v-else class="text-base-content/50">—</span>
                </td>
                <td class="whitespace-nowrap">
                  <template v-if="m.role === 'power' && m.run_on_ms">{{
                    seconds(m.run_on_ms)
                  }}</template>
                  <span v-else class="text-base-content/50">—</span>
                </td>
                <td class="whitespace-nowrap" :data-testid="`pulse-${m.id}`">
                  <!--
                    #182: a strike's profile. The hold is stated either way,
                    because a latch that is NOT held during open access is the
                    one an administrator authoring a window needs to know about.
                  -->
                  <template v-if="m.role === 'strike'">
                    {{ seconds(m.pulse_ms ?? 0) }},
                    {{ m.hold_open_access === false ? 'not held' : 'held' }} for open access
                  </template>
                  <span v-else class="text-base-content/50">—</span>
                </td>
              </tr>
            </tbody>
          </table>

          <h3 class="font-semibold mt-4">Interlocks</h3>
          <div v-if="tool.interlocks.length === 0" class="text-sm text-base-content/70">
            None configured.
          </div>
          <table v-else class="table table-sm">
            <thead>
              <tr>
                <th>Kind</th>
                <th>Condition</th>
                <th>Enforced at</th>
                <th>Latches</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="i in tool.interlocks" :key="i.id">
                <td>
                  <span class="badge" :class="i.kind === 'trip' ? 'badge-error' : 'badge-info'">{{
                    i.kind === 'trip' ? 'trip' : 'start gate'
                  }}</span>
                </td>
                <td>{{ i.condition }}</td>
                <td>
                  <!--
                    The tier is worth showing next to the rule: `edge` means the
                    trip travels through the coordinator, which is a different
                    survivability story from `firmware`, and the person reading
                    this is the one who has to know which they have.
                  -->
                  <span class="badge badge-outline" :data-testid="`enforcement-${i.id}`">{{
                    i.enforcement
                  }}</span>
                </td>
                <td>{{ i.latch ? 'yes' : 'no' }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { useReloadOnReactivate } from '@/composables/useReloadOnReactivate'
import { doorsApi, oauthAdminApi, resourcesApi, toolModulesApi, toolsApi } from '@/utils/api'
import type {
  Door,
  OAuthClient,
  OpenSessionView,
  ResourceSummary,
  SecondFactor,
  ToolModuleStateTool,
  UpdateResourceRequest,
} from '@/types'
import type { Tool } from '@/types/tools'

defineProps<{ embedded?: boolean }>()

const loading = ref(true)
const error = ref('')
const resources = ref<ToolModuleStateTool[]>([])
const catalog = ref<Tool[]>([])
const doors = ref<Door[]>([])
const sessions = ref<OpenSessionView[]>([])
const settings = ref<ResourceSummary[]>([])
const apps = ref<OAuthClient[]>([])

const wired = computed(() => resources.value)

/** `1500` → `1.5 s`: the timings are stored in ms and read in seconds. */
function seconds(ms: number): string {
  return `${(ms / 1000).toLocaleString(undefined, { maximumFractionDigits: 1 })} s`
}

/** Prefer the resource's real name; fall back to whatever identifier we have. */
function resourceName(t: ToolModuleStateTool): string {
  const match =
    t.kind === 'door'
      ? doors.value.find((d) => d.id === t.tool_id)
      : catalog.value.find((c) => c.id === t.tool_id)
  return match?.name ?? t.external_id ?? t.tool_id
}

/** The profile as shipped, or the kind's default from a server that predates it. */
function profileOf(t: ToolModuleStateTool): string {
  if (t.session_profile) return t.session_profile
  return t.kind === 'door' ? 'momentary' : 'until_stopped'
}

async function setProfile(t: ToolModuleStateTool, profile: string) {
  if (profile !== 'momentary' && profile !== 'until_stopped') return
  const r = await resourcesApi.update(t.tool_id, { session_profile: profile })
  if (r.success) {
    t.session_profile = profile
    error.value = ''
  } else {
    error.value = r.error || 'The session profile was not changed'
  }
}

/** #228: whether this tool names the member on a yes. */
function identifiesMember(t: ToolModuleStateTool): boolean {
  return settings.value.find((s) => s.id === t.tool_id)?.identify_member ?? false
}

async function setIdentify(t: ToolModuleStateTool, box: HTMLInputElement) {
  const identify = box.checked
  // A sign-in names its member (#241), so turning identifying off turns the
  // sign-in off with it, in the one request the server accepts that in.
  const body: UpdateResourceRequest = { identify_member: identify }
  if (!identify && signInAppOf(t)) body.sign_in_app = ''
  if (!(await saveSetting(t, body, 'Identifying the member was not changed'))) {
    // Refused: the switch shows what the server holds, not what was clicked.
    box.checked = identifiesMember(t)
  }
}

/** #249: whether messages about this resource must be signed. */
function requiresSigned(t: ToolModuleStateTool): boolean {
  return settings.value.find((s) => s.id === t.tool_id)?.require_signed ?? false
}

async function setRequireSigned(t: ToolModuleStateTool, box: HTMLInputElement) {
  const body: UpdateResourceRequest = { require_signed: box.checked }
  if (!(await saveSetting(t, body, 'Requiring signed messages was not changed'))) {
    box.checked = requiresSigned(t)
  }
}

/** Send a change of a resource's settings, keeping what the server answers. */
async function saveSetting(
  t: ToolModuleStateTool,
  body: UpdateResourceRequest,
  failed: string
): Promise<boolean> {
  const r = await resourcesApi.update(t.tool_id, body)
  if (r.success && r.data) {
    const updated = r.data
    settings.value = [...settings.value.filter((s) => s.id !== t.tool_id), updated]
    error.value = ''
    return true
  }
  error.value = r.error || failed
  return false
}

/** #241: the app a yes on this tool signs the member in to, if any. */
function signInAppOf(t: ToolModuleStateTool): string | null {
  return settings.value.find((s) => s.id === t.tool_id)?.sign_in_app ?? null
}

/** #241: who also gives a code; off from a server that predates the setting. */
function secondFactorOf(t: ToolModuleStateTool): SecondFactor {
  return settings.value.find((s) => s.id === t.tool_id)?.second_factor ?? 'off'
}

/**
 * #241: an app CS:S may sign a member in to on a card tap: the space's own
 * (it skips consent), able to keep a secret, and enabled. The server holds
 * the same rule (`oidc::card::app_for`) and refuses anything else.
 */
function signInEligible(a: OAuthClient): boolean {
  return a.skip_consent && a.confidential && !a.disabled
}

/** The apps to offer: the eligible ones, and the current one even if it no longer is. */
function appsFor(t: ToolModuleStateTool): OAuthClient[] {
  const current = signInAppOf(t)
  return apps.value.filter((a) => signInEligible(a) || a.id === current)
}

/** The current app when the app list does not have it, so it is never shown as nothing. */
function unlistedApp(t: ToolModuleStateTool): string | null {
  const current = signInAppOf(t)
  return current && !apps.value.some((a) => a.id === current) ? current : null
}

async function setSignInApp(t: ToolModuleStateTool, app: string) {
  await saveSetting(t, { sign_in_app: app }, 'The sign-in app was not changed')
}

async function setSecondFactor(t: ToolModuleStateTool, factor: string) {
  if (factor !== 'off' && factor !== 'admins' && factor !== 'everyone') return
  await saveSetting(t, { second_factor: factor }, 'The second factor was not changed')
}

/** The open session on this resource, if the platform knows of one (#190). */
function sessionOf(t: ToolModuleStateTool): OpenSessionView | undefined {
  return sessions.value.find((s) => s.resource_id === t.tool_id)
}

async function endSession(t: ToolModuleStateTool) {
  const r = await resourcesApi.stop(t.tool_id)
  if (r.success) {
    sessions.value = sessions.value.filter((s) => s.resource_id !== t.tool_id)
    error.value = ''
  } else {
    error.value = r.error || 'The session was not ended'
  }
}

/**
 * The fail-safe verdict is about power modules. A door with a strike and a
 * coordinator has none, and "cannot fail safe" on it would be a warning about
 * a plug it does not have.
 */
function hasPower(t: ToolModuleStateTool): boolean {
  return t.modules.some((m) => m.role === 'power')
}

async function load() {
  loading.value = true
  error.value = ''
  try {
    const [snapshot, all, allDoors, open, attrs, clients] = await Promise.all([
      toolModulesApi.state(),
      toolsApi.getTools(),
      doorsApi.list(),
      resourcesApi.sessions(),
      resourcesApi.list(),
      oauthAdminApi.list(),
    ])
    if (snapshot?.success && snapshot.data) resources.value = snapshot.data.tools ?? []
    if (all?.success && all.data) catalog.value = all.data
    if (allDoors?.success && allDoors.data) doors.value = allDoors.data
    if (open?.success && open.data) sessions.value = open.data
    if (attrs?.success && attrs.data) settings.value = attrs.data
    // Without the app list (an admin who may not manage apps), a tool already
    // signing in still shows its app, by id, rather than nothing.
    if (clients?.success && clients.data) apps.value = clients.data
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load resource wiring'
  } finally {
    loading.value = false
  }
}

useReloadOnReactivate(load)
</script>
