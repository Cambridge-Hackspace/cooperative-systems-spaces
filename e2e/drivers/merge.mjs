// Tier: merging one member record into another (#118), against the live stack.
//
// Two accounts become one. Every claim here is about rows that moved between
// users, and a source check cannot see rows: a merge that re-pointed the
// ledger but not the cards reads identically to one that did both.
//
//   * the preview names what will move and every warning, and changes nothing;
//   * the commit is refused (409) without the full set of acknowledged codes --
//     none, and a partial set, asserted separately;
//   * after the commit the absorbed id resolves nowhere (404, and its username
//     no longer signs in) while its email signs in AS THE SURVIVOR;
//   * the survivor's balance is the SUM of both ledgers, its role is the union,
//     the absorbed card and address are now the survivor's;
//   * attribution survives: an audit event written about the absorbed account
//     before the merge is attributed to the survivor after it;
//   * the survivor's old sessions are revoked (roles changed).
//
// run.sh follows with the database-side oracle: no absorbed id named in
// user_merges is still a user.

import {
  assertEq,
  main,
  ok,
  GET,
  POST,
  PUT,
  RUN_TAG,
  account,
  adminAccount,
  login,
} from './lib.mjs'

const view = async (token) => (await GET('/api/membership', { token })).json?.data ?? {}

await main(async () => {
  const admin = await adminAccount('merge_admin')
  const survivor = await account('merge_survivor')
  const absorbed = await account('merge_absorbed')

  // ---- furnish both accounts --------------------------------------------------
  for (const who of [survivor, absorbed]) {
    const pay = await POST('/api/admin/membership/payments', {
      token: admin.token,
      body: { user_id: who.user.id, amount: '25.00', description: 'merge fixture' },
    })
    assertEq(`merge/fixture-payment/${who.username}`, 200, pay.status, pay.text.slice(0, 200))
  }
  // 25 in, one period of dues (10) out: 15 each, so the merged balance is 30.
  const survivorBefore = await view(survivor.token)
  assertEq('merge/fixture-survivor-balance', '15.00', survivorBefore.balance, JSON.stringify(survivorBefore))

  const staff = await PUT(`/api/admin/users/${absorbed.user.id}/role`, { token: admin.token, body: { role: 'staff' } })
  assertEq('merge/fixture-absorbed-is-staff', 200, staff.status, staff.text.slice(0, 200))

  const cardCode = `MERGE${RUN_TAG}`.slice(0, 16)
  const card = await POST(`/api/cards/user/${absorbed.user.id}`, { token: admin.token, body: { code: cardCode } })
  assertEq('merge/fixture-card-issued', 200, card.status, card.text.slice(0, 200))
  const cardId = card.json?.data?.id

  const extra = `extra.${RUN_TAG}@e2e.invalid`
  const added = await POST(`/api/users/${absorbed.user.id}/emails`, { token: admin.token, body: { email: extra } })
  assertEq('merge/fixture-secondary-added', 200, added.status, added.text.slice(0, 200))

  // ---- the preview --------------------------------------------------------------
  const previewPath = `/api/admin/users/${survivor.user.id}/merge/preview`
  const mergePath = `/api/admin/users/${survivor.user.id}/merge`

  const preview = await POST(previewPath, { token: admin.token, body: { absorbed_id: absorbed.user.id } })
  assertEq('merge/preview-accepted', 200, preview.status, preview.text.slice(0, 300))
  const plan = preview.json?.data ?? {}
  const codes = (plan.warnings ?? []).map((w) => w.code)
  ok('merge/preview-names-the-parties', plan.survivor?.id === survivor.user.id && plan.absorbed?.id === absorbed.user.id, JSON.stringify(plan))
  ok('merge/preview-warns-login-lost', codes.includes('absorbed_login_lost'), `warnings: ${codes.join(', ')}`)
  ok('merge/preview-warns-roles-raise-level', codes.includes('roles_raise_level'), `warnings: ${codes.join(', ')}`)
  ok(
    'merge/preview-warns-absorbed-logged-in-more-recently',
    codes.includes('absorbed_logged_in_more_recently'),
    `the absorbed account signed in after the survivor; warnings: ${codes.join(', ')}`
  )
  ok('merge/preview-counts-the-ledger', (plan.moves?.['membership_ledger.user_id'] ?? 0) >= 2, JSON.stringify(plan.moves))
  assertEq('merge/preview-counts-the-card', 1, plan.moves?.['user_cards.user_id'], JSON.stringify(plan.moves))
  assertEq('merge/preview-counts-both-addresses', 2, plan.moves?.['user_emails.user_id'], JSON.stringify(plan.moves))
  assertEq('merge/preview-counts-the-new-role', 1, plan.moves?.['user_roles.user_id'], JSON.stringify(plan.moves))

  // A preview changes nothing: the absorbed account is intact afterwards.
  const stillThere = await GET(`/api/users/${absorbed.user.id}`, { token: admin.token })
  assertEq('merge/preview-changes-nothing', 200, stillThere.status)

  // ---- refusals -----------------------------------------------------------------
  const self = await POST(previewPath, { token: admin.token, body: { absorbed_id: survivor.user.id } })
  assertEq('merge/self-merge-refused', 400, self.status)
  const own = await POST(`/api/admin/users/${admin.user.id}/merge/preview`, { token: admin.token, body: { absorbed_id: absorbed.user.id } })
  assertEq('merge/own-account-refused', 400, own.status)
  const peer = await POST(previewPath, { token: survivor.token, body: { absorbed_id: absorbed.user.id } })
  assertEq('merge/non-admin-refused', 403, peer.status)

  const noAck = await POST(mergePath, { token: admin.token, body: { absorbed_id: absorbed.user.id } })
  assertEq('merge/commit-without-acknowledgement-refused', 409, noAck.status, noAck.text.slice(0, 200))
  const partial = await POST(mergePath, { token: admin.token, body: { absorbed_id: absorbed.user.id, acknowledged: codes.slice(0, 1) } })
  assertEq('merge/commit-with-partial-acknowledgement-refused', 409, partial.status, partial.text.slice(0, 200))
  // Refusals change nothing either.
  const stillThere2 = await GET(`/api/users/${absorbed.user.id}`, { token: admin.token })
  assertEq('merge/refusals-change-nothing', 200, stillThere2.status)

  // ---- the commit ---------------------------------------------------------------
  const commit = await POST(mergePath, { token: admin.token, body: { absorbed_id: absorbed.user.id, acknowledged: codes } })
  assertEq('merge/commit-accepted', 200, commit.status, commit.text.slice(0, 300))
  const outcome = commit.json?.data ?? {}
  ok('merge/outcome-has-a-ledger-id', typeof outcome.merge_id === 'string', JSON.stringify(outcome))
  assertEq('merge/outcome-moved-the-card', 1, outcome.moved?.['user_cards.user_id'], JSON.stringify(outcome.moved))

  // ---- the absorbed identity is gone ----------------------------------------------
  const gone = await GET(`/api/users/${absorbed.user.id}`, { token: admin.token })
  assertEq('merge/absorbed-id-resolves-nowhere', 404, gone.status)
  const oldLogin = await login(absorbed.username)
  assertEq('merge/absorbed-username-no-longer-signs-in', 401, oldLogin.status)
  const viaEmail = await login(absorbed.email)
  assertEq('merge/absorbed-email-still-signs-in', 200, viaEmail.status, viaEmail.text.slice(0, 200))
  ok('merge/absorbed-email-signs-in-as-the-survivor', viaEmail.json?.data?.user?.id === survivor.user.id, `signed in as ${viaEmail.json?.data?.user?.id}`)
  const again = await POST(mergePath, { token: admin.token, body: { absorbed_id: absorbed.user.id, acknowledged: codes } })
  assertEq('merge/second-merge-of-the-same-account-is-404', 404, again.status)

  // ---- the survivor holds everything ------------------------------------------------
  // The survivor's roles changed, so its pre-merge session is revoked.
  const stale = await GET('/api/auth/me', { token: survivor.token })
  assertEq('merge/survivor-old-session-revoked', 401, stale.status)
  const fresh = await login(survivor.username)
  assertEq('merge/survivor-signs-in-again', 200, fresh.status)
  const token = fresh.json?.data?.token
  assertEq('merge/survivor-role-is-the-union', 'staff', fresh.json?.data?.user?.role, JSON.stringify(fresh.json?.data?.user))

  const balance = await view(token)
  assertEq('merge/survivor-balance-is-the-sum', '30.00', balance.balance, JSON.stringify(balance))
  const ledger = await GET(`/api/admin/membership/users/${survivor.user.id}/ledger`, { token: admin.token })
  assertEq('merge/survivor-ledger-has-both-histories', 4, (ledger.json?.data ?? []).length, `entries: ${(ledger.json?.data ?? []).length}`)

  const emails = await GET(`/api/users/${survivor.user.id}/emails`, { token })
  const rows = emails.json?.data ?? []
  const movedPrimary = rows.find((r) => r.email === absorbed.email)
  const movedExtra = rows.find((r) => r.email === extra)
  ok('merge/absorbed-address-is-now-a-secondary', movedPrimary && movedPrimary.is_primary === false, JSON.stringify(rows))
  ok('merge/absorbed-secondary-came-along', !!movedExtra, JSON.stringify(rows))
  ok('merge/survivor-keeps-its-primary', rows.find((r) => r.is_primary)?.email === survivor.email, JSON.stringify(rows))

  const cards = await GET(`/api/cards/user/${survivor.user.id}`, { token: admin.token })
  ok('merge/absorbed-card-is-now-the-survivors', (cards.json?.data ?? []).some((c) => c.id === cardId), JSON.stringify(cards.json?.data))

  // ---- attribution survives -----------------------------------------------------------
  // The address-added event was written about the absorbed account before the
  // merge; it must now be attributed to the survivor rather than to nobody.
  const addedEvents = await GET('/api/admin/audit-logs?event_type=user_email_added&per_page=100', { token: admin.token })
  const mine = (addedEvents.json?.data ?? []).find((e) => e.event_data?.address === extra)
  ok('merge/audit-attribution-moves-to-the-survivor', mine?.user_id === survivor.user.id, `event: ${JSON.stringify(mine)}`)
  const mergedEvents = await GET('/api/admin/audit-logs?event_type=user_merged&per_page=100', { token: admin.token })
  const mergedEvent = (mergedEvents.json?.data ?? []).find((e) => e.event_data?.absorbed_id === absorbed.user.id)
  ok('merge/audit-records-the-merge', mergedEvent?.user_id === survivor.user.id && mergedEvent?.actor_id === admin.user.id, JSON.stringify(mergedEvent))
})
