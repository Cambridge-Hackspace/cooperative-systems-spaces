// Tier 10: the two WebAuthn ceremonies, completed for real.
//
// This is the branch TESTING.md named as the one thing nothing covered.
// `finish_passkey_registration` and `finish_passkey_authentication` need a
// signed attestation and a signed assertion, and no other tier can produce
// one: the unit tests reach `start_passkey_*` and stop, because those are pure
// and the finishes are not; the stack driver asserts every refusal *around*
// the signature; and Tier 5's fake never checks a signature at all.
//
// A virtual authenticator closes it. Chromium implements one behind the
// DevTools Protocol's `WebAuthn` domain -- a real software authenticator with
// a real P-256 key, producing real CTAP2 responses through the browser's own
// WebAuthn stack. Nothing here is stubbed: the page calls
// `navigator.credentials.create()` and `.get()` exactly as it does on a
// member's laptop, and `webauthn-rs` verifies the results exactly as it does
// in production. What is simulated is the hardware, and only the hardware.
//
// WHY THE PAGE IS LOADED FROM `localhost` AND NOT 127.0.0.1, which every other
// test in this directory uses. The browser refuses an rp_id that is not a
// registrable suffix of the page's own domain, and it refuses it *before* the
// server sees anything -- so the ceremony cannot be run at all from an origin
// that disagrees with the relying party. The stack config sets
// `relying_party_id = "localhost"` because `WebauthnBuilder::new` validates the
// rp_id through `Url::domain()`, which is None for an IP literal. So the two
// constraints meet at exactly one value, and `CSS_RP_ORIGIN` carries it here
// from the same place `e2e/run.sh` gets the port. The first test asserts the
// application is actually reachable there, so a name-resolution problem in the
// container names itself instead of surfacing as a WebAuthn error.
//
// The watchdog from audit.spec.ts is deliberately duplicated rather than
// shared: these tests exercise a path where a 5xx is a real possibility (a
// rejected attestation that reached the 500 class rather than a 400), and a
// tier whose oracle lives in another file is a tier whose oracle can be
// removed from another file.

import { expect, test, type Page } from '@playwright/test'

/**
 * The origin the relying party is configured for. Set by `e2e/run.sh` from the
 * same SERVER_PORT it writes into the stack config; the default matches the
 * default there so a hand-run works too.
 */
const RP_ORIGIN = process.env.CSS_RP_ORIGIN ?? 'http://localhost:4399'
const PASSWORD = 'e2e-password-1234'

const TAG = `pk${Date.now().toString(36)}${Math.floor(Math.random() * 1e4)}`

type Seen = { serverErrors: string[]; pageErrors: string[] }
const seen = new WeakMap<Page, Seen>()

function watch(page: Page) {
  const s: Seen = { serverErrors: [], pageErrors: [] }
  seen.set(page, s)
  page.on('response', (res) => {
    if (res.status() >= 500) {
      s.serverErrors.push(`${res.status()} ${res.request().method()} ${res.url()}`)
    }
  })
  page.on('pageerror', (err) => s.pageErrors.push(String(err)))
  return s
}

test.afterEach(({ page }) => {
  const s = seen.get(page)
  if (!s) return
  expect(
    s.serverErrors,
    'the browser received a 5xx during a passkey ceremony. A malformed or ' +
      'rejected credential must be a 4xx: the client sent something the ' +
      'server did not like, which is not the server failing.'
  ).toEqual([])
  expect(s.pageErrors, 'an uncaught exception reached the page').toEqual([])
})

/**
 * Attach a virtual authenticator to this page's browser context.
 *
 * `hasUserVerification` and `isUserVerified` are both required, and not
 * cosmetic: `start_passkey_registration` and `start_passkey_authentication`
 * both set `UserVerificationPolicy::Required`, so an authenticator that does
 * not assert UV produces a credential the server correctly refuses -- and the
 * test would then be asserting the refusal path while claiming to cover the
 * success path.
 *
 * `hasResidentKey: false` matches `require_resident_key(false)` on the server.
 */
async function attachAuthenticator(page: Page) {
  const client = await page.context().newCDPSession(page)
  await client.send('WebAuthn.enable')
  const { authenticatorId } = await client.send('WebAuthn.addVirtualAuthenticator', {
    options: {
      protocol: 'ctap2',
      transport: 'internal',
      hasResidentKey: false,
      hasUserVerification: true,
      isUserVerified: true,
      automaticPresenceSimulation: true,
    },
  })
  return { client, authenticatorId }
}

async function registerMember(page: Page, suffix: string) {
  const username = `${TAG}_${suffix}`
  // Through the page's own origin, so the account and the ceremony share one.
  const res = await page.request.post(`${RP_ORIGIN}/api/auth/register`, {
    data: {
      username,
      email: `${username}@e2e.invalid`,
      password: PASSWORD,
      full_name: `Passkey ${suffix}`,
    },
  })
  expect(res.status(), `registering ${username} answered ${res.status()}`).toBeLessThan(300)
  return username
}

async function signInWithPassword(page: Page, username: string) {
  await page.goto(`${RP_ORIGIN}/login`)
  await page
    .getByLabel(/username|email/i)
    .first()
    .fill(username)
  await page
    .getByLabel(/password/i)
    .first()
    .fill(PASSWORD)
  await page
    .getByRole('button', { name: /sign in|log ?in/i })
    .first()
    .click()
}

/** Enroll a passkey through the settings page. Returns the label used. */
async function enrollPasskey(page: Page, label = 'Virtual Key') {
  await page.goto(`${RP_ORIGIN}/profile/mfa`)
  await expect(page.getByRole('heading', { name: /two-factor authentication/i })).toBeVisible()

  await page.getByLabel(/label for this key/i).fill(label)
  await page.getByRole('button', { name: /add security key/i }).click()

  // The row appearing is the whole ceremony having succeeded: the server only
  // returns a credential row after `finish_passkey_registration` verified the
  // attestation and the row was inserted.
  await expect(
    page.getByRole('cell', { name: label }),
    'the enrolled key never appeared, so finish_passkey_registration did not ' +
      'accept the attestation this authenticator produced'
  ).toBeVisible({ timeout: 20_000 })
  return label
}

test.describe('a passkey, end to end', () => {
  test.beforeEach(({ page }) => {
    watch(page)
  })

  test('the application is reachable at the relying-party origin', async ({ page }) => {
    // First, and separate, so that a container that cannot resolve `localhost`
    // to the IPv4 loopback the server binds fails here -- with a message about
    // reachability -- rather than three tests later as an inscrutable WebAuthn
    // error.
    const res = await page.goto(`${RP_ORIGIN}/login`)
    expect(
      res?.status(),
      `${RP_ORIGIN} did not serve the application. The relying party is ` +
        'configured for this origin and the ceremony cannot run from any other, ' +
        'so this is a stack wiring problem, not a WebAuthn one.'
    ).toBeLessThan(400)
    await expect(page.getByLabel(/password/i).first()).toBeVisible()
  })

  test('can be registered, and the server accepts the attestation', async ({ page }, testInfo) => {
    await attachAuthenticator(page)
    const username = await registerMember(page, `reg_${testInfo.project.name}`)
    await signInWithPassword(page, username)
    await page.waitForURL((u) => !u.pathname.startsWith('/login'), { timeout: 20_000 })

    await enrollPasskey(page)

    // Enrolling a factor is what flips the account to MFA-enrolled, which is
    // the state the next test depends on and the one a user actually cares
    // about: from here, a password alone is no longer enough.
    await expect(page.getByText(/security keys/i).first()).toBeVisible()
  })

  test('then gates the login, and the server accepts the assertion', async ({ page }, testInfo) => {
    // The whole point. Everything before this proves a credential can be
    // stored; this proves it is *required* and that signing with it works.
    await attachAuthenticator(page)
    const username = await registerMember(page, `auth_${testInfo.project.name}`)
    await signInWithPassword(page, username)
    await page.waitForURL((u) => !u.pathname.startsWith('/login'), { timeout: 20_000 })
    await enrollPasskey(page)

    // Drop the session and come back with the password only.
    await page.evaluate(() => window.localStorage.clear())
    await signInWithPassword(page, username)

    await expect(
      page.getByText(/two-factor verification/i),
      'a password alone still signed in an account with a passkey enrolled'
    ).toBeVisible({ timeout: 20_000 })
    expect(
      await page.evaluate(() => window.localStorage.getItem('css_token')),
      'a token was persisted before the passkey was presented'
    ).toBeNull()

    // The security key tab is the default when one is enrolled.
    await page.getByRole('button', { name: /use security key/i }).click()

    await page.waitForURL((u) => !u.pathname.startsWith('/login'), { timeout: 20_000 })
    expect(
      await page.evaluate(() => window.localStorage.getItem('css_token')),
      'the ceremony completed but no session was established, so ' +
        'finish_passkey_authentication did not issue a JWT'
    ).not.toBeNull()
  })

  test('a tampered signature is refused by the server, not by the browser', async ({
    page,
  }, testInfo) => {
    // The assertion that actually proves `finish_passkey_authentication`
    // *verifies* rather than merely parses.
    //
    // The `clearCredentials` test below is weaker than it looks, and its own
    // comment says so: with no matching credential the browser rejects the
    // ceremony before a single byte reaches the server, so what it proves is
    // that the browser discriminates. Useful -- an application that signed
    // somebody in anyway would be a real defect -- but it is not evidence
    // about the server.
    //
    // So this one lets the ceremony succeed completely and corrupts the
    // signature in flight. The credential is real, the challenge is real, the
    // authenticator data is untouched, and only the signature is wrong: the
    // exact shape of an attacker replaying a captured assertion against a
    // fresh challenge. It has to be refused, and it has to be refused with a
    // 4xx -- a 500 here would mean a bad signature crashes the verifier, which
    // the watchdog in the afterEach would also catch.
    await attachAuthenticator(page)
    const username = await registerMember(page, `tamper_${testInfo.project.name}`)
    await signInWithPassword(page, username)
    await page.waitForURL((u) => !u.pathname.startsWith('/login'), { timeout: 20_000 })
    await enrollPasskey(page)

    let tampered = false
    await page.route('**/api/auth/mfa/verify', async (route) => {
      const body = route.request().postDataJSON() as {
        response?: { response?: { signature?: string } }
      }
      const inner = body?.response?.response
      const sig = inner?.signature
      if (!inner || typeof sig !== 'string' || sig.length < 8) {
        // Refuse to continue silently. A shape change here would otherwise
        // turn this test into "the login worked", which is the opposite of
        // what it exists to assert.
        throw new Error(
          `expected a base64url signature at response.response.signature, got ` +
            `${JSON.stringify(body).slice(0, 300)}`
        )
      }
      // Flip one character to another base64url character, keeping the length
      // and the alphabet -- so it still decodes, and the request reaches
      // signature verification rather than dying in deserialization.
      const at = Math.floor(sig.length / 2)
      const swapped = sig[at] === 'A' ? 'B' : 'A'
      inner.signature = sig.slice(0, at) + swapped + sig.slice(at + 1)
      tampered = true
      await route.continue({ postData: JSON.stringify(body) })
    })

    await page.evaluate(() => window.localStorage.clear())
    await signInWithPassword(page, username)
    await expect(page.getByText(/two-factor verification/i)).toBeVisible({ timeout: 20_000 })
    await page.getByRole('button', { name: /use security key/i }).click()

    await expect(page.locator('.alert-error')).toBeVisible({ timeout: 20_000 })
    expect(tampered, 'the verify request was never intercepted, so nothing was tampered').toBe(true)
    expect(
      await page.evaluate(() => window.localStorage.getItem('css_token')),
      'the server accepted an assertion whose signature had been altered'
    ).toBeNull()
  })

  test('a key from another account does not open this one', async ({ page }, testInfo) => {
    // The browser's half of the discrimination: an authenticator that holds no
    // credential matching the allow-list cannot produce an assertion at all.
    // This is deliberately NOT the evidence that the server verifies anything
    // -- the rejection happens client-side, before the request is made. The
    // tampered-signature test above is what covers the server.
    const { client, authenticatorId } = await attachAuthenticator(page)

    const mine = await registerMember(page, `own_${testInfo.project.name}`)
    await signInWithPassword(page, mine)
    await page.waitForURL((u) => !u.pathname.startsWith('/login'), { timeout: 20_000 })
    await enrollPasskey(page)

    // Discard the credential this authenticator holds and give it a different
    // one. The account still has a passkey registered server-side, so the
    // challenge is still offered -- but nothing present can satisfy it.
    await client.send('WebAuthn.clearCredentials', { authenticatorId })

    await page.evaluate(() => window.localStorage.clear())
    await signInWithPassword(page, mine)
    await expect(page.getByText(/two-factor verification/i)).toBeVisible({ timeout: 20_000 })

    await page.getByRole('button', { name: /use security key/i }).click()

    // The ceremony fails in the browser (no credential matches the allow-list)
    // or at the server (a signature it cannot verify). Either way the session
    // must not start, and the page must say something rather than hanging.
    await expect(page.getByText(/two-factor verification/i)).toBeVisible()
    expect(
      await page.evaluate(() => window.localStorage.getItem('css_token')),
      'an authenticator holding no registered credential completed the login'
    ).toBeNull()
    await expect(
      page.locator('.alert-error'),
      'the ceremony failed and the page said nothing, leaving the user with a ' +
        'button that appears to do nothing'
    ).toBeVisible({ timeout: 20_000 })
  })
})
