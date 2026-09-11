// Tier: role gating (403). The complement to the contract tier's 401 matrix.
//
// `server/tests/contract_matrix.rs` proves every guarded route rejects a
// request with NO valid credential (401). It deliberately does not exercise the
// difference between roles -- it says so itself (`guarded_routes_are_not_\
// asserted_for_role_gating`) and points at a live-DB tier that did not exist.
// This is that tier, against the real stack: a fully authenticated but
// low-privilege account must be REFUSED (403) by every Admin/Staff route.
//
// Why it is safe to probe every verb, including the mutating ones: the role
// guard is a request-parts extractor that runs before the handler and before
// the JSON body, exactly as the 401 does in the contract tier. A low-privilege
// token is turned away at the door, so a bogus path id or an empty body never
// reaches a mutation.
//
// Two oracles, so a green run means the gate discriminates rather than blanket-
// denies:
//   * the low-privilege token gets 403 on every Admin/Staff route;
//   * an admin token does NOT get 403 on the read-only Admin GETs -- it passes
//     the same gate the newbie was stopped at. GETs only, so this half mutates
//     nothing either.

import { main, ok, assertEq, req, GET, account, adminAccount } from './lib.mjs'
import { readFileSync } from 'node:fs'

// The same nonexistent id the contract tier uses: a rejection at the guard
// happens before this is ever looked up.
const BOGUS_ID = '00000000-0000-4000-8000-000000000001'
const fill = (template) => template.split('{id}').join(BOGUS_ID)

main(async () => {
  const corpus = JSON.parse(
    readFileSync(new URL('../corpus/endpoints.json', import.meta.url), 'utf8'),
  )
  const endpoints = corpus.endpoints ?? corpus

  const roleGated = endpoints.filter((e) => e.guard === 'Admin' || e.guard === 'Staff')

  // Anti-vacuity: if the corpus lost its guard field or the filter matched
  // nothing, the loop below would pass over an empty set.
  ok(
    'roles/corpus-has-role-gated-routes',
    roleGated.length >= 50,
    `only ${roleGated.length} Admin/Staff routes in the corpus; expected 50+ ` +
      `(the guard field or the route table changed shape)`,
  )

  // A freshly registered account is the lowest role in the hierarchy -- below
  // both Staff and Admin. Assert that before trusting the denials below: if a
  // fresh account were somehow privileged, every 403 assertion would be
  // meaningless.
  const low = await account('role_probe')
  const role = low.user?.role
  ok(
    'roles/probe-is-low-privilege',
    !!role && role !== 'Admin' && role !== 'Staff',
    `the probe account resolved as role ${role}; a fresh registration must be ` +
      `below Staff for this tier to mean anything`,
  )

  // (1) Every Admin/Staff route must refuse the low-privilege token with 403 --
  //     not 401 (it IS authenticated), not 2xx (it got in), not 404/400 (the
  //     handler ran before the role was checked).
  let denied = 0
  for (const e of roleGated) {
    const res = await req(e.method, fill(e.template), { token: low.token })
    assertEq(
      `roles/deny ${e.method} ${e.template} (${e.guard})`,
      403,
      res.status,
      `a ${role} was not refused with 403 (got ${res.status})`,
    )
    if (res.status === 403) denied++
  }
  ok(
    'roles/all-role-gated-denied',
    denied === roleGated.length,
    `${denied}/${roleGated.length} role-gated routes refused the low-privilege token`,
  )

  // (2) Non-vacuity: prove the 403 is about the ROLE, not a route that denies
  //     everyone. An admin token must pass the gate the newbie was stopped at.
  //     Read-only Admin GETs only, so this probe mutates nothing; the admin may
  //     legitimately get 200/404/400, just never 403.
  const admin = await adminAccount('roles_admin')
  const adminGets = endpoints.filter((e) => e.guard === 'Admin' && e.method === 'GET')
  ok(
    'roles/has-admin-gets-for-control',
    adminGets.length >= 5,
    `only ${adminGets.length} Admin GET routes; too few to prove the gate ` +
      `discriminates by role`,
  )
  let adminThrough = 0
  for (const e of adminGets) {
    const res = await GET(fill(e.template), { token: admin.token })
    ok(
      `roles/admin-not-forbidden ${e.template}`,
      res.status !== 403,
      `an admin got 403 on ${e.template} (status ${res.status}) -- the gate ` +
        `denies even the role it is supposed to admit`,
    )
    if (res.status !== 403) adminThrough++
  }
  ok(
    'roles/admin-passes-the-gate',
    adminThrough === adminGets.length,
    `${adminThrough}/${adminGets.length} Admin GETs admitted the admin token`,
  )
})
