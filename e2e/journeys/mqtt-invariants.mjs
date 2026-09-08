// The MQTT-outage invariants: losing the broker must not disturb the HTTP
// server, and getting the broker back must restore message delivery.
//
// Kept pure and separate from the driver on purpose, exactly as
// journeys/invariants.mjs is separated from journeys.mjs: mqtt-selftest.mjs
// feeds these what a hung server would produce and confirms they fire. An
// invariant that has never been seen to fire is indistinguishable from one that
// cannot, and the whole mqttloss stage rests on these being right.
//
// These exist because of a real defect. `MqttService::start` consumed messages
// through paho's *synchronous* receiver and called `recv()` on it inside an
// async task, which parks the OS thread rather than the task. When the broker
// went away the runtime starved and axum stopped calling `accept()`: the
// process stayed up, the port stayed open, and the listening socket's accept
// queue filled with connections nobody would ever answer. A structural check
// can see that construct; only this tier can see the behaviour, and only this
// tier would notice a *different* construct (a blocking database call, a
// std::sync::Mutex held across an await) reintroducing the same outage.

/**
 * The HTTP surface was unaffected by the broker going away.
 *
 * Deliberately demanding about the sampling itself, not just the samples. An
 * oracle that accepts "no observations" passes hardest when the stage is most
 * broken -- a sampler that crashed on its first iteration would otherwise look
 * identical to a server that sailed through the outage. The absence being
 * asserted here is "no failed request", and asserting an absence is only
 * meaningful with time allowed to pass, so the span is part of the claim.
 *
 * @param {{minSamples:number, minSpanSeconds:number}} model
 * @param {{samples: Array<{atMs:number, code:number}>}} observed
 * @returns {string|null} null when satisfied, else the violation.
 */
export function httpSurvivedBrokerLoss(model, observed) {
  const samples = observed?.samples ?? []

  if (samples.length < model.minSamples) {
    return `only ${samples.length} sample(s) taken during the outage, needed at least ${model.minSamples}; the sampler did not run long enough to prove anything`
  }

  const span = (Math.max(...samples.map((s) => s.atMs)) - Math.min(...samples.map((s) => s.atMs))) / 1000
  if (span < model.minSpanSeconds) {
    return `samples span only ${span.toFixed(1)}s of the outage, needed at least ${model.minSpanSeconds}s; a server that hangs takes a moment to stop accepting`
  }

  // 0 is how a client reports "no response at all" -- the connection sat in the
  // accept queue until the timeout. That is precisely the observed symptom of
  // the defect, so it is called out rather than lumped in with a 5xx.
  const hung = samples.filter((s) => s.code === 0)
  if (hung.length) {
    return `${hung.length} of ${samples.length} request(s) got no response at all during the broker outage -- the HTTP server stopped accepting connections`
  }

  const bad = samples.filter((s) => s.code !== 200)
  if (bad.length) {
    const codes = [...new Set(bad.map((s) => s.code))].join(', ')
    return `${bad.length} of ${samples.length} request(s) failed during the broker outage (status ${codes}); the HTTP surface must be unaffected by the broker`
  }

  return null
}

/**
 * The consumer re-subscribed after the connection came back.
 *
 * The session is created with `clean_session(true)`, so the broker discards our
 * subscriptions when the link drops. A client that reconnects without
 * re-subscribing is connected and *deaf*: it looks healthy from every angle --
 * process up, socket connected, logs quiet -- while silently delivering no
 * heartbeats, device data or door events. Nothing short of watching a message
 * actually land can tell those two states apart, which is why this compares a
 * real received message before and after rather than counting subscribe calls
 * in the source.
 *
 * The observable is deliberately "the server handled a heartbeat for this
 * specific id", not "a device row changed". The stack's default cluster is
 * LATIN1 on purpose (it starts hostile), and a device cannot be registered
 * there at all -- the invite code is astral-plane emoji. Keying on an id the
 * stage invents means this tier runs on the default profile instead of only
 * under `--profile utf8`, which is the difference between a check that runs and
 * one that does not.
 *
 * @param {{beforeSeen: boolean}} model  a heartbeat published before the outage was received.
 * @param {{afterSeen: boolean}} observed  one published after the reconnect was received.
 * @returns {string|null} null when satisfied, else the violation.
 */
export function subscriptionsRestored(model, observed) {
  // Checked first: without it, a post-outage silence cannot be distinguished
  // from "was never subscribed", and the run would report the wrong defect.
  if (model?.beforeSeen !== true) {
    return 'the pre-outage heartbeat was never received, so the precondition did not hold; a post-outage result proves nothing about re-subscribing'
  }

  if (observed?.afterSeen !== true) {
    return 'no heartbeat was received after the broker came back; the reconnected client is not subscribed (connected but deaf)'
  }

  return null
}

export const MQTT_INVARIANTS = [
  { name: 'http-survived-broker-loss', fn: httpSurvivedBrokerLoss },
  { name: 'subscriptions-restored', fn: subscriptionsRestored },
]
