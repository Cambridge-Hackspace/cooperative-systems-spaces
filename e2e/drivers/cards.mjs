// Tier: first-class access-card resolution, lifecycle, and the revoked-card
// fraud signal (#33). Drives the toolguard endpoints (and the device sync) end
// to end against a real stack.
//
// Coverage, mapped to the issue's acceptance criteria:
//   * an ACTIVE card grants; a member holding TWO active cards keeps access
//     through either, and disabling one revokes only that credential;
//   * a DISABLED or RELEASED card is denied AND raises exactly one
//     `revoked_card_presented` event, attributable to the member;
//   * an UNKNOWN code is denied QUIETLY -- no such event, even after time passes;
//   * a RELEASED code can be reissued to a DIFFERENT member, and the live
//     (active) owner wins over the released one on resolution;
//   * `last_used_at` moves when a card opens a tool;
//   * the admin lifecycle actions are audited (card_issued/disabled/released);
//   * issuing a code a live card already holds is a 409; an empty code is a 400;
//   * a session already open settles even if its card is revoked mid-use;
//   * a member's active card codes are pushed to a device's sync allow-list, and
//     a revoked one is not.
//
// A free, no-training tool isolates card resolution from the billing and
// training gates; members are funded (which also enrolls them) so nothing but
// the card can be the reason a denial happens.

import { main, ok, assertEq, GET, POST, account, adminAccount } from './lib.mjs'

const TOOL_KEY = 'e2e-card-tool-key'
const EXTERNAL_ID = 'CARDS-TOOL-1'

const q = (path, params) => path + '?' + new URLSearchParams(params).toString()
const toolOn = (card) =>
  GET(q('/api/toolguard/tool-on', { card, tool_id: EXTERNAL_ID, api_key: TOOL_KEY }))
const toolOff = (card) =>
  GET(q('/api/toolguard/tool-off', { card, tool_id: EXTERNAL_ID, api_key: TOOL_KEY }))
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function createFreeTool(admin) {
  const res = await POST('/api/tools', {
    token: admin.token,
    body: {
      name: `E2E cards ${EXTERNAL_ID}`,
      category: 'other',
      requires_training: false,
      external_id: EXTERNAL_ID,
      external_api_key: TOOL_KEY,
      usage_flat_fee: null,
      usage_rate_per_min: null,
      usage_max_session_minutes: null,
    },
  })
  ok('cards/tool-created', res.status === 200 || res.status === 201, res.text.slice(0, 200))
}

async function fund(admin, member, amount) {
  const res = await POST('/api/admin/membership/payments', {
    token: admin.token,
    body: { user_id: member.user.id, amount, entry_type: 'cash_payment' },
  })
  assertEq('cards/funded', 200, res.status, res.text.slice(0, 200))
}

async function issueCard(admin, userId, code) {
  const res = await POST(`/api/cards/user/${userId}`, { token: admin.token, body: { code } })
  assertEq(`cards/issued:${code}`, 200, res.status, res.text.slice(0, 200))
  return res.json.data
}
async function disableCard(admin, cardId) {
  const res = await POST(`/api/cards/${cardId}/disable`, {
    token: admin.token,
    body: { reason: 'e2e: reported lost' },
  })
  assertEq('cards/disabled', 200, res.status, res.text.slice(0, 200))
  return res.json.data
}
async function releaseCard(admin, cardId) {
  const res = await POST(`/api/cards/${cardId}/release`, { token: admin.token, body: {} })
  assertEq('cards/released', 200, res.status, res.text.slice(0, 200))
  return res.json.data
}
async function listCards(admin, userId) {
  const res = await GET(`/api/cards/user/${userId}`, { token: admin.token })
  assertEq('cards/list', 200, res.status, res.text.slice(0, 200))
  return res.json.data ?? []
}

// Count audit rows of `eventType` attributable to `userId`.
async function auditCount(admin, eventType, userId) {
  const res = await GET(q('/api/admin/audit-logs', { event_type: eventType, per_page: '100' }), {
    token: admin.token,
  })
  assertEq(`cards/audit-query:${eventType}`, 200, res.status)
  return (res.json?.data ?? []).filter((r) => r.user_id === userId).length
}
// The revoked event is written fire-and-forget; poll for a rise.
async function waitRevoked(admin, userId, atLeast, timeoutMs = 4000) {
  const start = Date.now()
  let n = await auditCount(admin, 'revoked_card_presented', userId)
  while (n < atLeast && Date.now() - start < timeoutMs) {
    await sleep(100)
    n = await auditCount(admin, 'revoked_card_presented', userId)
  }
  return n
}

main(async () => {
  const admin = await adminAccount('cards')
  const member = await account('cards')
  const other = await account('cardstwo')
  await createFreeTool(admin)
  await fund(admin, member, '20.00')
  await fund(admin, other, '20.00')

  // 1. An active card opens the tool, and using it stamps last_used_at.
  const active = await issueCard(admin, member.user.id, 'CARDS-ACTIVE')
  const beforeUse = (await listCards(admin, member.user.id)).find((c) => c.id === active.id)
  ok('cards/last-used-null-before-use', beforeUse && beforeUse.last_used_at == null, JSON.stringify(beforeUse))
  const on1 = await toolOn('CARDS-ACTIVE')
  ok('cards/active-grants', on1.json?.tool_on === true, JSON.stringify(on1.json))
  assertEq('cards/active-off', 200, (await toolOff('CARDS-ACTIVE')).status)
  const afterUse = (await listCards(admin, member.user.id)).find((c) => c.id === active.id)
  ok('cards/last-used-set-after-use', afterUse && afterUse.last_used_at != null, JSON.stringify(afterUse))

  const base = await auditCount(admin, 'revoked_card_presented', member.user.id)
  ok('cards/no-fraud-events-yet', base === 0, `expected 0, got ${base}`)

  // 2. Disabling that card revokes access and raises the fraud signal.
  await disableCard(admin, active.id)
  ok('cards/disabled-denies', (await toolOn('CARDS-ACTIVE')).json?.tool_on === false, 'disabled granted')
  const afterDisable = await waitRevoked(admin, member.user.id, base + 1)
  ok('cards/disabled-raises-fraud-event', afterDisable === base + 1, `expected ${base + 1}, got ${afterDisable}`)

  // 3. A released card is likewise denied and raises the signal.
  const released = await issueCard(admin, member.user.id, 'CARDS-RELEASED')
  await releaseCard(admin, released.id)
  ok('cards/released-denies', (await toolOn('CARDS-RELEASED')).json?.tool_on === false, 'released granted')
  const afterRelease = await waitRevoked(admin, member.user.id, afterDisable + 1)
  ok('cards/released-raises-fraud-event', afterRelease === afterDisable + 1, `got ${afterRelease}`)

  // 4. An unknown code is denied quietly: no fraud event, even after a beat.
  ok('cards/unknown-denies', (await toolOn('CARDS-NEVER-ISSUED')).json?.tool_on === false, 'unknown granted')
  await sleep(750)
  const afterUnknown = await auditCount(admin, 'revoked_card_presented', member.user.id)
  ok('cards/unknown-is-quiet', afterUnknown === afterRelease, `expected ${afterRelease}, got ${afterUnknown}`)

  // C. The admin lifecycle actions were audited.
  ok('cards/audit-issued', (await auditCount(admin, 'card_issued', member.user.id)) >= 1, 'no card_issued')
  ok('cards/audit-disabled', (await auditCount(admin, 'card_disabled', member.user.id)) >= 1, 'no card_disabled')
  ok('cards/audit-released', (await auditCount(admin, 'card_released', member.user.id)) >= 1, 'no card_released')

  // 1 (multi-card). A member may hold several active cards; disabling one
  // revokes only that credential, and the others keep working.
  const m1 = await issueCard(admin, member.user.id, 'MULTI-1')
  await issueCard(admin, member.user.id, 'MULTI-2')
  ok('cards/multi-first-grants', (await toolOn('MULTI-1')).json?.tool_on === true, 'MULTI-1 denied')
  await toolOff('MULTI-1')
  ok('cards/multi-second-grants', (await toolOn('MULTI-2')).json?.tool_on === true, 'MULTI-2 denied')
  await toolOff('MULTI-2')
  await disableCard(admin, m1.id)
  ok('cards/multi-disabled-one-denies', (await toolOn('MULTI-1')).json?.tool_on === false, 'disabled MULTI-1 granted')
  ok('cards/multi-other-retains-access', (await toolOn('MULTI-2')).json?.tool_on === true, 'MULTI-2 lost access')
  await toolOff('MULTI-2')

  // 2b/precedence. A released code can be reissued to a DIFFERENT member, and a
  // live (active) row wins over a released one sharing the code.
  const rx = await issueCard(admin, member.user.id, 'REISSUE-X')
  await releaseCard(admin, rx.id)
  const reissue = await POST(`/api/cards/user/${other.user.id}`, {
    token: admin.token,
    body: { code: 'REISSUE-X' },
  })
  ok('cards/reissue-released-code-succeeds', reissue.status === 200, `${reissue.status}: ${reissue.text.slice(0, 150)}`)
  const revBefore = await auditCount(admin, 'revoked_card_presented', member.user.id)
  ok('cards/reissue-grants-to-new-owner', (await toolOn('REISSUE-X')).json?.tool_on === true, 'reissued code denied')
  await toolOff('REISSUE-X')
  await sleep(500)
  // Active (other) won over released (member): the released owner gets no event.
  ok(
    'cards/reissue-active-wins-over-released',
    (await auditCount(admin, 'revoked_card_presented', member.user.id)) === revBefore,
    'released owner wrongly got a revoked event -- resolution picked the released row',
  )

  // B. Uniqueness and validation on issue.
  await issueCard(admin, member.user.id, 'DUP-CODE')
  const dup = await POST(`/api/cards/user/${member.user.id}`, { token: admin.token, body: { code: 'DUP-CODE' } })
  ok('cards/duplicate-live-code-conflicts', dup.status === 409, `${dup.status}: ${dup.text.slice(0, 150)}`)
  const empty = await POST(`/api/cards/user/${member.user.id}`, { token: admin.token, body: { code: '   ' } })
  ok('cards/empty-code-rejected', empty.status === 400, `${empty.status}: ${empty.text.slice(0, 150)}`)

  // D. A session already open settles even if its card is revoked mid-use.
  const settle = await issueCard(admin, member.user.id, 'SETTLE')
  ok('cards/settle-session-open', (await toolOn('SETTLE')).json?.tool_on === true, 'SETTLE did not open')
  await disableCard(admin, settle.id)
  const soff = await toolOff('SETTLE')
  ok(
    'cards/settle-through-after-revoke',
    soff.status === 200 && soff.json?.error !== 'Unknown card',
    `${soff.status}: ${JSON.stringify(soff.json)}`,
  )

  // The device sync allow-list -- get_toolguard_sync_data's union of a member's
  // ACTIVE user_cards codes -- is covered by a source check
  // (checks/tests/edge_sync_includes_active_cards.rs) rather than here.
  // Exercising the device /sync endpoint needs a registered device, and a device
  // invite code is emoji requiring a UTF-8 database that only the `utf8` reaper
  // profile provides; the default battery's stack cannot store one.
})
