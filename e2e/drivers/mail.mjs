// Tier 6: transactional email, over a real SMTP conversation.
//
// This is the only tier that can establish that the server *speaks SMTP*.
// Everything below it can prove a message was constructed -- the mailer's unit
// tests assert the From header and the transport selection -- and none of it
// can prove a byte ever left the process. The issue this was written for is the
// reminder: `[email]` sat fully configured, documented, validated on every
// boot, and the deployed binary contained zero occurrences of EHLO or STARTTLS.
//
// The destination is css-smtp-sink, an in-repo binary started by stack.sh, and
// the assertions read what arrived in `$CSS_STACK_DIR/mail` rather than what
// the server said it sent.
//
// Two properties here cannot be asserted at any cheaper tier:
//
//   * the reset request answers byte-identically for a real and a made-up
//     address. Structure tests pin the shape of the handler; only this can
//     compare two live responses.
//   * a token works exactly once. The claim is a single filtered UPDATE, which
//     reads correctly and would also read correctly if it were wrong.
//
// What this does NOT prove: delivery. The sink accepts everything, so this
// establishes that a well-formed message was handed over -- not that a real
// relay would take it, and not that it would survive SPF or DKIM alignment.

import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

import { assertEq, main, ok, record, POST, RUN_TAG, PASSWORD, account, login } from './lib.mjs'

const MAILDIR = join(process.env.CSS_STACK_DIR ?? '/stack', 'mail')

/** Every message the sink has stored, newest last. */
function messages() {
  let names
  try {
    names = readdirSync(MAILDIR).filter((n) => n.endsWith('.eml'))
  } catch {
    return []
  }
  return names
    .sort()
    .map((n) => ({ name: n, text: readFileSync(join(MAILDIR, n), 'utf8') }))
}

/**
 * Wait for a message matching `predicate`.
 *
 * The server awaits the final 250 before answering the HTTP request, so by the
 * time a caller gets here the file is written -- but the sink renames it into
 * place from a separate thread, so a short poll is the difference between a
 * reliable assertion and an occasional one.
 */
async function waitForMessage(predicate, timeoutMs = 5000) {
  const deadline = Date.now() + timeoutMs
  for (;;) {
    const found = messages().filter(predicate)
    if (found.length > 0) return found[found.length - 1]
    if (Date.now() > deadline) return null
    await new Promise((r) => setTimeout(r, 100))
  }
}

const subjectIs = (subject) => (m) => m.text.includes(`Subject: ${subject}`)
const addressedTo = (addr) => (m) => m.text.includes(`X-Sink-Rcpt-To: <${addr}>`)

/**
 * Undo quoted-printable, the way a mail client does.
 *
 * lettre sends the body `Content-Transfer-Encoding: quoted-printable`, which
 * does two things that matter here: it encodes `=` as `=3D`, so `?token=`
 * arrives as `?token=3D`, and it soft-wraps at 76 columns with a trailing `=`,
 * which lands in the middle of a 64-character token.
 *
 * The sink stores what arrived rather than decoding it, deliberately -- a test
 * double that quietly normalizes its input cannot show you what the server
 * actually put on the wire. Decoding belongs here, where a mail client would do
 * it.
 *
 * This was a defect in the first version of this driver, and it cost three
 * failing cases that all looked like a broken server: the raw text was scanned
 * for a token, `3Da4af97b292910fb85effeb74c9565f` came back -- the `=3D` prefix
 * plus everything up to the soft break -- and the server correctly refused it.
 */
function decodeQuotedPrintable(text) {
  return text
    .replace(/=\r?\n/g, '')
    .replace(/=([0-9A-Fa-f]{2})/g, (_, hex) => String.fromCharCode(parseInt(hex, 16)))
}

/** The reset or confirmation token out of a link in a message body. */
function tokenIn(text) {
  const match = decodeQuotedPrintable(text).match(/[?&]token=([0-9a-f]+)/)
  return match ? match[1] : null
}

await main(async () => {
  // ----------------------------------------------------------------------
  // Registration sends a confirmation, whether or not it is required
  // ----------------------------------------------------------------------
  const user = await account('mail')

  const welcome = await waitForMessage(
    (m) => addressedTo(user.email)(m) && subjectIs('Confirm your email address')(m)
  )
  ok('mail/registration-sends-confirmation', welcome !== null, `nothing arrived for ${user.email}`)

  if (welcome) {
    // The envelope, not just the header. They are different things and can
    // disagree; a test that only reads `From:` cannot tell whether
    // `from_email` reached the envelope at all.
    ok(
      'mail/envelope-sender-is-configured',
      welcome.text.includes('X-Sink-Mail-From: <noreply@example.invalid>'),
      `envelope sender was not the configured from_email:\n${welcome.text.slice(0, 300)}`
    )
    ok(
      'mail/from-header-carries-display-name',
      /^From: .*CSS E2E.*<noreply@example\.invalid>/m.test(welcome.text),
      `From header did not carry from_name and from_email:\n${welcome.text.slice(0, 300)}`
    )
  }

  // ----------------------------------------------------------------------
  // The request endpoint says the same thing either way
  // ----------------------------------------------------------------------
  const known = await POST('/api/auth/password-reset/request', { body: { email: user.email } })
  assertEq('mail/reset-request-accepted', 200, known.status)

  const unknown = await POST('/api/auth/password-reset/request', {
    body: { email: `nobody_${RUN_TAG}@e2e.invalid` },
  })

  assertEq('mail/unknown-address-same-status', known.status, unknown.status)
  assertEq(
    'mail/unknown-address-same-body',
    known.text,
    unknown.text,
    'the reset request must not reveal whether an address has an account. Any ' +
      'difference at all -- a word, a field, a status -- is an enumeration oracle.'
  )

  const nothingForNobody = await waitForMessage(addressedTo(`nobody_${RUN_TAG}@e2e.invalid`), 1500)
  ok(
    'mail/nothing-sent-to-an-unknown-address',
    nothingForNobody === null,
    'a message was sent to an address with no account'
  )

  // ----------------------------------------------------------------------
  // The link works, once
  // ----------------------------------------------------------------------
  const resetMail = await waitForMessage(
    (m) => addressedTo(user.email)(m) && subjectIs('Reset your password')(m)
  )
  ok('mail/reset-link-arrives', resetMail !== null, `no reset message for ${user.email}`)

  const token = resetMail ? tokenIn(resetMail.text) : null

  // Exactly 64 lowercase hex, not "at least 32".
  //
  // The loose version of this assertion is why the first run of this stage
  // reported three failures instead of one: a quoted-printable soft break cut
  // the token in half, the truncation was 32 characters, `>= 32` accepted it,
  // and the real cause only became visible three assertions later as a 400 from
  // the server. An assertion that passes on a corrupted value defers the
  // diagnosis to whatever fails next.
  ok(
    'mail/reset-link-carries-a-whole-token',
    typeof token === 'string' && /^[0-9a-f]{64}$/.test(token),
    `expected 64 hex characters, got ${token === null ? 'no match' : `${token.length}: ${token}`}`
  )

  if (token) {
    const NEW_PASSWORD = 'e2e-reset-password-5678'

    const consumed = await POST('/api/auth/password-reset/consume', {
      body: { token, new_password: NEW_PASSWORD },
    })
    assertEq('mail/reset-consumed', 200, consumed.status)

    const stale = await login(user.username, PASSWORD)
    assertEq('mail/old-password-rejected', 401, stale.status)

    const fresh = await login(user.username, NEW_PASSWORD)
    assertEq('mail/new-password-accepted', 200, fresh.status)

    // No token in the consume response, deliberately: a reset that issued a
    // session would let anyone holding the mailbox past MFA without presenting
    // a second factor.
    ok(
      'mail/consume-issues-no-session',
      consumed.json?.data?.token === undefined,
      'the reset response carried a token, which would make a mailbox a full ' +
        'MFA bypass'
    )

    // The single most important assertion in this file. The claim is one
    // filtered UPDATE, which reads correctly whether or not it is correct.
    //
    // Guarded on the first consume having actually succeeded, because otherwise
    // it is vacuous -- and it *was* vacuous on this stage's first run. A
    // corrupted token made the first consume answer 400, the replay answered
    // 400 for the same uninteresting reason, and this passed while proving
    // nothing about single use at all. A test whose premise failed must say so
    // rather than report the conclusion it never reached.
    if (consumed.status !== 200) {
      record(
        'mail/token-works-exactly-once',
        'skip',
        `the first consume answered ${consumed.status}, so a second 400 would ` +
          'prove nothing about single use'
      )
    } else {
      const replay = await POST('/api/auth/password-reset/consume', {
        body: { token, new_password: 'e2e-should-not-work-9999' },
      })
      assertEq(
        'mail/token-works-exactly-once',
        400,
        replay.status,
        'a spent reset token was accepted a second time'
      )
      ok('mail/replay-is-not-a-401', replay.status !== 401)
    }
  }

  // ----------------------------------------------------------------------
  // The throttle is not itself an oracle
  // ----------------------------------------------------------------------
  //
  // Four rapid requests trip a limit of three. If the throttle only recorded
  // attempts for addresses that exist, a 429 would mean "this address is
  // registered" -- the same disclosure the uniform response is there to
  // prevent, arriving by another route.
  const burst = async (address) => {
    let last = null
    for (let i = 0; i < 4; i++) {
      last = await POST('/api/auth/password-reset/request', { body: { email: address } })
    }
    return last
  }

  const real = await account('mailthrottle')
  assertEq('mail/throttled-for-a-real-address', 429, (await burst(real.email)).status)
  assertEq(
    'mail/throttled-for-an-unknown-address',
    429,
    (await burst(`ghost_${RUN_TAG}@e2e.invalid`)).status,
    'the throttle must fire for an address with no account too, or a 429 says ' +
      'the address is registered'
  )
})
