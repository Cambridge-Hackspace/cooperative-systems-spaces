// Door access policy, exercised against a real stack (#101, folding #140).
//
// What this proves, and what nothing else did: that the door decision engine
// reaches a runtime answer. Until this stage existed, door policy had no
// behavioural oracle anywhere. `door_card_resolution.rs` is a *source* check,
// the door MQTT path is shell-only, and -- worse than an absent stage -- the
// door module itself was switched OFF in this suite's configuration, so
// `POST /api/doors/{id}/checkin` answered 403 on every run and the whole of
// `DoorService::evaluate` / `access_engine::may` was unreached. The `[door]`
// section in stack-config.toml exists because of this file; see the note there.
//
// The claims, in the order they are asserted:
//
//   1. a door with no rules denies -- doors are `Restricted`, so "no matching
//      rule" is a refusal rather than a default-open.
//   2. an allow rule grants.
//   3. a deny rule beats that allow. This is the runtime half of a safety claim
//      that previously had only a unit/vector half: `contracts/door_rules.json`
//      asserts deny-precedence in the pure engine, and nothing asserted it
//      end-to-end through the real handler, database and rule loader.
//   4. deleting the deny rule restores access -- revocation is not one-way.
//   5. a role rule is a tier gate, proven in BOTH directions: a `staff` rule
//      refuses a guest, a `guest` rule admits one. A role rule that matched
//      everybody would pass assertion 2 and be a hole.
//   6. a disabled door refuses regardless of its rules.
//   7. the check-in rate limit refuses the 6th attempt in the window. It counts
//      every attempt, granted or not, so a flood cannot be laundered through
//      successful check-ins.
//   8. every decision, granted and denied, is recorded as a door access event
//      with its reason -- the audit trail is part of the behaviour.
//
// What this does NOT prove: that a strike physically opens. Nothing here
// switches hardware, and these doors have no edge device bound, so a granted
// check-in is logged and published to nobody. It proves the decision, not the
// actuation. `admin_unlock` is deliberately not asserted here -- it bypasses the
// engine today and becomes rule-subject in the same epic, so its assertions live
// with that change rather than being written here and immediately flipped.
//
// Budget note: check-in is throttled to 5 attempts / 30s per (user, door), and
// every attempt counts. Each scenario therefore gets its own door and member
// rather than looping, and the counts below are deliberate.

import { GET, POST, PATCH, DELETE, account, adminAccount, assertEq, ok, record, main } from './lib.mjs'

const ENCODING = process.env.CSS_DB_ENCODING ?? 'UTF8'
const CAN_REGISTER_DEVICE = ENCODING === 'UTF8' || ENCODING === 'SQL_ASCII'

main(async () => {
  const admin = await adminAccount('doors_admin')
  const T = { token: admin.token }
  const tag = admin.username

  // --- two places, because every door connects two -------------------------
  // `is_special` roots with a free-form type, so this stage does not depend on
  // the deployment's configured place-type vocabulary or its ordering rules.
  const mkPlace = async (name) => {
    const res = await POST('/api/admin/places', {
      token: admin.token,
      body: { name: `${name}-${tag}`, place_type: 'Outside', is_special: true },
    })
    return { id: res.json?.data?.id, status: res.status, text: res.text }
  }
  const outside = await mkPlace('doors-outside')
  const inside = await mkPlace('doors-inside')
  ok('doors/places-created', !!outside.id && !!inside.id,
    `places -> ${outside.status} ${inside.status}: ${outside.text.slice(0, 160)}`)
  if (!outside.id || !inside.id) return

  const mkDoor = async (name) => {
    const res = await POST('/api/admin/doors', {
      token: admin.token,
      body: { name: `${name}-${tag}`, place_id_from: outside.id, place_id_to: inside.id },
    })
    return { id: res.json?.data?.id, status: res.status, text: res.text }
  }
  const addRule = (doorId, body) =>
    POST(`/api/admin/doors/${doorId}/rules`, { token: admin.token, body })
  const checkin = (doorId, token) => POST(`/api/doors/${doorId}/checkin`, { token })
  /** A check-in's verdict. Denials are 200 + `unlocked: false`, not an error. */
  const verdict = (res) => ({
    status: res.status,
    unlocked: res.json?.data?.unlocked,
    reason: res.json?.data?.reason,
  })

  // --- 1-4: rule precedence on one door, one member ------------------------
  const doorA = await mkDoor('doors-precedence')
  ok('doors/door-created', !!doorA.id, `POST /api/admin/doors -> ${doorA.status} ${doorA.text.slice(0, 200)}`)
  if (!doorA.id) return
  const memberA = await account('doors_member_a')

  // Anti-vacuity, and the reason this file can be trusted at all. A 403 here is
  // the door module being disabled, which would make every assertion below pass
  // on a server that never evaluated a rule.
  const first = verdict(await checkin(doorA.id, memberA.token))
  if (first.status === 403) {
    record('doors/module-is-enabled', 'fail',
      `POST /api/doors/{id}/checkin answered 403, which means [door] enabled is false in ` +
      `the stack config. Every assertion in this stage would then be judging a disabled ` +
      `module rather than door policy. Set [door] enabled = true.`)
    return
  }
  record('doors/module-is-enabled', 'ok')

  assertEq('doors/no-rule-denies', false, first.unlocked,
    'doors are Restricted: no matching rule must refuse, not default open')
  ok('doors/no-rule-denial-says-why', /no matching/i.test(String(first.reason ?? '')),
    `expected a "no matching access rule" reason, got ${JSON.stringify(first.reason)}`)

  const allowRule = await addRule(doorA.id, {
    kind: 'user',
    value: memberA.user.id,
    effect: 'allow',
  })
  assertEq('doors/allow-rule-created', 201, allowRule.status)
  const allowed = verdict(await checkin(doorA.id, memberA.token))
  assertEq('doors/allow-rule-grants', true, allowed.unlocked,
    `an allow rule naming this user must grant: ${JSON.stringify(allowed)}`)

  // The precedence claim. A deny on top of a standing allow must win.
  const denyRule = await addRule(doorA.id, {
    kind: 'user',
    value: memberA.user.id,
    effect: 'deny',
  })
  assertEq('doors/deny-rule-created', 201, denyRule.status)
  const denied = verdict(await checkin(doorA.id, memberA.token))
  assertEq('doors/deny-beats-allow', false, denied.unlocked,
    'an explicit deny must beat a standing allow -- this is the runtime half of a ' +
    'claim the vector file only made about the pure engine')
  ok('doors/deny-says-it-was-a-rule', /access rule/i.test(String(denied.reason ?? '')),
    `expected a "denied by access rule" reason, got ${JSON.stringify(denied.reason)}`)

  // And revocation is not one-way: removing the deny restores the allow.
  const denyId = denyRule.json?.data?.id
  const removed = await DELETE(`/api/admin/doors/${doorA.id}/rules/${denyId}`, T)
  assertEq('doors/deny-rule-removed', 200, removed.status)
  const restored = verdict(await checkin(doorA.id, memberA.token))
  assertEq('doors/removing-the-deny-restores-access', true, restored.unlocked,
    `deleting the deny rule must let the standing allow apply again: ${JSON.stringify(restored)}`)

  // --- 5-6: role rules are a tier gate, and a disabled door refuses --------
  const doorB = await mkDoor('doors-roles')
  const memberB = await account('doors_member_b')
  ok('doors/second-door-created', !!doorB.id, `-> ${doorB.status}`)
  if (doorB.id) {
    // A freshly registered account holds `guest` (level 1). A `staff` rule
    // (level 4) must therefore refuse it: role rules compare tiers, and a rule
    // that matched any authenticated user would be a hole this proves closed.
    const staffRule = await addRule(doorB.id, { kind: 'role', value: 'staff', effect: 'allow' })
    assertEq('doors/staff-role-rule-created', 201, staffRule.status)
    const belowTier = verdict(await checkin(doorB.id, memberB.token))
    assertEq('doors/role-rule-refuses-a-lower-tier', false, belowTier.unlocked,
      `a guest must not satisfy a staff rule: ${JSON.stringify(belowTier)}`)

    // The same mechanism admits when the tier is met.
    const guestRule = await addRule(doorB.id, { kind: 'role', value: 'guest', effect: 'allow' })
    assertEq('doors/guest-role-rule-created', 201, guestRule.status)
    const atTier = verdict(await checkin(doorB.id, memberB.token))
    assertEq('doors/role-rule-admits-at-tier', true, atTier.unlocked,
      `a guest must satisfy a guest rule: ${JSON.stringify(atTier)}`)

    // A disabled door refuses whatever its rules say. Availability outranks any
    // positive grant, so this is asserted with the grant still in place.
    const disable = await PATCH(`/api/admin/doors/${doorB.id}`, {
      token: admin.token,
      body: { enabled: false },
    })
    assertEq('doors/door-disabled', 200, disable.status)
    const whileDisabled = verdict(await checkin(doorB.id, memberB.token))
    assertEq('doors/disabled-door-refuses', false, whileDisabled.unlocked,
      `a disabled door must refuse even a granted member: ${JSON.stringify(whileDisabled)}`)
    ok('doors/disabled-reason-names-the-door',
      /disabled/i.test(String(whileDisabled.reason ?? '')),
      `expected a "door is disabled" reason, got ${JSON.stringify(whileDisabled.reason)}`)
  }

  // --- 7: the throttle refuses a flood ------------------------------------
  // Its own door and member, because the window is per (user, door) and the
  // scenarios above deliberately stay under it.
  const doorC = await mkDoor('doors-throttle')
  const memberC = await account('doors_member_c')
  if (doorC.id) {
    // 5 are permitted per 30s. Every attempt counts toward the window whether it
    // is granted or refused, so an attacker cannot launder a flood through
    // successful check-ins -- which is why this door has no allow rule and the
    // refusals below still consume the budget.
    let last = null
    for (let i = 0; i < 5; i += 1) {
      last = await checkin(doorC.id, memberC.token)
    }
    assertEq('doors/attempts-within-the-window-are-answered', 200, last.status)
    const flooded = await checkin(doorC.id, memberC.token)
    assertEq('doors/sixth-checkin-in-the-window-is-throttled', 429, flooded.status,
      `the 6th attempt in 30s must be refused: ${flooded.text.slice(0, 200)}`)
  }

  // --- 8: the decisions are on the record ---------------------------------
  const events = await GET(`/api/admin/doors/${doorA.id}/events`, T)
  assertEq('doors/events-listed', 200, events.status)
  const rows = events.json?.data?.events ?? events.json?.data ?? []
  ok('doors/every-decision-was-recorded', Array.isArray(rows) && rows.length >= 4,
    `doorA saw 4 check-ins; the event log has ${Array.isArray(rows) ? rows.length : 'none'}: ` +
    `${JSON.stringify(rows).slice(0, 300)}`)
  ok('doors/the-log-holds-both-outcomes',
    Array.isArray(rows) && rows.some((r) => r.granted === true) && rows.some((r) => r.granted === false),
    `a log that only records grants (or only refusals) cannot be audited: ` +
    `${JSON.stringify(rows.map?.((r) => r.granted)).slice(0, 200)}`)
  ok('doors/a-refusal-carries-its-reason',
    Array.isArray(rows) && rows.some((r) => r.granted === false && !!r.reason),
    'a denied event with no reason tells an operator nothing about why')

  // --- a door's coordinator is a device binding (#101) ----------------------
  // The last ad-hoc device association: a tool named its devices through a
  // binding row, a door named exactly one through `doors.edge_device_id`. Now
  // both go through `device_bindings`, so this proves the binding endpoint
  // accepts a DOOR as its resource and that the server drives the strike through
  // whatever is bound in role `edge`.
  //
  // Registering a device needs an eight-emoji invite code, so this half runs only
  // where the cluster can store one -- the same guard as toolmodules/bypass/lease.
  if (!CAN_REGISTER_DEVICE) {
    record('doors/coordinator-binding-not-run-on-this-cluster', 'skip',
      `binding a coordinator needs a registered device, a device needs an invite, and this ` +
      `cluster (${ENCODING}) cannot store an eight-emoji invite code.`)
    return
  }

  const doorD = await mkDoor('doors-coordinator')
  ok('doors/coordinator-door-created', !!doorD.id, `-> ${doorD.status}`)
  if (!doorD.id) return

  // A standing staff grant, so these assertions are about the COORDINATOR and not
  // about whether the remote unlock is authorized -- admin outranks staff, so this
  // holds both now and once admin_unlock becomes rule-subject.
  const staffGrant = await addRule(doorD.id, { kind: 'role', value: 'staff', effect: 'allow' })
  assertEq('doors/coordinator-door-granted-to-staff', 201, staffGrant.status)

  // With nothing bound there is no strike to drive, and the refusal says so.
  const unboundUnlock = await POST(`/api/admin/doors/${doorD.id}/unlock`, T)
  assertEq('doors/unlock-without-a-coordinator-is-refused', 400, unboundUnlock.status)

  const mkDevice = async (name, roles, mac) => {
    const inv = await POST('/api/admin/devices/invite', {
      token: admin.token,
      body: { expires_in_hours: 1 },
    })
    const reg = await POST('/api/devices/register', {
      body: {
        device_code: inv.json?.data?.device_code,
        name: `${name}-${tag}`,
        capabilities: { roles },
        mac_address: mac,
        software_version: '0.0.0-e2e',
        platform: 'linux',
      },
    })
    return { id: reg.json?.data?.device_id ?? reg.json?.device_id, status: reg.status, text: reg.text }
  }

  const coordinator = await mkDevice('door-edge', ['edge'], '02:00:00:00:85:01')
  ok('doors/edge-device-registered', !!coordinator.id,
    `register -> ${coordinator.status} ${coordinator.text.slice(0, 160)}`)

  // The unified model's point: the binding endpoint takes a door, not just a tool.
  const bound = await POST('/api/admin/device-bindings', {
    token: admin.token,
    body: {
      resource_id: doorD.id,
      device_id: coordinator.id,
      role: 'edge',
      name: 'door coordinator',
    },
  })
  assertEq('doors/a-door-can-be-bound-a-coordinator', 201, bound.status)

  // And the server resolves the strike through that binding: the same unlock that
  // had nothing to drive a moment ago is now accepted.
  const boundUnlock = await POST(`/api/admin/doors/${doorD.id}/unlock`, T)
  assertEq('doors/unlock-uses-the-bound-coordinator', 200, boundUnlock.status,
    `after binding an edge device the remote unlock must resolve it: ${boundUnlock.text.slice(0, 200)}`)

  // The capability check applies to a door exactly as it does to a tool: a device
  // that only declares `reader` cannot be a coordinator.
  const readerOnly = await mkDevice('door-reader', ['reader'], '02:00:00:00:85:02')
  const wrongCap = await POST('/api/admin/device-bindings', {
    token: admin.token,
    body: {
      resource_id: doorD.id,
      device_id: readerOnly.id,
      role: 'edge',
      name: 'not a coordinator',
    },
  })
  assertEq('doors/coordinator-must-declare-the-edge-role', 400, wrongCap.status)
})
