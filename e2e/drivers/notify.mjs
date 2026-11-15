// notify -- the notification relay (#226), against a real stack and its mail sink.
//
// An app holding an app token (client credentials, notify:members) declares
// its kinds and messages a member by `sub`. What is asserted is what reaches
// the member's inbox -- read from the SMTP sink, not inferred from the API's
// answer -- under each choice they can make, and that the app's answer is the
// same whether anything reached them or not.
//
// Absence is asserted with time allowed to pass, and only after a message the
// same way HAS arrived, so "nothing came" can't mean "the mail path is down".

import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { BASE, GET, POST, PUT, PATCH, RUN_TAG, account, adminAccount, assertEq, main, ok } from './lib.mjs'

const MAILDIR = join(process.env.CSS_STACK_DIR ?? '/stack', 'mail')
function messages() {
  try {
    return readdirSync(MAILDIR)
      .filter((n) => n.endsWith('.eml'))
      .sort()
      .map((n) => readFileSync(join(MAILDIR, n), 'utf8'))
  } catch {
    return []
  }
}
const plain = (m) => m.replace(/=\r?\n/g, '').replace(/=([0-9A-Fa-f]{2})/g, (_, x) => String.fromCharCode(parseInt(x, 16)))
const to = (addr) => (m) => m.includes(`X-Sink-Rcpt-To: <${addr}>`)
const subject = (s) => (m) => plain(m).includes(`Subject: ${s}`)
async function waitFor(pred, ms = 8000) {
  const deadline = Date.now() + ms
  for (;;) {
    const found = messages().filter(pred)
    if (found.length) return found[found.length - 1]
    if (Date.now() > deadline) return null
    await new Promise((r) => setTimeout(r, 100))
  }
}
const countOf = (pred) => messages().filter(pred).length

async function form(path, fields, id, secret) {
  const res = await fetch(new URL(path, BASE), {
    method: 'POST',
    headers: {
      'Content-Type': 'application/x-www-form-urlencoded',
      Authorization: 'Basic ' + Buffer.from(`${encodeURIComponent(id)}:${encodeURIComponent(secret)}`).toString('base64'),
    },
    body: new URLSearchParams(fields).toString(),
  })
  return { status: res.status, json: await res.json().catch(() => null) }
}

await main(async () => {
  const admin = await adminAccount('notify_admin')
  const member = await account('notify_member')
  const T = { token: admin.token }

  // The member's address must be confirmed: CS:S mails only confirmed ones.
  const confirm = await waitFor((m) => to(member.email)(m) && /[?&]token=/.test(plain(m)))
  const vtoken = confirm ? plain(confirm).match(/[?&]token=([0-9a-f]+)/)?.[1] : null
  if (vtoken) await POST('/api/auth/email/verify', { body: { token: vtoken } })
  ok('notify/the-member-s-address-is-confirmed', !!vtoken, 'no confirmation mail reached the member')

  // ---- an app and its own token ---------------------------------------------------
  const appName = `Relay ${RUN_TAG}`
  const app = (await POST('/api/admin/oauth/clients', {
    ...T,
    body: { name: appName, redirect_uris: [], allowed_scopes: ['openid', 'notify:members'] },
  })).json?.data
  const APP = { id: app.client.client_id, secret: app.client_secret }
  const tok = await form('/api/oauth/token', { grant_type: 'client_credentials', scope: 'notify:members' }, APP.id, APP.secret)
  assertEq('notify/the-app-gets-a-token', 200, tok.status, JSON.stringify(tok.json).slice(0, 200))
  const A = { token: tok.json?.access_token }

  assertEq('notify/no-token-is-401', 401, (await GET('/api/notify/kinds')).status)
  assertEq('notify/a-member-session-is-not-an-app-token', 401, (await GET('/api/notify/kinds', { token: member.token })).status)

  // ---- kinds ------------------------------------------------------------------------
  const badKind = await PUT('/api/notify/kinds', { ...A, body: { kinds: [{ kind: 'Print Done', label: 'x' }] } })
  assertEq('notify/a-malformed-kind-is-refused', 400, badKind.status)
  const twice = await PUT('/api/notify/kinds', {
    ...A,
    body: { kinds: [{ kind: 'print.done', label: 'a' }, { kind: 'print.done', label: 'b' }] },
  })
  assertEq('notify/a-kind-declared-twice-is-refused', 400, twice.status)
  const declared = await PUT('/api/notify/kinds', {
    ...A,
    body: {
      kinds: [
        { kind: 'print.done', label: 'Your print is done' },
        { kind: 'queue.your_turn', label: 'It is your turn for a printer' },
      ],
    },
  })
  assertEq('notify/kinds-are-declared', 200, declared.status, declared.text.slice(0, 200))
  assertEq('notify/both-kinds-are-held', 2, (declared.json?.data ?? []).length)

  const send = (sub, body) => POST(`/api/notify/members/${sub}`, { ...A, body })
  const done = (extra = {}) => ({ kind: 'print.done', title: 'Your print is done', body: 'Collect it from the X1.', ...extra })

  assertEq('notify/an-undeclared-kind-is-refused', 400, (await send(member.user.id, done({ kind: 'secret.thing' }))).status)
  assertEq('notify/an-unknown-member-is-404', 404,
    (await send('00000000-0000-4000-8000-00000000beef', done())).status)
  assertEq('notify/a-title-with-a-line-break-is-refused', 400,
    (await send(member.user.id, done({ title: 'Done\r\nBcc: someone@e2e.invalid' }))).status)
  assertEq('notify/a-script-link-is-refused', 400,
    (await send(member.user.id, done({ link: 'javascript:alert(1)' }))).status)

  // ---- with no choice made: high reaches them, normal waits ---------------------
  const highTitle = `High ${RUN_TAG}`
  const high = await send(member.user.id, done({ title: highTitle, urgency: 'high', link: 'https://pop.e2e.invalid/jobs/1' }))
  assertEq('notify/a-message-is-accepted', 202, high.status, high.text.slice(0, 200))
  const highMail = await waitFor((m) => to(member.email)(m) && subject(`[${appName}] ${highTitle}`)(m))
  ok('notify/a-high-urgency-message-reaches-a-member-who-has-not-chosen', !!highMail, 'no mail arrived')
  ok('notify/the-mail-carries-the-body-and-link',
    !!highMail && plain(highMail).includes('Collect it from the X1.') && plain(highMail).includes('https://pop.e2e.invalid/jobs/1'))
  ok('notify/the-mail-says-how-to-change-it', !!highMail && plain(highMail).includes('/profile/notifications'))

  const normalTitle = `Normal ${RUN_TAG}`
  const normal = await send(member.user.id, done({ title: normalTitle }))
  assertEq('notify/a-normal-message-is-accepted-the-same-way', 202, normal.status)
  ok('notify/the-two-answers-have-the-same-shape',
    JSON.stringify(Object.keys(high.json?.data ?? {}).sort()) === JSON.stringify(Object.keys(normal.json?.data ?? {}).sort()) &&
      high.json?.data?.accepted === true && normal.json?.data?.accepted === true,
    `${high.text} / ${normal.text}`)
  await new Promise((r) => setTimeout(r, 2500))
  assertEq('notify/a-normal-message-does-not-reach-a-member-who-has-not-chosen', 0,
    countOf((m) => to(member.email)(m) && subject(`[${appName}] ${normalTitle}`)(m)))

  // ---- the member's choices ---------------------------------------------------------
  const prefs = await GET('/api/notify/preferences', { token: member.token })
  const mine = (prefs.json?.data ?? []).find((a) => a.client === app.client.id)
  ok('notify/the-member-sees-the-app-and-its-kinds', mine?.app === appName && (mine?.kinds ?? []).length === 2,
    JSON.stringify(prefs.json).slice(0, 300))
  const optIn = await PUT(`/api/notify/preferences/${app.client.id}`, { token: member.token, body: { kinds: { 'print.done': true } } })
  assertEq('notify/a-member-can-opt-in-to-a-kind', 200, optIn.status, optIn.text.slice(0, 200))
  const optedTitle = `Opted ${RUN_TAG}`
  await send(member.user.id, done({ title: optedTitle }))
  ok('notify/an-opted-in-kind-reaches-them',
    !!(await waitFor((m) => to(member.email)(m) && subject(`[${appName}] ${optedTitle}`)(m))))

  const off = await PUT(`/api/notify/preferences/${app.client.id}`, { token: member.token, body: { enabled: false } })
  assertEq('notify/a-member-can-switch-an-app-off', 200, off.status)
  const offTitle = `Off ${RUN_TAG}`
  const afterOff = await send(member.user.id, done({ title: offTitle, urgency: 'high' }))
  assertEq('notify/a-message-to-a-member-who-switched-it-off-is-still-202', 202, afterOff.status)
  ok('notify/and-its-answer-is-the-same-shape',
    JSON.stringify(Object.keys(afterOff.json?.data ?? {}).sort()) === JSON.stringify(Object.keys(high.json?.data ?? {}).sort()))
  await new Promise((r) => setTimeout(r, 2500))
  assertEq('notify/nothing-reaches-a-member-who-switched-the-app-off', 0,
    countOf((m) => to(member.email)(m) && subject(`[${appName}] ${offTitle}`)(m)))
  await PUT(`/api/notify/preferences/${app.client.id}`, { token: member.token, body: { enabled: true } })

  // ---- dedupe and the rate limit (6 an hour per member, in the stack config) ------
  const dupTitle = `Dup ${RUN_TAG}`
  const d1 = await send(member.user.id, done({ title: dupTitle, dedupe_key: `job-${RUN_TAG}` }))
  const d2 = await send(member.user.id, done({ title: dupTitle, dedupe_key: `job-${RUN_TAG}` }))
  assertEq('notify/a-repeated-dedupe-key-returns-the-first-id', d1.json?.data?.id, d2.json?.data?.id)
  await waitFor((m) => to(member.email)(m) && subject(`[${appName}] ${dupTitle}`)(m))
  await new Promise((r) => setTimeout(r, 2000))
  assertEq('notify/a-repeated-dedupe-key-sends-once', 1,
    countOf((m) => to(member.email)(m) && subject(`[${appName}] ${dupTitle}`)(m)))
  // Five accepted so far: high, normal, opted, off, and the dedupe pair's one
  // (the repeat added nothing). So the sixth is accepted and the seventh is
  // refused.
  const sixth = await send(member.user.id, done({ title: `Sixth ${RUN_TAG}` }))
  assertEq('notify/the-sixth-message-in-an-hour-is-accepted', 202, sixth.status, sixth.text.slice(0, 200))
  const seventh = await send(member.user.id, done({ title: `Seventh ${RUN_TAG}` }))
  assertEq('notify/the-seventh-message-in-an-hour-is-refused', 429, seventh.status, seventh.text.slice(0, 200))
  const retry = await send(member.user.id, done({ title: dupTitle, dedupe_key: `job-${RUN_TAG}` }))
  assertEq('notify/a-retry-of-a-sent-message-is-not-rate-limited', 202, retry.status)

  // ---- the record ----------------------------------------------------------------------
  const audit = (await GET('/api/admin/audit-logs?event_type=member_notified&per_page=100', T)).json?.data ?? []
  const forMember = audit.filter((l) => l.user_id === member.user.id && l.event_data?.client_id === APP.id)
  ok('notify/each-message-is-audited', forMember.length >= 5, `${forMember.length} audit rows`)
  ok('notify/the-record-says-what-became-of-it',
    forMember.some((l) => l.event_data?.outcome === 'delivered') && forMember.some((l) => l.event_data?.outcome === 'suppressed'),
    JSON.stringify(forMember.map((l) => l.event_data?.outcome)))
  ok('notify/the-record-holds-no-address', !JSON.stringify(forMember).includes(member.email), 'an address reached the audit log')

  // ---- an app that is disabled stops at once ------------------------------------------
  await PATCH(`/api/admin/oauth/clients/${app.client.id}`, { ...T, body: { disabled: true } })
  assertEq('notify/a-disabled-app-cannot-send', 401, (await send(member.user.id, done())).status)
})
