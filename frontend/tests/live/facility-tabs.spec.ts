// Tier 10: issue #11, driven through a real browser against the real stack.
//
// The component tier already covers this -- DoorManagement.spec.ts mounts the
// real component inside a real KeepAlive that toggles. This exists because the
// reported defect was a *browser* observation, and the component tier reaches
// it by constructing the KeepAlive itself rather than by using the one
// FacilityManagement actually renders. Those are different claims, and the gap
// between them is exactly where a bug like this lives: everything can be
// individually correct and the assembled page still stale.
//
// So this walks the reporter's own steps, in Chromium, against a Postgres with
// LATIN1 and C collation, and asserts what they saw.
//
// It signs in as an administrator, which no other spec in this directory does.
// `[initial_setup] setup_admin_email` grants Admin to whoever registers that
// address first, which is also how `e2e/drivers/lib.mjs` gets one -- and if an
// earlier stage already claimed it, signing in with the shared password gets
// the same account.

import { expect, test, type Page } from '@playwright/test'

const ADMIN_EMAIL = 'admin@e2e.invalid'
const PASSWORD = 'e2e-password-1234'
const TAG = `fac${Date.now().toString(36)}${Math.floor(Math.random() * 1e4)}`

/** The boot overlay. Fixed and full-viewport, so it swallows every click. */
const OVERLAY = '.fixed.inset-0'

test.describe.configure({ mode: 'serial' })

/**
 * Sign in as an administrator, registering the privileged address if it is
 * still free.
 *
 * Registration is allowed to fail: on a full battery an earlier stage owns that
 * address, and the password is the same for every account this suite creates,
 * so signing in works either way. That is a property of the fixture, not of the
 * product.
 */
async function signInAsAdmin(page: Page) {
  await page.request.post('/api/auth/register', {
    data: {
      username: `admin_${TAG}`,
      email: ADMIN_EMAIL,
      password: PASSWORD,
      full_name: 'Facility Tabs Admin',
    },
    failOnStatusCode: false,
  })

  await page.goto('/login')
  await page
    .getByLabel(/username|email/i)
    .first()
    .fill(ADMIN_EMAIL)
  await page
    .getByLabel(/password/i)
    .first()
    .fill(PASSWORD)
  await page
    .getByRole('button', { name: /sign in|log ?in/i })
    .first()
    .click()
  await page.waitForURL((url) => !url.pathname.startsWith('/login'), { timeout: 20_000 })
}

const tab = (page: Page, name: string) => page.getByRole('tab', { name, exact: true })

async function openFacility(page: Page, which: 'places' | 'doors') {
  await page.goto(`/admin/facility?tab=${which}`)
  await expect(page.locator(OVERLAY), 'the boot overlay never cleared').toHaveCount(0, {
    timeout: 20_000,
  })
}

/** Create a room on the Places tab and wait for it to appear in the list. */
async function addRoom(page: Page, name: string) {
  await tab(page, 'Places').click()
  await page.getByRole('button', { name: '+ New root place' }).click()

  // By placeholder, not by position: the first input in this dialog is the
  // "special place" toggle, and `openCreate` already presets the type, so the
  // name is the only field to fill.
  const dialog = page.locator('.modal-box').filter({ hasText: 'New place' }).last()
  await dialog.getByPlaceholder('Room 5').fill(name)
  await dialog.getByRole('button', { name: 'Create', exact: true }).click()

  await expect(
    page.getByText(name, { exact: false }).first(),
    `the room ${name} was never listed on the Places tab, so the rest of this ` +
      'test would be asserting against a room that does not exist'
  ).toBeVisible({ timeout: 15_000 })
}

test.describe('the facility tabs, over a real stack', () => {
  test('a room added on Places is offered on Doors without a page refresh', async ({ page }) => {
    const serverErrors: string[] = []
    page.on('response', (r) => {
      if (r.status() >= 500) serverErrors.push(`${r.status()} ${r.url()}`)
    })

    await signInAsAdmin(page)

    // Visit Doors FIRST. This is what makes the test meaningful: KeepAlive
    // caches the tab on that first visit, and every later visit reuses the same
    // instance. A test that only ever opened Doors once would pass against the
    // broken build, because the first mount does load places.
    await openFacility(page, 'doors')
    await expect(page.getByRole('button', { name: '+ New door' })).toBeVisible({ timeout: 20_000 })

    const room = `Workshop ${TAG}`
    await addRoom(page, room)

    // Back to Doors. Nothing was refreshed; the tab is being shown again.
    await tab(page, 'Doors').click()
    await page.getByRole('button', { name: '+ New door' }).click()

    const form = page.locator('.modal-box').filter({ hasText: 'New door' }).last()

    // One assertion, on the whole dialog, and it is the whole claim: the room
    // that exists must be offerable as a location.
    //
    // Not asserted on a `select` by position -- the dialog's first one is the
    // edge-device picker, and the From/To pickers are behind `v-if="places.length"`,
    // so on the broken build they do not exist to assert against at all. An
    // assertion that cannot see its subject when the bug is present reports the
    // wrong thing. Matching the dialog's text covers both the picker's options
    // and, by their absence, the "you need at least one place" banner that
    // replaces them.
    await expect(
      form,
      `the new-door dialog does not offer ${room}, which was created on the ` +
        'Places tab moments ago. This is issue #11: the tab is cached by ' +
        'KeepAlive, so it still holds the places it loaded when it first ' +
        'mounted, and only a page refresh clears it.'
    ).toContainText(room, { timeout: 10_000 })

    expect(serverErrors, `the browser saw server errors: ${serverErrors.join(', ')}`).toHaveLength(
      0
    )
  })

  test('the places tab itself is not left behind either', async ({ page }) => {
    // The same rule in the other direction, and the reason the fix was applied
    // to all four tabs rather than only the one that was reported. Nothing
    // distinguished Doors except that somebody hit it first.
    await signInAsAdmin(page)
    await openFacility(page, 'places')

    const first = `Bay ${TAG}a`
    await addRoom(page, first)

    await tab(page, 'Doors').click()
    await expect(page.getByRole('button', { name: '+ New door' })).toBeVisible({ timeout: 20_000 })

    await tab(page, 'Places').click()
    await expect(
      page.getByText(first, { exact: false }).first(),
      'a room vanished from the Places tab after a round trip through Doors'
    ).toBeVisible({ timeout: 15_000 })
  })
})
