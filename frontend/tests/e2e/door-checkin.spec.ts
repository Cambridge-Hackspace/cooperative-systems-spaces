// Tier 5: the QR door check-in flow, under each of the four failure shapes.
//
// This is the screen the tier was built for. It is opened cold from a phone
// camera by somebody standing at a door, it energises a relay, and it is the
// subject of `92afb4c` — whose fix is a `|| 'Failed to load door'` fallback that
// **only a transport-level abort reaches**. axios attaches a `response` to every
// HTTP error, so a suite injecting 500s takes the first branch every time and
// reports the fallback as covered without ever running it.
//
// The mobile project matters here specifically. A QR link arrives on a handset
// and nowhere else, so testing this at 1280x720 tests the one size it is never
// used at.
//
// WHAT THIS DOES NOT PROVE. That the door opens. The relay is behind MQTT and
// an edge device, and only the stack battery can see it. The claim here is
// narrower and still the one that matters at 9pm in a corridor: the page says
// something true, and it never says nothing.

import { expect, test } from '@playwright/test'

import { arm, reset, signIn } from './fake'

const DOOR = '/door/door-1/checkin'

test.beforeEach(async ({ request }) => {
  await reset(request)
})

test('shows the door and offers the unlock button', async ({ page }) => {
  await signIn(page)
  await page.goto(DOOR)

  await expect(page.getByText('Front Door')).toBeVisible()
  await expect(page.getByText('You are authorized.')).toBeVisible()
  await expect(page.getByRole('button', { name: /I'm here/i })).toBeEnabled()
})

test('unlocks and says so', async ({ page }) => {
  await signIn(page)
  await page.goto(DOOR)
  await page.getByRole('button', { name: /I'm here/i }).click()

  await expect(page.getByText('Door unlocked')).toBeVisible()
})

test.describe('when the load fails', () => {
  test('an HTTP error shows the server’s reason', async ({ page, request }) => {
    await signIn(page)
    await arm(request, 'failNext', '/doors', {
      status: 404,
      body: { success: false, error: 'Door not found' },
    })
    await page.goto(DOOR)

    await expect(page.getByText('Door not found')).toBeVisible()
  })

  test('a dropped connection shows the fallback, not an empty box', async ({ page, request }) => {
    // **The 92afb4c branch.** `e.response` is undefined for a transport
    // failure, so `e?.response?.data?.error` is undefined and without the `||`
    // the alert renders with no text at all — a red box with nothing in it, on
    // somebody's phone, in a corridor.
    await signIn(page)
    await arm(request, 'abortNext', '/doors')
    await page.goto(DOOR)

    const alert = page.locator('.alert-error')
    await expect(alert).toBeVisible()
    await expect(alert).toContainText('Failed to load door')
    // And the assertion that the empty-box case is what is being prevented:
    // any non-empty text would satisfy `toContainText` on its own if the
    // message were wrong, but an empty element satisfies nothing.
    await expect(alert).not.toHaveText('')
  })

  test('a malformed success does not leave the page blank', async ({ page, request }) => {
    // A 200 whose body is not a DoorInfo. The component reads `r.success &&
    // r.data`; anything else must land in the error branch rather than
    // rendering a card with undefined everywhere.
    await signIn(page)
    await arm(request, 'malformNext', '/doors', {
      body: { success: true, data: null },
    })
    await page.goto(DOOR)

    await expect(page.locator('.alert-error')).toBeVisible()
    await expect(page.getByRole('button', { name: /I'm here/i })).toHaveCount(0)
  })

  test('a request that never answers leaves the spinner and no button', async ({
    page,
    request,
  }) => {
    // Recorded rather than asserted as correct. There is no timeout on this
    // load, so the spinner is forever. Whether that should become an error
    // after N seconds is a product decision; what the tier can say is that the
    // page does not offer an unlock button while it does not know whether the
    // person is authorized.
    await signIn(page)
    await arm(request, 'hangNext', '/doors')
    await page.goto(DOOR)

    await expect(page.locator('.loading-spinner')).toBeVisible()
    await expect(page.getByRole('button', { name: /I'm here/i })).toHaveCount(0)
  })
})

test.describe('when the check-in fails', () => {
  test('a dropped connection is reported, not silence', async ({ page, request }) => {
    // Silence after pressing the button is indistinguishable from success to
    // somebody standing at a door that did not open.
    await signIn(page)
    await page.goto(DOOR)
    await expect(page.getByRole('button', { name: /I'm here/i })).toBeEnabled()

    await arm(request, 'abortNext', '/doors/door-1/checkin')
    await page.getByRole('button', { name: /I'm here/i }).click()

    await expect(page.getByText('Did not unlock')).toBeVisible()
    await expect(page.getByText('Check-in failed')).toBeVisible()
  })

  test('a refusal is presented as a refusal', async ({ page, request }) => {
    await signIn(page)
    await page.goto(DOOR)
    await arm(request, 'failNext', '/doors/door-1/checkin', {
      status: 403,
      body: { success: false, error: 'Outside opening hours' },
    })
    await page.getByRole('button', { name: /I'm here/i }).click()

    await expect(page.getByText('Did not unlock')).toBeVisible()
    await expect(page.getByText('Outside opening hours')).toBeVisible()
  })
})
