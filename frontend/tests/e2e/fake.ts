// Helpers for driving the fake API from a browser test.
//
// Every one of these talks to `/__fake`, which is outside `/api` so nothing the
// application can reach touches it. Arming a fault is a request from the test
// runner, not from the page, so the page never knows a fault was arranged.

import type { Page, APIRequestContext } from '@playwright/test'

export type Injection = 'failNext' | 'abortNext' | 'hangNext' | 'malformNext'

/** Put the world back to its starting state. Call this in a beforeEach. */
export async function reset(request: APIRequestContext) {
  const res = await request.get('/__fake/reset')
  if (!res.ok()) throw new Error(`fake reset failed: ${res.status()}`)
}

/**
 * Arm one fault for the next request whose path starts with `path`.
 *
 * `path` is relative to `/api`, so `'/doors'` matches `/api/doors/x/info`.
 * Prefix matching rather than exact, because a spec cares about "the next call
 * to the door endpoints" and should not have to know which one the component
 * makes first — which is an implementation detail the spec exists to be
 * independent of.
 */
export async function arm(
  request: APIRequestContext,
  kind: Injection,
  path: string,
  extra: { status?: number; body?: unknown } = {}
) {
  const res = await request.post('/__fake/arm', {
    data: { kind, path, ...extra },
  })
  if (!res.ok()) throw new Error(`fake arm failed: ${res.status()}`)
}

/** Every request the fake has seen. Useful for asserting on retries. */
export async function requests(
  request: APIRequestContext
): Promise<Array<{ method: string; path: string }>> {
  const res = await request.get('/__fake/requests')
  // Annotated rather than trusted. `res.json()` is `any`, and an `any` flowing
  // into a spec's assertion is how a control endpoint that changed shape shows
  // up as a confusing failure three tests later.
  const body = (await res.json()) as { data?: Array<{ method: string; path: string }> }
  return body.data ?? []
}

export const PASSWORD = 'fake-password'

/**
 * Sign in through the real login form.
 *
 * Deliberately through the UI rather than by writing a token into
 * localStorage. Half the interesting failures in this application are in the
 * login path, and a suite that skips it to save four seconds per test skips the
 * thing most likely to be broken.
 */
export async function signIn(page: Page, username = 'grace') {
  await page.goto('/login')
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
  await page.waitForURL((url) => !url.pathname.startsWith('/login'), { timeout: 10_000 })
}
