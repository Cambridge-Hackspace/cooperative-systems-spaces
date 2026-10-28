// A scan reply must name the resource it is an answer about (#166, #192).
//
// WHY THIS TIER AND NOT A UNIT TEST. `resource/response/scan` is published
// by the edge from three separate decision paths, and until this driver existed
// *nothing in e2e subscribed to that topic at all* -- the battery's only MQTT
// publish was the plug's power report. So a full green battery said nothing
// whatsoever about the payload a module actually receives, which is the payload
// that decides whether a machine energizes. `checks/` can prove the field
// cannot be structurally omitted and that every call site passes `&req.resource`;
// only this tier can prove the bytes arrive on the wire.
//
// THE DEFECT IT GUARDS. The reply used to be `{ authorized, reason }` with
// nothing saying what it answered. These topics are unnamespaced and carry no
// device id, so every module on a local broker receives every reply and could
// not tell its own verdict from the one for the reader at the next machine:
// two readers on one broker, and a yes at one starts a session at the other
// where somebody is waiting for a no. The door half of the same resource model
// (#101) had always echoed `door_id`; the tool half never did.
//
// WHY TWO REQUESTS AND NOT ONE. A single request asserting "the reply names
// laser-01" is satisfied by an edge that echoes a constant, or the wrong
// variable, or the tool it happens to have handy. The second request names a
// tool that exists nowhere, and the assertion is that *its own* id comes back --
// so the echo has to vary with the request. That is the whole claim; one
// request cannot make it.
//
// WHAT THIS DOES NOT PROVE: that any module filters on the field (that is
// firmware's oracle, and `spaces-firmware` has host tests for it), that the
// authorization decision is right, or that the two paths not exercised here --
// a power lockout and the online-synchronous forward -- echo it too. Those are
// covered structurally in `checks/tests/tool_on_reply_is_addressable.rs`, which
// asserts every call site passes the request's own resource. Both tiers
// together are the claim; neither is it alone.

import fs from 'node:fs'
import path from 'node:path'
import { record, main } from './lib.mjs'

const STACK_DIR = process.env.CSS_STACK_DIR ?? '/stack'

/** Every JSON object on its own line in a mosquitto_sub capture. */
function replies(file) {
  let raw
  try {
    raw = fs.readFileSync(path.join(STACK_DIR, file), 'utf8')
  } catch {
    return []
  }
  return raw
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => l.length > 0)
    .map((l) => {
      try {
        return JSON.parse(l)
      } catch {
        // A non-JSON line on this topic is itself worth seeing rather than
        // silently dropping, so it is kept as a marker the assertions below
        // will fail on rather than skip over.
        return { __unparseable: l }
      }
    })
}

async function assertEcho() {
  const tool = fs.readFileSync(path.join(STACK_DIR, 'lease-tool-id'), 'utf8').trim()
  const other = fs
    .readFileSync(path.join(STACK_DIR, 'toolon-other-id'), 'utf8')
    .trim()

  const own = replies('toolon-reply-own.txt')
  const notOurs = replies('toolon-reply-other.txt')

  // The precondition, asserted BEFORE the content checks so a failure reads as
  // "nothing answered" rather than as a missing field thirty seconds later.
  // An empty capture makes every `some()` below vacuously false, which would
  // otherwise present as the echo being wrong.
  if (own.length === 0) {
    record(
      'toolonreply/the-edge-answers-at-all',
      'fail',
      `no message captured on resource/response/scan for ${tool}; ` +
        `the edge did not answer, so nothing below is about the echo`,
    )
    return
  }
  record('toolonreply/the-edge-answers-at-all', 'ok', `${own.length} message(s)`)

  const bad = own.concat(notOurs).filter((r) => r.__unparseable !== undefined)
  if (bad.length > 0) {
    record(
      'toolonreply/every-reply-is-json',
      'fail',
      `${bad.length} line(s) on this topic are not JSON, first: ${bad[0].__unparseable.slice(0, 120)}`,
    )
  } else {
    record('toolonreply/every-reply-is-json', 'ok')
  }

  // 1. The reply names the resource that was asked about.
  if (own.some((r) => r.resource === tool)) {
    record('toolonreply/names-the-tool-asked-about', 'ok', tool)
  } else {
    record(
      'toolonreply/names-the-tool-asked-about',
      'fail',
      `asked about ${tool}; replies carried resource ` +
        `${JSON.stringify(own.map((r) => r.resource ?? null))}. A module cannot ` +
        `tell this answer from the answer to the reader at the next machine.`,
    )
  }

  // 2. And the verdict field is still where firmware looks for it. A rename
  //    here would be invisible to assertion 1 and would stop every module.
  const verdicts = own.filter((r) => typeof r.authorized === 'boolean')
  if (verdicts.length > 0) {
    record('toolonreply/still-carries-a-boolean-authorized', 'ok')
  } else {
    record(
      'toolonreply/still-carries-a-boolean-authorized',
      'fail',
      `no reply carried a boolean "authorized"; got ${JSON.stringify(own[0])}. ` +
        `Absence is not permission, so firmware fails closed -- as a tool that ` +
        `never starts, with no error at either end.`,
    )
  }

  // 3. THE ECHO VARIES WITH THE REQUEST. Without this, a hardcoded string or
  //    the wrong variable passes assertion 1.
  if (notOurs.length === 0) {
    record(
      'toolonreply/the-echo-varies-with-the-request',
      'fail',
      `no reply captured for the second request (${other}), so assertion 1 ` +
        `cannot be distinguished from an edge echoing a constant`,
    )
  } else if (notOurs.some((r) => r.resource === other)) {
    record('toolonreply/the-echo-varies-with-the-request', 'ok', other)
  } else {
    record(
      'toolonreply/the-echo-varies-with-the-request',
      'fail',
      `asked about ${other}; replies carried resource ` +
        `${JSON.stringify(notOurs.map((r) => r.resource ?? null))}. The field is ` +
        `present but is not the request's own value, which is a reply that is ` +
        `well-formed, consistently wrong, and indistinguishable from a neighbour's.`,
    )
  }

  // 4. THE ALIAS IS GONE (#192). The same request on the old tool-on name got
  //    no answer on the old reply name AND none on the resource reply name,
  //    inside a window longer than the edge took to answer the request above.
  //    The capture before this one is the control: the edge was answering a
  //    second earlier, so emptiness here is the alias being unsubscribed and
  //    not the edge being dead. Precondition first, so a dead edge presents as
  //    "nothing answered the real request" and never as a passing absence.
  const aliasReply = replies('toolon-alias-reply.txt')
  const aliasLeak = replies('toolon-alias-leak.txt')
  if (notOurs.length === 0) {
    record(
      'toolonreply/the-alias-absence-has-a-control',
      'fail',
      'the resource request just before the alias request got no reply, so an empty alias window proves nothing',
    )
    return
  }
  record('toolonreply/the-alias-absence-has-a-control', 'ok')
  if (aliasReply.length === 0) {
    record('toolonreply/the-old-request-topic-gets-no-reply-on-the-old-name', 'ok')
  } else {
    record(
      'toolonreply/the-old-request-topic-gets-no-reply-on-the-old-name',
      'fail',
      `toolguard/response/tool-on answered a toolguard/request/tool-on after #192: ` +
        `${JSON.stringify(aliasReply[0]).slice(0, 200)}`,
    )
  }
  if (aliasLeak.length === 0) {
    record('toolonreply/the-old-request-topic-gets-no-reply-on-the-new-name', 'ok')
  } else {
    record(
      'toolonreply/the-old-request-topic-gets-no-reply-on-the-new-name',
      'fail',
      `a toolguard/request/tool-on was answered on resource/response/scan after #192 -- the ` +
        `edge still subscribes to the alias: ${JSON.stringify(aliasLeak[0]).slice(0, 200)}`,
    )
  }
}

main(async () => {
  const phase = process.argv[2]
  if (phase === 'assert') {
    await assertEcho()
    return
  }
  record('toolonreply/driver-phase', 'fail', `unknown phase ${JSON.stringify(phase)}`)
})
