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

import { main, ok, assertEq, req, GET, POST, PUT, DELETE, account, adminAccount, RUN_TAG } from './lib.mjs'
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

  // (3) The RBAC write API (#65 Phase 3c) end to end: create a role, grant it a
  //     permission, assign it to the low-privilege account, and prove the
  //     assignment GRANTS access -- then that removing it revokes access. This
  //     is the one place the multi-role union is exercised through the real
  //     stack. Plus the guard rails: an inheritance cycle is refused, and a
  //     system role cannot be deleted.
  const memberRoute = endpoints.find((e) => e.guard === 'Member' && e.method === 'GET')
  ok(
    'roles/has-a-member-route',
    !!memberRoute,
    'no Member GET route in the corpus to probe assignment-granted access with',
  )

  const created = await POST('/api/admin/rbac/roles', {
    token: admin.token,
    body: { name: `e2e_rbac_${RUN_TAG}`, description: 'e2e write-API probe', level: 2 },
  })
  assertEq('roles/create-role', 200, created.status, `create role -> ${created.status}`)
  const roleId = created.json?.data?.id
  ok('roles/create-role-returned-id', typeof roleId === 'string' && roleId.length > 0, 'no role id returned')

  const granted = await PUT(`/api/admin/rbac/roles/${roleId}/permissions`, {
    token: admin.token,
    body: { permissions: ['member.access'] },
  })
  assertEq('roles/grant-member-access', 200, granted.status, `grant -> ${granted.status}`)

  // Unknown permission key is refused (a grant of a permission nothing checks).
  const badGrant = await PUT(`/api/admin/rbac/roles/${roleId}/permissions`, {
    token: admin.token,
    body: { permissions: ['not.a.real.permission'] },
  })
  assertEq('roles/grant-unknown-permission-rejected', 400, badGrant.status, `bad grant -> ${badGrant.status}`)

  if (memberRoute && roleId) {
    const fillM = (t) => t.split('{id}').join(BOGUS_ID)
    const before = await req(memberRoute.method, fillM(memberRoute.template), { token: low.token })
    assertEq('roles/member-route-denied-before-assign', 403, before.status, `newbie on member route -> ${before.status}`)

    const assigned = await POST(`/api/admin/users/${low.user.id}/roles`, {
      token: admin.token,
      body: { role_id: roleId },
    })
    assertEq('roles/assign-role', 200, assigned.status, `assign -> ${assigned.status}`)

    // Same token: enforcement re-reads user_roles each request, so the grant
    // takes effect without re-issuing the JWT.
    const after = await req(memberRoute.method, fillM(memberRoute.template), { token: low.token })
    ok(
      'roles/member-route-allowed-after-assign',
      after.status !== 403,
      `after assign, the newbie is still 403 on the member route (${after.status}) -- ` +
        `the multi-role union did not take effect`,
    )

    const unassigned = await DELETE(`/api/admin/users/${low.user.id}/roles/${roleId}`, { token: admin.token })
    assertEq('roles/unassign-role', 200, unassigned.status, `unassign -> ${unassigned.status}`)

    const afterRemove = await req(memberRoute.method, fillM(memberRoute.template), { token: low.token })
    assertEq('roles/member-route-denied-after-unassign', 403, afterRemove.status, `after unassign -> ${afterRemove.status}`)
  }

  // Cycle rejection: A inherits B is fine; B inherits A closes a cycle and must
  // be refused before anything is written.
  const roleB = await POST('/api/admin/rbac/roles', {
    token: admin.token,
    body: { name: `e2e_rbac_b_${RUN_TAG}`, description: 'e2e cycle probe', level: 1 },
  })
  assertEq('roles/create-role-b', 200, roleB.status, `create role B -> ${roleB.status}`)
  const roleBId = roleB.json?.data?.id
  if (roleId && roleBId) {
    const inhAB = await PUT(`/api/admin/rbac/roles/${roleId}/inheritance`, {
      token: admin.token,
      body: { inherits: [roleBId] },
    })
    assertEq('roles/set-inheritance', 200, inhAB.status, `A inherits B -> ${inhAB.status}`)
    const cycle = await PUT(`/api/admin/rbac/roles/${roleBId}/inheritance`, {
      token: admin.token,
      body: { inherits: [roleId] },
    })
    assertEq('roles/cycle-rejected', 400, cycle.status, `B inherits A (cycle) -> ${cycle.status}, expected 400`)
  }

  // A system role cannot be deleted.
  const snapshot = await GET('/api/admin/rbac', { token: admin.token })
  const systemRole = (snapshot.json?.data?.roles ?? []).find((r) => r.is_system)
  if (systemRole) {
    const delSys = await DELETE(`/api/admin/rbac/roles/${systemRole.id}`, { token: admin.token })
    assertEq('roles/system-role-delete-forbidden', 403, delSys.status, `deleting system role -> ${delSys.status}`)
  }

  // Clean up the custom roles.
  if (roleId) {
    const delA = await DELETE(`/api/admin/rbac/roles/${roleId}`, { token: admin.token })
    assertEq('roles/delete-role', 200, delA.status, `delete role A -> ${delA.status}`)
  }
  if (roleBId) {
    await DELETE(`/api/admin/rbac/roles/${roleBId}`, { token: admin.token })
  }
})
