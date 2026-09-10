// Power-topology stage (#42): circuits -> outlets -> receptacles, and a tool's
// receptacle assignment, exercised against a real stack.
//
// What this proves: the CRUD round-trips; the circuit trunk self-reference
// rejects self-parenting and cycles (the places-tree guard, applied to
// circuits); an outlet must name a real room; a receptacle drives at most one
// tool (the partial-unique index, from BOTH sides -- a second claim is a 409
// and the first tool keeps it); assignment can be cleared; the numeric
// guards hold; and every mutation lands an audit row (two oracles: the row
// exists AND the audit records it).
//
// What this does NOT prove: nothing here energizes hardware -- that is #44.

import { GET, POST, PATCH, PUT, DELETE, adminAccount, account, assertEq, ok, main } from './lib.mjs'

main(async () => {
  const admin = await adminAccount('circuits_admin')
  const T = { token: admin.token }

  // --- a room to hang outlets on -------------------------------------------
  const placeRes = await POST('/api/admin/places', {
    token: admin.token,
    body: { name: `Power Room ${admin.username}`, place_type: 'Room' },
  })
  ok('circuits/room-created', placeRes.status === 201 || placeRes.status === 200,
    `POST /api/admin/places -> ${placeRes.status}`)
  const placeId = placeRes.json?.data?.id

  // --- circuit CRUD ---------------------------------------------------------
  const c1 = await POST('/api/admin/power/circuits', {
    token: admin.token,
    body: { breaker_label: 'A-12', voltage_rating: 120, amperage_limit: '20' },
  })
  assertEq('circuits/create', 201, c1.status)
  const circuitId = c1.json?.data?.id
  ok('circuits/create-returns-id', !!circuitId, `no data.id: ${c1.text.slice(0, 200)}`)

  const cGet = await GET(`/api/admin/power/circuits/${circuitId}`, T)
  assertEq('circuits/get', 200, cGet.status)
  assertEq('circuits/get-echoes-label', 'A-12', cGet.json?.data?.breaker_label)
  ok('circuits/get-has-outlets-array', Array.isArray(cGet.json?.data?.outlets),
    'detail should carry an outlets array')

  const cList = await GET('/api/admin/power/circuits', T)
  assertEq('circuits/list', 200, cList.status)
  ok('circuits/list-contains-created',
    Array.isArray(cList.json?.data) && cList.json.data.some((c) => c.id === circuitId),
    'the created circuit is missing from the list')

  const cPatch = await PATCH(`/api/admin/power/circuits/${circuitId}`, {
    token: admin.token,
    body: { amperage_limit: '30' },
  })
  assertEq('circuits/update', 200, cPatch.status)
  assertEq('circuits/update-applied', '30', String(cPatch.json?.data?.amperage_limit))

  // --- numeric guards -------------------------------------------------------
  const badAmp = await POST('/api/admin/power/circuits', {
    token: admin.token,
    body: { breaker_label: 'bad', voltage_rating: 120, amperage_limit: '0' },
  })
  assertEq('circuits/reject-nonpositive-amperage', 400, badAmp.status)
  const badVolt = await POST('/api/admin/power/circuits', {
    token: admin.token,
    body: { breaker_label: 'bad', voltage_rating: 0, amperage_limit: '20' },
  })
  assertEq('circuits/reject-nonpositive-voltage', 400, badVolt.status)
  const badLabel = await POST('/api/admin/power/circuits', {
    token: admin.token,
    body: { breaker_label: '  ', voltage_rating: 120, amperage_limit: '20' },
  })
  assertEq('circuits/reject-empty-label', 400, badLabel.status)

  // --- trunk hierarchy: self-parent + cycle rejected ------------------------
  const selfParent = await PATCH(`/api/admin/power/circuits/${circuitId}`, {
    token: admin.token,
    body: { parent_circuit_id: circuitId },
  })
  assertEq('circuits/reject-self-parent', 400, selfParent.status)

  // Build A <- B (B's trunk is A), then try to make A's trunk B: a cycle.
  const cB = await POST('/api/admin/power/circuits', {
    token: admin.token,
    body: { breaker_label: 'B-sub', voltage_rating: 120, amperage_limit: '15', parent_circuit_id: circuitId },
  })
  assertEq('circuits/create-sub', 201, cB.status)
  const subId = cB.json?.data?.id
  const cycle = await PATCH(`/api/admin/power/circuits/${circuitId}`, {
    token: admin.token,
    body: { parent_circuit_id: subId },
  })
  assertEq('circuits/reject-cycle', 400, cycle.status)

  // --- outlets: must name a real room --------------------------------------
  const badOutlet = await POST('/api/admin/power/outlets', {
    token: admin.token,
    body: {
      circuit_id: circuitId,
      place_id: '00000000-0000-4000-8000-000000000009',
      label: 'no-such-room',
    },
  })
  assertEq('outlets/reject-unknown-place', 400, badOutlet.status)

  const o1 = await POST('/api/admin/power/outlets', {
    token: admin.token,
    body: { circuit_id: circuitId, place_id: placeId, label: 'Bench 1', location: 'north wall' },
  })
  assertEq('outlets/create', 201, o1.status)
  const outletId = o1.json?.data?.id

  const oByCircuit = await GET(`/api/admin/power/outlets?circuit_id=${circuitId}`, T)
  assertEq('outlets/list-by-circuit', 200, oByCircuit.status)
  ok('outlets/list-by-circuit-contains',
    Array.isArray(oByCircuit.json?.data) && oByCircuit.json.data.some((o) => o.id === outletId),
    'the outlet is missing from its circuit listing')

  // --- receptacles ----------------------------------------------------------
  const r1 = await POST('/api/admin/power/receptacles', {
    token: admin.token,
    body: { outlet_id: outletId, label: 'Left' },
  })
  assertEq('receptacles/create', 201, r1.status)
  const recId = r1.json?.data?.id

  const oDetail = await GET(`/api/admin/power/outlets/${outletId}`, T)
  assertEq('outlets/detail', 200, oDetail.status)
  ok('outlets/detail-has-receptacle',
    Array.isArray(oDetail.json?.data?.receptacles) &&
      oDetail.json.data.receptacles.some((r) => r.id === recId),
    'outlet detail should list its receptacle')

  // --- tool <-> receptacle: uniqueness from both sides ---------------------
  const toolA = await POST('/api/tools', { token: admin.token, body: { name: `PowerToolA ${admin.username}`, category: 'safety' } })
  const toolB = await POST('/api/tools', { token: admin.token, body: { name: `PowerToolB ${admin.username}`, category: 'safety' } })
  const toolAId = toolA.json?.data?.id
  const toolBId = toolB.json?.data?.id
  ok('tools/created-two', !!toolAId && !!toolBId, 'need two tools for the uniqueness check')

  const assignA = await PUT(`/api/admin/power/tools/${toolAId}/receptacle`, {
    token: admin.token,
    body: { receptacle_id: recId },
  })
  assertEq('assign/first-tool-ok', 200, assignA.status)

  // Oracle A: the second tool claiming the same receptacle is refused (409).
  const assignB = await PUT(`/api/admin/power/tools/${toolBId}/receptacle`, {
    token: admin.token,
    body: { receptacle_id: recId },
  })
  assertEq('assign/second-tool-conflict', 409, assignB.status)

  // Oracle B: and the first tool still holds it -- the refusal did not quietly
  // move the receptacle. (Assert the precondition, then the outcome.)
  const toolAAfter = await GET(`/api/tools/${toolAId}`, T)
  assertEq('assign/first-tool-still-holds', recId, toolAAfter.json?.data?.receptacle_id)
  const toolBAfter = await GET(`/api/tools/${toolBId}`, T)
  assertEq('assign/second-tool-unset', null, toolBAfter.json?.data?.receptacle_id ?? null)

  // Clearing (unplug) works and frees the receptacle.
  const clearA = await PUT(`/api/admin/power/tools/${toolAId}/receptacle`, {
    token: admin.token,
    body: { receptacle_id: null },
  })
  assertEq('assign/clear-ok', 200, clearA.status)
  const toolACleared = await GET(`/api/tools/${toolAId}`, T)
  assertEq('assign/cleared-is-null', null, toolACleared.json?.data?.receptacle_id ?? null)
  // Now the second tool can take it.
  const assignBNow = await PUT(`/api/admin/power/tools/${toolBId}/receptacle`, {
    token: admin.token,
    body: { receptacle_id: recId },
  })
  assertEq('assign/freed-receptacle-reusable', 200, assignBNow.status)

  // --- authorization: a member cannot manage the topology ------------------
  const member = await account('circuits_member')
  const memberCreate = await POST('/api/admin/power/circuits', {
    token: member.token,
    body: { breaker_label: 'nope', voltage_rating: 120, amperage_limit: '20' },
  })
  assertEq('authz/member-cannot-create-circuit', 403, memberCreate.status)

  // --- audit: two oracles -- the row exists AND the mutation is recorded ----
  const audit = await GET('/api/admin/audit-logs?limit=200', T)
  const events = Array.isArray(audit.json?.data)
    ? audit.json.data
    : audit.json?.data?.logs ?? audit.json?.data?.items ?? []
  const types = new Set(events.map((e) => e.event_type))
  ok('audit/circuit-created-recorded', types.has('power_circuit_created'),
    'no power_circuit_created audit event after creating a circuit')
  ok('audit/outlet-created-recorded', types.has('power_outlet_created'),
    'no power_outlet_created audit event after creating an outlet')
  ok('audit/receptacle-created-recorded', types.has('power_receptacle_created'),
    'no power_receptacle_created audit event after creating a receptacle')

  // --- deletion guards ------------------------------------------------------
  // A circuit with outlets can't be deleted (RESTRICT -> 409).
  const delBusy = await DELETE(`/api/admin/power/circuits/${circuitId}`, T)
  assertEq('circuits/delete-restricted-while-outlets', 409, delBusy.status)
  // Deleting the outlet cascades its receptacle (and SET NULL frees the tool).
  const delOutlet = await DELETE(`/api/admin/power/outlets/${outletId}`, T)
  assertEq('outlets/delete', 200, delOutlet.status)
  const recGone = await GET(`/api/admin/power/receptacles/${recId}`, T)
  assertEq('receptacles/cascade-deleted-with-outlet', 404, recGone.status)
  const toolBFreed = await GET(`/api/tools/${toolBId}`, T)
  assertEq('assign/set-null-on-receptacle-delete', null, toolBFreed.json?.data?.receptacle_id ?? null)
  // Now the (childless) sub-circuit and the circuit delete cleanly.
  assertEq('circuits/delete-sub', 200, (await DELETE(`/api/admin/power/circuits/${subId}`, T)).status)
  assertEq('circuits/delete', 200, (await DELETE(`/api/admin/power/circuits/${circuitId}`, T)).status)
  assertEq('circuits/get-after-delete', 404, (await GET(`/api/admin/power/circuits/${circuitId}`, T)).status)
})
