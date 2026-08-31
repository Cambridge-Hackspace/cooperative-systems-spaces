// Tier 6, `mfa`: does a second factor actually gate the JWT?
//
// This is the only place in the repository that can answer that. Every other
// tier stops one step short:
//
//   * Tier 1 (`server/src/mfa.rs`) proves a code verifies against a secret, in
//     memory, with no user and no database.
//   * Tier 2 (`LoginView.spec.ts`) proves the view sends what it should and
//     renders what it gets back, against a double.
//   * Tier 4 (`contract_matrix.rs`) proves the eleven MFA routes refuse an
//     anonymous caller. It cannot prove one accepts a legitimate one, because
//     every route that would reaches the database.
//   * Tier 5 (`mfa-login.spec.ts`) drives a real browser, but against a fake
//     that accepts a constant.
//
// Here there is a real Postgres, a real Argon2, a real HMAC and a real JWT, and
// the account is created and enrolled entirely through the shipping HTTP API.
//
// THE CROSS-IMPLEMENTATION PROPERTY. `totpCode` below is a second, independent
// implementation of RFC 6238, written from the RFC rather than from
// `server/src/mfa.rs`, using `node:crypto` instead of `totp-rs`. When the
// server accepts a code this file generated, two independent implementations
// agreed on the same six digits from the same secret at the same instant --
// which is a materially stronger statement than "the server agreed with
// itself", and it is the one thing the Rust unit tests structurally cannot say.

import { createHmac } from 'node:crypto'

import {
  GET, POST, DELETE,
  account, adminAccount, login,
  assertEq, ok, record, main,
} from './lib.mjs'

// ---------------------------------------------------------------------------
// RFC 6238, from the RFC
// ---------------------------------------------------------------------------

const B32 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'

function base32Decode(text) {
  let bits = 0
  let value = 0
  const out = []
  for (const ch of text.replace(/=+$/, '').toUpperCase()) {
    const idx = B32.indexOf(ch)
    if (idx < 0) throw new Error(`not RFC 4648 base32: ${JSON.stringify(ch)}`)
    value = (value << 5) | idx
    bits += 5
    if (bits >= 8) {
      out.push((value >>> (bits - 8)) & 0xff)
      bits -= 8
    }
  }
  return Buffer.from(out)
}

/** A TOTP code for `secretB32` at `atSeconds`. SHA-1, 6 digits, 30s step. */
function totpCode(secretB32, atSeconds = Math.floor(Date.now() / 1000)) {
  const counter = Math.floor(atSeconds / 30)
  const buf = Buffer.alloc(8)
  buf.writeUInt32BE(Math.floor(counter / 2 ** 32), 0)
  buf.writeUInt32BE(counter >>> 0, 4)
  const mac = createHmac('sha1', base32Decode(secretB32)).update(buf).digest()
  const offset = mac[mac.length - 1] & 0x0f
  const bin =
    ((mac[offset] & 0x7f) << 24) |
    ((mac[offset + 1] & 0xff) << 16) |
    ((mac[offset + 2] & 0xff) << 8) |
    (mac[offset + 3] & 0xff)
  return String(bin % 1_000_000).padStart(6, '0')
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const verify = (body) => POST('/api/auth/mfa/verify', { body })

/** Sign in with a password and return the raw response. */
async function passwordStep(usernameOrEmail) {
  const res = await login(usernameOrEmail)
  if (res.status !== 200) {
    throw new Error(`login ${usernameOrEmail} -> ${res.status}: ${res.text.slice(0, 300)}`)
  }
  return res.json?.data ?? {}
}

/** Password step, asserting it produced a challenge, and return the token. */
async function challengeFor(usernameOrEmail, label) {
  const data = await passwordStep(usernameOrEmail)
  if (data.mfa_required !== true || typeof data.challenge_token !== 'string') {
    throw new Error(`${label}: expected a challenge, got ${JSON.stringify(data).slice(0, 300)}`)
  }
  return data.challenge_token
}

/** Enroll TOTP for a freshly created account. Returns secret + recovery codes. */
async function enrollTotp(token) {
  const setup = await POST('/api/auth/mfa/totp/setup', { token })
  if (setup.status !== 200) {
    throw new Error(`totp/setup -> ${setup.status}: ${setup.text.slice(0, 300)}`)
  }
  const secret = setup.json.data.secret_base32
  const confirm = await POST('/api/auth/mfa/totp/confirm', {
    token,
    body: { code: totpCode(secret) },
  })
  if (confirm.status !== 200) {
    throw new Error(`totp/confirm -> ${confirm.status}: ${confirm.text.slice(0, 300)}`)
  }
  return { secret, setup: setup.json.data, recoveryCodes: confirm.json.data.recovery_codes }
}

/**
 * RFC 6238 Appendix B, the SHA-1 rows, truncated to six digits.
 *
 * The cross-implementation claim in this file's header is only worth anything
 * if this implementation is right, and "the server accepted it" cannot
 * establish that -- a driver whose TOTP was subtly wrong would simply fail
 * every verification and read as a broken server. So the arithmetic is checked
 * against the specification's own vectors first, before the stack is touched,
 * and a failure here names the driver rather than the product.
 *
 * The last row matters more than it looks: T = 20000000000 pushes the counter
 * past 2^32, which is the half of the 8-byte big-endian encoding that a
 * `writeUInt32BE(counter)` on its own silently drops.
 */
const RFC6238_SHA1 = [
  [59, '287082'],
  [1111111109, '081804'],
  [1111111111, '050471'],
  [1234567890, '005924'],
  [2000000000, '279037'],
  [20000000000, '353130'],
]

/** The ASCII secret "12345678901234567890", base32-encoded, from the RFC. */
const RFC6238_SECRET = 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ'

main(async () => {
  // -----------------------------------------------------------------------
  // This driver's own arithmetic, before anything is claimed about the server
  // -----------------------------------------------------------------------
  for (const [at, expected] of RFC6238_SHA1) {
    assertEq(`mfa/rfc6238-vector-at-${at}`, expected, totpCode(RFC6238_SECRET, at))
  }

  // -----------------------------------------------------------------------
  // The feature has to be switched on, or every assertion below is vacuous
  // -----------------------------------------------------------------------
  // `require_enabled` answers 403 for the whole enrollment surface when
  // `[auth.mfa] enabled` is false. A stage that ran against that config would
  // report a tidy row of 403s and prove nothing at all, so this is checked
  // first and loudly.
  const alice = await account('mfa')
  const status0 = await GET('/api/auth/mfa/status', { token: alice.token })
  assertEq('mfa/status-reachable', 200, status0.status)
  if (status0.json?.data?.enabled !== true) {
    record(
      'mfa/is-enabled-server-side',
      'fail',
      'MFA is disabled in the stack config, so every assertion in this stage ' +
        'would pass against a server that does nothing. Set [auth.mfa] enabled ' +
        '= true in e2e/stack-config.toml.',
    )
    return
  }
  record('mfa/is-enabled-server-side', 'ok')

  assertEq('mfa/starts-unenrolled', false, status0.json.data.totp_enrolled)
  assertEq('mfa/starts-with-no-recovery-codes', 0, status0.json.data.recovery_codes_remaining)

  // -----------------------------------------------------------------------
  // Enrollment
  // -----------------------------------------------------------------------
  const setup = await POST('/api/auth/mfa/totp/setup', { token: alice.token })
  assertEq('mfa/setup-succeeds', 200, setup.status)
  const secret = setup.json?.data?.secret_base32
  ok('mfa/setup-returns-a-secret', typeof secret === 'string' && secret.length === 32,
    `secret was ${JSON.stringify(secret)}`)
  ok(
    'mfa/otpauth-uri-carries-the-secret',
    typeof setup.json?.data?.otpauth_uri === 'string' &&
      setup.json.data.otpauth_uri.includes(`secret=${secret}`),
    // Without the secret in the URI the QR code enrolls an authenticator
    // against nothing, and the failure only appears at the confirm step.
    `otpauth_uri was ${JSON.stringify(setup.json?.data?.otpauth_uri)}`,
  )

  const badConfirm = await POST('/api/auth/mfa/totp/confirm', {
    token: alice.token,
    body: { code: '000000' },
  })
  assertEq('mfa/a-wrong-code-does-not-confirm', 400, badConfirm.status)

  const stillOff = await GET('/api/auth/mfa/status', { token: alice.token })
  assertEq('mfa/a-failed-confirm-enrolls-nobody', false, stillOff.json?.data?.totp_enrolled)

  // The cross-implementation moment.
  const confirm = await POST('/api/auth/mfa/totp/confirm', {
    token: alice.token,
    body: { code: totpCode(secret) },
  })
  assertEq('mfa/a-code-this-driver-computed-is-accepted', 200, confirm.status)

  const recovery = confirm.json?.data?.recovery_codes ?? []
  assertEq('mfa/enrollment-issues-recovery-codes', 10, recovery.length)
  ok(
    'mfa/recovery-codes-have-the-documented-shape',
    recovery.every((c) => /^[A-HJ-NP-Z2-9]{4}-[A-HJ-NP-Z2-9]{4}-[A-HJ-NP-Z2-9]{4}$/.test(c)),
    `first code was ${JSON.stringify(recovery[0])}`,
  )

  const enrolled = await GET('/api/auth/mfa/status', { token: alice.token })
  assertEq('mfa/status-reports-enrollment', true, enrolled.json?.data?.totp_enrolled)
  assertEq('mfa/status-counts-recovery-codes', 10, enrolled.json?.data?.recovery_codes_remaining)

  // -----------------------------------------------------------------------
  // THE HEADLINE: a password alone stops issuing a token
  // -----------------------------------------------------------------------
  const afterEnroll = await passwordStep(alice.username)
  assertEq('mfa/password-alone-no-longer-authenticates', true, afterEnroll.mfa_required === true)
  ok(
    'mfa/the-challenge-carries-no-token',
    afterEnroll.token === undefined,
    // The single most important assertion in this file. A regression that
    // issued the JWT *and* the challenge would leave every other test green:
    // the client would route to the challenge form and the session behind it
    // would already be live.
    `the login response carried a token alongside the challenge: ${JSON.stringify(afterEnroll).slice(0, 200)}`,
  )
  ok(
    'mfa/the-challenge-offers-the-enrolled-methods',
    Array.isArray(afterEnroll.methods) &&
      afterEnroll.methods.includes('totp') &&
      afterEnroll.methods.includes('recovery') &&
      !afterEnroll.methods.includes('webauthn'),
    `methods were ${JSON.stringify(afterEnroll.methods)}`,
  )

  // -----------------------------------------------------------------------
  // Verification
  // -----------------------------------------------------------------------
  const good = await verify({
    challenge_token: afterEnroll.challenge_token,
    method: 'totp',
    code: totpCode(secret),
  })
  assertEq('mfa/a-correct-code-completes-the-login', 200, good.status)
  const jwt = good.json?.data?.token
  ok('mfa/verification-issues-a-jwt', typeof jwt === 'string' && jwt.length > 0)

  const me = await GET('/api/auth/me', { token: jwt })
  assertEq('mfa/the-issued-jwt-actually-works', 200, me.status)
  assertEq('mfa/the-jwt-belongs-to-the-right-user', alice.username, me.json?.data?.username)

  // A spent challenge is spent.
  const replay = await verify({
    challenge_token: afterEnroll.challenge_token,
    method: 'totp',
    code: totpCode(secret),
  })
  assertEq('mfa/a-challenge-token-cannot-be-spent-twice', 401, replay.status)

  const unknown = await verify({
    challenge_token: 'not-a-real-challenge-token',
    method: 'totp',
    code: totpCode(secret),
  })
  assertEq('mfa/an-unknown-challenge-token-is-refused', 401, unknown.status)

  // -----------------------------------------------------------------------
  // A wrong code burns the challenge -- pinned
  // -----------------------------------------------------------------------
  const burn = await challengeFor(alice.username, 'wrong-code run')
  const wrong = await verify({ challenge_token: burn, method: 'totp', code: '000000' })
  assertEq('mfa/a-wrong-code-is-refused', 401, wrong.status)

  const retry = await verify({ challenge_token: burn, method: 'totp', code: totpCode(secret) })
  assertEq(
    'mfa/a-wrong-code-destroys-the-challenge',
    401,
    retry.status,
    // FINDING, pinned. Burning the challenge is the correct security choice --
    // it is the only thing standing between a captured challenge_token and an
    // unlimited offline grind through a six-digit space, and `/verify` has no
    // rate limit of its own. What is wrong is that nothing tells the user: the
    // frontend leaves them on the challenge form with a dead token, and their
    // retry fails with "Unknown or expired challenge_token", which is a
    // different message and a misleading one. Recorded in TESTING.md and pinned
    // in tests/e2e/mfa-login.spec.ts from the browser's side.
    'one mistyped digit costs the whole login; if this is now 200, /verify ' +
      'allows repeated attempts against one challenge -- check it gained a rate ' +
      'limit before deleting this pin',
  )

  // -----------------------------------------------------------------------
  // Recovery codes
  // -----------------------------------------------------------------------
  const recChallenge = await challengeFor(alice.username, 'recovery run')
  const usedRecovery = await verify({
    challenge_token: recChallenge,
    method: 'recovery',
    code: recovery[0],
  })
  assertEq('mfa/a-recovery-code-completes-the-login', 200, usedRecovery.status)
  const recJwt = usedRecovery.json?.data?.token

  const afterOne = await GET('/api/auth/mfa/status', { token: recJwt })
  assertEq('mfa/using-a-recovery-code-spends-it', 9, afterOne.json?.data?.recovery_codes_remaining)

  const reuse = await challengeFor(alice.username, 'recovery reuse run')
  const reused = await verify({ challenge_token: reuse, method: 'recovery', code: recovery[0] })
  assertEq(
    'mfa/a-spent-recovery-code-cannot-be-reused',
    401,
    reused.status,
    // A recovery code that survived its own use would be a permanent second
    // password, printed on paper, that nothing rotates.
    'a spent recovery code was accepted a second time',
  )

  const stillNine = await GET('/api/auth/mfa/status', { token: recJwt })
  assertEq(
    'mfa/a-refused-attempt-spends-nothing',
    9,
    stillNine.json?.data?.recovery_codes_remaining,
  )

  // -----------------------------------------------------------------------
  // Cross-account: one member's factor must not open another's account
  // -----------------------------------------------------------------------
  const bob = await account('mfa2')
  const bobEnrollment = await enrollTotp(bob.token)

  const aliceChallenge = await challengeFor(alice.username, 'cross-account run')
  const crossed = await verify({
    challenge_token: aliceChallenge,
    method: 'totp',
    code: totpCode(bobEnrollment.secret),
  })
  assertEq(
    'mfa/another-members-authenticator-does-not-open-this-account',
    401,
    crossed.status,
    'a code from a different user\'s secret completed this user\'s login',
  )

  const crossedRecovery = await challengeFor(alice.username, 'cross-account recovery run')
  const crossedRec = await verify({
    challenge_token: crossedRecovery,
    method: 'recovery',
    code: bobEnrollment.recoveryCodes[0],
  })
  assertEq(
    'mfa/another-members-recovery-code-does-not-open-this-account',
    401,
    crossedRec.status,
  )

  // -----------------------------------------------------------------------
  // Method handling
  // -----------------------------------------------------------------------
  const nonsense = await challengeFor(alice.username, 'unknown method run')
  const badMethod = await verify({ challenge_token: nonsense, method: 'telepathy', code: '1' })
  assertEq('mfa/an-unknown-method-is-a-client-error', 400, badMethod.status)

  const noCode = await challengeFor(alice.username, 'missing code run')
  const missing = await verify({ challenge_token: noCode, method: 'totp' })
  assertEq('mfa/a-missing-code-is-a-client-error', 400, missing.status)

  const noWebauthn = await challengeFor(alice.username, 'webauthn-without-state run')
  const wa = await verify({ challenge_token: noWebauthn, method: 'webauthn', response: {} })
  assertEq(
    'mfa/webauthn-on-a-challenge-with-no-passkey-is-a-client-error',
    400,
    wa.status,
    // Reached only when the account has no passkey, which is exactly the state
    // every account in this suite is in -- so this is the branch a stack with
    // no authenticator can still cover.
    'expected the "no WebAuthn state" refusal',
  )

  // -----------------------------------------------------------------------
  // Regenerating recovery codes retires the old set
  // -----------------------------------------------------------------------
  const regen = await POST('/api/auth/mfa/recovery-codes/regenerate', { token: recJwt })
  assertEq('mfa/recovery-codes-can-be-regenerated', 200, regen.status)
  const fresh = regen.json?.data?.recovery_codes ?? []
  assertEq('mfa/regeneration-issues-a-full-set', 10, fresh.length)

  const afterRegen = await GET('/api/auth/mfa/status', { token: recJwt })
  assertEq(
    'mfa/regeneration-resets-the-remaining-count',
    10,
    afterRegen.json?.data?.recovery_codes_remaining,
  )

  const retired = await challengeFor(alice.username, 'retired-code run')
  const retiredTry = await verify({
    challenge_token: retired,
    method: 'recovery',
    code: recovery[1],
  })
  assertEq(
    'mfa/regenerating-invalidates-the-previous-set',
    401,
    retiredTry.status,
    // The point of the button. A user regenerates because they think the old
    // sheet of paper is compromised; a code from it still working would make
    // the whole action theatre.
    'a code from the retired set still completed a login',
  )

  const freshWorks = await challengeFor(alice.username, 'fresh-code run')
  const freshTry = await verify({
    challenge_token: freshWorks,
    method: 'recovery',
    code: fresh[0],
  })
  assertEq('mfa/a-freshly-issued-recovery-code-works', 200, freshTry.status)

  // -----------------------------------------------------------------------
  // The passkey enrollment ceremony, as far as it can be driven without one
  // -----------------------------------------------------------------------
  // `finish_passkey_registration` needs a signed attestation from a real
  // authenticator, and there is none in any environment this suite runs in --
  // so what is asserted here is everything *around* the signature: the
  // ceremony starts, the options are well formed, the challenge token is
  // single-use, it is bound to the user who started it, and every refusal is a
  // 4xx rather than a 500. TESTING.md §7 names the virtual authenticator that
  // would close the remaining gap.
  const wList = await GET('/api/auth/mfa/webauthn', { token: recJwt })
  assertEq('mfa/webauthn-credentials-can-be-listed', 200, wList.status)
  ok(
    'mfa/a-user-with-no-passkey-has-an-empty-list',
    Array.isArray(wList.json?.data) && wList.json.data.length === 0,
    `list was ${JSON.stringify(wList.json?.data)}`,
  )

  const noLabel = await POST('/api/auth/mfa/webauthn/register/begin', {
    token: recJwt,
    body: { label: '   ' },
  })
  assertEq('mfa/a-passkey-needs-a-label', 400, noLabel.status)

  const begin = await POST('/api/auth/mfa/webauthn/register/begin', {
    token: recJwt,
    body: { label: 'E2E Key' },
  })
  assertEq('mfa/a-passkey-ceremony-starts', 200, begin.status)
  const beginToken = begin.json?.data?.challenge_token
  ok(
    'mfa/the-ceremony-returns-creation-options',
    typeof begin.json?.data?.options?.publicKey?.challenge === 'string' &&
      begin.json.data.options.publicKey.rp?.id === 'localhost',
    // The rp id comes from the config, and getting it wrong does not fail
    // loudly: `WebauthnBuilder::new` refuses an rp_id that is not the effective
    // domain of the origin, `MfaService::new` catches that and sets
    // `webauthn: None`, and every passkey endpoint then answers 403 while the
    // config file still reads as though passkeys were enabled. This assertion
    // is what makes that visible -- it is why the stack config uses `localhost`
    // rather than the 127.0.0.1 every driver connects to.
    `options were ${JSON.stringify(begin.json?.data?.options).slice(0, 300)}`,
  )

  const garbage = await POST('/api/auth/mfa/webauthn/register/finish', {
    token: recJwt,
    body: { challenge_token: beginToken, response: { not: 'a credential' } },
  })
  assertEq(
    'mfa/an-unparseable-attestation-is-a-client-error',
    400,
    garbage.status,
    'a malformed credential must not reach the 500 class',
  )

  const beginReplay = await POST('/api/auth/mfa/webauthn/register/finish', {
    token: recJwt,
    body: { challenge_token: beginToken, response: { not: 'a credential' } },
  })
  assertEq(
    'mfa/a-registration-challenge-is-single-use',
    400,
    beginReplay.status,
    'a registration ceremony survived being finished once',
  )

  const bobBegin = await POST('/api/auth/mfa/webauthn/register/begin', {
    token: bob.token,
    body: { label: 'Bob Key' },
  })
  const stolen = await POST('/api/auth/mfa/webauthn/register/finish', {
    token: recJwt,
    body: { challenge_token: bobBegin.json?.data?.challenge_token, response: {} },
  })
  assertEq(
    'mfa/a-registration-challenge-is-bound-to-the-user-who-started-it',
    403,
    stolen.status,
    // Without the ownership check this would enroll one member's authenticator
    // against another member's account.
    'one user finished another user\'s passkey ceremony',
  )

  const gone = await DELETE(
    '/api/auth/mfa/webauthn/00000000-0000-4000-8000-000000000009',
    { token: recJwt },
  )
  assertEq('mfa/removing-a-credential-that-is-not-there-is-a-404', 404, gone.status)

  // -----------------------------------------------------------------------
  // Admin lockout recovery
  // -----------------------------------------------------------------------
  // The escape hatch for a member who lost their phone. It is the one path that
  // can un-enroll somebody who cannot log in, so it had better work -- and it
  // had better not be reachable by anyone else.
  const carol = await account('mfa3')
  await enrollTotp(carol.token)
  const carolChallenged = await passwordStep(carol.username)
  assertEq('mfa/the-third-account-is-really-enrolled', true, carolChallenged.mfa_required === true)

  const notAdmin = await DELETE(`/api/admin/users/${carol.user.id}/mfa`, { token: alice.token })
  assertEq(
    'mfa/a-member-cannot-reset-another-members-mfa',
    403,
    notAdmin.status,
    'any member could strip a second factor off any account',
  )

  const admin = await adminAccount('mfa_admin')
  const reset = await DELETE(`/api/admin/users/${carol.user.id}/mfa`, { token: admin.token })
  assertEq('mfa/an-admin-can-reset-a-locked-out-member', 200, reset.status)

  const carolAfter = await passwordStep(carol.username)
  ok(
    'mfa/a-reset-member-can-sign-in-with-a-password-again',
    typeof carolAfter.token === 'string' && carolAfter.mfa_required === undefined,
    `login after the reset answered ${JSON.stringify(carolAfter).slice(0, 200)}`,
  )

  const carolStatus = await GET('/api/auth/mfa/status', { token: carolAfter.token })
  assertEq('mfa/a-reset-clears-the-totp-enrollment', false, carolStatus.json?.data?.totp_enrolled)
  assertEq(
    'mfa/a-reset-clears-the-recovery-codes-too',
    0,
    carolStatus.json?.data?.recovery_codes_remaining,
    // Unlike `totp_disable`, which leaves them behind -- see the pinned
    // finding at the end of this file. `reset_user_mfa` is the one that clears
    // everything, which is what makes the difference deliberate rather than an
    // oversight in both places.
    'an admin reset left recovery codes behind',
  )

  // -----------------------------------------------------------------------
  // Turning it off
  // -----------------------------------------------------------------------
  const finalChallenge = await challengeFor(alice.username, 'disable run')
  const finalVerify = await verify({
    challenge_token: finalChallenge,
    method: 'totp',
    code: totpCode(secret),
  })
  const liveToken = finalVerify.json?.data?.token

  const disabled = await DELETE('/api/auth/mfa/totp', { token: liveToken })
  assertEq('mfa/totp-can-be-disabled', 200, disabled.status)

  const afterDisable = await passwordStep(alice.username)
  ok(
    'mfa/disabling-the-last-factor-restores-password-only-login',
    typeof afterDisable.token === 'string' && afterDisable.mfa_required === undefined,
    // The lockout guard. If `recompute_user_mfa_enrolled` ever counted recovery
    // codes as a factor, a user who disabled TOTP would keep being challenged
    // for a code no authenticator can produce any more.
    `login after disabling answered ${JSON.stringify(afterDisable).slice(0, 200)}`,
  )

  const leftovers = await GET('/api/auth/mfa/status', { token: afterDisable.token })
  assertEq(
    'mfa/disabling-totp-leaves-the-recovery-codes-in-the-database',
    9,
    leftovers.json?.data?.recovery_codes_remaining,
    // FINDING, pinned. `totp_disable` deletes the TOTP row and recomputes the
    // enrollment flag, but never touches `user_recovery_codes`. Nine unspent
    // Argon2-hashed codes stay behind for a user who has deliberately turned
    // their second factor off.
    //
    // Not currently exploitable: with no factor enrolled the login never
    // issues a challenge, so there is no endpoint that will accept them, and
    // re-enrolling TOTP calls `replace_user_recovery_codes` and overwrites the
    // set. It is pinned because "unreachable today" is a property of two other
    // functions rather than of this one, and because credential material that
    // outlives the credential it belonged to is worth someone deciding about
    // on purpose. If this is now 0, `totp_disable` learned to clear them --
    // delete the pin.
    'unspent recovery codes survive disabling the factor they belonged to',
  )
})
