#!/usr/bin/env node
//
// The oracle before the tier.
//
// Tier 9's whole value rests on its invariants being right. A journey that runs
// a thousand actions past a broken invariant reports a thousand successes, and
// there is nothing in the output to distinguish that from a healthy system —
// which is worse than no tier at all, because it is a green light somebody will
// believe.
//
// So each invariant is fed **what a broken server would send**, and must fire.
// Every case here is modeled on a defect this project actually had, or on one
// the design predicted and the stack battery then confirmed:
//
//   * a 403 on the member profile-config read                    (5c2fa3c)
//   * a rule accepted and absent from the list                   (92afb4c)
//   * two devices from one single-use invite                     (register_device)
//   * version numbering of [1, 2, 2, 3]                          (the lost-update race)
//   * a duplicate roster entry after a role change
//
// This runs with **no stack, no database and no network**, so it works on the
// FreeBSD workstation where css-server cannot even be compiled. That is
// deliberate: the cheapest thing to run is the thing that decides whether the
// most expensive thing means anything.
//
//   node e2e/journeys/selftest.mjs
//
// Exit 0 if every invariant fires when it should and stays quiet when it
// should. Non-zero, with the case named, otherwise.

import { INVARIANTS } from './invariants.mjs'

// ---------------------------------------------------------------------------
// A world the invariants should be happy with
// ---------------------------------------------------------------------------
function healthyModel() {
  return {
    users: [
      { id: 'u1', username: 'ada', email: 'ada@e.invalid', role: 'Admin', exists: true, deactivated: false },
      { id: 'u2', username: 'grace', email: 'grace@e.invalid', role: 'Member', exists: true, deactivated: false },
      { id: 'u3', username: 'alan', email: 'alan@e.invalid', role: 'Newbie', exists: true, deactivated: true },
      { id: 'u4', username: 'gone', email: 'gone@e.invalid', role: 'Newbie', exists: false, deactivated: false },
    ],
    doorRules: [
      { id: 'r1', doorId: 'd1', kind: 'role', value: 'Member', accepted: true, deleted: false },
      { id: 'r2', doorId: 'd1', kind: 'card', value: 'A1', accepted: true, deleted: true },
      { id: 'r3', doorId: 'd1', kind: 'card', value: 'B2', accepted: false, deleted: false },
    ],
    devices: [
      { id: 'dev1', inviteCode: 'i1', registered: true },
      { id: 'dev2', inviteCode: 'i2', registered: true },
    ],
  }
}

function healthyObservations() {
  return {
    'roster-matches': [
      { id: 'u1', role: 'Admin', is_active: true },
      { id: 'u2', role: 'Member', is_active: true },
      { id: 'u3', role: 'Newbie', is_active: false },
    ],
    'roles-match': [
      { id: 'u1', role: 'Admin', is_active: true },
      { id: 'u2', role: 'Member', is_active: true },
      { id: 'u3', role: 'Newbie', is_active: false },
    ],
    'accepted-rules-are-present': [{ id: 'r1', doorId: 'd1' }],
    'versions-are-contiguous': [{ version: 1 }, { version: 2 }, { version: 3 }],
    'invites-are-single-use': [
      { id: 'dev1', inviteCode: 'i1' },
      { id: 'dev2', inviteCode: 'i2' },
    ],
    'deactivations-held': [
      { id: 'u1', role: 'Admin', is_active: true },
      { id: 'u2', role: 'Member', is_active: true },
      { id: 'u3', role: 'Newbie', is_active: false },
    ],
  }
}

// ---------------------------------------------------------------------------
// The broken worlds, one per invariant, each named after the defect it models
// ---------------------------------------------------------------------------
const BROKEN = {
  'roster-matches': [
    {
      why: 'a duplicate roster entry after a role change -- the update inserted instead',
      observed: [
        { id: 'u1', role: 'Admin', is_active: true },
        { id: 'u2', role: 'Member', is_active: true },
        { id: 'u2', role: 'Member', is_active: true },
        { id: 'u3', role: 'Newbie', is_active: false },
      ],
      expect: /duplicate ids/,
    },
    {
      why: 'a user the driver created and the roster does not list',
      observed: [
        { id: 'u1', role: 'Admin', is_active: true },
        { id: 'u3', role: 'Newbie', is_active: false },
      ],
      expect: /absent from the roster/,
    },
    {
      why: 'a deleted user the roster still lists',
      observed: [
        { id: 'u1', role: 'Admin', is_active: true },
        { id: 'u2', role: 'Member', is_active: true },
        { id: 'u3', role: 'Newbie', is_active: false },
        { id: 'u4', role: 'Newbie', is_active: true },
      ],
      expect: /never created/,
    },
  ],

  'roles-match': [
    {
      why: 'a role change that answered 200 and did not take -- the 5c2fa3c shape',
      observed: [
        { id: 'u1', role: 'Admin', is_active: true },
        { id: 'u2', role: 'Newbie', is_active: true },
        { id: 'u3', role: 'Newbie', is_active: false },
      ],
      expect: /grace: model says Member, server says Newbie/,
    },
  ],

  'accepted-rules-are-present': [
    {
      why: 'a door rule the server accepted and does not list -- 92afb4c exactly',
      observed: [],
      expect: /accepted these rules and does not list them/,
    },
  ],

  'versions-are-contiguous': [
    {
      why: 'the lost-update race: two writers both allocated version 2',
      observed: [{ version: 1 }, { version: 2 }, { version: 2 }, { version: 3 }],
      expect: /duplicate version numbers/,
    },
    {
      why: 'a version allocated and lost -- an update that silently did not happen',
      observed: [{ version: 1 }, { version: 2 }, { version: 4 }],
      expect: /gap between 2 and 4/,
    },
    {
      why: 'the boot bootstrap did not run at all',
      observed: [],
      expect: /no profile config versions at all/,
    },
    {
      why: 'numbering that does not start at 1',
      observed: [{ version: 2 }, { version: 3 }],
      expect: /starts at 2, not 1/,
    },
  ],

  'invites-are-single-use': [
    {
      why: 'two devices from one invite -- register_device has no transaction',
      observed: [
        { id: 'dev1', inviteCode: 'i1' },
        { id: 'dev1b', inviteCode: 'i1' },
        { id: 'dev2', inviteCode: 'i2' },
      ],
      expect: /invite i1 produced 2 devices/,
    },
    {
      why: 'a server that answers with no devices at all, which must not satisfy this',
      observed: [],
      expect: /2 device\(s\) were registered and the server lists 0/,
    },
  ],

  'deactivations-held': [
    {
      why: 'a deactivated member reported active again after later writes',
      observed: [
        { id: 'u1', role: 'Admin', is_active: true },
        { id: 'u2', role: 'Member', is_active: true },
        { id: 'u3', role: 'Newbie', is_active: true },
      ],
      expect: /deactivated and reported active: alan/,
    },
  ],
}

// ---------------------------------------------------------------------------
// The self-test
// ---------------------------------------------------------------------------
let failures = 0
let cases = 0

function ok(name, detail = '') {
  cases += 1
  console.log(`  ok    ${name}${detail ? ` -- ${detail}` : ''}`)
}

function fail(name, detail) {
  cases += 1
  failures += 1
  console.log(`  FAIL  ${name} -- ${detail}`)
}

console.log('Tier 9 oracle self-test')
console.log('')

// --- 1. every invariant is quiet on a healthy world -------------------------
// Run first. An invariant that fires on everything would satisfy every case
// below, and the suite would read as a complete success.
console.log('a healthy world:')
{
  const model = healthyModel()
  const observations = healthyObservations()
  for (const { name, fn } of INVARIANTS) {
    const observed = observations[name]
    if (observed === undefined) {
      fail(`${name}/has-a-healthy-fixture`, 'no healthy observation for this invariant')
      continue
    }
    const result = fn(model, observed)
    if (result === null) ok(`${name}/quiet-when-satisfied`)
    else fail(`${name}/quiet-when-satisfied`, `fired on a healthy world: ${result}`)
  }
}

// --- 2. every invariant fires on the world it exists for --------------------
console.log('')
console.log('broken worlds:')
{
  const model = healthyModel()
  for (const { name, fn } of INVARIANTS) {
    const scenarios = BROKEN[name]
    if (!scenarios || scenarios.length === 0) {
      fail(`${name}/has-a-broken-fixture`,
        'no broken world for this invariant. An invariant that has never been ' +
        'seen to fire is indistinguishable from one that cannot.')
      continue
    }
    for (const s of scenarios) {
      const result = fn(model, s.observed)
      if (result === null) {
        fail(`${name}: ${s.why}`, 'did not fire')
      } else if (!s.expect.test(result)) {
        fail(`${name}: ${s.why}`,
          `fired with the wrong message -- expected ${s.expect}, got: ${result}`)
      } else {
        ok(`${name}: ${s.why}`)
      }
    }
  }
}

// --- 3. the two lists have not drifted apart -------------------------------
console.log('')
console.log('the self-test covers the invariants that exist:')
{
  const named = new Set(INVARIANTS.map((i) => i.name))
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
  console.log('The invariants are not trustworthy, so the tier that uses them is not either.')
}
process.exit(failures > 0 ? 1 : 0)
