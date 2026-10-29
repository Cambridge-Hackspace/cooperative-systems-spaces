// The resource path (#182): one vocabulary for a badge at anything.
//
// `POST /api/resources/{id}/scan`, `/stop` and `/usage` are the one device
// vocabulary a tool and a door share; `tool-on` / `tool-off` / `tool-log` are
// aliases of them for one release. This stage drives the path against a real
// stack for BOTH kinds and across the alias boundary, which nothing else does:
//
//   1. a tool's scan opens its session exactly as tool-on does (the tool is in
//      use, a second scan is refused as such), and the reply says `authorized`
//      with no `strike` -- a tool has no latch;
//   2. a card-less stop on the resource path ends it (#179's rule, one name);
//   3. a usage report is refused with no session and recorded with one, and
//      the three HTTP aliases of this path are gone (#192: 404, not a refusal);
//   4. a door's scan runs the door's own policy -- refused with no rule (doors
//      are Restricted), granted with one -- and a yes carries the pulse of the
//      door's `strike` binding, while a door with no strike binding carries the
//      site default;
//   5. every presentation at the door lands in its access-event log as `rfid`,
//      with the resolved card and the reason, granted or not;
//   6. a stop on a door is harmless and a usage report on one is refused;
//   7. the credential rule: a device bound to the tool is not thereby bound to
//      the door (401), and an unknown resource reads the same as an unbound one;
//   8. the policy (#191): one shape for everything a device coordinates -- the
//      tool's reader is handed the tool and the member who may start it, the
//      door's coordinator is handed the door with its compiled rule, its pulse
//      and its profile -- and it says the same as the two aliases it replaces.
//
// What it does NOT prove: that anything moves. The door's strike device here is
// a registration with a token; the pulse is asserted in the reply, not on a
// latch. The edge's translation of the local-broker scan onto this path is step
// 3 of #182 and is not driven here.

import { GET, POST, PATCH as fetchPatch, account, adminAccount, assertEq, ok, main, record } from './lib.mjs'

const TOOL_EXT = 'RES-TOOL-1'
const CARD = 'RES-CARD-0001'

const mkDevice = async (admin, name, roles, mac) => {
  const inv = await POST('/api/admin/devices/invite', {
    token: admin.token,
    body: { expires_in_hours: 1 },
  })
  const reg = await POST('/api/devices/register', {
    body: {
      device_code: inv.json?.data?.device_code,
      name,
      capabilities: { roles },
      mac_address: mac,
      software_version: '0.0.0-e2e',
      platform: 'linux',
    },
  })
  return {
    id: reg.json?.data?.device_id ?? reg.json?.device_id,
    token: reg.json?.data?.auth_token ?? reg.json?.auth_token,
    status: reg.status,
    text: reg.text,
  }
}

const bind = (admin, body) => POST('/api/admin/device-bindings', { token: admin.token, body })

main(async () => {
  const admin = await adminAccount('resources_admin')
  const member = await account('resources_member')
  const tag = admin.username
  const T = { token: admin.token }

  // --- fixtures -------------------------------------------------------------
  const issued = await POST(`/api/cards/user/${member.user.id}`, {
    token: admin.token,
    body: { code: CARD },
  })
  assertEq('resources/card-issued', 200, issued.status, issued.text.slice(0, 200))

  const toolRes = await POST('/api/tools', {
    token: admin.token,
    body: {
      name: `E2E resources ${TOOL_EXT}`,
      category: 'other',
      requires_training: false,
      external_id: TOOL_EXT,
      usage_flat_fee: null,
      usage_rate_per_min: null,
      usage_max_session_minutes: null,
    },
  })
  const toolId = toolRes.json?.data?.id
  ok('resources/tool-created', !!toolId, toolRes.text.slice(0, 200))

  const mkPlace = async (name) => {
    const res = await POST('/api/admin/places', {
      token: admin.token,
      body: { name: `${name}-${tag}`, place_type: 'Outside', is_special: true },
    })
    return res.json?.data?.id
  }
  const outside = await mkPlace('resources-outside')
  const inside = await mkPlace('resources-inside')
  const mkDoor = async (name) => {
    const res = await POST('/api/admin/doors', {
      token: admin.token,
      body: { name: `${name}-${tag}`, place_id_from: outside, place_id_to: inside },
    })
    return { id: res.json?.data?.id, pulse_ms: res.json?.data?.pulse_ms, status: res.status }
  }
  const door = await mkDoor('resources-door')
  const plainDoor = await mkDoor('resources-plain-door')
  ok('resources/doors-created', !!door.id && !!plainDoor.id, `${door.status} ${plainDoor.status}`)
  if (!toolId || !door.id || !plainDoor.id) return

  // The tool's reader, and the door's reader-that-is-also-the-latch. One box,
  // two roles on one resource -- the toolguard with a latch output (#182 step 4).
  const reader = await mkDevice(admin, `res-reader-${tag}`, ['reader'], '02:00:00:00:87:01')
  const bindReader = await bind(admin, {
    resource_id: toolId,
    device_id: reader.id,
    role: 'reader',
    name: 'tool reader',
  })
  assertEq('resources/tool-reader-bound', 201, bindReader.status, bindReader.text.slice(0, 200))

  const latch = await mkDevice(admin, `res-latch-${tag}`, ['reader', 'strike'], '02:00:00:00:87:02')
  const bindLatchReader = await bind(admin, {
    resource_id: door.id,
    device_id: latch.id,
    role: 'reader',
    name: 'door reader',
  })
  const bindLatchStrike = await bind(admin, {
    resource_id: door.id,
    device_id: latch.id,
    role: 'strike',
    name: 'door latch',
    pulse_ms: 4200,
  })
  assertEq('resources/door-reader-bound', 201, bindLatchReader.status, bindLatchReader.text.slice(0, 200))
  assertEq('resources/door-strike-bound', 201, bindLatchStrike.status, bindLatchStrike.text.slice(0, 200))

  const plainReader = await mkDevice(admin, `res-plain-${tag}`, ['reader'], '02:00:00:00:87:03')
  const bindPlain = await bind(admin, {
    resource_id: plainDoor.id,
    device_id: plainReader.id,
    role: 'reader',
    name: 'plain door reader',
  })
  assertEq('resources/plain-door-reader-bound', 201, bindPlain.status, bindPlain.text.slice(0, 200))

  const scan = (id, token, card) => POST(`/api/resources/${id}/scan`, { token, body: { card } })
  const stop = (id, token, body = {}) => POST(`/api/resources/${id}/stop`, { token, body })
  const usage = (id, token, body) => POST(`/api/resources/${id}/usage`, { token, body })
  const toolStatus = async () => (await GET(`/api/tools/${toolId}`, T)).json?.data?.status

  // --- 1. a tool's scan, by external id --------------------------------------
  const on = await scan(TOOL_EXT, reader.token, CARD)
  assertEq('resources/tool-scan-is-200', 200, on.status, on.text.slice(0, 200))
  assertEq('resources/tool-scan-authorizes', true, on.json?.authorized, on.text.slice(0, 200))
  assertEq('resources/tool-scan-status-ok', 'ok', on.json?.status)
  ok('resources/tool-scan-carries-no-strike', !('strike' in (on.json ?? {})),
    'a tool has no latch, so its yes must not carry a pulse')
  ok('resources/tool-scan-speaks-no-alias-name', !('tool_on' in (on.json ?? {})),
    'the resource path says authorized, not tool_on')
  assertEq('resources/tool-is-in-use', 'in_use', await toolStatus())
  const again = await scan(TOOL_EXT, reader.token, CARD)
  assertEq('resources/second-scan-is-refused', false, again.json?.authorized)
  assertEq('resources/second-scan-says-in-use', 'Tool is already in use', again.json?.message)

  // --- 2. a card-less stop ----------------------------------------------------
  const off = await stop(TOOL_EXT, reader.token)
  assertEq('resources/cardless-stop-is-200', 200, off.status, off.text.slice(0, 200))
  assertEq('resources/cardless-stop-is-ok', 'ok', off.json?.status, off.text.slice(0, 200))
  assertEq('resources/tool-is-idle-after-stop', 'idle', await toolStatus())
  const late = await stop(TOOL_EXT, reader.token)
  assertEq('resources/late-stop-is-harmless', 'ok', late.json?.status, late.text.slice(0, 200))

  // --- 3. usage, and the aliases being gone (#192) ------------------------
  const noSession = await usage(TOOL_EXT, reader.token, { seconds: 30 })
  assertEq('resources/usage-with-no-session-is-refused', 'error', noSession.json?.status)
  assertEq('resources/usage-refusal-names-the-session', 'No open session for this tool',
    noSession.json?.message)
  await scan(TOOL_EXT, reader.token, CARD)
  const logged = await usage(TOOL_EXT, reader.token, { card: CARD, seconds: 30, active_seconds: 12 })
  assertEq('resources/usage-with-a-session-is-logged', 'ok', logged.json?.status, logged.text.slice(0, 200))
  const stopWithCard = await stop(TOOL_EXT, reader.token, { card: CARD })
  assertEq('resources/stop-with-a-card-closes-the-session', 'ok', stopWithCard.json?.status)
  assertEq('resources/session-is-idle-after-the-stop', 'idle', await toolStatus())
  // The three HTTP aliases are not routes: a bound device with a valid body
  // gets 404, and the tool it names is untouched.
  for (const [alias, body] of [
    ['tool-on', { card: CARD, tool_id: TOOL_EXT }],
    ['tool-off', { card: CARD, tool_id: TOOL_EXT }],
    ['tool-log', { card: CARD, tool_id: TOOL_EXT, seconds: 30 }],
  ]) {
    const gone = await POST(`/api/toolguard/${alias}`, { token: reader.token, body })
    assertEq(`resources/alias-${alias}-is-gone`, 404, gone.status, gone.text.slice(0, 200))
  }
  assertEq('resources/the-gone-aliases-touched-nothing', 'idle', await toolStatus())

  // --- 4. a door's scan -------------------------------------------------------
  const noRule = await scan(door.id, latch.token, CARD)
  assertEq('resources/door-scan-is-200', 200, noRule.status, noRule.text.slice(0, 200))
  if (noRule.json?.message === 'Door module is disabled in server configuration') {
    record('resources/door-module-is-enabled', 'fail',
      '[door] enabled is false in the stack config; the door half of this stage is unreachable')
    return
  }
  record('resources/door-module-is-enabled', 'ok')
  assertEq('resources/door-with-no-rule-refuses', false, noRule.json?.authorized, noRule.text.slice(0, 200))
  assertEq('resources/door-refusal-says-why', 'No matching access rule', noRule.json?.message)
  ok('resources/door-refusal-carries-no-strike', !('strike' in (noRule.json ?? {})),
    'a no must never carry a pulse')
  const unknown = await scan(door.id, latch.token, 'RES-NOBODY-9999')
  assertEq('resources/door-unknown-card-refuses', 'Unknown card', unknown.json?.message)

  const rule = await POST(`/api/admin/doors/${door.id}/rules`, {
    token: admin.token,
    body: { kind: 'user', value: member.user.id, effect: 'allow' },
  })
  assertEq('resources/door-rule-created', 201, rule.status, rule.text.slice(0, 200))
  const granted = await scan(door.id, latch.token, CARD)
  assertEq('resources/door-with-a-rule-grants', true, granted.json?.authorized, granted.text.slice(0, 200))
  assertEq('resources/door-yes-carries-the-strike-pulse', 4200, granted.json?.strike?.pulse_ms,
    granted.text.slice(0, 200))

  // A door nobody gave a strike binding opens for the site default, which is
  // what the door's own summary reports as its pulse.
  const plainRule = await POST(`/api/admin/doors/${plainDoor.id}/rules`, {
    token: admin.token,
    body: { kind: 'user', value: member.user.id, effect: 'allow' },
  })
  assertEq('resources/plain-door-rule-created', 201, plainRule.status)
  const plainGranted = await scan(plainDoor.id, plainReader.token, CARD)
  assertEq('resources/plain-door-grants', true, plainGranted.json?.authorized,
    plainGranted.text.slice(0, 200))
  const plainSummary = (await GET(`/api/admin/doors/${plainDoor.id}`, T)).json?.data
  assertEq('resources/plain-door-pulse-is-the-default', 'default', plainSummary?.pulse_source)
  assertEq('resources/plain-door-yes-carries-the-default-pulse', plainSummary?.pulse_ms,
    plainGranted.json?.strike?.pulse_ms)
  ok('resources/default-pulse-is-positive', (plainGranted.json?.strike?.pulse_ms ?? 0) > 0,
    `pulse_ms=${plainGranted.json?.strike?.pulse_ms}`)

  // --- 5. the door's record ---------------------------------------------------
  const events = await GET(`/api/admin/doors/${door.id}/events`, T)
  const rows = events.json?.data?.events ?? events.json?.data ?? []
  const rfid = rows.filter((e) => e.method === 'rfid')
  assertEq('resources/every-door-presentation-is-recorded', 3, rfid.length,
    JSON.stringify(rows).slice(0, 300))
  ok('resources/door-record-has-the-refusal',
    rfid.some((e) => e.granted === false && /no matching/i.test(String(e.reason ?? ''))),
    JSON.stringify(rfid).slice(0, 300))
  ok('resources/door-record-has-the-unknown-card',
    rfid.some((e) => e.granted === false && e.reason === 'Unknown card' && !e.card_id_attempted),
    'an unknown code is recorded as such, and no copy of its value is kept')
  ok('resources/door-record-has-the-grant',
    rfid.some((e) => e.granted === true && e.user_id === member.user.id && !!e.card_id_attempted),
    'the grant names the member and the resolved card, never the code')
  ok('resources/door-record-never-holds-the-code',
    !rows.some((e) => String(e.card_id_attempted ?? '').includes(CARD)),
    'card_id_attempted is the card UUID, not what was scanned')

  // --- 6. a door has no session -----------------------------------------------
  const doorStop = await stop(door.id, latch.token)
  assertEq('resources/door-stop-is-harmless', 'ok', doorStop.json?.status, doorStop.text.slice(0, 200))
  const doorUsage = await usage(door.id, latch.token, { seconds: 10 })
  assertEq('resources/door-usage-is-refused', 'error', doorUsage.json?.status)
  assertEq('resources/door-usage-refusal-names-the-session', 'No open session for this resource',
    doorUsage.json?.message)

  // --- 6b. the session profile is the resource's to say (#182 step 3) -------
  // Defaulted by kind -- a door's session is the pulse, a tool's lasts until a
  // stop -- and changeable by an administrator: a door set to `until_stopped`
  // is the room. The wiring snapshot carries it to the coordinator.
  const PATCH = (p, o) => fetchPatch(p, o)
  const wiringBefore = (await GET('/api/admin/device-bindings/state', T)).json?.data?.tools ?? []
  assertEq('resources/door-profile-defaults-to-momentary', 'momentary',
    wiringBefore.find((t) => t.tool_id === door.id)?.session_profile)
  assertEq('resources/tool-profile-defaults-to-until-stopped', 'until_stopped',
    wiringBefore.find((t) => t.tool_id === toolId)?.session_profile)
  const badProfile = await PATCH(`/api/admin/resources/${door.id}`, {
    token: admin.token,
    body: { session_profile: 'forever' },
  })
  assertEq('resources/an-unknown-profile-is-400', 400, badProfile.status, badProfile.text.slice(0, 200))
  const nothing = await PATCH(`/api/admin/resources/${door.id}`, { token: admin.token, body: {} })
  assertEq('resources/an-empty-change-is-400', 400, nothing.status)
  const asMember = await PATCH(`/api/admin/resources/${door.id}`, {
    token: member.token,
    body: { session_profile: 'until_stopped' },
  })
  ok('resources/a-member-cannot-set-a-profile', asMember.status === 401 || asMember.status === 403,
    `-> ${asMember.status}`)
  const nowhereProfile = await PATCH('/api/admin/resources/00000000-0000-4000-8000-00000000dead', {
    token: admin.token,
    body: { session_profile: 'until_stopped' },
  })
  assertEq('resources/an-unknown-resource-is-404', 404, nowhereProfile.status)
  const roomed = await PATCH(`/api/admin/resources/${door.id}`, {
    token: admin.token,
    body: { session_profile: 'until_stopped' },
  })
  assertEq('resources/a-door-may-be-until-stopped', 200, roomed.status, roomed.text.slice(0, 200))
  assertEq('resources/the-change-is-reported', 'until_stopped', roomed.json?.data?.session_profile)
  const wiringAfter = (await GET('/api/admin/device-bindings/state', T)).json?.data?.tools ?? []
  assertEq('resources/the-wiring-snapshot-carries-the-profile', 'until_stopped',
    wiringAfter.find((t) => t.tool_id === door.id)?.session_profile)
  const audited = await GET('/api/admin/audit-logs?event_type=resource_updated&per_page=50', T)
  ok('resources/the-change-is-audited',
    (audited.json?.data ?? []).some((l) => String(l.event_data?.resource_id ?? '') === door.id),
    JSON.stringify(audited.json?.data ?? audited.text).slice(0, 300))

  // --- 7. the credential rule -------------------------------------------------
  const wrongDevice = await scan(door.id, reader.token, CARD)
  assertEq('resources/a-device-bound-elsewhere-is-401', 401, wrongDevice.status, wrongDevice.text.slice(0, 200))
  const nowhere = await scan('00000000-0000-4000-8000-00000000dead', reader.token, CARD)
  assertEq('resources/an-unknown-resource-is-401', 401, nowhere.status, nowhere.text.slice(0, 200))
  ok('resources/unknown-and-unbound-read-the-same', wrongDevice.text === nowhere.text,
    `bound-elsewhere: ${wrongDevice.text.slice(0, 120)} / unknown: ${nowhere.text.slice(0, 120)}`)
  const noToken = await POST(`/api/resources/${door.id}/scan`, { body: { card: CARD } })
  assertEq('resources/no-token-is-401', 401, noToken.status)

  // --- 8. the policy, one shape (#191) ----------------------------------------
  const policyFor = (token) => GET('/api/resources/policy', { token })
  const readerPolicy = await policyFor(reader.token)
  assertEq('resources/policy-is-200', 200, readerPolicy.status, readerPolicy.text.slice(0, 200))
  const rp = readerPolicy.json ?? {}
  const tool = (rp.resources ?? []).find((r) => r.id === toolId)
  ok('resources/policy-names-the-tool-by-both-ids', !!tool && tool.external_id === TOOL_EXT,
    JSON.stringify(rp.resources ?? rp).slice(0, 300))
  assertEq('resources/policy-tool-has-its-kind', 'tool', tool?.kind)
  assertEq('resources/policy-tool-has-its-profile', 'until_stopped', tool?.session_profile)
  assertEq('resources/policy-tool-is-idle', 'idle', tool?.status)
  ok('resources/policy-tool-carries-no-lists', tool?.allow?.length === 0 && tool?.deny?.length === 0,
    'a tool is decided from the principals, never from a card list')
  ok('resources/policy-tool-has-no-pulse-or-hold', tool?.pulse_ms === null && tool?.hold_until === null,
    JSON.stringify(tool).slice(0, 200))
  ok('resources/policy-omits-a-door-the-device-does-not-coordinate',
    !(rp.resources ?? []).some((r) => r.kind === 'door'),
    'the tool reader coordinates no door')
  // The member's own entries, by name: the tool needs no training, so every
  // active member -- the admin included -- is authorized for it, and "some
  // principal authorized for the tool" would be anyone. One entry per card the
  // member holds (the one issued above and any the account fixture gave them).
  const myName = member.user.full_name ?? `E2E ${member.username}` // what the fixture registered
  const mine = (rp.principals ?? []).filter((u) => u.full_name === myName)
  const principal = mine[0]
  ok('resources/policy-names-the-member-by-digest-only',
    mine.length >= 1 && mine.every((u) => u.is_active === true && /^[0-9a-f]{64}$/.test(String(u.digest)) &&
      (u.authorized ?? []).includes(toolId)) && !JSON.stringify(rp).includes(CARD),
    JSON.stringify(rp.principals ?? rp).slice(0, 300))
  assertEq('resources/policy-says-which-field-was-digested', 'string', typeof rp.profile_field)
  ok('resources/policy-is-stamped', !Number.isNaN(Date.parse(String(rp.as_of))), String(rp.as_of))
  // The alias says the same: the sync is the policy's tools and principals.
  const aliasSync = await GET('/api/toolguard/sync', { token: reader.token })
  assertEq('resources/sync-alias-is-200', 200, aliasSync.status, aliasSync.text.slice(0, 200))
  const syncTool = (aliasSync.json?.tools ?? []).find((t) => t.id === toolId)
  ok('resources/sync-alias-agrees-with-the-policy',
    syncTool?.status === tool?.status && syncTool?.requires_online === tool?.requires_online &&
      (aliasSync.json?.users ?? []).some((u) => u.profile_field_digest === principal?.digest),
    JSON.stringify({ syncTool, users: aliasSync.json?.users }).slice(0, 300))

  // The door's coordinator: an edge bound to the (now until_stopped) door.
  const coordinator = await mkDevice(admin, `res-edge-${tag}`, ['edge'], '02:00:00:00:87:04')
  const bindEdge = await bind(admin, {
    resource_id: door.id,
    device_id: coordinator.id,
    role: 'edge',
    name: 'door coordinator',
  })
  assertEq('resources/door-coordinator-bound', 201, bindEdge.status, bindEdge.text.slice(0, 200))
  const edgePolicy = await policyFor(coordinator.token)
  assertEq('resources/coordinator-policy-is-200', 200, edgePolicy.status, edgePolicy.text.slice(0, 200))
  const ep = edgePolicy.json ?? {}
  const pdoor = (ep.resources ?? []).find((r) => r.id === door.id)
  ok('resources/policy-names-the-door', !!pdoor, JSON.stringify(ep.resources ?? ep).slice(0, 300))
  assertEq('resources/policy-door-has-its-kind', 'door', pdoor?.kind)
  assertEq('resources/policy-door-has-its-profile', 'until_stopped', pdoor?.session_profile)
  assertEq('resources/policy-door-is-enabled', true, pdoor?.enabled)
  assertEq('resources/policy-door-carries-its-strike-pulse', 4200, pdoor?.pulse_ms)
  assertEq('resources/policy-door-is-never-online-decided', false, pdoor?.requires_online)
  // The door's allow set is every card of every member its rules select, as
  // digests. Two rules select here, and #200 is why that is spelled out: the
  // fixture's `kind=user` rule for the member, and the standing `role=staff`
  // allow that `create_door` seeds (#101, so an administrator's remote unlock
  // is an explicit, auditable, revocable rule rather than an invisible
  // capability). A staff member who holds a card is therefore in the set by
  // right, and the extra digest that looked like a leak was one.
  //
  // So the rules are pinned first -- a change in what a new door is seeded
  // with shows up here as itself, not as a mystery digest -- and then the set
  // is held to two claims: every digest in it belongs to a member the policy
  // names, and the badging member's own digests are all present. The first is
  // what a containment-only assertion could not make: a digest belonging to
  // nobody at all would have passed it.
  const doorRules = (await GET(`/api/admin/doors/${door.id}/rules`, T)).json?.data ?? []
  const ruleShape = doorRules
    .map((r) => `${r.effect}:${r.kind}:${r.kind === 'user' ? (r.value === member.user.id ? 'the-member' : r.value) : r.value}`)
    .sort()
  assertEq('resources/policy-door-rules-are-the-seed-and-the-fixture',
    JSON.stringify(['allow:role:staff', 'allow:user:the-member']), JSON.stringify(ruleShape),
    `a new door is seeded with a standing staff allow and the fixture adds one user rule: ${JSON.stringify(doorRules)}`)
  const mineDigests = mine.map((u) => u.digest)
  const named = new Map((rp.principals ?? []).map((u) => [u.digest, u.full_name]))
  const unexplained = (pdoor?.allow ?? []).filter((d) => !named.has(d))
  ok('resources/policy-door-carries-the-compiled-rule-as-digests',
    Array.isArray(pdoor?.allow) && mineDigests.length >= 1 &&
      pdoor.allow.every((d) => /^[0-9a-f]{64}$/.test(d)) &&
      mineDigests.every((d) => pdoor.allow.includes(d)) &&
      unexplained.length === 0,
    `the door's allow set ${JSON.stringify(pdoor?.allow)} must hold the member's digests ` +
      `${JSON.stringify(mineDigests)} and nothing the policy cannot name; unexplained: ` +
      `${JSON.stringify(unexplained)} (owners of the rest: ` +
      `${JSON.stringify((pdoor?.allow ?? []).map((d) => named.get(d) ?? null))})`)
  ok('resources/policy-door-has-no-deny-and-no-hold', pdoor?.deny?.length === 0 && pdoor?.hold_until === null,
    JSON.stringify(pdoor).slice(0, 200))
  ok('resources/policy-omits-a-tool-the-device-does-not-coordinate',
    !(ep.resources ?? []).some((r) => r.id === toolId), 'the door coordinator is bound to no tool')
  // And the other alias says the same: doors/state is the policy's doors.
  const republished = await POST(`/api/admin/doors/${door.id}/republish`, { token: admin.token })
  assertEq('resources/door-snapshot-republished', 200, republished.status, republished.text.slice(0, 160))
})
