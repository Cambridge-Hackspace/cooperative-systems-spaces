#!/usr/bin/env node
//
// The oracle before the Groups.io stack stage.
//
// The groupsio stage's value rests on `groupsioReconcileHonored` being right: a
// driver that reads a broken roster past a broken invariant reports success. So
// each invariant is fed what a broken reconcile would leave behind and must
// fire, and is fed a healthy world and must stay quiet. Every broken world is
// modeled on a defect this feature could actually have:
//
//   * an intended member the reconcile never added
//   * a stranger the reconcile failed to remove -- the platform does not own the
//     list after all
//   * a protected address the reconcile wrongly removed -- the "own the list"
//     posture evicting the group's own owner
//
// Runs with no stack, no database and no network, so it works on the FreeBSD
// workstation where css-server cannot even be compiled -- the cheapest thing to
// run is the thing that decides whether the most expensive thing means anything.
//
//   node e2e/journeys/groupsio-selftest.mjs
//
// Exit 0 if every invariant fires when it should and stays quiet when it should.

import { GROUPSIO_INVARIANTS } from './groupsio-invariants.mjs'

// A world every invariant must be happy with. `observed` deliberately carries an
// extra address (admin) the model does not name, to prove the per-address
// invariant does not treat an unenumerated member as a violation.
const HEALTHY = {
  'groupsio-reconcile-honored': {
    model: { present: ['member@e2e.invalid', 'owner@e2e.invalid'], absent: ['stranger@e2e.invalid'] },
    observed: ['member@e2e.invalid', 'owner@e2e.invalid', 'admin@e2e.invalid'],
  },
}

const BROKEN = {
  'groupsio-reconcile-honored': [
    {
      why: 'an intended member the reconcile never added',
      model: { present: ['member@e2e.invalid'], absent: [] },
      observed: ['owner@e2e.invalid'],
      expect: /should be on the group/,
    },
    {
      why: 'a stranger the reconcile failed to remove -- the platform does not own the list',
      model: { present: [], absent: ['stranger@e2e.invalid'] },
      observed: ['stranger@e2e.invalid', 'member@e2e.invalid'],
      expect: /should have been removed/,
    },
    {
      why: 'a protected address the reconcile wrongly removed',
      model: { present: ['owner@e2e.invalid'], absent: [] },
      observed: ['member@e2e.invalid'],
      expect: /should be on the group/,
    },
  ],
}

let cases = 0
let failures = 0
function ok(name, detail = '') {
  cases += 1
  console.log(`  ok    ${name}${detail ? ` -- ${detail}` : ''}`)
}
function fail(name, detail) {
  cases += 1
  failures += 1
  console.log(`  FAIL  ${name} -- ${detail}`)
}

console.log('Groups.io oracle self-test')
console.log('')

console.log('a healthy world:')
for (const { name, fn } of GROUPSIO_INVARIANTS) {
  const h = HEALTHY[name]
  if (!h) {
    fail(`${name}/has-a-healthy-fixture`, 'no healthy world for this invariant')
    continue
  }
  const result = fn(h.model, h.observed)
  if (result === null) ok(`${name}/quiet-when-satisfied`)
  else fail(`${name}/quiet-when-satisfied`, `fired on a healthy world: ${result}`)
}

console.log('')
console.log('broken worlds:')
for (const { name, fn } of GROUPSIO_INVARIANTS) {
  const scenarios = BROKEN[name]
  if (!scenarios || scenarios.length === 0) {
    fail(`${name}/has-a-broken-fixture`, 'no broken world; an invariant never seen to fire is indistinguishable from one that cannot')
    continue
  }
  for (const s of scenarios) {
    const result = fn(s.model, s.observed)
    if (result === null) fail(`${name}: ${s.why}`, 'did not fire')
    else if (!s.expect.test(result)) fail(`${name}: ${s.why}`, `fired with the wrong message -- expected ${s.expect}, got: ${result}`)
    else ok(`${name}: ${s.why}`)
  }
}

console.log('')
console.log('the self-test covers the invariants that exist:')
{
  const named = new Set(GROUPSIO_INVARIANTS.map((i) => i.name))
  const tested = new Set(Object.keys(BROKEN))
  const untested = [...named].filter((n) => !tested.has(n))
  const orphaned = [...tested].filter((n) => !named.has(n))
  if (untested.length === 0) ok('every invariant has at least one broken world')
  else fail('every invariant has at least one broken world', untested.join(', '))
  if (orphaned.length === 0) ok('every broken world names an invariant that exists')
  else fail('every broken world names an invariant that exists', orphaned.join(', '))
}

console.log('')
console.log(`${cases} case(s), ${failures} failure(s)`)
if (failures > 0) {
  console.log('')
  console.log('The invariant is not trustworthy, so the stage that uses it is not either.')
}
process.exit(failures > 0 ? 1 : 0)
