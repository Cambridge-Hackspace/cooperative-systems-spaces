// The lease stage's timed claims (#179), judged against the stage timeline.
//
// Start sequencing and run-on are timers on the edge: the second plug is
// leased START_DELAY after the first CONFIRMS it energized, and keeps its
// lease RUN_ON after the member signs off. These used to be judged by four
// back-to-back mqtt_sub windows of 2 and 4 s, opened by the shell after the
// publish that starts the timer. On a loaded guest the exec that opens a
// window lags, the window slides past the timer it sits inside, and a correct
// edge fails: 2026-11-17's release battery caught the second plug granted
// "inside" its delay and refused "inside" its run-on, each about 1 s after
// the timer really ran out.
//
// So each slice here is measured from the timestamp the recorder gave the
// confirm and the stop themselves. A slow exec now delays the event and its
// consequences together, and the slices stay where the timers are.
//
// The tolerance is one lease tick (TICK_MS, the stage's
// module_lease_interval_ms), and it is applied only where the edge cannot be
// exact: a slice that must be silent or granted ends a tick before the timer
// does, a slice that must be refused starts a tick after the event, because
// a lease tick already in flight when the stop arrives is not the edge
// failing to stop. Nothing is widened past a tick.

import { firstAfter, during } from './timeline.mjs'

export const START_DELAY_MS = 3000
export const RUN_ON_MS = 3000
export const TICK_MS = 500

const LEASE = 'resource/lease'

/**
 * @param rows   parseTimeline() of the stage timeline
 * @param marks  parseMarks() of its marks: `confirm`, `stop`, `end`
 * @param ids    { toolId (as the stop names it), deviceId (the first plug), plug2Id, sensorId }
 * @returns [{ name, pass, detail }] in the order they should be reported
 */
export function judgeLeaseTimeline(rows, marks, ids) {
  const out = []
  const check = (name, pass, detail) => out.push({ name, pass: !!pass, detail })

  const confirm = marks.has('confirm')
    ? firstAfter(
        rows,
        marks.get('confirm'),
        'resource/request/power',
        (j) => j.device_id === ids.deviceId && j.relay_on === true
      )
    : null
  const stop = marks.has('stop')
    ? firstAfter(rows, marks.get('stop'), 'resource/request/stop', (j) => j.resource === ids.toolId)
    : null
  const end = marks.get('end') ?? Infinity

  // The precondition, asserted first: without both anchors every slice below
  // is empty, and an empty slice would read as silence.
  check(
    'lease/the-timeline-holds-the-confirm-and-the-stop',
    confirm && stop && stop.line > confirm.line,
    `confirm ${confirm ? `at line ${confirm.line}` : 'not recorded'}, stop ${stop ? `at line ${stop.line}` : 'not recorded'}; ` +
      `${rows.length} line(s) recorded`
  )
  if (!confirm || !stop) return out

  const s = (ms) => ms / 1000
  const ofPlug2 = (j) => j.device_id === ids.plug2Id
  const ofSensor = (j) => j.device_id === ids.sensorId
  const ofFirst = (j) => j.device_id === ids.deviceId
  const fmt = (rows_) =>
    JSON.stringify(rows_.map((r) => (r.grant ? 'grant' : `refuse:${r.reason}`)))

  // 2a: confirmed, inside the start delay.
  // Ends at the stop if that comes first: after it nothing is waiting.
  const delayEnd = Math.min(confirm.t + s(START_DELAY_MS - TICK_MS), stop.t)
  const plug2InDelay = during(rows, LEASE, confirm.t, delayEnd, ofPlug2)
  check(
    'lease/the-second-stage-is-silent-inside-its-start-delay',
    plug2InDelay.length === 0,
    `the second stage was addressed within ${START_DELAY_MS - TICK_MS} ms of the confirm: ${fmt(plug2InDelay)}`
  )
  const sensorInDelay = during(rows, LEASE, confirm.t, delayEnd, ofSensor)
  check(
    'lease/a-sensor-is-no-stage-and-is-leased-while-the-second-stage-waits',
    sensorInDelay.length >= 1 && sensorInDelay.every((r) => r.grant === true),
    `inside the second stage's start delay the sensor must already hold its lease: ${fmt(sensorInDelay)}`
  )

  // 2b: confirmed and delayed -- granted, up to a tick before the stop. Not
  // to the stop's own line: a refusal sent the instant the stop lands can be
  // stamped a hair ahead of the stop itself, and is the stop's, not 2b's.
  const sequenced = during(rows, LEASE, confirm.t, stop.t - s(TICK_MS), ofPlug2)
  check(
    'lease/the-second-stage-is-leased-after-confirmation-and-delay',
    sequenced.length >= 1 && sequenced[sequenced.length - 1].grant === true,
    `the second stage never got its lease after the first confirmed and the delay ran: ${fmt(sequenced)}`
  )

  // #192: the old alias name carries nothing, anywhere in the recording. The
  // grant just above is the control: the edge was leasing on the real topic
  // while this was recorded, so an empty alias is the alias gone.
  check(
    'lease/the-alias-lease-window-has-a-control',
    sequenced.length >= 1,
    'the timeline carried no lease to the second stage, so an empty alias topic proves nothing'
  )
  const alias = rows.filter((r) => r.topic === 'toolguard/lease')
  check(
    'lease/the-old-lease-topic-is-silent',
    alias.length === 0,
    `toolguard/lease still carries leases after #192: ${alias
      .slice(0, 3)
      .map((r) => r.payload)
      .join(' ')}`
  )

  // 2c: signed off, inside the run-on.
  const refusedFrom = stop.t + s(TICK_MS)
  const runOnEnd = stop.t + s(RUN_ON_MS - TICK_MS)
  const firstAfterStop = during(rows, LEASE, refusedFrom, runOnEnd, ofFirst)
  check(
    'lease/a-stop-refuses-the-first-stage-at-once',
    firstAfterStop.length >= 1 &&
      firstAfterStop.every((r) => r.grant === false && r.reason === 'NotInUse'),
    `from a lease tick after tool-off the first stage must be refused with NotInUse: ${fmt(firstAfterStop)}`
  )
  const sensorAfterStop = during(rows, LEASE, refusedFrom, runOnEnd, ofSensor)
  check(
    'lease/a-sensor-is-refused-at-once-after-the-stop',
    sensorAfterStop.length >= 1 && sensorAfterStop.every((r) => r.grant === false),
    `a sensor gets no run-on; from a lease tick after tool-off every lease to it must refuse: ${fmt(sensorAfterStop)}`
  )
  const plug2RunOn = during(rows, LEASE, stop.t, runOnEnd, ofPlug2)
  check(
    'lease/run-on-keeps-the-second-stage-leased-after-the-stop',
    plug2RunOn.length >= 1 && plug2RunOn.every((r) => r.grant === true),
    `inside its run-on the second stage must still be granted: ${fmt(plug2RunOn)}`
  )

  // 2d: the run-on is over, and stays over.
  const overFrom = stop.t + s(RUN_ON_MS + TICK_MS)
  const plug2Over = rows
    .filter((r) => r.topic === LEASE && r.t > overFrom && r.line < end && r.json && ofPlug2(r.json))
    .map((r) => r.json)
  check(
    'lease/run-on-ends',
    plug2Over.length >= 1 && plug2Over.every((r) => r.grant === false),
    `a tick past its run-on the second stage must be refused, and stay refused: ${fmt(plug2Over)}`
  )
  return out
}
