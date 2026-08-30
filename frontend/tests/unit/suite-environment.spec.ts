// Two harness invariants that other tests lean on without saying so, asserted
// here once so a change to `vitest.config.ts` or `tests/setup.ts` fails loudly
// rather than quietly making a hundred date assertions non-discriminating.

import { describe, expect, it } from 'vitest'

describe('the clock is frozen', () => {
  it('reads the same instant every run', () => {
    expect(new Date().toISOString()).toBe('2026-01-15T12:00:00.000Z')
  })
})

describe('the timezone is pinned, and is not UTC', () => {
  // Under UTC, code that calls `toISOString().split('T')[0]` where it meant the
  // user's local date is indistinguishable from code that gets it right -- the
  // two agree for every instant. A GitHub runner is UTC by default, so without
  // this pin the suite is at its least discriminating exactly where it runs
  // most often.
  it('is America/Chicago', () => {
    expect(Intl.DateTimeFormat().resolvedOptions().timeZone).toBe('America/Chicago')
  })

  it('actually disagrees with UTC about what day it is', () => {
    const lateEvening = new Date('2026-01-16T02:00:00Z')
    expect(lateEvening.getDate(), 'local date').toBe(15)
    expect(lateEvening.getUTCDate(), 'UTC date').toBe(16)
  })
})
