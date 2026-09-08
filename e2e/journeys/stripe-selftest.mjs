#!/usr/bin/env node
//
// The oracle before the stripe stack stage.
//
// The stripe stage's value rests on `membershipHonored` being right: a driver
// that reads a broken state past a broken invariant reports success. So the
// invariant is fed what a broken lifecycle would leave behind and must fire, and
// is fed a healthy world and must stay quiet. Every broken world is modeled on a
// defect this feature could actually have:
//
//   * a paid member the grant never promoted (right balance, wrong role)
//   * a lapse that drove the balance negative -- the non-negative rule violated
//   * a lapse that cleared enrollment but left the role a member
//   * a credit posted twice (balance too high)
//
// Runs with no stack, no database and no network, so it works on the FreeBSD
// workstation where css-server cannot even be compiled.
//
//   node e2e/journeys/stripe-selftest.mjs
//
// Exit 0 if the invariant fires when it should and stays quiet when it should.

import { STRIPE_INVARIANTS } from './stripe-invariants.mjs'

// A healthy world: a member in good standing, balance zeroed by the first dues
// deduction, non-negative. The invariant must be happy.
const HEALTHY = {
  'membership-honored': {
    model: { role: 'member', enrolled: true, balance: 0, nonNegative: true },
    observed: { role: 'Member', enrolled: true, balance: '0.00' },
  },
}

const BROKEN = {
  'membership-honored': [
    {
      why: 'a paid member the grant never promoted (right balance, wrong role)',
      model: { role: 'member', balance: 0 },
      observed: { role: 'Newbie', enrolled: true, balance: '0.00' },
      expect: /role should be/,
    },
    {
      why: 'a lapse that drove the balance negative -- the non-negative rule violated',
      model: { nonNegative: true },
      observed: { role: 'Newbie', enrolled: false, balance: '-10.00' },
      expect: /non-negative/,
    },
    {
      why: 'a lapse that cleared enrollment but left the role a member',
      model: { enrolled: false },
      observed: { role: 'Member', enrolled: true, balance: '0.00' },
      expect: /enrolled should be/,
    },
    {
      why: 'a credit posted twice (balance too high)',
      model: { balance: 0 },
      observed: { role: 'Member', enrolled: true, balance: '10.00' },
      expect: /balance should be/,
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

console.log('Stripe/membership oracle self-test')
console.log('')

console.log('a healthy world:')
for (const { name, fn } of STRIPE_INVARIANTS) {
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
for (const { name, fn } of STRIPE_INVARIANTS) {
  const scenarios = BROKEN[name]
  if (!scenarios || scenarios.length === 0) {
    fail(
      `${name}/has-a-broken-fixture`,
      'no broken world; an invariant never seen to fire is indistinguishable from one that cannot',
    )
    continue
  }
  for (const s of scenarios) {
    const result = fn(s.model, s.observed)
    if (result === null) fail(`${name}: ${s.why}`, 'did not fire')
    else if (!s.expect.test(result))
      fail(`${name}: ${s.why}`, `fired with the wrong message -- expected ${s.expect}, got: ${result}`)
    else ok(`${name}: ${s.why}`)
  }
}

console.log('')
console.log('the self-test covers the invariants that exist:')
{
  const named = new Set(STRIPE_INVARIANTS.map((i) => i.name))
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
