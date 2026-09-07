#!/usr/bin/env node
//
// The oracle before the mqttloss stack stage.
//
// The stage's value rests entirely on these invariants being right: a stage
// that samples a hung server past a broken invariant reports success, and
// nothing in the output distinguishes that from a server that sailed through
// the outage. So each invariant is fed what a broken system would produce and
// must fire, and is fed a healthy world and must stay quiet. Every broken world
// is modeled on a defect this code could actually have:
//
//   * requests getting no response at all -- the observed symptom of the
//     blocking-recv defect, where the accept queue fills and nobody answers
//   * requests failing with a status -- the same outage seen through a proxy,
//     which reports 503 rather than hanging
//   * too few samples, or too short a span -- a sampler that died on its first
//     iteration, which is the case that would otherwise pass most convincingly
//   * a post-outage heartbeat that never lands -- reconnected but deaf, the
//     clean_session re-subscribe bug
//   * last_seen_at that does not advance -- the same deafness, seen where a
//     stale value could be mistaken for a fresh one
//   * no pre-outage heartbeat -- the precondition failing, so the post-outage
//     one proves nothing
//
// Runs with no stack, no database and no network, so it works on the FreeBSD
// workstation where css-server cannot even be compiled -- the cheapest thing to
// run is the thing that decides whether the most expensive thing means
// anything.
//
//   node e2e/journeys/mqtt-selftest.mjs
//
// Exit 0 if every invariant fires when it should and stays quiet when it should.

import { MQTT_INVARIANTS } from './mqtt-invariants.mjs'

const T0 = 1_700_000_000_000
const sample = (i, code) => ({ atMs: T0 + i * 1000, code })
const healthySamples = Array.from({ length: 20 }, (_, i) => sample(i, 200))

// A world every invariant must be happy with.
const HEALTHY = {
  'http-survived-broker-loss': {
    model: { minSamples: 10, minSpanSeconds: 15 },
    observed: { samples: healthySamples },
  },
  'subscriptions-restored': {
    model: { beforeSeen: true },
    observed: { afterSeen: true },
  },
}

const BROKEN = {
  'http-survived-broker-loss': [
    {
      // Deliberately long enough to clear the sampling checks: the point is to
      // exercise the hung-request assertion, and a fixture that trips the span
      // check first would report a pass for the wrong reason. (It did, the
      // first time this file ran -- which is the entire argument for having a
      // self-test.)
      why: 'requests got no response at all -- the accept queue filled and nobody answered',
      model: { minSamples: 10, minSpanSeconds: 15 },
      observed: { samples: [...healthySamples, sample(20, 0), sample(21, 0)] },
      expect: /no response at all/,
    },
    {
      why: 'the same outage behind a proxy, which answers 503 instead of hanging',
      model: { minSamples: 10, minSpanSeconds: 15 },
      observed: { samples: [...healthySamples.slice(0, 15), sample(15, 503)] },
      expect: /failed during the broker outage/,
    },
    {
      why: 'the sampler died on its first iteration; almost no observations',
      model: { minSamples: 10, minSpanSeconds: 15 },
      observed: { samples: [sample(0, 200)] },
      expect: /sample\(s\) taken during the outage/,
    },
    {
      why: 'enough samples but taken in a burst -- no time was allowed to pass',
      model: { minSamples: 10, minSpanSeconds: 15 },
      observed: { samples: Array.from({ length: 20 }, (_, i) => ({ atMs: T0 + i * 10, code: 200 })) },
      expect: /span only/,
    },
    {
      why: 'no samples at all',
      model: { minSamples: 10, minSpanSeconds: 15 },
      observed: { samples: [] },
      expect: /sample\(s\) taken during the outage/,
    },
  ],
  'subscriptions-restored': [
    {
      why: 'reconnected but deaf: the post-outage heartbeat never arrived',
      model: { beforeSeen: true },
      observed: { afterSeen: false },
      expect: /connected but deaf/,
    },
    {
      why: 'the precondition never held -- nothing was flowing before the outage either',
      model: { beforeSeen: false },
      observed: { afterSeen: true },
      expect: /precondition did not hold/,
    },
    {
      why: 'nothing flowed at all, in either direction',
      model: { beforeSeen: false },
      observed: { afterSeen: false },
      expect: /precondition did not hold/,
    },
    {
      // A missing field must not read as a pass. `undefined !== true` is the
      // whole guard, and it is worth pinning: an observation the stage failed
      // to record would otherwise look like a healthy one.
      why: 'the stage recorded no observation at all',
      model: { beforeSeen: true },
      observed: {},
      expect: /connected but deaf/,
    },
  ],
}

let failures = 0
const fail = (msg) => {
  console.error(`FAIL ${msg}`)
  failures += 1
}

for (const { name, fn } of MQTT_INVARIANTS) {
  const healthy = HEALTHY[name]
  if (!healthy) {
    fail(`${name}: no healthy world defined; every invariant must be shown to stay quiet`)
    continue
  }
  const quiet = fn(healthy.model, healthy.observed)
  if (quiet !== null) {
    fail(`${name}: fired on a healthy world: ${quiet}`)
  } else {
    console.log(`ok   ${name}: quiet on a healthy world`)
  }

  const broken = BROKEN[name] ?? []
  if (!broken.length) {
    fail(`${name}: no broken world defined; an invariant never seen to fire is unmeasured`)
  }
  for (const c of broken) {
    const got = fn(c.model, c.observed)
    if (got === null) {
      fail(`${name}: stayed quiet on a broken world (${c.why})`)
    } else if (c.expect && !c.expect.test(got)) {
      fail(`${name}: fired on "${c.why}" but with the wrong complaint: ${got}`)
    } else {
      console.log(`ok   ${name}: fired on ${c.why}`)
    }
  }
}

if (failures) {
  console.error(`\n${failures} self-test failure(s)`)
  process.exit(1)
}
console.log('\nmqtt invariants self-test passed')
