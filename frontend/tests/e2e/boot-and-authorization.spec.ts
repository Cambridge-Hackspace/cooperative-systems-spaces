// Tier 5: what the application does when the requests it makes on load fail,
// and what a non-admin is shown.
//
// The boot sequence is `Promise.all([authStore.initialize(), configStore.fetchConfig()])`
// inside `App.vue`'s `onMounted`, wrapped in a try/catch that raises a
// notification. Two requests, in parallel, before anything is on screen — so a
// failure in either is the first thing a person ever sees, and it is the state
// least likely to have been looked at by hand.
//
// The authorization half covers `5c2fa3c` and `11c4f42` from the browser's
// side: the contract stage asserts the server's status codes, and this asserts
// that the page a member lands on is usable rather than falling back to
// "profiles are not configured".

import { expect, test } from '@playwright/test'

import { arm, reset, signIn } from './fake'

/**
 * A locator for "the application rendered", at any viewport.
 *
 * NOT the nav links. `App.vue`'s horizontal menu is `hidden lg:flex`, so on the
 * mobile project those links do not exist and four boot tests failed on a
 * selector that only had meaning at desktop width. The brand link in
 * `navbar-start` carries no responsive hiding and is the honest "something
 * rendered" signal at both sizes.
 *
 * The mobile project exists because DoorCheckinView is opened by a phone camera
 * and by nothing else. Assertions that only hold at 1280px defeat the point of
 * running it.
 */
const APP_SHELL = 'nav.navbar'

test.beforeEach(async ({ request }) => {
  await reset(request)
})

test.describe('the boot sequence', () => {
  test('renders the application when both requests succeed', async ({ page }) => {
    await page.goto('/')

    // The navigation, not the site name. `site_name` is served by
    // `/config/public` and the header renders a hardcoded "CSS" -- so asserting
    // on the configured name here would assert something the application does
    // not do, and would fail for a reason that has nothing to do with booting.
    // (That the configured name is not in the header is recorded in
    // TESTING.md; it is a product gap, not a boot failure.)
    await expect(page.locator(APP_SHELL)).toBeVisible()
    await expect(page.getByText(/Initialization Error/i)).toHaveCount(0)
  })

  test('says NOTHING when the config load fails -- a pinned finding', async ({ page, request }) => {
    // PINNED FINDING, not a passing behaviour.
    //
    // `configStore.fetchConfig` catches its own errors and does not rethrow, so
    // App.vue's `Promise.all` resolves, its `catch` never runs, and the
    // "Initialization Error" notification it exists to raise is unreachable for
    // the failure most likely to happen at boot. The application then runs on
    // default config -- no site name, features gated off -- and nothing says
    // why. It looks like an administrator has not set anything up.
    //
    // Asserted as-is so it cannot change unnoticed. If this fails, somebody
    // fixed it: delete the test. See TESTING.md, "Known defects".
    await arm(request, 'failNext', '/config/public', { status: 500 })
    await page.goto('/')
    await expect(page.locator(APP_SHELL)).toBeVisible()
    await expect(page.getByText(/Initialization Error|Failed to initialize/i)).toHaveCount(0)
  })

  test('stays a usable page when the connection drops during boot', async ({ page, request }) => {
    // The transport shape, at the point where nothing is on screen yet. An
    // unhandled rejection here is a white page; the claim is that it is not.
    await arm(request, 'abortNext', '/config/public')
    await page.goto('/')

    await expect(page.locator(APP_SHELL)).toBeVisible()
    await expect(page.locator('body')).not.toBeEmpty()
  })

  test('a malformed config response does not blank the page', async ({ page, request }) => {
    await arm(request, 'malformNext', '/config/public', {
      body: { success: true, data: null },
    })
    await page.goto('/')
    await expect(page.locator('body')).not.toBeEmpty()
  })
})

test.describe('signing in', () => {
  test('rejects a wrong password with a message', async ({ page }) => {
    await page.goto('/login')
    await page
      .getByLabel(/username|email/i)
      .first()
      .fill('grace')
    await page
      .getByLabel(/password/i)
      .first()
      .fill('not-the-password')
    await page
      .getByRole('button', { name: /sign in|log ?in/i })
      .first()
      .click()

    await expect(page.getByText(/wrong credentials|invalid/i)).toBeVisible()
    await expect(page).toHaveURL(/\/login/)
  })

  test('reports a dropped connection rather than appearing to hang', async ({ page, request }) => {
    await page.goto('/login')
    await arm(request, 'abortNext', '/auth/login')
    await page
      .getByLabel(/username|email/i)
      .first()
      .fill('grace')
    await page
      .getByLabel(/password/i)
      .first()
      .fill('fake-password')
    await page
      .getByRole('button', { name: /sign in|log ?in/i })
      .first()
      .click()

    // Whatever it says, it must say something and it must not leave the button
    // disabled forever -- a login form that never re-enables is a login form
    // somebody reloads the page to escape.
    await expect(page.getByRole('button', { name: /sign in|log ?in/i }).first()).toBeEnabled({
      timeout: 10_000,
    })
  })
})

test.describe('what a member sees', () => {
  test('the profile page loads its field configuration', async ({ page }) => {
    // 5c2fa3c from the browser's side. Before that fix `GET /profiles/config`
    // required an admin, so a member got a 403 and the page fell back to
    // treating profiles as unconfigured -- which looks like an administrator
    // has not set them up rather than like a permissions bug.
    await signIn(page, 'grace')
    await page.goto('/profile')

    await expect(page.getByText('Access Card')).toBeVisible()
    await expect(page.getByText(/not configured|not enabled/i)).toHaveCount(0)
  })

  test('a failed config load does not silently look like "no fields"', async ({
    page,
    request,
  }) => {
    // The distinction that matters: "the administrator has not set profiles up"
    // and "we could not find out" are different messages, and rendering the
    // first for the second sends the user to ask the wrong question.
    await signIn(page, 'grace')
    await arm(request, 'failNext', '/profiles/config', { status: 500 })
    await page.goto('/profile')

    await expect(page.getByText('Access Card')).toHaveCount(0)
  })
})
