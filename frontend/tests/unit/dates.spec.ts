// Tier 1: the local-date helpers.
//
// `new Date().toISOString().split('T')[0]` appeared in five components and is
// the date in UTC, not the user's date. The suite pins TZ to America/Chicago
// (tests/unit/suite-environment.spec.ts) precisely so the two can be told
// apart -- under UTC a correct implementation and a broken one are
// indistinguishable, which is how this survived.

import { describe, expect, it, vi } from 'vitest'
import { localDate, localDateOf, localDateTime, utcDateOf } from '@/lib/dates'

describe('localDate', () => {
  it('reads the local calendar date, not the UTC one', () => {
    // 02:00Z on the 16th is 20:00 on the 15th in the suite's timezone.
    const evening = new Date('2026-01-16T02:00:00Z')
    expect(evening.getUTCDate()).toBe(16)
    expect(localDate(evening)).toBe('2026-01-15')
  })

  it('agrees with UTC when the two agree', () => {
    expect(localDate(new Date('2026-01-15T12:00:00Z'))).toBe('2026-01-15')
  })

  it('pads month and day to two digits', () => {
    expect(localDate(new Date('2026-03-05T18:00:00Z'))).toBe('2026-03-05')
  })

  it('defaults to now', () => {
    // The clock is frozen at 2026-01-15T12:00:00Z, which is 06:00 local.
    expect(localDate()).toBe('2026-01-15')

    vi.setSystemTime(new Date('2026-01-16T02:00:00Z'))
    expect(localDate()).toBe('2026-01-15')
    vi.setSystemTime(new Date('2026-01-15T12:00:00.000Z'))
  })

  it('rolls over at local midnight, not at 00:00Z', () => {
    // 05:59Z is 23:59 on the previous day locally; 06:01Z is 00:01 on this one.
    expect(localDate(new Date('2026-01-16T05:59:00Z'))).toBe('2026-01-15')
    expect(localDate(new Date('2026-01-16T06:01:00Z'))).toBe('2026-01-16')
  })
})

describe('localDateTime', () => {
  it('renders the value a datetime-local control round-trips', () => {
    expect(localDateTime(new Date('2026-02-01T15:30:00Z'))).toBe('2026-02-01T09:30')
  })

  it('pads hours and minutes', () => {
    expect(localDateTime(new Date('2026-02-01T14:05:00Z'))).toBe('2026-02-01T08:05')
  })
})

describe('utcDateOf', () => {
  // The other question. A trainer expiry chosen as "1 March" is stored as
  // `2026-03-01T00:00:00Z`; rendering that instant locally shows 28 February
  // west of UTC, which is not the date anybody picked -- and it walks back one
  // more day every time the form is opened and saved.
  it('reads the date component that was stored, not the local one', () => {
    expect(utcDateOf('2026-03-01T00:00:00Z')).toBe('2026-03-01')
    expect(localDateOf('2026-03-01T00:00:00Z')).toBe('2026-02-28')
  })

  it('does not walk backwards when a value is read and written repeatedly', () => {
    let stored = '2026-03-01T00:00:00Z'
    for (let i = 0; i < 5; i++) {
      const shown = utcDateOf(stored)
      expect(shown).toBe('2026-03-01')
      stored = `${shown}T00:00:00Z`
    }
  })

  it('returns empty for nothing and for nonsense', () => {
    expect(utcDateOf(null)).toBe('')
    expect(utcDateOf('')).toBe('')
    expect(utcDateOf('not a date')).toBe('')
  })
})

describe('localDateOf', () => {
  it('converts an RFC-3339 instant to the local calendar date', () => {
    expect(localDateOf('2026-01-16T02:00:00Z')).toBe('2026-01-15')
  })

  it('returns empty for nothing, rather than a date for nothing', () => {
    // The callers put this straight into an `<input type="date">`, where a
    // wrong-but-plausible value is worse than a blank one.
    expect(localDateOf(null)).toBe('')
    expect(localDateOf(undefined)).toBe('')
    expect(localDateOf('')).toBe('')
  })

  it('returns empty for something it cannot parse', () => {
    expect(localDateOf('not a date')).toBe('')
  })
})
