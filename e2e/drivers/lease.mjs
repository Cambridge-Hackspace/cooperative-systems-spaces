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
// #170 added a window 1b between 1 and 2, and two more bindings to the tool.
// The plug's power report used to be the only thing that could satisfy
// `fail_off`, and a reader or a sensor -- which has no power to report -- could
// never satisfy it, so the default denied the tool its lease forever. Now:
//
//   1b. after the PLUG has reported but the fail_off SENSOR has not -- leases
//       still refuse, and the reason names the sensor. This is the
//       discriminating step: it is what shows the sensor's silence is what
//       denies, rather than the plug's.
//   2.  (as before) after the sensor's alive pulse -- grants. The READER on
//       the same tool never pulses at all, so this grant is also the proof
//       that a reader's default no longer withholds the tool.
//
// #179 added the in-use rule and start sequencing, and a SECOND plug on the
// tool, bound at stage 1 with a 3 s start delay and a 3 s run-on:
//
//   1c. liveness satisfied, nobody signed on -- leases still refuse, with
//       NotInUse. A lease is for a member at the machine; before #179 a
//       standalone plug followed the interlocks alone and was on all day.
//   2.  after a member's tool-on at the edge -- the FIRST stage is granted,
//       and the second stage gets NO lease at all: it waits for the first to
//       confirm it energized, and "not yet" is silence, not a refusal.
//   2a. after the first plug reports relay_on true -- still nothing for the
//       second, inside its start delay.
//   2b. two seconds later -- the second stage is granted.
//   2c. after the member's tool-off -- the first stage is refused at once
//       (NotInUse) while the second keeps its grant: run-on.
//   2d. four seconds on -- the second stage's run-on has ended.
//
// What this does NOT prove: that a physical relay opens. Nothing here switches
// hardware. It proves the decision leaves the coordinator correctly addressed
// and correctly timed; what a module does with it is firmware's half of the
// contract, and FIRMWARE.md is where that half is stated.
//
// Cluster encoding: this stage used to skip on a non-UTF8 cluster, because a
// device invite code is eight emoji and LATIN1 cannot store one. #120 (#137)
// made invite codes hex-at-rest -- the emoji are generated, returned over HTTP
// and matched as hex, so none reaches the database -- and the guard outlived its
// reason, skipping this stage entirely on the default cluster. Removed.

import fs from 'node:fs'
import path from 'node:path'
import { GET, POST, PUT, account, adminAccount, assertEq, ok, record, main } from './lib.mjs'

const STACK_DIR = process.env.CSS_STACK_DIR ?? '/stack'

const FIXTURE = path.join(STACK_DIR, 'lease-fixture.json')
const SKIPPED = path.join(STACK_DIR, 'lease-skipped')

/**
 * The server's own view of one device's liveness -- `space_devices.last_seen_at`
 * as the admin list reports it. #161: a module behind an edge cannot heartbeat
 * on the site broker, so this column is the only place its liveness can land,
 * and it lands there only because the forwarded power report names the module.
 */
async function serverLiveness(adminToken, deviceId) {
  const r = await GET('/api/admin/devices', { token: adminToken })
  const rows = r.json?.data ?? []
  return rows.find((d) => d.id === deviceId) ?? null
}

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
  // Clears a sentinel left by an older checkout: this stage used to skip on a
  // non-UTF8 cluster, and the later phases read this file to know it had.
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
  const claim = async (name, role, mac) => {
    const invite = await POST('/api/admin/devices/invite', {
      token: admin.token,
      body: { expires_in_hours: 1 },
    })
    const reg = await POST('/api/devices/register', {
      body: {
        device_code: invite.json?.data?.device_code,
        name,
        capabilities: { roles: [role] },
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

  const plug = await claim(`lease-plug-${tag}`, 'power', '02:00:00:00:83:02')
  ok('lease/plug-registered', !!plug.id, `register -> ${plug.status} ${plug.text.slice(0, 160)}`)

  // #161 negative case: a registered device bound to NOTHING. The relay rule
  // must turn on the coordinator's binding, not merely on holding a valid
  // token -- a power reading feeds circuit aggregation, which can trip, and the
  // bypass detector. Without this arm, "the relay was accepted" would be
  // consistent with accepting a relay from anybody.
  const stranger = await claim(`lease-stranger-${tag}`, 'edge', '02:00:00:00:83:03')
  ok('lease/stranger-registered', !!stranger.id, `register -> ${stranger.status}`)

  const authToken = edge.token

  // on_disconnect is left unset so the server applies its own default. The
  // default is `fail_off`, and testing against the default is the point: it is
  // what a real power module gets, and it is the policy whose liveness
  // requirement was unsatisfiable until the ingest existed.
  const binding = await POST('/api/admin/device-bindings', {
    token: admin.token,
    body: { resource_id: toolId, device_id: plug.id, role: 'power', name: `lease plug ${tag}` },
  })
  assertEq('lease/module-bound', 201, binding.status)

  // #161: the EDGE's own binding, in the `edge` role -- "this coordinator speaks
  // for this tool", the same statement a door already requires of its
  // coordinator. Without it the server refuses every power report the edge
  // relays, because `authorize_toolguard` asks for the CALLER's binding and the
  // edge is a courier rather than a party to the plug's.
  //
  // Bound here rather than left out because the fixture should model the real
  // wiring: a plug behind a coordinator is the normal shape, and the refusal it
  // used to produce was silent at both ends.
  const edgeBinding = await POST('/api/admin/device-bindings', {
    token: admin.token,
    body: { resource_id: toolId, device_id: edge.id, role: 'edge', name: `lease edge ${tag}` },
  })
  assertEq('lease/coordinator-bound', 201, edgeBinding.status)
  assertEq(
    'lease/binding-defaults-to-fail-off',
    'fail_off',
    binding.json?.data?.on_disconnect,
    'the whole stage is about the policy that must not be granted while silent'
  )

  // #170: two more roles on the same tool, each taking the server's own default.
  //
  // A READER has no power to report, so under the old rules it could never be
  // heard from -- and with the old default of `fail_off` it denied this tool its
  // lease forever while the member saw ACCESS GRANTED. Its default is now
  // `ignore`: a dead reader cannot produce a swipe, so there is nothing for
  // `fail_off` to protect, and its silence must not withhold the tool. It is
  // bound here and NEVER pulses, so window 2's grant is also the proof of that.
  //
  // A SENSOR keeps `fail_off`: a silent safety sensor is an unobserved
  // condition. It cannot send a power report either, so the alive pulse is the
  // only way it can satisfy that -- which windows 1b and 2 prove between them.
  const reader = await claim(`lease-reader-${tag}`, 'reader', '02:00:00:00:83:04')
  ok('lease/reader-registered', !!reader.id, `register -> ${reader.status}`)
  const sensor = await claim(`lease-sensor-${tag}`, 'sensor', '02:00:00:00:83:05')
  ok('lease/sensor-registered', !!sensor.id, `register -> ${sensor.status}`)

  const readerBinding = await POST('/api/admin/device-bindings', {
    token: admin.token,
    body: { resource_id: toolId, device_id: reader.id, role: 'reader', name: `lease reader ${tag}` },
  })
  assertEq('lease/reader-bound', 201, readerBinding.status)
  assertEq(
    'lease/a-reader-binding-defaults-to-ignore',
    'ignore',
    readerBinding.json?.data?.on_disconnect,
    '#170: a reader has no power to report, so a fail_off default could never be satisfied'
  )
  const sensorBinding = await POST('/api/admin/device-bindings', {
    token: admin.token,
    body: { resource_id: toolId, device_id: sensor.id, role: 'sensor', name: `lease sensor ${tag}` },
  })
  assertEq('lease/sensor-bound', 201, sensorBinding.status)
  // The negative beside the positive: the reader default must not have widened
  // to every non-power role. A sensor's silence is a hazard.
  assertEq(
    'lease/a-sensor-binding-still-defaults-to-fail-off',
    'fail_off',
    sensorBinding.json?.data?.on_disconnect,
    'a silent safety sensor is an unobserved condition; the reader default must not have widened to it'
  )

  // #179: a member with a card, so a tool-on at the edge can open a session,
  // and a second plug behind the first in the start sequence.
  const member = await account(`lease_member_${tag}`)
  const card = `LEASE-CARD-${tag}`.toUpperCase()
  const cardSet = await PUT(`/api/profiles/${member.user.id}`, {
    token: member.token,
    body: { profile: { card_id: card } },
  })
  assertEq('lease/member-card-set', 200, cardSet.status, cardSet.text.slice(0, 160))

  const plug2 = await claim(`lease-plug2-${tag}`, 'power', '02:00:00:00:83:06')
  ok('lease/second-plug-registered', !!plug2.id, `register -> ${plug2.status}`)
  const plug2Binding = await POST('/api/admin/device-bindings', {
    token: admin.token,
    body: {
      resource_id: toolId,
      device_id: plug2.id,
      role: 'power',
      name: `lease plug2 ${tag}`,
      start_stage: 1,
      start_delay_ms: 3000,
      run_on_ms: 3000,
    },
  })
  assertEq('lease/second-plug-bound', 201, plug2Binding.status, plug2Binding.text.slice(0, 200))
  ok(
    'lease/sequencing-is-stored-on-the-binding',
    plug2Binding.json?.data?.start_stage === 1 &&
      plug2Binding.json?.data?.start_delay_ms === 3000 &&
      plug2Binding.json?.data?.run_on_ms === 3000,
    `the binding did not echo the timings: ${JSON.stringify(plug2Binding.json?.data)}`
  )
  // The timings are a property of a thing that switches power. On any other
  // role they would be stored and never read, so they are refused by name.
  const onReader = await POST('/api/admin/device-bindings', {
    token: admin.token,
    body: { resource_id: toolId, device_id: reader.id, role: 'reader', name: `x ${tag}`, run_on_ms: 1000 },
  })
  ok(
    'lease/sequencing-is-refused-on-a-non-power-binding',
    onReader.status === 400 && /run_on_ms/.test(onReader.text),
    `expected a 400 naming run_on_ms, got ${onReader.status} ${onReader.text.slice(0, 160)}`
  )
  const negative = await POST('/api/admin/device-bindings', {
    token: admin.token,
    body: { resource_id: toolId, device_id: plug2.id, role: 'power', name: `y ${tag}`, start_delay_ms: -1 },
  })
  assertEq('lease/a-negative-timing-is-refused', 400, negative.status, negative.text.slice(0, 160))

  const fixture = {
    tag,
    toolId,
    plug2Id: plug2.id,
    card,
    externalId: `lease-${tag}`,
    readerId: reader.id,
    sensorId: sensor.id,
    // The device the lease is ADDRESSED to -- the plug, not the coordinator.
    deviceId: plug.id,
    edgeId: edge.id,
    moduleId: binding.json?.data?.id,
    strangerToken: stranger.token,
  }
  // #161 precondition, asserted before the thing it is a precondition for: a
  // freshly registered module has never been heard from, so the server must
  // show it with no liveness at all. If this is already set, the assertion in
  // the assert phase proves nothing -- it would pass whether or not the power
  // report did anything.
  const freshPlug = await serverLiveness(admin.token, plug.id)
  ok(
    'lease/module-starts-with-no-liveness',
    freshPlug !== null && !freshPlug.last_seen_at,
    `the plug must start with last_seen_at unset, got ${JSON.stringify(freshPlug?.last_seen_at)}`
  )

  // #170 preconditions, same reasoning: the assert phase claims the sensor's
  // pulse reached the server and the reader's absence of one did not. Neither
  // claim means anything unless both start at nothing.
  for (const [label, id] of [
    ['sensor', sensor.id],
    ['reader', reader.id],
  ]) {
    const fresh = await serverLiveness(admin.token, id)
    ok(
      `lease/${label}-starts-with-no-liveness`,
      fresh !== null && !fresh.last_seen_at,
      `the ${label} must start with last_seen_at unset, got ${JSON.stringify(fresh?.last_seen_at)}`
    )
  }

  fs.writeFileSync(FIXTURE, JSON.stringify(fixture, null, 2))
  // Two opaque strings the shell needs, as plain files, so the stage does not
  // have to parse JSON to publish one MQTT message.
  fs.writeFileSync(path.join(STACK_DIR, 'lease-device-id'), plug.id)
  fs.writeFileSync(path.join(STACK_DIR, 'lease-tool-id'), `lease-${tag}`)
  // The sensor's id, for the stage to publish its alive pulse (#170).
  fs.writeFileSync(path.join(STACK_DIR, 'lease-sensor-id'), sensor.id)
  // #179: the second plug's id and the member's card, for the stage's
  // pulse, confirmation, tool-on and tool-off.
  fs.writeFileSync(path.join(STACK_DIR, 'lease-plug2-id'), plug2.id)
  fs.writeFileSync(path.join(STACK_DIR, 'lease-card'), card)

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
  const midway = captured('lease-window-1b.txt')
  const nobodyOn = captured('lease-window-1c.txt')
  const inDelay = captured('lease-window-2a.txt')
  const sequenced = captured('lease-window-2b.txt')
  const afterStop = captured('lease-window-2c.txt')
  const runOnOver = captured('lease-window-2d.txt')
  const toolOnReply = captured('lease-toolon-reply.txt')

  ok(
    'lease/windows-were-captured',
    [before, midway, nobodyOn, after, inDelay, sequenced, afterStop, runOnOver, stopped, toolOnReply].every(
      (w) => w !== null
    ),
    'one or more capture files is missing; the stage did not run to completion'
  )
  const ofPlug2 = (rows) => (rows ?? []).filter((r) => r.device_id === fixture.plug2Id)

  // ── #161: the SERVER's view of the module, the other side of the same event ─
  //
  // Two oracles for one claim, which is the point of asserting it here rather
  // than only in the lease windows. The windows prove the EDGE believed the
  // module was alive -- that is what moved the lease from refuse to grant. This
  // proves the SERVER was told as well, which is a different fact with a
  // different mechanism and was false until #161: the edge forwarded the power
  // report under its own token and never named the module, so `last_seen_at`
  // stayed NULL, `classify_liveness` read NULL as silent, and the bypass sweep
  // recorded the module silent once and then never changed its mind. A module
  // that genuinely went quiet produced no transition at all.
  //
  // The precondition for this ran in the setup phase
  // (`lease/module-starts-with-no-liveness`), deliberately before the power
  // report, so a failure here cannot be read as "it was always set".
  const admin = await adminAccount('lease_admin')
  const plugNow = await serverLiveness(admin.token, fixture.deviceId)
  ok(
    'lease/power-report-gave-the-module-liveness',
    !!plugNow?.last_seen_at,
    `the server must have last_seen_at for ${fixture.deviceId} after a power report ` +
      `naming it; got ${JSON.stringify(plugNow?.last_seen_at)}. Without this the silence ` +
      `detector is blind to every module behind an edge.`
  )

  // The negative, asserted next to the positive so the pair reads as one claim:
  // relaying is a coordinator's privilege, not a token-holder's. `stranger`
  // declares the `edge` role in its capabilities and holds a valid token, and is
  // bound to nothing -- so if a declared capability were doing the work here
  // rather than the binding, this would pass.
  const refused = await POST('/api/toolguard/power-report', {
    token: fixture.strangerToken,
    body: { tool_id: fixture.externalId, device_id: fixture.deviceId, draw_now: '0.0' },
  })
  ok(
    'lease/an-unbound-device-cannot-relay',
    refused.status === 401 || refused.status === 403,
    `a device bound to nothing must not relay a power report; got ${refused.status} ` +
      `${refused.text.slice(0, 140)}`
  )

  // And the edge is NOT the thing credited. The defect's shape was that the
  // forwarded report could only ever refresh the relayer, so asserting the
  // module is live says nothing unless the module is a different row from the
  // edge -- which is why the fixture registers two devices.
  ok(
    'lease/liveness-landed-on-the-module-not-the-edge',
    fixture.deviceId !== fixture.edgeId,
    'the fixture must use two distinct devices or this assertion is vacuous'
  )

  // ── #170: the pulse reaches the SERVER, for a device with no power binding ─
  //
  // The windows prove the edge believed the sensor was alive. This proves the
  // server was told too, which was impossible before: the edge could only
  // forward a POWER report, and the server refuses a relayed power report for
  // any device not bound as `power`. The sensor is bound as `sensor`, so
  // `last_seen_at` here can only have come through `/api/toolguard/alive`.
  const sensorNow = await serverLiveness(admin.token, fixture.sensorId)
  ok(
    'lease/an-alive-pulse-gives-a-non-power-device-liveness',
    !!sensorNow?.last_seen_at,
    `the server must have last_seen_at for the sensor ${fixture.sensorId} after its ` +
      `alive pulse was relayed; got ${JSON.stringify(sensorNow?.last_seen_at)}. ` +
      `A sensor behind an edge is otherwise invisible to the silence detector.`
  )
  // And the reader, which never pulsed, must NOT have been credited -- or the
  // grant in window 2 could be read as the reader having reported rather than
  // as its default no longer withholding the tool.
  const readerNow = await serverLiveness(admin.token, fixture.readerId)
  ok(
    'lease/a-device-that-never-pulsed-has-no-liveness',
    readerNow !== null && !readerNow.last_seen_at,
    `the reader never pulsed and must still have last_seen_at unset, got ` +
      `${JSON.stringify(readerNow?.last_seen_at)}; something credited it anyway`
  )
  // Vouching is a coordinator's privilege. `stranger` holds a valid token and is
  // bound to nothing, so if a token alone could report another device alive, a
  // dead sensor could be kept "alive" by anything on the network.
  const refusedAlive = await POST('/api/toolguard/alive', {
    token: fixture.strangerToken,
    body: { device_id: fixture.sensorId },
  })
  ok(
    'lease/an-unbound-device-cannot-vouch-for-another',
    refusedAlive.status === 401 || refusedAlive.status === 403,
    `a device bound to nothing must not report another device alive; got ` +
      `${refusedAlive.status} ${refusedAlive.text.slice(0, 140)}`
  )
  // But any registered device may say it, itself, is alive -- that is the whole
  // point of the pulse, and it needs no binding to express.
  const selfAlive = await POST('/api/toolguard/alive', {
    token: fixture.strangerToken,
    body: {},
  })
  assertEq(
    'lease/any-registered-device-may-report-itself-alive',
    200,
    selfAlive.status,
    `a registered device reporting itself alive was refused: ${selfAlive.text.slice(0, 140)}`
  )

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

  // ── window 1b (#170): the plug has reported; the sensor has not ───────────
  //
  // The discriminating step. If this window GRANTED, the sensor's silence would
  // not be what denies, and window 2's grant would prove nothing about the
  // pulse. It must refuse, and it must say the sensor is why.
  const midRefusals = mine(midway)
  ok(
    'lease/leases-still-arrive-with-the-plug-alive-and-the-sensor-silent',
    midRefusals.length >= 1,
    `no lease addressed to ${fixture.deviceId} in window 1b (saw ${(midway ?? []).length} in total)`
  )
  assertEq(
    'lease/a-silent-fail-off-sensor-still-denies-the-tool',
    false,
    midRefusals[midRefusals.length - 1]?.grant,
    'the plug reported but the fail_off sensor had not; a silent safety sensor is an ' +
      'unobserved condition and must withhold the lease'
  )
  ok(
    'lease/the-refusal-names-the-silent-sensor',
    midRefusals.length > 0 &&
      midRefusals.every(
        (r) => typeof r.reason === 'string' && r.reason.includes('ModuleOffline') && /sensor/.test(r.reason)
      ),
    `window 1b's refusals should cite ModuleOffline naming the sensor, got ` +
      `${JSON.stringify(midRefusals.map((r) => r.reason))}. If this names the plug, the ` +
      `power report is not crediting it; if it names the reader, the reader default ` +
      `has not taken effect.`
  )

  // ── window 1c (#179): alive, interlocked, and nobody signed on ────────────
  //
  // Every liveness condition is now satisfied, and the answer must STILL be no:
  // a lease is for a member at the machine. Before #179 this window granted,
  // and a standalone plug followed the interlocks alone -- on all day.
  const idleRefusals = mine(nobodyOn)
  ok(
    'lease/leases-still-arrive-with-nobody-signed-on',
    idleRefusals.length >= 1,
    `no lease addressed to ${fixture.deviceId} in window 1c (saw ${(nobodyOn ?? []).length} in total)`
  )
  ok(
    'lease/nobody-signed-on-refuses-with-not-in-use',
    idleRefusals.length > 0 && idleRefusals.every((r) => r.grant === false && r.reason === 'NotInUse'),
    `with every module alive and nobody signed on, every lease must refuse with NotInUse; got ` +
      `${JSON.stringify(idleRefusals.map((r) => [r.grant, r.reason]))}`
  )

  // The tool-on that opens the session, answered by the EDGE from its synced
  // allow-list. A precondition for everything after it, asserted as one so a
  // failure here is not read as a sequencing defect three windows later.
  // #182: the scan went out on `resource/request/scan` and the reply is read
  // from `resource/response/scan`, so this is also the proof that the
  // resource-shaped wire reaches the edge's decision and comes back named.
  const reply = (toolOnReply ?? []).find(
    (r) => r.resource === fixture.externalId || r.resource === fixture.toolId
  )
  ok(
    'lease/the-member-is-authorized-at-the-edge',
    reply?.authorized === true,
    `the edge did not authorize the member's card on the resource wire: ${JSON.stringify(toolOnReply)}`
  )
  ok(
    'lease/a-tool-scan-reply-carries-no-strike',
    reply !== undefined && reply.strike === undefined,
    `a tool is not a latch; its scan reply must carry no strike: ${JSON.stringify(reply)}`
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

  // ── #179: the second stage waits for the first to CONFIRM ─────────────────
  //
  // In window 2 the first plug is leased but has not yet reported its relay
  // closed, so the second must receive nothing -- silence, not a refusal,
  // because a refusal would end a reader-and-relay box's session.
  assertEq(
    'lease/the-second-stage-waits-for-the-first-to-confirm',
    0,
    ofPlug2(after).length,
    `the second stage was addressed before the first confirmed it energized: ` +
      `${JSON.stringify(ofPlug2(after))}. A timer stood in for a confirmation, or ` +
      `the wait was sent as a refusal.`
  )
  // 2a: the first plug has confirmed; the second is inside its 3 s start delay.
  assertEq(
    'lease/the-second-stage-is-silent-inside-its-start-delay',
    0,
    ofPlug2(inDelay).length,
    `the second stage was leased before its start delay had passed: ${JSON.stringify(ofPlug2(inDelay))}`
  )
  // 2b: confirmed and delayed -- granted.
  const second = ofPlug2(sequenced)
  ok(
    'lease/the-second-stage-is-leased-after-confirmation-and-delay',
    second.length >= 1 && second[second.length - 1]?.grant === true,
    `the second stage never got its lease after the first confirmed and the delay ran: ` +
      `${JSON.stringify(second)}`
  )
  // #182: the same leases, on the resource-shaped topic, captured in parallel
  // with window 2b. `resource` names the tool where the alias says `tool_id`;
  // everything else is the same message. Without this the new topic could be
  // declared and never published, and a device built to the new wire would
  // wait for a lease that never comes.
  const mirrored = captured('lease-window-2b-resource.txt') ?? []
  const mirroredPlug2 = mirrored.filter((r) => r.device_id === fixture.plug2Id)
  ok(
    'lease/the-lease-is-published-on-the-resource-topic-too',
    mirroredPlug2.length >= 1 &&
      mirroredPlug2.every((r) => r.resource === fixture.toolId && typeof r.ttl_ms === 'number') &&
      mirroredPlug2[mirroredPlug2.length - 1]?.grant === true,
    `resource/lease did not mirror the grant: ${JSON.stringify(mirroredPlug2)} (alias window had ` +
      `${second.length})`
  )

  // ── #179: run-on ──────────────────────────────────────────────────────────
  //
  // The member signs off. The first stage is refused at once; the second,
  // with a 3 s run-on, keeps its grant -- and then loses it.
  const firstAfterStop = mine(afterStop)
  ok(
    'lease/a-stop-refuses-the-first-stage-at-once',
    firstAfterStop.length >= 1 &&
      firstAfterStop[firstAfterStop.length - 1]?.grant === false &&
      firstAfterStop[firstAfterStop.length - 1]?.reason === 'NotInUse',
    `after tool-off the first stage should be refused with NotInUse: ${JSON.stringify(firstAfterStop)}`
  )
  const secondAfterStop = ofPlug2(afterStop)
  ok(
    'lease/run-on-keeps-the-second-stage-leased-after-the-stop',
    secondAfterStop.length >= 1 && secondAfterStop.every((r) => r.grant === true),
    `inside its run-on the second stage must still be granted: ${JSON.stringify(secondAfterStop)}`
  )
  const secondLater = ofPlug2(runOnOver)
  ok(
    'lease/run-on-ends',
    secondLater.length >= 1 && secondLater[secondLater.length - 1]?.grant === false,
    `the run-on never ended: ${JSON.stringify(secondLater)}`
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
