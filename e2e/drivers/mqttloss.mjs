// Tier: the HTTP server is not coupled to the MQTT broker.
//
// The defect this exists for: `MqttService::start` consumed messages through
// paho's *synchronous* receiver and called `recv()` on it inside an async task,
// which parks the OS thread rather than the task. A parked thread cannot be
// preempted and its work cannot be stolen, so when the broker went away the
// runtime starved and axum stopped calling `accept()`. The process stayed up,
// the port stayed open, and the listening socket's accept queue filled with
// connections nobody would ever answer.
//
// A source-level check can see that construct, and one exists
// (checks/tests/mqtt_never_blocks_the_runtime.rs). What it cannot see is the
// behaviour -- and it would not notice a *different* construct (a blocking
// database call, a std::sync::Mutex held across an await) reintroducing the
// same outage. That is this tier's question, and no cheaper tier can answer it.
//
// The stage in run.sh does the observing, because everything it has to do --
// take the broker away, sample HTTP while it is gone, publish once it is back,
// read the server log -- is the stack's business rather than the API's. This
// driver is the judgment: it applies the invariants to what the stage wrote, so
// the rules live somewhere mqtt-selftest.mjs can prove they actually fire.
//
// WHAT THIS DOES NOT PROVE: that every HTTP route survives an outage (the stage
// samples /status), or that message ordering is preserved.

import fs from 'node:fs'
import path from 'node:path'
import { main, record } from './lib.mjs'
import { httpSurvivedBrokerLoss, subscriptionsRestored } from '../journeys/mqtt-invariants.mjs'

const STACK_DIR = process.env.CSS_STACK_DIR ?? '/stack'
const OBSERVED = path.join(STACK_DIR, 'mqttloss-observed.json')

await main(async () => {
  let observed
  try {
    observed = JSON.parse(fs.readFileSync(OBSERVED, 'utf8'))
  } catch (e) {
    // Not a soft failure: with no observations there is nothing to judge, and
    // reporting "ok" here would be the exact shape of a suite that passes for
    // work it did not do.
    record('mqttloss/observations-readable', 'fail', `could not read what the stage observed: ${e.message}`)
    return
  }
  record('mqttloss/observations-readable', 'ok')

  // Oracle 1: the HTTP surface was untouched by the broker going away.
  const httpViolation = httpSurvivedBrokerLoss(
    { minSamples: 8, minSpanSeconds: 15 },
    { samples: observed.samples ?? [] },
  )
  record(
    'mqttloss/http-survived-broker-loss',
    httpViolation === null ? 'ok' : 'fail',
    httpViolation ?? '',
  )

  // Oracle 2: delivery resumed. A different claim from "the process survived",
  // and the one the clean_session re-subscribe fix is for -- a client that
  // reconnects without re-subscribing satisfies oracle 1 perfectly while being
  // completely deaf.
  const subViolation = subscriptionsRestored(
    { beforeSeen: observed.beforeSeen === true },
    { afterSeen: observed.afterSeen === true },
  )
  record(
    'mqttloss/subscriptions-restored',
    subViolation === null ? 'ok' : 'fail',
    subViolation ?? '',
  )
})
