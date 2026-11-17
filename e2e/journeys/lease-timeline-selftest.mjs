#!/usr/bin/env node
//
// The oracle before the lease stage's timed claims (lease-timeline.mjs).
//
// Those claims moved from shell-clocked windows to a recorded timeline because
// the windows failed a correct edge on a slow guest. A judge that has stopped
// failing a correct edge is only worth anything if it still fails a broken
// one, so this feeds it a simulated edge in three kinds of world:
//
//   * healthy, and SLOW-but-healthy -- the stage acting seconds late and the
//     recorder stamping late and unevenly, which is what 2026-11-17's release
//     battery was. Both must pass; the slow one is the flake this replaces.
//   * broken, one defect at a time, each a way the edge could really be
//     wrong: no start delay, a short one, no second-stage grant at all, no
//     run-on, a run-on that never ends or comes back, a first stage or a
//     sensor that keeps its lease past the stop, a sensor held like a stage,
//     a lease on #192's retired alias. Each must fail the claim it breaks.
//   * a recording missing its anchor -- the recorder died -- which must fail
//     the precondition and not report the slices as passing on silence.
//
// The simulation's jitter is seeded: SEED in the environment replays a run.
//
//   node e2e/journeys/lease-timeline-selftest.mjs

import { parseMarks, parseTimeline } from './timeline.mjs'
import { judgeLeaseTimeline, START_DELAY_MS, RUN_ON_MS, TICK_MS } from './lease-timeline.mjs'

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

const IDS = { toolId: 'lease-tool', deviceId: 'plug-1', plug2Id: 'plug-2', sensorId: 'sensor' }

/**
 * An edge ticking every TICK_MS, as the stage drives it: the member is on,
 * the first plug confirms at `confirmAt`, the member signs off at `stopAt`.
 * `edge` overrides the edge's behaviour to make a broken one; `stage` moves
 * the stage's actions; `jitterMs` is the most the recorder stamps late.
 */
function world(
  { edge = {}, stage = {}, jitterMs = 50, drop = () => false, extra = [] } = {},
  seed = SEED
) {
  const rand = rng(seed)
  const delay = edge.startDelayMs ?? START_DELAY_MS
  const runOn = edge.runOnMs ?? RUN_ON_MS
  const T0 = 1_794_895_000
  const confirmAt = T0 + (stage.confirmAfter ?? 4)
  const stopAt = confirmAt + (stage.stopAfter ?? 6)
  // As the stage paces it: two seconds past the run-on's end, and never
  // less than six after the stop.
  const endAt = stopAt + Math.max(runOn / 1000 + 2.5, 6)
  const phase = rand() * (TICK_MS / 1000)
  const events = []
  const at = (t, topic, json) => events.push({ t, topic, json })

  at(confirmAt, 'resource/request/power', {
    resource: IDS.toolId,
    device_id: IDS.deviceId,
    draw_now: 0.3,
    relay_on: true,
  })
  at(stopAt, 'resource/request/stop', { card: 'c', resource: IDS.toolId })
  for (let t = T0 + phase; t < endAt + 1; t += TICK_MS / 1000) {
    const on = t < stopAt
    const lease = (device_id, grant) =>
      at(t, 'resource/lease', {
        device_id,
        resource: IDS.toolId,
        grant,
        reason: grant ? null : 'NotInUse',
        ttl_ms: 3000,
      })
    // The first stage.
    lease(IDS.deviceId, edge.firstKeepsLease ? true : on)
    // The sensor: no stage, no run-on -- unless broken.
    if (edge.sensorIsAStage) {
      if (t >= confirmAt + delay / 1000) lease(IDS.sensorId, on)
    } else lease(IDS.sensorId, edge.sensorRunsOn ? t < stopAt + runOn / 1000 : on)
    // The second stage: silent until confirm + delay, then granted, run on.
    if (t >= confirmAt + delay / 1000 && !edge.neverGrantsSecond) {
      if (edge.runOnNeverEnds) lease(IDS.plug2Id, true)
      else if (edge.runOnReturns && t > stopAt + runOn / 1000 + 1.5) lease(IDS.plug2Id, true)
      else lease(IDS.plug2Id, t < stopAt + runOn / 1000)
    }
  }
  for (const e of extra) at(T0 + e.after, e.topic, e.json)

  // The recorder: every stamp late by up to jitterMs, lines in arrival order.
  const recorded = events
    .filter((e) => !drop(e))
    .map((e) => ({ ...e, stamp: e.t + (rand() * jitterMs) / 1000 }))
    .sort((a, b) => a.stamp - b.stamp)
  const text =
    recorded
      .map((e) => `${e.stamp.toFixed(9)}\t0\t${e.topic}\t${JSON.stringify(e.json)}`)
      .join('\n') + '\n'
  // Marks are taken before the action, and the host may not yet have seen the
  // last couple of lines: so a mark can sit a little EARLIER than the action's
  // own line, never later.
  const lineOf = (pred) => recorded.findIndex(pred) + 1
  const markBefore = (line) => Math.max(0, line - 1 - Math.floor(rand() * 3))
  const confirmLine = lineOf((e) => e.topic === 'resource/request/power')
  const stopLine = lineOf((e) => e.topic === 'resource/request/stop')
  const endLine = recorded.filter((e) => e.stamp < endAt).length
  const marks = [
    confirmLine > 0 ? `confirm\t${markBefore(confirmLine)}` : 'confirm\t0',
    stopLine > 0 ? `stop\t${markBefore(stopLine)}` : 'stop\t0',
    `end\t${endLine}`,
  ].join('\n')
  return judgeLeaseTimeline(parseTimeline(text), parseMarks(marks), IDS)
}

let failures = 0
function expect(label, results, failing) {
  const failed = results.filter((r) => !r.pass).map((r) => r.name)
  const want = [...failing].sort()
  const got = [...failed].sort()
  const good = JSON.stringify(want) === JSON.stringify(got)
  if (!good) failures++
  console.log(
    `${good ? 'ok  ' : 'FAIL'} ${label}${good ? '' : `\n     expected failing: ${JSON.stringify(want)}\n     got failing:      ${JSON.stringify(got)}`}`
  )
  if (!good)
    for (const r of results.filter((x) => !x.pass)) console.log(`       ${r.name}: ${r.detail}`)
}

const ALL = world().map((r) => r.name)
if (ALL.length !== 10) {
  console.log(`FAIL the judge reported ${ALL.length} claims, expected 10: ${JSON.stringify(ALL)}`)
  failures++
}

// Worlds that must pass, across many seeds, so the jitter's whole range is used.
for (let i = 0; i < 200; i++) {
  const s = SEED + i
  const results = [
    world({}, s),
    // 2026-11-17: the stage acts late and the recorder stamps late and
    // unevenly. The old windows failed a correct edge here.
    world({ stage: { confirmAfter: 9, stopAfter: 11 }, jitterMs: 200 }, s),
  ]
  for (const r of results.flat()) {
    if (!r.pass) {
      failures++
      console.log(`FAIL a healthy edge failed ${r.name} at seed ${s}: ${r.detail}`)
    }
  }
}
console.log('ok   a healthy edge, and a slow healthy one, pass across 200 seeds')

// Each defect fails the claim it breaks, and only that.
expect('no start delay', world({ edge: { startDelayMs: 0 } }), [
  'lease/the-second-stage-is-silent-inside-its-start-delay',
])
expect('a 1.5 s start delay', world({ edge: { startDelayMs: 1500 } }), [
  'lease/the-second-stage-is-silent-inside-its-start-delay',
])
expect('the second stage is never granted', world({ edge: { neverGrantsSecond: true } }), [
  'lease/the-second-stage-is-leased-after-confirmation-and-delay',
  'lease/the-alias-lease-window-has-a-control',
  'lease/run-on-keeps-the-second-stage-leased-after-the-stop',
  'lease/run-on-ends',
])
expect('no run-on', world({ edge: { runOnMs: 0 } }), [
  'lease/run-on-keeps-the-second-stage-leased-after-the-stop',
])
expect('a run-on that never ends', world({ edge: { runOnNeverEnds: true } }), ['lease/run-on-ends'])
expect('a run-on that ends and comes back', world({ edge: { runOnReturns: true } }), [
  'lease/run-on-ends',
])
expect('the first stage keeps its lease', world({ edge: { firstKeepsLease: true } }), [
  'lease/a-stop-refuses-the-first-stage-at-once',
])
expect('the sensor runs on', world({ edge: { sensorRunsOn: true } }), [
  'lease/a-sensor-is-refused-at-once-after-the-stop',
])
expect('the sensor is held like a stage', world({ edge: { sensorIsAStage: true } }), [
  'lease/a-sensor-is-no-stage-and-is-leased-while-the-second-stage-waits',
])
expect(
  'a lease on the retired alias',
  world({
    extra: [{ after: 12, topic: 'toolguard/lease', json: { device_id: IDS.plug2Id, grant: true } }],
  }),
  ['lease/the-old-lease-topic-is-silent']
)

// The recorder missed the confirm: the precondition fails, and the slices are
// not judged at all -- an empty slice must never pass as silence.
const blind = world({ drop: (e) => e.topic === 'resource/request/power' })
expect('the confirm was not recorded', blind, ['lease/the-timeline-holds-the-confirm-and-the-stop'])
if (blind.length !== 1) {
  failures++
  console.log(`FAIL with no anchor the judge still reported ${blind.length - 1} slice claim(s)`)
}

// The parser: the recorder's real format, a payload with a tab in it, and a
// line that is not one.
const parsed = parseTimeline(
  '1794895164.582369962\t0\ta/b\t{"x":1}\n1794895165.2\t1\ta/c\t\nnot a line\n1.5\t0\ta/d\tx\ty\n'
)
const parserGood =
  parsed.length === 3 &&
  parsed[0].t === 1794895164.582369962 &&
  parsed[0].json?.x === 1 &&
  parsed[1].retained === true &&
  parsed[1].json === null &&
  parsed[2].line === 4 &&
  parsed[2].payload === 'x\ty'
if (!parserGood) failures++
console.log(
  `${parserGood ? 'ok  ' : 'FAIL'} the parser reads the recorder's format: ${parserGood ? '' : JSON.stringify(parsed)}`
)

if (failures) {
  console.log(`\n${failures} failure(s)`)
  process.exit(1)
}
console.log('\nevery claim fails when it should and holds when it should')
