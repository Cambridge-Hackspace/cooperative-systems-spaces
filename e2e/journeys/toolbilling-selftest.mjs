#!/usr/bin/env node
//
// The oracle before the tool-billing stack stage.
//
// The stage's value rests on `toolBillingHonored` being right: a driver that
// reads a broken balance past a broken invariant reports success. So the
// invariant is fed a healthy world (must stay quiet) and broken worlds modeled
// on real defects (must fire):
//
//   * a hold placed but not reflected in available (available too high)
//   * a charge that released the hold without debiting (balance too high)
//   * a settle that drove available negative -- the prepaid rule violated
//
// Runs with no stack, no database and no network.
//
//   node e2e/journeys/toolbilling-selftest.mjs

import { TOOLBILLING_INVARIANTS } from './toolbilling-invariants.mjs'

const HEALTHY = {
  'tool-billing-honored': {
    // Balance 10, a 1.50 hold open -> available 8.50, non-negative.
    model: { balance: 10, held: 1.5, available: 8.5, nonNegative: true },
    observed: { balance: '10.00', held: '1.50', available: '8.50' },
  },
}

const BROKEN = {
  'tool-billing-honored': [
    {
      why: 'a hold placed but not subtracted from available',
      model: { available: 8.5 },
      observed: { balance: '10.00', held: '1.50', available: '10.00' },
      expect: /available should be/,
    },
    {
      why: 'a charge that released the hold without debiting the ledger',
      model: { balance: 8.5 },
      observed: { balance: '10.00', held: '0.00', available: '10.00' },
      expect: /balance should be/,
    },
    {
      why: 'a settle that drove available negative -- prepaid rule violated',
      model: { nonNegative: true },
      observed: { balance: '-1.00', held: '0.00', available: '-1.00' },
      expect: /non-negative/,
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

console.log('Tool-billing oracle self-test')
console.log('')

console.log('a healthy world:')
for (const { name, fn } of TOOLBILLING_INVARIANTS) {
  const h = HEALTHY[name]
  if (!h) {
    fail(`${name}/has-a-healthy-fixture`, 'no healthy world')
    continue
  }
  const result = fn(h.model, h.observed)
  if (result === null) ok(`${name}/quiet-when-satisfied`)
  else fail(`${name}/quiet-when-satisfied`, `fired on a healthy world: ${result}`)
}

console.log('')
console.log('broken worlds:')
for (const { name, fn } of TOOLBILLING_INVARIANTS) {
  const scenarios = BROKEN[name]
  if (!scenarios || scenarios.length === 0) {
    fail(`${name}/has-a-broken-fixture`, 'no broken world')
    continue
  }
  for (const s of scenarios) {
    const result = fn(s.model, s.observed)
    if (result === null) fail(`${name}: ${s.why}`, 'did not fire')
    else if (!s.expect.test(result))
      fail(`${name}: ${s.why}`, `wrong message -- expected ${s.expect}, got: ${result}`)
    else ok(`${name}: ${s.why}`)
  }
}

console.log('')
console.log('the self-test covers the invariants that exist:')
{
  const named = new Set(TOOLBILLING_INVARIANTS.map((i) => i.name))
  const tested = new Set(Object.keys(BROKEN))
  const untested = [...named].filter((n) => !tested.has(n))
  if (untested.length === 0) ok('every invariant has at least one broken world')
  else fail('every invariant has at least one broken world', untested.join(', '))
}

console.log('')
console.log(`${cases} case(s), ${failures} failure(s)`)
process.exit(failures > 0 ? 1 : 0)
