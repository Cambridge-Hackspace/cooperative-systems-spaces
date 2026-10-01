// Tier 6: a user's several email addresses (#118), against the live stack.
//
// Before #118 an account was one address: one column, one unique constraint,
// one place to send mail. Now `user_emails` holds every address a user has and
// `users.email` mirrors the primary. The properties below cannot be shown by a
// source check because every one of them is about which of two rows a query
// picked, and a query that picks the wrong one reads exactly like one that
// picks the right one:
//
//   * a confirmation link for a secondary reaches THE SECONDARY, and spending
//     it confirms that row and not the primary (the token names the row);
//   * any address signs in, and a second account cannot register under a
//     member's secondary;
//   * a reset requested through an UNCONFIRMED secondary reaches nobody --
//     asserted as an absence, with the confirmed-secondary case as the
//     precondition that proves the mailer was listening;
//   * the primary cannot be removed, and an unconfirmed address cannot be
//     promoted;
//   * a self-service add needs the current password; a manager's does not.
//
// The mirror itself (users.email == the primary row) is checked by run.sh
// straight out of the database after this driver, where a divergence would
// actually show -- every endpoint reads the mirror, so none of them can.

import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

import {
  assertEq,
  main,
  ok,
  GET,
  POST,
  PUT,
  DELETE,
  RUN_TAG,
  PASSWORD,
  account,
  adminAccount,
  login,
  register,
} from './lib.mjs'

const STACK_DIR = process.env.CSS_STACK_DIR ?? '/stack'
const MAILDIR = join(STACK_DIR, 'mail')

// --- maildir (mirrors mail.mjs) --------------------------------------------
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

await main(async () => {
  const me = await account('emails')
  const base = `/api/users/${me.user.id}/emails`

  // ---- the primary is seeded for every account --------------------------------
  const initial = await GET(base, { token: me.token })
  assertEq('emails/list-ok', 200, initial.status)
  const seeded = initial.json?.data ?? []
  assertEq('emails/one-row-at-registration', 1, seeded.length, `rows: ${JSON.stringify(seeded)}`)
  ok(
    'emails/seeded-row-is-the-primary',
    seeded[0]?.is_primary === true && seeded[0]?.email === me.email,
    `seeded row: ${JSON.stringify(seeded[0])}`
  )
  const primaryId = seeded[0]?.id

  // ---- self-service add needs the current password -----------------------------
  const second = `second.${RUN_TAG}@e2e.invalid`
  const noPw = await POST(base, { token: me.token, body: { email: second } })
  assertEq('emails/self-add-needs-current-password', 400, noPw.status, `add without password -> ${noPw.status}`)

  const added = await POST(base, { token: me.token, body: { email: second, current_password: PASSWORD } })
  assertEq('emails/self-add-with-password', 200, added.status, `add -> ${added.status} ${added.text.slice(0, 200)}`)
  const secondId = added.json?.data?.id
  ok('emails/added-row-is-unconfirmed', added.json?.data?.verified_at === null, `row: ${added.text.slice(0, 200)}`)
  ok('emails/added-row-is-not-primary', added.json?.data?.is_primary === false, `row: ${added.text.slice(0, 200)}`)

  // ---- the link reaches the secondary, and confirms exactly that row -------------
  const confirm = await waitForMessage(
    (m) => addressedTo(second)(m) && subjectIs('Confirm your email address')(m)
  )
  ok('emails/confirmation-reaches-the-secondary', confirm !== null, `nothing arrived for ${second}`)
  const token = confirm ? tokenIn(confirm.text) : null
  ok('emails/confirmation-carries-token', !!token, 'no token in the link')

  // Precondition for the "cannot promote unconfirmed" claim below, taken
  // BEFORE the token is spent so the refusal is about state, not timing.
  const earlyPromote = await PUT(`${base}/${secondId}/primary`, {
    token: me.token,
    body: { current_password: PASSWORD },
  })
  assertEq('emails/unconfirmed-cannot-become-primary', 400, earlyPromote.status, `promote unconfirmed -> ${earlyPromote.status}`)

  if (token) {
    const v = await POST('/api/auth/email/verify', { body: { token } })
    assertEq('emails/verify-succeeds', 200, v.status, v.text.slice(0, 200))
  }
  const afterVerify = await GET(base, { token: me.token })
  const rows = afterVerify.json?.data ?? []
  const secondRow = rows.find((r) => r.id === secondId)
  const primaryRow = rows.find((r) => r.id === primaryId)
  ok('emails/verify-confirms-the-secondary-row', !!secondRow?.verified_at, `secondary: ${JSON.stringify(secondRow)}`)
  // The token named the secondary; the primary's state must be untouched by it.
  // Registration sends the primary its own link, which this driver never
  // spends, so the primary is still unconfirmed here -- and if the verify had
  // confirmed "the user" instead of "the row", it would not be.
  ok(
    'emails/verify-leaves-the-primary-alone',
    primaryRow?.verified_at === null,
    `primary after verifying the secondary: ${JSON.stringify(primaryRow)}`
  )

  // ---- any address signs in; nobody else can register it -------------------------
  const viaSecondary = await login(second)
  assertEq('emails/login-via-secondary', 200, viaSecondary.status, `login as ${second} -> ${viaSecondary.status}`)
  ok(
    'emails/login-via-secondary-is-the-same-account',
    viaSecondary.json?.data?.user?.id === me.user.id,
    `signed in as ${viaSecondary.json?.data?.user?.id}, expected ${me.user.id}`
  )
  const squat = await register(`squat_${RUN_TAG}`, second.toUpperCase())
  assertEq('emails/secondary-blocks-registration', 409, squat.status, `register with a member's secondary -> ${squat.status}`)

  // ---- reset mail: confirmed secondary yes, unconfirmed secondary no --------------
  const resetViaConfirmed = await POST('/api/auth/password-reset/request', { body: { email: second } })
  assertEq('emails/reset-request-accepted', 200, resetViaConfirmed.status)
  const resetMail = await waitForMessage((m) => addressedTo(second)(m) && subjectIs('Reset your password')(m))
  ok('emails/reset-reaches-a-confirmed-secondary', resetMail !== null, `no reset mail for ${second}`)

  const third = `third.${RUN_TAG}@e2e.invalid`
  const addedThird = await POST(base, { token: me.token, body: { email: third, current_password: PASSWORD } })
  assertEq('emails/add-third', 200, addedThird.status, addedThird.text.slice(0, 200))
  const thirdId = addedThird.json?.data?.id
  // Its confirmation link arrives (that is the add); we do not spend it.
  const thirdConfirm = await waitForMessage((m) => addressedTo(third)(m) && subjectIs('Confirm your email address')(m))
  ok('emails/third-gets-a-confirmation-link', thirdConfirm !== null, `no confirmation for ${third}`)
  const resetViaUnconfirmed = await POST('/api/auth/password-reset/request', { body: { email: third } })
  assertEq('emails/reset-via-unconfirmed-answers-uniformly', 200, resetViaUnconfirmed.status)
  // Absence, with time allowed to pass. The confirmed-secondary reset above is
  // the precondition that proves the mailer delivers reset mail at all.
  const leaked = await waitForMessage((m) => addressedTo(third)(m) && subjectIs('Reset your password')(m), 3000)
  ok('emails/reset-never-reaches-an-unconfirmed-secondary', leaked === null, `a reset link was mailed to the unconfirmed ${third}`)

  // ---- promote, then the old primary is a secondary; remove rules ----------------
  const promote = await PUT(`${base}/${secondId}/primary`, { token: me.token, body: { current_password: PASSWORD } })
  assertEq('emails/promote-confirmed-secondary', 200, promote.status, promote.text.slice(0, 200))
  const afterPromote = (promote.json?.data ?? []).find((r) => r.id === secondId)
  ok('emails/promoted-row-is-primary', afterPromote?.is_primary === true, JSON.stringify(promote.json?.data))
  const meNow = await GET(`/api/users/${me.user.id}`, { token: me.token })
  assertEq('emails/user-email-follows-the-primary', second, meNow.json?.data?.email, `users.email after promote: ${meNow.json?.data?.email}`)
  ok('emails/user-verified-follows-the-primary', meNow.json?.data?.email_verified === true, `email_verified: ${meNow.json?.data?.email_verified}`)

  const removePrimary = await DELETE(`${base}/${secondId}`, { token: me.token })
  assertEq('emails/primary-cannot-be-removed', 400, removePrimary.status, `remove primary -> ${removePrimary.status}`)
  const removeOld = await DELETE(`${base}/${primaryId}`, { token: me.token })
  assertEq('emails/old-primary-removable-once-demoted', 200, removeOld.status, removeOld.text.slice(0, 200))
  const oldLogin = await login(me.email)
  assertEq('emails/removed-address-no-longer-signs-in', 401, oldLogin.status, `login as removed ${me.email} -> ${oldLogin.status}`)
  const reclaim = await register(`reclaim_${RUN_TAG}`, me.email)
  ok('emails/removed-address-is-free-again', reclaim.status === 200 || reclaim.status === 201, `re-register ${me.email} -> ${reclaim.status}`)

  // ---- a manager acts without a password; a peer cannot ---------------------------
  const admin = await adminAccount('emails_admin')
  const fourth = `fourth.${RUN_TAG}@e2e.invalid`
  const byAdmin = await POST(base, { token: admin.token, body: { email: fourth } })
  assertEq('emails/manager-adds-without-password', 200, byAdmin.status, byAdmin.text.slice(0, 200))
  const peer = await account('emails_peer')
  const byPeer = await POST(base, { token: peer.token, body: { email: `peer.${RUN_TAG}@e2e.invalid` } })
  assertEq('emails/peer-cannot-add-to-another-account', 403, byPeer.status, `peer add -> ${byPeer.status}`)
  const peerList = await GET(base, { token: peer.token })
  assertEq('emails/peer-cannot-list-another-account', 403, peerList.status, `peer list -> ${peerList.status}`)

  // Removing the unconfirmed third frees it: a stale, unconfirmed claim must
  // never squat an address, and an explicit remove is the immediate form.
  const removeThird = await DELETE(`${base}/${thirdId}`, { token: me.token })
  assertEq('emails/remove-unconfirmed', 200, removeThird.status, removeThird.text.slice(0, 200))
})
