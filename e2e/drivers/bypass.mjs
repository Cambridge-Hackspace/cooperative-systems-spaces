// Bypass detection stage (#84), exercised against a real stack.
//
// What this proves: the liveness sweep notices a bound module that is not
// reporting and records it ONCE, not once per sweep. That "once" is the whole
// design -- nothing prunes audit_logs, so a detector that logged a level rather
// than a transition would write a row every couple of seconds forever and bury
// the signal in itself. The stage therefore sweeps several times over and
// asserts the count did not grow.
//
// It also proves the row says what it can and no more: silence, for how long,
// with an explicit note that the cause is not distinguishable from here.
//
// What this does NOT prove: that anything was actually bypassed. A registered
// device that never sends a heartbeat is silent for an innocent reason, and the
// detector cannot tell that apart from a module pulled off the wall -- which is
// exactly the limit the event text states rather than papers over.
//
// Cluster encoding: binding a module needs a registered device, which needs an
// invite code of eight emoji. On a cluster that cannot store one the whole
// stage is skipped with that reason, as in toolmodules.mjs and concurrency.mjs.

import { GET, POST, adminAccount, assertEq, ok, record, main } from './lib.mjs'

const ENCODING = process.env.CSS_DB_ENCODING ?? 'UTF8'
const CAN_REGISTER_DEVICE = ENCODING === 'UTF8' || ENCODING === 'SQL_ASCII'

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

/** Audit rows of one type, newest first. */
async function auditRows(token, eventType) {
  const res = await GET(`/api/admin/audit-logs?limit=300&event_type=${eventType}`, { token })
  const body = res.json?.data
  const rows = Array.isArray(body) ? body : (body?.logs ?? [])
  return rows.filter((r) => r.event_type === eventType)
}

main(async () => {
  const admin = await adminAccount('bypass_admin')
  const T = { token: admin.token }
  const tag = admin.username

  if (!CAN_REGISTER_DEVICE) {
    record(
      'bypass/not-run-on-this-cluster',
      'skip',
      `the liveness sweep needs a module bound to a registered device, a device needs an ` +
        `invite, and this cluster (${ENCODING}) cannot store an eight-emoji invite code.`
    )
    return
  }

  // --- a tool with a power module whose device has never reported ------------
  const tool = await POST('/api/tools', {
    token: admin.token,
    body: { name: `Bypass Laser ${tag}`, category: 'laser_cutting', external_id: `bp-${tag}` },
  })
  const toolId = tool.json?.data?.id
  ok('bypass/tool-created', !!toolId, `POST /api/tools -> ${tool.status}`)

  const invite = await POST('/api/admin/devices/invite', {
    token: admin.token,
    body: { expires_in_hours: 1 },
  })
  const reg = await POST('/api/devices/register', {
    body: {
      device_code: invite.json?.data?.device_code,
      name: `silent-plug-${tag}`,
      kind: 'power_controller',
      mac_address: '02:00:00:00:84:01',
      software_version: '0.0.0-e2e',
      platform: 'linux',
    },
  })
  const deviceId = reg.json?.data?.device_id ?? reg.json?.device_id
  ok('bypass/device-registered', !!deviceId, `register -> ${reg.status} ${reg.text.slice(0, 160)}`)

  const binding = await POST('/api/admin/tool-modules', {
    token: admin.token,
    body: { tool_id: toolId, device_id: deviceId, role: 'power', name: `silent plug ${tag}` },
  })
  assertEq('bypass/module-bound', 201, binding.status)
  const moduleId = binding.json?.data?.id

  // --- the sweep notices ----------------------------------------------------
  // stack-config sets module_silence_secs=1 and liveness_sweep_secs=2, so a
  // couple of sweeps have run by the time this resolves.
  await sleep(6000)

  const silent = await auditRows(admin.token, 'tool_module_silent')
  const mine = silent.filter((r) => r.event_data?.module_id === moduleId)
  ok(
    'bypass/silent-module-is-recorded',
    mine.length >= 1,
    `no tool_module_silent row for module ${moduleId} among ${silent.length} such rows`
  )

  // The row states silence and declines to guess the cause. An event that
  // claimed "power disconnected" would be wrong whenever the real cause was the
  // network, which is most of the time.
  const row = mine[0]
  ok(
    'bypass/row-names-the-module',
    row?.event_data?.module_name?.includes(tag),
    `event_data did not carry the module name: ${JSON.stringify(row?.event_data ?? {}).slice(0, 200)}`
  )
  ok(
    'bypass/row-declines-to-guess-the-cause',
    String(row?.event_data?.note ?? '').includes('not distinguishable'),
    'the row should say the cause cannot be distinguished, not imply it knows'
  )

  // --- and does not keep saying it -----------------------------------------
  // The transition rule, asserted the only way that means anything: let several
  // more sweeps run and show the count did not move.
  const before = mine.length
  await sleep(6000)
  const after = (await auditRows(admin.token, 'tool_module_silent')).filter(
    (r) => r.event_data?.module_id === moduleId
  ).length
  assertEq('bypass/silence-is-reported-once-not-per-sweep', before, after)

  // --- unauthorized power ---------------------------------------------------
  // The server's record of "allowed to be on" is tools.status: tool_on sets it
  // InUse. This tool was never turned on, so a module reporting its relay closed
  // is the physical side-button case -- energized with nothing authorizing it.
  const powerTool = await POST('/api/tools', {
    token: admin.token,
    body: {
      name: `Side Button ${tag}`,
      category: 'other',
      external_id: `bp-power-${tag}`,
      external_api_key: `bp-key-${tag}`,
    },
  })
  const powerToolId = powerTool.json?.data?.id
  ok('bypass/power-tool-created', !!powerToolId, `POST /api/tools -> ${powerTool.status}`)

  const report = (body) =>
    POST('/api/toolguard/power-report', {
      body: { tool_id: `bp-power-${tag}`, api_key: `bp-key-${tag}`, ...body },
    })

  // Two reports: the first starts the evidence clock, the second is past the
  // 1s debounce this stack configures.
  assertEq('bypass/relay-report-accepted', 200, (await report({ relay_on: true })).status)
  await sleep(2000)
  await report({ relay_on: true })
  await sleep(4000)

  const found = await auditRows(admin.token, 'unauthorized_power_detected')
  const mineP = found.filter((r) => r.event_data?.tool_id === powerToolId)
  ok(
    'bypass/unauthorized-power-is-recorded',
    mineP.length >= 1,
    `no unauthorized_power_detected row for ${powerToolId} among ${found.length}`
  )
  // Which oracle fired is part of the record: "the plug said it was on" and
  // "the circuit was pulling six amps" are different claims.
  assertEq(
    'bypass/evidence-names-the-relay-oracle',
    true,
    mineP[0]?.event_data?.evidence?.relay_reported_on
  )
  assertEq(
    'bypass/evidence-does-not-claim-draw-it-did-not-see',
    false,
    mineP[0]?.event_data?.evidence?.draw_observed
  )
  ok(
    'bypass/row-says-it-did-not-act',
    String(mineP[0]?.event_data?.note ?? '').includes('not acted on'),
    'the row should say it recorded rather than acted'
  )

  // And, like silence, it is reported once per episode rather than per sweep.
  const beforeP = mineP.length
  await report({ relay_on: true })
  await sleep(5000)
  const afterP = (await auditRows(admin.token, 'unauthorized_power_detected')).filter(
    (r) => r.event_data?.tool_id === powerToolId
  ).length
  assertEq('bypass/unauthorized-power-reported-once-per-episode', beforeP, afterP)

  // Switching the relay off clears the evidence clock, so the tool stops being
  // a finding rather than staying flagged forever.
  await report({ relay_on: false })
  await sleep(3000)
  const afterOff = (await auditRows(admin.token, 'unauthorized_power_detected')).filter(
    (r) => r.event_data?.tool_id === powerToolId
  ).length
  assertEq('bypass/no-new-finding-once-power-is-off', beforeP, afterOff)
})
