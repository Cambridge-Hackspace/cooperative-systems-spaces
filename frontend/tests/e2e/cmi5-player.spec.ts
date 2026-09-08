// Tier 5 (browser): the cmi5 learner surface, in a real browser.
//
// This is the render proof the offline tiers cannot give: that the "My Modules"
// list and the embedded player actually mount and behave in Chromium. It runs
// against the fake backend, so the launched content is a fake stand-in whose
// script reports a pass to the fake LRS — enough to drive the full loop the
// learner sees: open a module, the content renders in the player's iframe and
// runs, and the module then reads as completed. The real content, LRS, and
// tool-access grant are the stack battery's `cmi5` stage; what this owns is that
// the browser-side of the workflow renders and reflects state.

import { expect, test } from '@playwright/test'

import { arm, reset, signIn } from './fake'

test.beforeEach(async ({ request }) => {
  await reset(request)
})

test('a learner opens a module and the player renders it to completion', async ({ page }) => {
  await signIn(page) // grace, a Member

  await page.goto('/modules')
  await expect(page.getByText('Fake Safety Module')).toBeVisible()
  const start = page.getByRole('button', { name: /start/i })
  await expect(start).toBeVisible()

  await start.click()

  // The player renders the launched content in an iframe; the content reports a
  // pass to the fake LRS and shows "Completed" inside the frame.
  const frame = page.frameLocator('iframe[title="cmi5 content"]')
  await expect(frame.getByText('Completed')).toBeVisible()

  // Back on the list, the module now reflects the completion.
  await page.goto('/modules')
  await expect(page.getByText('Completed')).toBeVisible()
})

test('a failed module load shows an error, not a blank page', async ({ page, request }) => {
  await signIn(page)
  await arm(request, 'failNext', '/cmi5/modules', {
    status: 500,
    body: { success: false, error: 'the module list is unavailable' },
  })
  await page.goto('/modules')

  const alert = page.locator('.alert-error')
  await expect(alert).toBeVisible()
  await expect(alert).not.toHaveText('')
})
