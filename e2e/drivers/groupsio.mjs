// Tier: Groups.io mailing-list sync, over a simulated Groups.io API.
//
// The destination is css-groupsio-sink, an in-repo stateful fake started by
// stack.sh. This is the only tier that establishes the sync *speaks to Groups.io
// and reshapes a real roster*: everything below it proves the reconcile plan is
// right (reconcile_plan unit tests) or that the client serializes a request
// (groupsio_client.rs), and none of it proves the server, wired to a live
// membership API, actually adds and removes the right people.
//
// It exercises the three moving parts against the fake:
//   * event push -- verifying a member's email makes them intended, and the
//     audit-event consumer adds them to the group without a reconcile;
//   * reconciliation -- a seeded stranger is removed (the platform owns the
//     list) while a configured protected address survives;
//   * the inbound webhook -- a correctly-signed unsubscribe opts a member out,
//     and a forged one is refused.
//
// The roster assertion goes through the shared, self-tested invariant
// (journeys/groupsio-invariants.mjs), whose broken worlds are proven to fire in
// journeys/groupsio-selftest.mjs -- so a green here is not a broken oracle.
//
// WHAT THIS DOES NOT PROVE: that reconcile removes *every* stranger (the
// invariant is per-address; reconcile_plan's unit tests own the full set), nor
// anything about the real Groups.io wire format -- the fake accepts what this
// server's client sends, and the true parameter names are settled against a live
// account.

import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { createHmac } from 'node:crypto'

import { main, ok, assertEq, GET, POST, account, adminAccount } from './lib.mjs'
import { groupsioReconcileHonored } from '../journeys/groupsio-invariants.mjs'

const STACK_DIR = process.env.CSS_STACK_DIR ?? '/stack'
const MAILDIR = join(STACK_DIR, 'mail')
const SINK = process.env.CSS_GROUPSIO_SINK_URL ?? 'http://127.0.0.1:4390'

// These MUST match e2e/stack-config.toml's [groupsio] block.
const WEBHOOK_SECRET = 'e2e-groupsio-webhook-secret'
const PROTECTED = 'owner@e2e.invalid'
const STRANGER = 'stranger@e2e.invalid'

// --- maildir, to read the confirmation token (mirrors mail.mjs) -------------
function messages() {
  let names
  try {
    names = readdirSync(MAILDIR).filter((n) => n.endsWith('.eml'))
  } catch {
    return []
  }
  return names.sort().map((n) => ({ name: n, text: readFileSync(join(MAILDIR, n), 'utf8') }))
}
async function waitForMessage(predicate, timeoutMs = 8000) {
  const deadline = Date.now() + timeoutMs
  for (;;) {
    const found = messages().filter(predicate)
    if (found.length > 0) return found[found.length - 1]
    if (Date.now() > deadline) return null
    await new Promise((r) => setTimeout(r, 100))
  }
}
const addressedTo = (addr) => (m) => m.text.includes(`X-Sink-Rcpt-To: <${addr}>`)
const subjectIs = (subject) => (m) => m.text.includes(`Subject: ${subject}`)
function decodeQuotedPrintable(text) {
  return text.replace(/=\r?\n/g, '').replace(/=([0-9A-Fa-f]{2})/g, (_, hex) => String.fromCharCode(parseInt(hex, 16)))
}
function tokenIn(text) {
  const match = decodeQuotedPrintable(text).match(/[?&]token=([0-9a-f]+)/)
  return match ? match[1] : null
}

// --- the fake Groups.io's control surface -----------------------------------
async function sinkReset() {
  await fetch(`${SINK}/_control/reset`, { method: 'POST' })
}
async function sinkSeed(members) {
  await fetch(`${SINK}/_control/seed`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ members }),
  })
}
async function sinkRoster() {
  const res = await fetch(`${SINK}/_control/roster`)
  const json = await res.json()
  return (json.members ?? []).map((e) => String(e).toLowerCase())
}
async function waitForSinkMember(email, timeoutMs = 10000) {
  const want = email.toLowerCase()
  const deadline = Date.now() + timeoutMs
  for (;;) {
    if ((await sinkRoster()).includes(want)) return true
    if (Date.now() > deadline) return false
    await new Promise((r) => setTimeout(r, 150))
  }
}

await main(async () => {
  const admin = await adminAccount('groupsio_admin')
  await sinkReset()

  // ---- event push: verifying a member adds them to the group ----------------
  const member = await account('groupsio')
  const confirm = await waitForMessage(
    (m) => addressedTo(member.email)(m) && subjectIs('Confirm your email address')(m)
  )
  ok('groupsio/confirmation-email-arrives', confirm !== null, `no confirmation for ${member.email}`)
  const token = confirm ? tokenIn(confirm.text) : null
  ok('groupsio/confirmation-carries-token', !!token, 'no token in the confirmation link')
  if (token) {
    const v = await POST('/api/auth/email/verify', { body: { token } })
    assertEq('groupsio/verify-succeeds', 200, v.status)
  }
  ok(
    'groupsio/event-push-adds-verified-member',
    await waitForSinkMember(member.email),
    `${member.email} was not added to the group after verification`
  )

  // ---- reconciliation owns the list -----------------------------------------
  await sinkSeed([STRANGER, PROTECTED])
  // Precondition before the success indicator: the stranger IS on the group
  // now, so its later absence is proof reconcile removed it, not proof it was
  // never there.
  ok(
    'groupsio/stranger-present-before-reconcile',
    (await sinkRoster()).includes(STRANGER),
    'seeding the fake did not take; a later "removed" result would be meaningless'
  )

  const rec = await POST('/api/admin/groupsio/reconcile', { token: admin.token })
  assertEq('groupsio/reconcile-accepted', 200, rec.status)
  ok('groupsio/reconcile-ok', rec.json?.data?.ok === true, `outcome not ok: ${rec.text.slice(0, 300)}`)

  const roster = await sinkRoster()
  const violation = groupsioReconcileHonored(
    { present: [member.email, PROTECTED], absent: [STRANGER] },
    roster
  )
  ok('groupsio/reconcile-honors-intent', violation === null, violation ?? '')

  // Second oracle: the admin status endpoint independently records the run and
  // the removal, so the claim rests on more than the fake's own roster.
  const status = await GET('/api/admin/groupsio/status', { token: admin.token })
  assertEq('groupsio/status-accepted', 200, status.status)
  const lastRun = status.json?.data?.recent_runs?.[0]
  ok('groupsio/status-records-a-successful-run', lastRun?.ok === true, `no ok run: ${status.text.slice(0, 300)}`)
  ok('groupsio/status-counts-the-removal', (lastRun?.removed ?? 0) >= 1, `expected removed>=1, got ${lastRun?.removed}`)

  // ---- inbound webhook -------------------------------------------------------
  const bodyObj = { action: 'leave', email: member.email }
  const raw = JSON.stringify(bodyObj)
  const sig = 'sha256=' + createHmac('sha256', WEBHOOK_SECRET).update(raw).digest('hex')
  const hook = await POST('/api/groupsio/webhook', {
    body: bodyObj,
    headers: { 'X-Groupsio-Signature': sig },
  })
  assertEq('groupsio/webhook-accepts-a-signed-event', 200, hook.status)
  ok('groupsio/webhook-handled', hook.json?.data?.handled === true, `not handled: ${hook.text.slice(0, 300)}`)
  const sub = await GET('/api/groupsio/subscription', { token: member.token })
  ok(
    'groupsio/webhook-opts-the-member-out',
    sub.json?.data?.subscribed === false,
    `member still subscribed after a webhook unsubscribe: ${sub.text.slice(0, 300)}`
  )

  // A forged signature is refused outright -- before it can change anyone.
  const forged = await POST('/api/groupsio/webhook', {
    body: { action: 'leave', email: 'nobody@e2e.invalid' },
    headers: { 'X-Groupsio-Signature': 'sha256=deadbeef' },
  })
  assertEq('groupsio/forged-webhook-refused', 401, forged.status)
})
