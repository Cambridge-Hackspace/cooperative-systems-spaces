// Tier 10: does the real application hold up over a world somebody else built?
//
// By the time this runs, the suite has registered dozens of accounts, changed
// roles, deactivated and deleted people, created doors and rules, and written
// profile-config versions -- against a LATIN1 cluster with C collation. That
// accumulated world is the point. Tier 5 drives a fake whose every response the
// test chose; this drives the real server over data the test did not choose and
// mostly does not know about.
//
// NOTHING IS INJECTED HERE. Tier 5 owns fault injection because it owns the
// world. Injecting into a live server that other stages are asserting against
// would produce findings belonging to whichever stage noticed first.
//
// So the oracle is inverted: instead of "what does the app do when the server
// misbehaves", this asks "does the server misbehave at all, and does the UI
// survive what it really returns".

import { expect, test, type Page, type APIRequestContext } from '@playwright/test'

/** Unique per run, so a re-run does not collide with its own leftovers. */
const TAG = `live${Date.now().toString(36)}${Math.floor(Math.random() * 1e4)}`
const PASSWORD = 'e2e-password-1234'

// The overlay App.vue puts up during boot. It is `fixed inset-0 ... z-50`, so it
// covers the viewport and swallows every click -- an application that renders
// perfectly and accepts no input. That failure has happened here before, which
// is why every route below checks for it rather than only the first.
const OVERLAY = '.fixed.inset-0'

// ---------------------------------------------------------------------------
// The watchdog
// ---------------------------------------------------------------------------
// Every test gets one. It records what the browser actually received, and the
// afterEach turns that into a failure -- so a 5xx on a page that still renders
// acceptably is caught, which a visual assertion would miss entirely.
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
  // An uncaught exception in a component stops Vue patching it, and the page
  // then holds whatever it last drew. The DOM can look entirely healthy.
  page.on('pageerror', (err) => {
    s.pageErrors.push(String(err))
  })
  return s
}

test.afterEach(async ({ page }) => {
  const s = seen.get(page)
  if (!s) return
  expect(
    s.serverErrors,
    'the browser received a 5xx from the real server. Tier 10 injects nothing, ' +
      'so this is the application failing on its own data.'
  ).toEqual([])
  expect(
    s.pageErrors,
    'an uncaught exception reached the page. Vue stops patching a component ' +
      'whose render throws, so the DOM may still look correct while the ' +
      'application has stopped responding.'
  ).toEqual([])
})

// ---------------------------------------------------------------------------
async function registerMember(request: APIRequestContext, suffix: string) {
  const username = `${TAG}_${suffix}`
  const res = await request.post('/api/auth/register', {
    data: {
      username,
      email: `${username}@e2e.invalid`,
      password: PASSWORD,
      full_name: `Live audit ${suffix}`,
    },
  })
  expect(res.status(), `registering the audit account answered ${res.status()}`).toBeLessThan(300)
  return username
}

async function signIn(page: Page, username: string) {
  await page.goto('/login')
  await page.getByLabel(/username|email/i).first().fill(username)
  await page.getByLabel(/password/i).first().fill(PASSWORD)
  await page.getByRole('button', { name: /sign in|log ?in/i }).first().click()
  await page.waitForURL((url) => !url.pathname.startsWith('/login'), { timeout: 20_000 })
}

/** The overlay must clear, and the page must have drawn something. */
async function settled(page: Page, where: string) {
  await expect(
    page.locator(OVERLAY),
    `${where}: the boot overlay is still up, so the page accepts no input`
  ).toHaveCount(0, { timeout: 20_000 })

  const text = (await page.locator('body').innerText()).trim()
  expect(text.length, `${where}: the page rendered no text at all`).toBeGreaterThan(0)
}

// ---------------------------------------------------------------------------
test.describe('the watchdog itself', () => {
  // The whole tier rests on this listener. Every other test here passes by the
  // watchdog staying silent, so a watchdog that can never fire would make the
  // entire stage a very slow way of rendering some pages -- the same reason
  // Tier 9's invariants have a self-test that feeds each of them a broken world.
  //
  // The 500 is fabricated in the BROWSER, with page.route, and never reaches the
  // server. That matters: this tier forbids injecting into a live stack other
  // stages are asserting against, and intercepting a response on its way into
  // the page is not that. Nothing on the server observes this test.
  test('records a 5xx the browser receives', async ({ page }) => {
    const s = watch(page)

    await page.route('**/api/config/public', (route) =>
      route.fulfill({ status: 503, contentType: 'application/json', body: '{}' })
    )
    await page.goto('/')

    expect(
      s.serverErrors.some((e) => e.startsWith('503')),
      'the watchdog did not record a 503 the browser plainly received, so every ' +
        'other assertion in this file passes for the wrong reason'
    ).toBe(true)

    // Cleared, or the afterEach fails on the error this test created on purpose.
    // The mechanism has been proven; the record is no longer wanted.
    s.serverErrors.length = 0
  })

  test('records an uncaught exception in the page', async ({ page }) => {
    const s = watch(page)
    await page.goto('/')
    await page.evaluate(() => {
      setTimeout(() => {
        throw new Error('watchdog self-test')
      }, 0)
    })
    await expect
      .poll(() => s.pageErrors.length, { timeout: 5_000 })
      .toBeGreaterThan(0)
    s.pageErrors.length = 0
  })
})

test.describe('the real application, over an accumulated world', () => {
  test('public routes render against real data', async ({ page }) => {
    watch(page)
    for (const path of ['/', '/about', '/login', '/register']) {
      await page.goto(path)
      await settled(page, path)
    }
  })

  test('a deep link is served by the application, not a 404', async ({ page }) => {
    watch(page)
    // The SPA fallback. This was once `.not_found_service`, which overrode the
    // status to 404 on every deep link -- the app worked if you clicked into it
    // and did not exist if you pasted the URL.
    const res = await page.goto('/admin/roster')
    expect(res?.status(), 'a deep link must be served with 200 and let the router decide')
      .toBeLessThan(400)
    await settled(page, '/admin/roster')
  })

  test('a member can sign in and reach their own pages', async ({ page, request }) => {
    watch(page)
    const username = await registerMember(request, 'member')
    await signIn(page, username)
    for (const path of ['/', '/profile', '/tools', '/training']) {
      await page.goto(path)
      await settled(page, path)
    }
  })

  test('admin routes refuse a member over the real guard', async ({ page, request }) => {
    watch(page)
    const username = await registerMember(request, 'guard')
    await signIn(page, username)

    // 5c2fa3c's shape at the browser level: a guard that is wrong lets a
    // newly-registered account onto the administration surface. Asserted on
    // what the user ends up looking at, because that is what the guard is for.
    for (const path of ['/admin', '/admin/roster', '/admin/audit', '/admin/devices']) {
      await page.goto(path)
      await settled(page, path)
      const body = (await page.locator('body').innerText()).toLowerCase()
      const kept = page.url().includes(path)
      const looksRefused =
        !kept || /not authori|permission|forbidden|access denied|sign in/i.test(body)
      expect(
        looksRefused,
        `${path} was reached by a freshly registered member and showed no refusal`
      ).toBe(true)
    }
  })

  test('the door check-in page loads on a phone viewport', async ({ page, request }) => {
    watch(page)
    // DoorCheckinView is opened by a camera app off a QR code and by nothing
    // else, so the viewport it is never used at is the desktop one. The id is
    // deliberately not a real door: this asserts the page handles a door it
    // cannot find without a 5xx and without freezing, which is the shape that
    // spun forever before 92afb4c.
    const username = await registerMember(request, 'door')
    await signIn(page, username)
    await page.goto('/door/00000000-0000-4000-8000-0000000000ff/checkin')
    await settled(page, '/door/{unknown}/checkin')
  })
})
