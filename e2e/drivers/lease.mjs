// The module lease (#83), exercised against a real broker and a real edge.
//
// What this proves, and what nothing else can: that the interlock engine's
// decisions reach a module. `edge/src/modules.rs` has had a complete, correct,
// heavily unit-tested lease engine since #83 and it ran nowhere -- `tick()` had
// thirty callers, every one of them a test. A unit suite cannot see that it is
// the only caller; that is the exact shape of its blind spot. So this stage
// subscribes to the broker and watches.
//
// Three windows, in order, because the claim is a sequence rather than a state:
//
//   1. before the module has ever reported -- leases arrive, and REFUSE. A
//      `fail_off` binding never heard from must not be granted, and proving the
//      refusal arrives is what separates "the watchdog is running" from "the
//      broker is quiet".
//   2. after one power report naming the device -- leases arrive and GRANT.
//      This is the liveness ingest, which was also called only from tests.
//   3. after the edge is stopped -- nothing arrives at all. The inversion:
//      permission is withdrawn by silence, not by a message, so an edge that
//      dies cannot leave a tool energized on a promise nobody is renewing.
//
// Window 3 is an assertion of absence, so window 1 is its precondition and is
// asserted first. Without that ordering a silent window 3 could equally mean
// "the lease never worked at all", and the failure would present several
// inferences away from its cause.
//
// What this does NOT prove: that a physical relay opens. Nothing here switches
// hardware. It proves the decision leaves the coordinator correctly addressed
// and correctly timed; what a module does with it is firmware's half of the
// contract, and FIRMWARE.md is where that half is stated.
//
// Cluster encoding: binding a module needs a registered device, which needs an
// invite code of eight emoji, so on a cluster that cannot store one the whole
// stage skips -- as in toolmodules.mjs and bypass.mjs.

import fs from 'node:fs'
import path from 'node:path'
import { GET, POST, adminAccount, assertEq, ok, record, main } from './lib.mjs'

const STACK_DIR = process.env.CSS_STACK_DIR ?? '/stack'
const ENCODING = process.env.CSS_DB_ENCODING ?? 'UTF8'
const CAN_REGISTER_DEVICE = ENCODING === 'UTF8' || ENCODING === 'SQL_ASCII'

const FIXTURE = path.join(STACK_DIR, 'lease-fixture.json')
const SKIPPED = path.join(STACK_DIR, 'lease-skipped')

const phase = process.argv[2]
const mqttPort = process.argv[3] ?? '1883'
const devicePepper = process.argv[4] ?? ''

/** Every JSON object mosquitto_sub captured, one per line, bad lines dropped. */
function captured(name) {
  const file = path.join(STACK_DIR, name)
  if (!fs.existsSync(file)) return null
  return fs
    .readFileSync(file, 'utf8')
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
    .map((l) => {
      try {
        return JSON.parse(l)
      } catch {
        return null
      }
    })
    .filter(Boolean)
}

// ── setup ────────────────────────────────────────────────────────────────────

async function setup() {
  if (!CAN_REGISTER_DEVICE) {
    fs.writeFileSync(SKIPPED, `${ENCODING}\n`)
    record(
      'lease/not-run-on-this-cluster',
      'skip',
      `the coordinator needs a module bound to a registered device, a device ` +
        `needs an invite, and this cluster (${ENCODING}) cannot store an ` +
        `eight-emoji invite code.`
    )
    return
  }
  if (fs.existsSync(SKIPPED)) fs.unlinkSync(SKIPPED)

  const admin = await adminAccount('lease_admin')
  const tag = admin.username

  const tool = await POST('/api/tools', {
    token: admin.token,
    body: { name: `Lease Laser ${tag}`, category: 'laser_cutting', external_id: `lease-${tag}` },
  })
  const toolId = tool.json?.data?.id
  ok('lease/tool-created', !!toolId, `POST /api/tools -> ${tool.status}`)

  // Two devices, because that is the shape #83 exists for: the coordinator and
  // the thing it coordinates are separate hardware. Binding the edge to itself
  // would pass just as well and would quietly stop testing the decoupling.
  const claim = async (name, kind, mac) => {
    const invite = await POST('/api/admin/devices/invite', {
      token: admin.token,
      body: { expires_in_hours: 1 },
    })
    const reg = await POST('/api/devices/register', {
      body: {
        device_code: invite.json?.data?.device_code,
        name,
        kind,
        mac_address: mac,
        software_version: '0.0.0-e2e',
        platform: 'linux',
      },
    })
    return {
      id: reg.json?.data?.device_id ?? reg.json?.device_id,
      token: reg.json?.data?.auth_token ?? reg.json?.auth_token,
      status: reg.status,
      text: reg.text,
    }
  }

  const edge = await claim(`lease-edge-${tag}`, 'edge', '02:00:00:00:83:01')
  ok('lease/edge-registered', !!edge.id, `register -> ${edge.status} ${edge.text.slice(0, 160)}`)
  ok('lease/edge-got-a-token', !!edge.token, 'registration must return an auth_token')

  const plug = await claim(`lease-plug-${tag}`, 'power_controller', '02:00:00:00:83:02')
  ok('lease/plug-registered', !!plug.id, `register -> ${plug.status} ${plug.text.slice(0, 160)}`)

  const authToken = edge.token

  // on_disconnect is left unset so the server applies its own default. The
  // default is `fail_off`, and testing against the default is the point: it is
  // what a real power module gets, and it is the policy whose liveness
  // requirement was unsatisfiable until the ingest existed.
  const binding = await POST('/api/admin/tool-modules', {
    token: admin.token,
    body: { tool_id: toolId, device_id: plug.id, role: 'power', name: `lease plug ${tag}` },
  })
  assertEq('lease/module-bound', 201, binding.status)
  assertEq(
    'lease/binding-defaults-to-fail-off',
    'fail_off',
    binding.json?.data?.on_disconnect,
    'the whole stage is about the policy that must not be granted while silent'
  )

  const fixture = {
    tag,
    toolId,
    externalId: `lease-${tag}`,
    // The device the lease is ADDRESSED to -- the plug, not the coordinator.
    deviceId: plug.id,
    edgeId: edge.id,
    moduleId: binding.json?.data?.id,
  }
  fs.writeFileSync(FIXTURE, JSON.stringify(fixture, null, 2))
  // Two opaque strings the shell needs, as plain files, so the stage does not
  // have to parse JSON to publish one MQTT message.
  fs.writeFileSync(path.join(STACK_DIR, 'lease-device-id'), plug.id)
  fs.writeFileSync(path.join(STACK_DIR, 'lease-tool-id'), `lease-${tag}`)

  // The edge's own configuration, written here because this is where the
  // credentials are. `Approved` is the registered state; websocket transport
  // keeps it off the site broker, which this stack shares with the local one.
  const base = process.env.CSS_BASE_URL ?? 'http://127.0.0.1:4399'
  const config = `name = "e2e-lease-edge"
auth_status = "Approved"
remote_transport = "websocket"
toolguard_sync_interval_secs = 5
calendar_mqtt_topic = "cs/spaces/calendar/events"
calendar_sync_interval_secs = 300

# The only card key a device gets (#109). Without it this edge refuses every
# card offline, which would make the lease stage's tool-on path fail for a
# reason that has nothing to do with leases.
card_device_pepper = "${devicePepper}"

# Renew twice per TTL so a single lost message cannot cut a running tool, and
# keep both well under the stage's observation windows.
module_lease_ttl_ms = 3000
module_lease_interval_ms = 500
# Long enough that the report in window 2 is still current in window 3's setup,
# so a grant that disappears there is the edge stopping and not a report aging
# out underneath the assertion.
module_offline_ms = 120000

[local_mqtt_config]
mqtt_instance_url = "tcp://127.0.0.1:${mqttPort}"
mqtt_client_id = "e2e-lease-edge"
mqtt_namespace = "cs/spaces"

[remote_device_info]
remote_id = "${edge.id}"
remote_auth_token = "${authToken}"
remote_instance_url = "${base}"
`
  fs.writeFileSync(path.join(STACK_DIR, 'edge-lease.config.toml'), config)
  record('lease/edge-config-written', 'ok')
}

// ── assertions ───────────────────────────────────────────────────────────────

async function assertWindows() {
  if (fs.existsSync(SKIPPED)) {
    record(
      'lease/not-run-on-this-cluster',
      'skip',
      `setup skipped on this cluster (${fs.readFileSync(SKIPPED, 'utf8').trim()})`
    )
    return
  }

  const fixture = JSON.parse(fs.readFileSync(FIXTURE, 'utf8'))
  const mine = (rows) => (rows ?? []).filter((r) => r.device_id === fixture.deviceId)

  const before = captured('lease-window-1.txt')
  const after = captured('lease-window-2.txt')
  const stopped = captured('lease-window-3.txt')

  ok('lease/windows-were-captured', before !== null && after !== null && stopped !== null,
    'one or more capture files is missing; the stage did not run to completion')

  // ── window 1: it refuses, and says so repeatedly ──────────────────────────
  const refusals = mine(before)
  ok(
    'lease/refusal-arrives-for-a-silent-module',
    refusals.length >= 1,
    `no lease addressed to ${fixture.deviceId} in window 1 (saw ${(before ?? []).length} in total). ` +
      `The watchdog is not publishing at all.`
  )
  // More than one, because a single message would equally be a coordinator that
  // published once and stopped -- which is indistinguishable from a dead one to
  // any module relying on renewal.
  ok(
    'lease/refusal-is-renewed-not-sent-once',
    refusals.length >= 2,
    `only ${refusals.length} lease(s) in window 1; a lease that is not republished ` +
      `is not a lease, it is an announcement`
  )
  assertEq(
    'lease/a-module-never-heard-from-is-refused',
    false,
    refusals[0]?.grant,
    'a fail_off binding that has never reported must not be granted'
  )
  // `every` on an empty array is true, so the non-emptiness is part of the
  // assertion rather than assumed from the case above. Without it this passed
  // on a coordinator publishing nothing whatsoever -- observed, not theorised:
  // it was the one green line in a window that saw zero messages.
  ok(
    'lease/refusal-names-the-tool',
    refusals.length > 0 &&
      refusals.every((r) => r.tool_id === fixture.toolId || r.tool_id === fixture.externalId),
    refusals.length === 0
      ? 'no leases at all, so nothing names the tool'
      : `lease addressed to the wrong tool: ${JSON.stringify(refusals[0] ?? {})}`
  )
  ok(
    'lease/carries-a-ttl',
    typeof refusals[0]?.ttl_ms === 'number' && refusals[0].ttl_ms > 0,
    `ttl_ms must be a positive number, got ${JSON.stringify(refusals[0]?.ttl_ms)}`
  )
  // The TTL has to outlast the renewal interval or the module cuts between
  // renewals on a healthy link, which would present as random tool dropouts.
  ok(
    'lease/ttl-outlasts-the-renewal-interval',
    (refusals[0]?.ttl_ms ?? 0) > 500,
    `ttl_ms ${refusals[0]?.ttl_ms} is not longer than the 500ms renewal interval ` +
      `this edge is configured with; a module obeying it would cut mid-session`
  )
  ok(
    'lease/refusal-explains-itself',
    typeof refusals[0]?.reason === 'string' && refusals[0].reason.length > 0,
    'a refusal should carry a reason for the log, even though firmware must not branch on it'
  )

  // ── window 2: one report is enough to change the answer ───────────────────
  const grants = mine(after)
  ok(
    'lease/leases-still-arrive-after-the-report',
    grants.length >= 1,
    'the coordinator stopped publishing after a power report, which is worse than never starting'
  )
  assertEq(
    'lease/a-module-that-reported-is-granted',
    true,
    grants[grants.length - 1]?.grant,
    'one power report naming the device must be enough to clear ModuleOffline; ' +
      'without the liveness ingest this stays false forever and no tool ever energizes'
  )

  // ── window 3: the inversion ───────────────────────────────────────────────
  // Asserted last and with time already allowed to pass, against a precondition
  // proven above: leases demonstrably flowed, so silence here is withdrawal.
  assertEq(
    'lease/renewals-stop-when-the-coordinator-does',
    0,
    mine(stopped).length,
    'leases were still arriving after the edge was stopped. Permission is ' +
      'supposed to lapse by silence; something is republishing, or the broker ' +
      'retained a lease and is redelivering it to every new subscriber -- which ' +
      'would hand a reconnecting plug a grant nobody is still making'
  )
}

main(async () => {
  if (phase === 'setup') return setup()
  if (phase === 'assert') return assertWindows()
  record('lease/driver-phase', 'fail', `unknown phase ${JSON.stringify(phase)}`)
})
