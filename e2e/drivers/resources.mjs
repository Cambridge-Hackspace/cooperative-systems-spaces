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
//      the alias and the resource path open and close each other's sessions;
//   4. a door's scan runs the door's own policy -- refused with no rule (doors
//      are Restricted), granted with one -- and a yes carries the pulse of the
//      door's `strike` binding, while a door with no strike binding carries the
//      site default;
//   5. every presentation at the door lands in its access-event log as `rfid`,
//      with the resolved card and the reason, granted or not;
//   6. a stop on a door is harmless and a usage report on one is refused;
//   7. the credential rule: a device bound to the tool is not thereby bound to
//      the door (401), and an unknown resource reads the same as an unbound one.
//
// What it does NOT prove: that anything moves. The door's strike device here is
// a registration with a token; the pulse is asserted in the reply, not on a
// latch. The edge's translation of the local-broker scan onto this path is step
// 3 of #182 and is not driven here.

import { GET, POST, account, adminAccount, assertEq, ok, main, record } from './lib.mjs'

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

  // --- 3. usage, and the alias boundary --------------------------------------
  const noSession = await usage(TOOL_EXT, reader.token, { seconds: 30 })
  assertEq('resources/usage-with-no-session-is-refused', 'error', noSession.json?.status)
  assertEq('resources/usage-refusal-names-the-session', 'No open session for this tool',
    noSession.json?.message)
  // Opened on the ALIAS, reported and stopped on the resource path.
  const aliasOn = await POST('/api/toolguard/tool-on', {
    token: reader.token,
    body: { card: CARD, tool_id: TOOL_EXT },
  })
  assertEq('resources/alias-tool-on-still-works', true, aliasOn.json?.tool_on, aliasOn.text.slice(0, 200))
  const logged = await usage(TOOL_EXT, reader.token, { card: CARD, seconds: 30, active_seconds: 12 })
  assertEq('resources/usage-with-a-session-is-logged', 'ok', logged.json?.status, logged.text.slice(0, 200))
  const stopWithCard = await stop(TOOL_EXT, reader.token, { card: CARD })
  assertEq('resources/stop-with-a-card-closes-the-alias-session', 'ok', stopWithCard.json?.status)
  assertEq('resources/alias-session-is-idle', 'idle', await toolStatus())
  // And the other way: opened on the resource path, closed on the alias.
  await scan(TOOL_EXT, reader.token, CARD)
  const aliasOff = await POST('/api/toolguard/tool-off', {
    token: reader.token,
    body: { tool_id: TOOL_EXT },
  })
  assertEq('resources/alias-tool-off-closes-the-resource-session', true, aliasOff.json?.tool_off,
    aliasOff.text.slice(0, 200))
  assertEq('resources/resource-session-is-idle', 'idle', await toolStatus())

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

  // --- 7. the credential rule -------------------------------------------------
  const wrongDevice = await scan(door.id, reader.token, CARD)
  assertEq('resources/a-device-bound-elsewhere-is-401', 401, wrongDevice.status, wrongDevice.text.slice(0, 200))
  const nowhere = await scan('00000000-0000-4000-8000-00000000dead', reader.token, CARD)
  assertEq('resources/an-unknown-resource-is-401', 401, nowhere.status, nowhere.text.slice(0, 200))
  ok('resources/unknown-and-unbound-read-the-same', wrongDevice.text === nowhere.text,
    `bound-elsewhere: ${wrongDevice.text.slice(0, 120)} / unknown: ${nowhere.text.slice(0, 120)}`)
  const noToken = await POST(`/api/resources/${door.id}/scan`, { body: { card: CARD } })
  assertEq('resources/no-token-is-401', 401, noToken.status)
})
