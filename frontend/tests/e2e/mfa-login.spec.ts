// Tier 5: the two-step login, in a real browser against the fake.
//
// The claim this file exists to make is the one nothing else could make. Tier 1
// proves a TOTP code verifies against a secret. Tier 2 proves `LoginView` sends
// what it should and renders what it gets. Tier 4 proves `/auth/mfa/verify` is
// reachable without a credential. None of them can answer the question a member
// standing at the login form actually has: **does a password alone get you in?**
//
// That needs a real navigation, a real router guard, real localStorage and a
// real page reload -- because the ways it could go wrong are a token written to
// storage before the second factor is judged, or a guard that lets a
// half-authenticated store through, and both are invisible to a mounted
// component.
//
// WHAT THIS TIER DOES NOT DECIDE. Whether six digits are the *right* six digits
// is not asked here: `tests/fake/world.ts` accepts a constant, and the reason it
// is allowed to is written there. Skew, digit count and wrong-secret rejection
// are settled in `server/src/mfa.rs`; the same flow against a real HMAC and a
// real database is the stack battery's `mfa` stage. What is settled here is the
// protocol around the code.

import { expect, test } from '@playwright/test'

import { PASSWORD, reset } from './fake'

/** The user in the fake world who has a second factor. */
const ENROLLED = 'hedy'
/** The code `tests/fake/world.ts` accepts. */
const GOOD_CODE = '123456'
const GOOD_RECOVERY = 'ABCD-EFGH-JKLM'

const APP_SHELL = 'nav.navbar'

test.beforeEach(async ({ request }) => {
  await reset(request)
})

/** Submit the password step and stop wherever it lands. */
async function submitPassword(page: import('@playwright/test').Page, who = ENROLLED) {
  await page.goto('/login')
  await page
    .getByLabel(/username|email/i)
    .first()
    .fill(who)
  await page
    .getByLabel(/password/i)
    .first()
    .fill(PASSWORD)
  await page
    .getByRole('button', { name: /sign in|log ?in/i })
    .first()
    .click()
}

const storedToken = (page: import('@playwright/test').Page) =>
  page.evaluate(() => window.localStorage.getItem('css_token'))

test.describe('a password is not enough on its own', () => {
  test('stops at the challenge and leaves the browser unauthenticated', async ({ page }) => {
    // The headline, and every assertion in it is negative. A regression that
    // authenticated first and challenged second would still show this screen.
    await submitPassword(page)

    await expect(page.getByText(/two-factor verification/i)).toBeVisible()
    expect(new URL(page.url()).pathname, 'the challenge navigated away').toBe('/login')
    expect(await storedToken(page), 'a token was persisted before the second factor').toBeNull()
    await expect(
      page.getByLabel(/password/i),
      'the password form is still on screen behind the challenge'
    ).toHaveCount(0)
  })

  test('a reload during the challenge lands back on the login form, not inside', async ({
    page,
  }) => {
    // The test the store cannot run. If anything had been written to
    // localStorage, `initialize()` would restore a session on reload and the
    // guard would wave the user through -- and the only place that is visible
    // is a browser that actually reloads.
    await submitPassword(page)
    await expect(page.getByText(/two-factor verification/i)).toBeVisible()

    await page.reload()

    await expect(page.getByLabel(/password/i).first()).toBeVisible()
    expect(await storedToken(page)).toBeNull()
  })

  test('a protected page is still refused while a challenge is outstanding', async ({ page }) => {
    await submitPassword(page)
    await expect(page.getByText(/two-factor verification/i)).toBeVisible()

    await page.goto('/profile')

    await expect(page).toHaveURL(/\/login/)
  })

  test('offers only the methods the account has', async ({ page }) => {
    await submitPassword(page)
    await expect(page.getByRole('tab', { name: /code/i })).toBeVisible()
    await expect(page.getByRole('tab', { name: /recovery/i })).toBeVisible()
    await expect(page.getByRole('tab', { name: /security key/i })).toHaveCount(0)
  })
})

test.describe('completing the second factor', () => {
  test('a correct code signs the user in', async ({ page }) => {
    await submitPassword(page)
    await page.getByLabel(/6-digit code/i).fill(GOOD_CODE)
    await page.getByRole('button', { name: /^verify$/i }).click()

    await page.waitForURL((url) => !url.pathname.startsWith('/login'), { timeout: 10_000 })
    await expect(page.locator(APP_SHELL)).toBeVisible()
    expect(await storedToken(page)).not.toBeNull()
  })

  test('the session survives a reload, which is what makes it a session', async ({ page }) => {
    await submitPassword(page)
    await page.getByLabel(/6-digit code/i).fill(GOOD_CODE)
    await page.getByRole('button', { name: /^verify$/i }).click()
    await page.waitForURL((url) => !url.pathname.startsWith('/login'), { timeout: 10_000 })

    await page.reload()

    await expect(page.locator(APP_SHELL)).toBeVisible()
    await expect(page).not.toHaveURL(/\/login/)
  })

  test('a recovery code works too, and is spent by using it', async ({ page }) => {
    await submitPassword(page)
    await page.getByRole('tab', { name: /recovery/i }).click()
    await page.getByLabel(/recovery code/i).fill(GOOD_RECOVERY)
    await page.getByRole('button', { name: /^verify$/i }).click()
    await page.waitForURL((url) => !url.pathname.startsWith('/login'), { timeout: 10_000 })

    // Spent: signing out and presenting the same code again must fail. A
    // recovery code that survived its own use would be a permanent password.
    await page.evaluate(() => window.localStorage.clear())
    await submitPassword(page)
    await page.getByRole('tab', { name: /recovery/i }).click()
    await page.getByLabel(/recovery code/i).fill(GOOD_RECOVERY)
    await page.getByRole('button', { name: /^verify$/i }).click()

    await expect(page.getByText(/invalid recovery code/i)).toBeVisible()
    expect(await storedToken(page)).toBeNull()
  })

  test('abandoning the challenge returns to the password form', async ({ page }) => {
    await submitPassword(page)
    await page.getByRole('button', { name: /use a different account/i }).click()

    await expect(page.getByLabel(/password/i).first()).toBeVisible()
    expect(await storedToken(page)).toBeNull()
  })
})

test.describe('a wrong code', () => {
  test('is refused, and does not sign anybody in', async ({ page }) => {
    await submitPassword(page)
    await page.getByLabel(/6-digit code/i).fill('000000')
    await page.getByRole('button', { name: /^verify$/i }).click()

    await expect(page.getByText(/invalid totp code/i)).toBeVisible()
    expect(new URL(page.url()).pathname).toBe('/login')
    expect(await storedToken(page)).toBeNull()
  })

  test('leaves the user on a form whose token is already dead -- a pinned finding', async ({
    page,
  }) => {
    // PINNED FINDING, not a passing behavior.
    //
    // `verify_login` consumes the challenge with `take_login` at the *top* of
    // the handler, before it looks at the code. Burning it is the right call --
    // it is what stops an attacker with a captured challenge_token grinding
    // through a million six-digit codes, and there is no other rate limit on
    // /verify.
    //
    // The defect is that the frontend does not reflect it. `LoginView` shows
    // the error and leaves `pendingMfa` set, so the user is looking at a form
    // with a cursor in it and a token that has already been destroyed. Their
    // second attempt -- the natural response to "Invalid TOTP code" -- fails
    // with "Unknown or expired challenge_token", which is both a different
    // message and a misleading one: nothing expired, and the user has no way to
    // learn that a single typo cost them the whole login.
    //
    // The fix is a product decision rather than a typo, which is why this is
    // pinned rather than repaired: either the view clears `pendingMfa` on a
    // rejected code and says "please sign in again", or the server allows a
    // bounded number of attempts against one challenge. The first is honest
    // about what happened; the second is kinder and costs a rate limiter.
    await submitPassword(page)
    await page.getByLabel(/6-digit code/i).fill('000000')
    await page.getByRole('button', { name: /^verify$/i }).click()
    await expect(page.getByText(/invalid totp code/i)).toBeVisible()

    // The retry any user would make, now with the correct code.
    await page.getByLabel(/6-digit code/i).fill(GOOD_CODE)
    await page.getByRole('button', { name: /^verify$/i }).click()

    await expect(
      page.getByText(/unknown or expired challenge/i),
      'the challenge survived a failed attempt; if that was fixed deliberately, ' +
        'delete this pin -- but check that /verify gained a rate limit first'
    ).toBeVisible()
    expect(
      await storedToken(page),
      'the correct code was accepted on a challenge that had already been spent'
    ).toBeNull()
  })
})
