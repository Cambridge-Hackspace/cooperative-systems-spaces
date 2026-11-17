#!/usr/bin/env node
//
// The oracle before the bridge stage's windows (bridge-timeline.mjs).
//
// The windows moved from shell-clocked captures to a recorded timeline because
// the captures failed a correct bridge on a slow guest (#234). So the judge is
// fed a simulated bridge and edge, driven step by step as run.sh drives the
// real ones, in three kinds of world:
//
//   * healthy, and SLOW-but-healthy: each of the stage's steps a few seconds
//     late, the recorder stamping late and unevenly. Both must pass; the slow
//     one is #234.
//   * broken, one defect at a time, each a way the bridge or edge could
//     really be wrong -- believing a retained replay, never asking /get,
//     reporting a silent plug as off, no on_time, no stop-on-loss, a door
//     lease that never ends or one that ends at the pulse, a foreign lease,
//     a contact reporting for the wrong resource, a trip that never comes --
//     and the shape #234 actually left: the session stopped a window early.
//     Each must fail the claim it breaks.
//   * a recording missing a step: the windows around it must say so, not
//     pass on silence.
//
// The jitter is seeded: SEED in the environment replays a run.
//
//   node e2e/journeys/bridge-timeline-selftest.mjs

import { parseMarks, parseTimeline } from './timeline.mjs'
import { judgeBridgeTimeline, PULSE_MS } from './bridge-timeline.mjs'

const SEED = Number(process.env.SEED ?? Date.now() % 2 ** 31)
console.log(`seed ${SEED} (replay with SEED=${SEED})`)
function rng(seed) {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = a
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

const IDS = {
  deviceId: 'plug',
  toolId: 'bridge-tool',
  friendly: 'e2e/plug/extractor',
  doorA: 'door-a',
  doorB: 'door-b',
  lockA: 'lock-a',
  lockB: 'lock-b',
  contactId: 'contact',
}
const Z = `zigbee2mqtt/${IDS.friendly}`

/**
 * One stage run. `bug` breaks the simulated system one way; `lag` is how late
 * (seconds) the stage takes each step after the event it waits for; `jitterMs`
 * is how late the recorder stamps.
 */
function run({ bug = {}, lag = 0.3, jitterMs = 50, drop = () => false } = {}, seed = SEED) {
  const rand = rng(seed)
  const events = []
  const marks = []
  let now = 1_794_895_000
  const emit = (topic, payload) =>
    events.push({
      t: now,
      topic,
      payload: typeof payload === 'string' ? payload : JSON.stringify(payload),
    })

  // The system's state.
  const s = {
    toolOn: false, // a member session on the tool
    tripped: false,
    // The edge names a trip once, on its first decision after it, and says
    // Latched from then on (as the conditions stage found): so a recording
    // holds exactly one Tripped.
    tripNamed: false,
    plug: 'silent', // what the bridge knows: silent | unknown | on | off
    contactOpen: false,
    doorAUntil: 0, // the momentary lease's end
    doorBOn: false,
    lockBGone: false,
  }
  const tick = () => {
    const toolGrant = s.toolOn && !s.tripped && !s.unobserved
    let reason = s.unobserved ? 'Unobserved("door_open")' : 'NotInUse'
    if (s.tripped) {
      reason = s.tripNamed ? 'Latched("door_open")' : 'Tripped("door_open")'
      s.tripNamed = true
    }
    emit('resource/lease', {
      device_id: IDS.deviceId,
      resource: IDS.toolId,
      grant: toolGrant,
      reason: toolGrant ? null : reason,
    })
    const aOn = now < s.doorAUntil
    emit('resource/lease', {
      device_id: IDS.lockA,
      resource: IDS.doorA,
      grant: aOn,
      reason: aOn ? null : 'NotInUse',
    })
    if (!(bug.doorBSilentAfterPulse && s.doorBSilent))
      emit('resource/lease', {
        device_id: IDS.lockB,
        resource: IDS.doorB,
        grant: s.doorBOn,
        reason: s.doorBOn ? null : 'NotInUse',
      })
    emit('resource/lease', {
      device_id: IDS.contactId,
      resource: IDS.doorA,
      grant: false,
      reason: 'NotInUse',
    })
    if (bug.foreignLease)
      emit('resource/lease', { device_id: 'someone-elses', grant: false, reason: 'NotInUse' })
    // Reports.
    const power = { resource: IDS.toolId, device_id: IDS.deviceId, draw_now: 0 }
    if (s.plug === 'on') power.relay_on = true
    if (s.plug === 'off') power.relay_on = false
    if (s.plug === 'silent' && bug.silentReportedOff) power.relay_on = false
    if (s.plug === 'unknown' && bug.believesRetained) power.relay_on = true
    if (s.plug !== 'down') emit('resource/request/power', power)
    emit('resource/request/power', {
      resource: IDS.doorA,
      device_id: IDS.lockA,
      ...(aOn ? { relay_on: true } : { relay_on: false }),
    })
    emit('resource/request/condition', {
      device_id: IDS.contactId,
      resource: bug.contactWrongResource ? 'elsewhere' : IDS.toolId,
      condition: 'door_open',
      asserted: s.contactOpen,
    })
  }
  const pass = (secs) => {
    for (let i = 0; i < secs / 0.5; i++) {
      now += 0.5
      tick()
    }
  }
  const step = (name) => {
    pass(lag)
    marks.push([name, events.length])
  }

  pass(3)
  // B
  step('scanB')
  emit('resource/request/scan', { card: 'c', resource: IDS.toolId })
  s.toolOn = true
  pass(0.5)
  if (!bug.noOnAtB) emit(`${Z}/set`, bug.noOnTime ? { state: 'ON' } : { state: 'ON', on_time: 60 })
  step('liveB')
  emit(Z, { state: 'ON' })
  // Believes a report only once it has asked for one.
  s.plug = bug.ignoresUnaskedReports ? 'unknown' : 'on'
  pass(2)
  // C
  s.plug = 'down'
  pass(1)
  step('restart')
  emit(Z, { state: 'ON' })
  s.plug = 'unknown'
  pass(1)
  if (!bug.noGet) emit(`${Z}/get`, '')
  pass(1)
  if (bug.stoppedDuringC) {
    // #234: the live ON came after the debounce, and the stop landed here.
    emit('resource/request/stop', { card: '', resource: IDS.toolId })
    s.toolOn = false
  }
  step('liveC')
  emit(Z, { state: 'ON' })
  // Stuck at unknown after a restart, whatever the plug says.
  s.plug = bug.stuckUnknown ? 'unknown' : 'on'
  pass(2)
  emit(Z, '')
  // D
  step('offline')
  emit(`${Z}/availability`, 'offline')
  s.plug = 'unknown'
  pass(10)
  if (!bug.noStopOnLoss && s.toolOn) {
    emit('resource/request/stop', { card: '', resource: IDS.toolId })
    s.toolOn = false
    pass(0.5)
    emit(`${Z}/set`, { state: 'OFF' })
  }
  pass(1)
  // E
  step('scanE')
  emit('resource/request/scan', { card: 'c', resource: IDS.doorA })
  // Crossed wiring: door A's badge leases nothing of door A's.
  s.doorAUntil = bug.crossedLocks ? 0 : bug.momentaryNeverEnds ? Infinity : now + PULSE_MS / 1000
  pass(PULSE_MS / 1000 + 2)
  s.doorAUntil = Math.min(s.doorAUntil, now) // a later badge must not keep it
  if (bug.momentaryNeverEnds) s.doorAUntil = Infinity
  // F
  step('scanF')
  emit('resource/request/scan', { card: 'c', resource: IDS.doorB })
  s.doorBOn = true
  // ...and door B's badge leases door A's lock for a pulse.
  if (bug.crossedLocks) s.doorAUntil = now + PULSE_MS / 1000
  const fGranted = now
  pass(PULSE_MS / 1000)
  if (bug.doorBStoppedAtPulse) {
    emit('resource/request/stop', { card: '', resource: IDS.doorB })
    s.doorBOn = false
  }
  if (bug.doorBEndsAtPulse) s.doorBOn = false
  // Stops addressing the lock at all: its lease lapses by TTL, unrefused.
  if (bug.doorBSilentAfterPulse) s.doorBSilent = true
  pass(Math.max(0, 6 - (now - fGranted)))
  step('injectF')
  pass(1)
  if (s.doorBOn) {
    pass(9)
    emit('resource/request/stop', { card: '', resource: IDS.doorB })
    s.doorBOn = false
  }
  pass(1)
  // G
  step('scanG')
  emit('resource/request/scan', { card: 'c', resource: IDS.toolId })
  s.toolOn = true
  s.tripped = false
  // Refused with the enclosure shut: the reading not believed.
  if (bug.refusedWhileShut) s.unobserved = true
  pass(0.5)
  if (!bug.refusedWhileShut) emit(`${Z}/set`, { state: 'ON', on_time: 60 })
  if (bug.tripsWhileShut) {
    // Tripped on nothing: the enclosure is still shut.
    s.tripped = true
    pass(0.5)
    emit(`${Z}/set`, { state: 'OFF' })
  }
  step('injectG')
  pass(0.5)
  s.contactOpen = true
  pass(0.5)
  if (!bug.noTrip && !bug.tripsWhileShut) {
    s.unobserved = false
    s.tripped = true
    pass(0.5)
    if (!bug.refusedWhileShut) emit(`${Z}/set`, { state: 'OFF' })
    // Switched back on after the trip.
    if (bug.reenergizesAfterTrip) {
      pass(0.5)
      emit(`${Z}/set`, { state: 'ON', on_time: 60 })
    }
  }
  pass(1)
  marks.push(['end', events.length])

  // The recorder: late by up to jitterMs, kept in arrival order. A mark counts
  // lines the host had seen, so it can sit a line or two early, never late.
  const recorded = events.map((e, i) => ({ ...e, i, stamp: e.t + (rand() * jitterMs) / 1000 }))
  recorded.sort((a, b) => a.stamp - b.stamp || a.i - b.i)
  // Numbered as the recorder that kept them would number them: a dropped
  // line renumbers everything after it, marks included.
  const kept = recorded.filter((e) => !drop(e))
  const lineOfEvent = new Map(kept.map((e, idx) => [e.i, idx + 1]))
  const text =
    kept.map((e) => `${e.stamp.toFixed(9)}\t0\t${e.topic}\t${e.payload}`).join('\n') + '\n'
  const markText = marks
    .map(([name, count]) => {
      // The first event at or after the mark, in arrival order, less 0-2 unflushed.
      const lines = events
        .slice(count)
        .map((e, k) => lineOfEvent.get(count + k))
        .filter((l) => l !== undefined)
      const line = lines.length ? Math.min(...lines) - 1 : kept.length
      return `${name}\t${Math.max(0, line - Math.floor(rand() * 3))}`
    })
    .join('\n')
  return judgeBridgeTimeline(parseTimeline(text), parseMarks(markText), IDS)
}

let failures = 0
function expect(label, results, failing) {
  const got = results
    .filter((r) => !r.pass)
    .map((r) => r.name)
    .sort()
  const want = failing.map((n) => `bridge/${n}`).sort()
  const good = JSON.stringify(want) === JSON.stringify(got)
  if (!good) failures++
  console.log(
    `${good ? 'ok  ' : 'FAIL'} ${label}${good ? '' : `\n     expected failing: ${JSON.stringify(want)}\n     got failing:      ${JSON.stringify(got)}`}`
  )
  if (!good)
    for (const r of results.filter((x) => !x.pass)) console.log(`       ${r.name}: ${r.detail}`)
}

const ALL = run().map((r) => r.name)
if (ALL.length !== 22) {
  failures++
  console.log(`FAIL the judge reported ${ALL.length} claims, expected 22: ${JSON.stringify(ALL)}`)
}

for (let i = 0; i < 100; i++) {
  for (const [label, opts] of [
    ['healthy', {}],
    // CI's host broker: every step taken within milliseconds of its event.
    ['fast', { lag: 0.01, jitterMs: 20 }],
    // #234: every step seconds late, the recorder late and uneven.
    ['slow', { lag: 3, jitterMs: 250 }],
  ]) {
    for (const r of run(opts, SEED + i)) {
      if (!r.pass) {
        failures++
        console.log(`FAIL a ${label} bridge failed ${r.name} at seed ${SEED + i}: ${r.detail}`)
      }
    }
  }
}
console.log('ok   a healthy bridge, and a slow healthy one, pass across 100 seeds')

expect('a silent plug reported as off', run({ bug: { silentReportedOff: true } }), [
  'A-silent-plug-omits-relay_on',
])
expect('an ON with no on_time', run({ bug: { noOnTime: true } }), ['B-on-carries-on_time'])
expect('a retained replay believed', run({ bug: { believesRetained: true } }), [
  'C-retained-replay-is-not-believed',
])
expect('no /get after a restart', run({ bug: { noGet: true } }), [
  'C-retained-replay-triggers-a-get',
])
expect("no stop on the plug's loss", run({ bug: { noStopOnLoss: true } }), [
  'D-loss-stops-the-resource',
  'D-refusal-switches-plug-off',
])
expect('#234: the session stopped a window early', run({ bug: { stoppedDuringC: true } }), [
  'D-loss-stops-the-resource',
  'D-refusal-switches-plug-off',
])
expect('a momentary lease that never ends', run({ bug: { momentaryNeverEnds: true } }), [
  'E-pulse-end-withdraws-the-lease',
])
expect('an until_stopped lease that ends at the pulse', run({ bug: { doorBEndsAtPulse: true } }), [
  'F-lease-outlives-the-pulse',
  'F-loss-stops-the-door',
])
expect(
  'an until_stopped lease the edge stops renewing at the pulse',
  run({ bug: { doorBSilentAfterPulse: true } }),
  ['F-lease-outlives-the-pulse']
)
expect('a trip while the enclosure is still shut', run({ bug: { tripsWhileShut: true } }), [
  'G-opening-the-enclosure-trips-the-tool',
])
expect('no ON for the first badge', run({ bug: { noOnAtB: true } }), [
  'B-grant-commands-plug-on',
  'B-on-carries-on_time',
])
expect('the tool refused with its enclosure shut', run({ bug: { refusedWhileShut: true } }), [
  'G-shut-enclosure-grants',
  'G-the-trip-switches-the-plug-off',
])
expect('the plug switched back on after the trip', run({ bug: { reenergizesAfterTrip: true } }), [
  'G-the-trip-switches-the-plug-off',
])
expect('a live report ignored until asked for', run({ bug: { ignoresUnaskedReports: true } }), [
  'B-reported-on-becomes-relay_on-true',
])
expect("door B's badge leases door A's lock", run({ bug: { crossedLocks: true } }), [
  'E-badge-leases-the-lock',
  'E-unlocked-is-reported-as-on',
  'E-pulse-end-withdraws-the-lease',
])
expect(
  'door B stopped at its pulse, not on its loss',
  run({ bug: { doorBStoppedAtPulse: true } }),
  ['F-lease-outlives-the-pulse', 'F-loss-stops-the-door']
)
expect('stuck at unknown after a restart', run({ bug: { stuckUnknown: true } }), [
  'C-live-report-resolves-unknown',
])
expect('a lease to a device this edge does not coordinate', run({ bug: { foreignLease: true } }), [
  'E-the-edge-leases-only-what-it-coordinates',
])
expect('a contact reporting for the wrong resource', run({ bug: { contactWrongResource: true } }), [
  'G-shut-is-door-open-false',
])
expect('an open enclosure that trips nothing', run({ bug: { noTrip: true } }), [
  'G-opening-the-enclosure-trips-the-tool',
  'G-the-trip-switches-the-plug-off',
])
expect('the recorder missed the offline', run({ drop: (e) => e.topic.endsWith('/availability') }), [
  'the-timeline-holds-every-step',
  'C-retained-replay-triggers-a-get',
  'C-retained-replay-is-not-believed',
  'C-live-report-resolves-unknown',
  'D-loss-stops-the-resource',
  'D-refusal-switches-plug-off',
])

if (failures) {
  console.log(`\n${failures} failure(s)`)
  process.exit(1)
}
console.log('\nevery window fails when it should and holds when it should')
