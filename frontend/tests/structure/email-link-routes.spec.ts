/**
 * Tier 3: the two routes reached from an email must not be guest-only.
 *
 * `requiresGuest` bounces an authenticated visitor to the home page
 * (`router/index.ts`, the `beforeEach` guard). That is right for /login and
 * /register, where an already-signed-in user has no business, and wrong for a
 * link arriving by email.
 *
 * The person clicking a reset link is very often signed in *in that browser* --
 * that is frequently why they are resetting: the session is live on their
 * laptop and they cannot remember the password on their phone. Under
 * `requiresGuest` the link silently redirects and appears to do nothing at all,
 * which is indistinguishable from a broken link and produces exactly the
 * support ticket the feature was meant to prevent.
 *
 * Asserted structurally rather than by driving the router, because the claim is
 * about the route table itself and reads the same way a person checking it
 * would. What this does NOT prove: that the guard behaves as documented. That
 * is the guard's own business, and it is unchanged by this work.
 */

import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const ROUTER = readFileSync(resolve(process.cwd(), 'src/router/index.ts'), 'utf8')

/** The `{ ... }` route definition block naming this path. */
function routeBlock(path: string): string {
  const anchor = ROUTER.indexOf(`path: '${path}'`)
  expect(anchor, `no route defined for ${path}`).toBeGreaterThan(-1)
  const end = ROUTER.indexOf('\n    },', anchor)
  return ROUTER.slice(anchor, end === -1 ? ROUTER.length : end)
}

describe('routes reached from an email', () => {
  it('found the route table', () => {
    // Anti-vacuity: every assertion below is a `not.toContain` over a slice, and
    // an empty slice satisfies all of them.
    expect(ROUTER.length).toBeGreaterThan(2000)
    expect(ROUTER).toContain('requiresGuest')
  })

  for (const path of ['/reset-password', '/verify-email']) {
    it(`${path} is reachable while signed in`, () => {
      expect(
        routeBlock(path),
        `${path} carries requiresGuest, so a signed-in user opening the emailed ` +
          `link is redirected to the home page and the link appears to do nothing`
      ).not.toContain('requiresGuest')
    })
  }

  it('/forgot-password is still guest-only, so the scan discriminates', () => {
    // The control. If `routeBlock` ever returned something that contains no
    // metadata at all, every assertion above would pass over nothing; this one
    // fails instead.
    expect(routeBlock('/forgot-password')).toContain('requiresGuest')
  })

  it('the catch-all is still last', () => {
    // A route added after it is unreachable. Cheap to assert while we are here,
    // and this change adds three routes.
    const catchAll = ROUTER.indexOf('pathMatch')
    expect(catchAll).toBeGreaterThan(-1)
    expect(
      ROUTER.indexOf("path: '/", catchAll + 1),
      'a route is defined after the catch-all, so it can never match'
    ).toBe(-1)
  })
})
