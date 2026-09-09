// Tier: training waivers and the requires_training access gate (#36).
//
// Drives the toolguard tool-on endpoint to prove the shared authorization rule
// end to end against a real stack:
//
//   * a tool with requires_training=false and no steps is OPEN (granted with no
//     waiver);
//   * a tool with requires_training=true and no steps is GATED — denied until a
//     waiver exists (this is what lets a migrated ToolPass grant restrict
//     access even though the tool has no training curriculum);
//   * granting a waiver opens it; revoking the waiver re-closes it;
//   * a waiver requires a non-empty reason (400 otherwise);
//   * grant/revoke are audited.
//
// A free tool isolates the training/waiver gate from billing; the member is
// funded (and thus active/enrolled) and holds an active card so tool-on can run.

import { main, ok, assertEq, GET, POST, DELETE, account, adminAccount } from './lib.mjs'

const TOOL_KEY = 'e2e-waiver-tool-key'
const GATED = 'WAIVER-GATED' // requires_training = true, no steps
const OPEN = 'WAIVER-OPEN' // requires_training = false
const CARD = 'WAIVER-CARD'

const q = (path, params) => path + '?' + new URLSearchParams(params).toString()
const toolOn = (ext) => GET(q('/api/toolguard/tool-on', { card: CARD, tool_id: ext, api_key: TOOL_KEY }))
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function createTool(admin, externalId, requiresTraining) {
  const res = await POST('/api/tools', {
    token: admin.token,
    body: {
      name: `E2E waiver ${externalId}`,
      category: 'other',
      requires_training: requiresTraining,
      external_id: externalId,
      external_api_key: TOOL_KEY,
      usage_flat_fee: null,
      usage_rate_per_min: null,
      usage_max_session_minutes: null,
    },
  })
  ok(`waivers/tool-created:${externalId}`, res.status === 200 || res.status === 201, res.text.slice(0, 200))
  return res.json.data // { id (uuid), external_id, ... }
}

async function fund(admin, member, amount) {
  const res = await POST('/api/admin/membership/payments', {
    token: admin.token,
    body: { user_id: member.user.id, amount, entry_type: 'cash_payment' },
  })
  assertEq('waivers/funded', 200, res.status, res.text.slice(0, 200))
}

async function issueCard(admin, userId, code) {
  const res = await POST(`/api/cards/user/${userId}`, { token: admin.token, body: { code } })
  assertEq('waivers/card-issued', 200, res.status, res.text.slice(0, 200))
}

const grantWaiver = (admin, userId, toolId, reason) =>
  POST(`/api/waivers/user/${userId}/tool/${toolId}`, { token: admin.token, body: { reason } })

async function auditCount(admin, eventType, userId) {
  const res = await GET(q('/api/admin/audit-logs', { event_type: eventType, per_page: '100' }), {
    token: admin.token,
  })
  assertEq(`waivers/audit-query:${eventType}`, 200, res.status)
  return (res.json?.data ?? []).filter((r) => r.user_id === userId).length
}

main(async () => {
  const admin = await adminAccount('waivers')
  const member = await account('waivers')
  await fund(admin, member, '20.00')
  await issueCard(admin, member.user.id, CARD)

  const openTool = await createTool(admin, OPEN, false)
  const gatedTool = await createTool(admin, GATED, true)

  // A non-gated tool (requires_training=false, no steps) is open to any active
  // member — no waiver needed.
  ok('waivers/open-tool-grants', (await toolOn(OPEN)).json?.tool_on === true, 'open tool denied')

  // A requires_training tool with no steps is gated: denied with no waiver.
  ok('waivers/gated-denies-without-waiver', (await toolOn(GATED)).json?.tool_on === false, 'gated tool granted with no waiver')

  // Reason is mandatory.
  const noReason = await grantWaiver(admin, member.user.id, gatedTool.id, '   ')
  ok('waivers/empty-reason-rejected', noReason.status === 400, `${noReason.status}: ${noReason.text.slice(0, 150)}`)

  // Grant a waiver -> the gated tool opens.
  const granted = await grantWaiver(admin, member.user.id, gatedTool.id, 'migrated from ToolPass')
  assertEq('waivers/granted', 200, granted.status, granted.text.slice(0, 200))
  const waiverId = granted.json?.data?.id
  ok('waivers/gated-grants-with-waiver', (await toolOn(GATED)).json?.tool_on === true, 'waiver did not open the gated tool')

  // The waiver is listed and the grant is audited.
  const list = await GET(`/api/waivers/user/${member.user.id}`, { token: admin.token })
  assertEq('waivers/list', 200, list.status)
  ok('waivers/listed', (list.json?.data ?? []).some((w) => w.id === waiverId && w.reason === 'migrated from ToolPass'), JSON.stringify(list.json?.data))
  await sleep(400)
  ok('waivers/grant-audited', (await auditCount(admin, 'training_waiver_granted', member.user.id)) >= 1, 'no training_waiver_granted')

  // Revoke -> the gated tool closes again, and the revoke is audited.
  const revoked = await DELETE(`/api/waivers/${waiverId}`, { token: admin.token })
  assertEq('waivers/revoked', 200, revoked.status, revoked.text.slice(0, 200))
  ok('waivers/gated-denies-after-revoke', (await toolOn(GATED)).json?.tool_on === false, 'gated tool still open after revoke')
  await sleep(400)
  ok('waivers/revoke-audited', (await auditCount(admin, 'training_waiver_revoked', member.user.id)) >= 1, 'no training_waiver_revoked')
})
