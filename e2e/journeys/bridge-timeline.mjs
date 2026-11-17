// The bridge stage's windows (spaces-iot-bridge against a real edge), judged
// against the stage timeline.
//
// Every window here used to be an mqtt_sub of a fixed number of seconds,
// opened by the shell, and the stage's steps were paced by `sleep`. Two
// timers run underneath them -- the bridge's 10 s stop-on-loss debounce and
// the doors' 3 s strike pulse -- and on a loaded guest the exec latency moved
// the windows past the events they were meant to hold (#234: window D found
// its session already stopped; window G closed 3.7 s before the enclosure
// opened). Some of that was the stage ACTING late against a timer, which no
// window can fix; the stage now waits for each event before the next step
// (run.sh). The rest was the window closing early, which this fixes: each
// window is now the stretch of the recording between the stage's own
// messages -- the badge, the plug's live report, the offline -- however long
// that stretch took.
//
// The slices are bounded by line numbers, which are arrival order, wherever
// the claim is about order ("after the badge, before the live report"); only
// F's "outlives the pulse" is about a duration, and is measured from the
// grant's own timestamp.

import { between, firstAfter, firstRawAfter } from './timeline.mjs'

export const PULSE_MS = 3000
export const TICK_MS = 500

/**
 * @param rows   parseTimeline() of the stage timeline
 * @param marks  parseMarks() of its marks
 * @param ids    { deviceId, toolId, friendly, doorA, doorB, lockA, lockB, contactId }
 * @returns [{ name, pass, detail }]
 */
export function judgeBridgeTimeline(rows, marks, ids) {
  const out = []
  const check = (name, pass, detail) => out.push({ name: `bridge/${name}`, pass: !!pass, detail })

  const POWER = 'resource/request/power'
  const LEASE = 'resource/lease'
  const SCAN = 'resource/request/scan'
  const STOP = 'resource/request/stop'
  const COND = 'resource/request/condition'
  const STATE = `zigbee2mqtt/${ids.friendly}`
  const SET = `${STATE}/set`
  const GET = `${STATE}/get`

  const after = (mark, topic, pred) =>
    marks.has(mark) ? firstAfter(rows, marks.get(mark), topic, pred) : null
  const anchors = {
    scanB: after('scanB', SCAN, (j) => j.resource === ids.toolId),
    liveB: after('liveB', STATE, (j) => j.state === 'ON'),
    retainedC: after('restart', STATE, (j) => j.state === 'ON'),
    liveC: after('liveC', STATE, (j) => j.state === 'ON'),
    offline: marks.has('offline')
      ? firstRawAfter(rows, marks.get('offline'), `${STATE}/availability`, 'offline')
      : null,
    scanE: after('scanE', SCAN, (j) => j.resource === ids.doorA),
    scanF: after('scanF', SCAN, (j) => j.resource === ids.doorB),
    // The lock going unavailable is Home Assistant's, not MQTT's: the mark
    // taken before it is written is the boundary. Taken before, so every
    // line past it is after the inject was written.
    injectF: marks.has('injectF') ? { line: marks.get('injectF') } : null,
    scanG: after('scanG', SCAN, (j) => j.resource === ids.toolId),
    openedG: after('injectG', COND, (j) => j.device_id === ids.contactId && j.asserted === true),
  }
  const end = marks.get('end') ?? Infinity
  const missing = Object.entries(anchors)
    .filter(([, a]) => !a)
    .map(([k]) => k)

  // The precondition, first: a step that is not in the recording leaves the
  // windows around it empty, and an empty window must not read as silence.
  check(
    'the-timeline-holds-every-step',
    missing.length === 0,
    `not recorded: ${missing.join(', ')} (${rows.length} line(s) recorded)`
  )
  /** Judge a window only if its anchors were recorded; otherwise fail each of its claims, naming why. */
  const windowOf = (needs, names, judge) => {
    const gone = needs.filter((k) => !anchors[k])
    if (gone.length) {
      for (const n of names) check(n, false, `not judged: ${gone.join(', ')} not in the timeline`)
      return
    }
    judge()
  }

  const mine = (j) => j.device_id === ids.deviceId
  const grants = (xs) => JSON.stringify(xs.map((x) => x.grant))
  const relayOn = (xs) => JSON.stringify(xs.map((x) => x.relay_on))

  // A: silent plug -> relay_on omitted on every one of OUR reports, from the
  // start of the recording to the badge.
  windowOf(
    ['scanB'],
    ['A-bridge-reports-while-plug-silent', 'A-silent-plug-omits-relay_on'],
    () => {
      const a = between(rows, POWER, 0, anchors.scanB.line, mine)
      check(
        'A-bridge-reports-while-plug-silent',
        a.length >= 1,
        `captured ${a.length} of our power reports`
      )
      check(
        'A-silent-plug-omits-relay_on',
        a.length >= 1 && a.every((r) => !('relay_on' in r)),
        `relay_on fields: ${relayOn(a)} -- "cannot report" must not be reported as off`
      )
    }
  )

  // B: the badge -> ON with on_time; the plug's live ON -> relay_on: true.
  windowOf(
    ['scanB', 'liveB', 'retainedC'],
    ['B-grant-commands-plug-on', 'B-on-carries-on_time', 'B-reported-on-becomes-relay_on-true'],
    () => {
      const setB = between(rows, SET, anchors.scanB.line, anchors.liveB.line)
      const onCmd = setB.find((c) => c.state === 'ON')
      check('B-grant-commands-plug-on', !!onCmd, `set commands: ${JSON.stringify(setB)}`)
      check(
        'B-on-carries-on_time',
        onCmd?.on_time === 60,
        `on_time: ${JSON.stringify(onCmd?.on_time)}`
      )
      const b = between(rows, POWER, anchors.liveB.line, anchors.retainedC.line, mine)
      check(
        'B-reported-on-becomes-relay_on-true',
        b.some((r) => r.relay_on === true),
        `relay_on after the plug reported ON: ${relayOn(b)}`
      )
    }
  )

  // C: restarted under a retained ON, it must /get, and omit relay_on from
  // the restart until a live report answers.
  windowOf(
    ['retainedC', 'liveC', 'offline'],
    [
      'C-retained-replay-triggers-a-get',
      'C-retained-replay-is-not-believed',
      'C-live-report-resolves-unknown',
    ],
    () => {
      const get = rows.filter(
        (r) => r.topic === GET && r.line > anchors.retainedC.line && r.line < anchors.liveC.line
      )
      check('C-retained-replay-triggers-a-get', get.length >= 1, `get requests: ${get.length}`)
      const c1 = between(rows, POWER, anchors.retainedC.line, anchors.liveC.line, mine)
      check(
        'C-retained-replay-is-not-believed',
        c1.length >= 1 && c1.every((r) => !('relay_on' in r)),
        `relay_on under a retained ON, before any live report: ${relayOn(c1)}`
      )
      const c2 = between(rows, POWER, anchors.liveC.line, anchors.offline.line, mine)
      check(
        'C-live-report-resolves-unknown',
        c2.some((r) => r.relay_on === true),
        `relay_on after a live ON: ${relayOn(c2)}`
      )
    }
  )

  // D: plug gone -> stop-on-loss -> refusal -> OFF, before door A's badge.
  windowOf(
    ['offline', 'scanE'],
    ['D-loss-stops-the-resource', 'D-refusal-switches-plug-off'],
    () => {
      const stops = between(
        rows,
        STOP,
        anchors.offline.line,
        anchors.scanE.line,
        (s) => s.resource === ids.toolId
      )
      check(
        'D-loss-stops-the-resource',
        stops.length >= 1,
        `stops for ${ids.toolId}: ${stops.length}`
      )
      const setD = between(rows, SET, anchors.offline.line, anchors.scanE.line)
      check(
        'D-refusal-switches-plug-off',
        setD.some((c) => c.state === 'OFF'),
        `set commands: ${JSON.stringify(setD)}`
      )
    }
  )

  // E: door A is momentary. The lease is the pulse, granted then withdrawn.
  windowOf(
    ['scanE', 'scanF'],
    ['E-badge-leases-the-lock', 'E-unlocked-is-reported-as-on', 'E-pulse-end-withdraws-the-lease'],
    () => {
      const leaseE = between(
        rows,
        LEASE,
        anchors.scanE.line,
        anchors.scanF.line,
        (l) => l.device_id === ids.lockA
      )
      check(
        'E-badge-leases-the-lock',
        leaseE.some((l) => l.grant === true),
        `leases for door A's lock: ${grants(leaseE)}`
      )
      const powerE = between(
        rows,
        POWER,
        anchors.scanE.line,
        anchors.scanF.line,
        (r) => r.device_id === ids.lockA
      )
      check(
        'E-unlocked-is-reported-as-on',
        powerE.some((r) => r.relay_on === true),
        `relay_on from door A's lock: ${relayOn(powerE)}`
      )
      const firstGrant = leaseE.findIndex((l) => l.grant === true)
      check(
        'E-pulse-end-withdraws-the-lease',
        firstGrant >= 0 && leaseE.slice(firstGrant).some((l) => l.grant === false),
        `after the pulse the lease must be refused: ${grants(leaseE)}`
      )
    }
  )
  // #214: every lease this edge publishes names a module it coordinates --
  // judged over the whole recording rather than one window of it. Sensor
  // bindings are leased too (edge/src/modules.rs), so the contact is ours.
  const ours = new Set([ids.deviceId, ids.lockA, ids.lockB, ids.contactId])
  const leased = rows.filter((r) => r.topic === LEASE && r.json)
  const foreign = [...new Set(leased.map((r) => r.json.device_id))].filter((d) => !ours.has(d))
  check(
    'E-the-edge-leases-only-what-it-coordinates',
    leased.length >= 1 && foreign.length === 0,
    `${leased.length} lease(s) recorded; ${foreign.length} device(s) leased or refused by this edge are bound to resources it does not coordinate`
  )

  // F: door B is until_stopped, so its lease outlives the pulse -- measured
  // from its first grant -- until Home Assistant loses the lock.
  windowOf(
    ['scanF', 'injectF', 'scanG'],
    ['F-lease-outlives-the-pulse', 'F-loss-stops-the-door'],
    () => {
      const first = rows.find(
        (r) =>
          r.topic === LEASE &&
          r.line > anchors.scanF.line &&
          r.json?.device_id === ids.lockB &&
          r.json.grant === true
      )
      const pastPulse = first
        ? rows
            .filter(
              (r) =>
                r.topic === LEASE &&
                r.json?.device_id === ids.lockB &&
                r.t >= first.t + (PULSE_MS + TICK_MS) / 1000 &&
                r.line <= anchors.injectF.line
            )
            .map((r) => r.json)
        : []
      check(
        'F-lease-outlives-the-pulse',
        pastPulse.length >= 2 && pastPulse.every((l) => l.grant === true),
        first
          ? `leases from ${PULSE_MS + TICK_MS} ms after the first grant to the lock's loss: ${grants(pastPulse)}`
          : 'door B was never granted'
      )
      const stopsF = between(
        rows,
        STOP,
        anchors.injectF.line,
        anchors.scanG.line,
        (s) => s.resource === ids.doorB
      )
      check('F-loss-stops-the-door', stopsF.length >= 1, `stops for door B: ${stopsF.length}`)
    }
  )

  // G: the contact as a sensor (#25 / CS:S #213). Shut since the bridge
  // connected; then a badge at the tool is granted with the enclosure shut,
  // and opening it trips the tool and switches the plug off.
  windowOf(['scanG'], ['G-the-contact-reports-its-condition', 'G-shut-is-door-open-false'], () => {
    const g0 = between(rows, COND, 0, anchors.scanG.line, (c) => c.device_id === ids.contactId)
    check(
      'G-the-contact-reports-its-condition',
      g0.length >= 1,
      `condition reports from the contact: ${g0.length}`
    )
    check(
      'G-shut-is-door-open-false',
      g0.length >= 1 &&
        g0.every(
          (c) => c.condition === 'door_open' && c.asserted === false && c.resource === ids.toolId
        ),
      `readings: ${JSON.stringify(g0.slice(0, 3))}`
    )
  })
  windowOf(
    ['scanG', 'openedG'],
    [
      'G-shut-enclosure-grants',
      'G-opening-the-enclosure-trips-the-tool',
      'G-the-trip-switches-the-plug-off',
    ],
    () => {
      const shut = between(rows, LEASE, anchors.scanG.line, anchors.openedG.line, mine)
      check(
        'G-shut-enclosure-grants',
        shut.some((l) => l.grant === true),
        `grants before the enclosure opened: ${grants(shut)}`
      )
      const opened = between(rows, LEASE, anchors.openedG.line, end, mine)
      const tripped = opened.filter(
        (l) =>
          l.grant === false &&
          typeof l.reason === 'string' &&
          l.reason.includes('Tripped') &&
          l.reason.includes('door_open')
      )
      check(
        'G-opening-the-enclosure-trips-the-tool',
        tripped.length >= 1,
        `reasons after it opened: ${JSON.stringify([...new Set(opened.map((l) => l.reason).filter(Boolean))])}`
      )
      const setG = between(rows, SET, anchors.scanG.line, end)
      check(
        'G-the-trip-switches-the-plug-off',
        setG.some((c) => c.state === 'ON') && setG[setG.length - 1]?.state === 'OFF',
        `set commands: ${JSON.stringify(setG)}`
      )
    }
  )
  return out
}
