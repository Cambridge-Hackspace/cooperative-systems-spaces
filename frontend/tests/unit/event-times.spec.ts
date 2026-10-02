// Tier 1: the zone arithmetic behind the calendar.
//
// These are the cheapest oracles for the defect the whole of #96 is about --
// an instant rendered in the wrong zone -- and they are the only ones that can
// state the claim without mounting anything. Every expectation is written as a
// wall clock in a named zone, which is the form a reader can check.
//
// America/New_York throughout, because that is the space's zone and because it
// observes daylight saving; a test written in UTC would pass with every one of
// these functions ignoring its zone argument.

import { describe, expect, it } from 'vitest'
import {
  addMonths,
  clockTime,
  dayKey,
  dayOfMonth,
  daysCovered,
  keyOf,
  lastDayOfMonth,
  monthAbbreviation,
  monthLabel,
  monthMatrix,
  monthOf,
  zoneLabel,
} from '@/lib/event-times'

const NY = 'America/New_York'
const BERLIN = 'Europe/Berlin'

describe('an instant in a named zone', () => {
  it('reads as the wall clock of that zone, not the viewer’s', () => {
    // 22:30Z is 6:30pm in New York: Open Project Night, as the Events page has
    // always said in prose.
    expect(clockTime('2026-10-06T22:30:00Z', NY)).toBe('6:30 PM')
    expect(clockTime('2026-10-06T22:30:00Z', BERLIN)).toBe('12:30 AM')
    expect(clockTime('2026-10-06T22:30:00Z', 'UTC')).toBe('10:30 PM')
  })

  it('names the zone, and the name follows daylight saving', () => {
    expect(zoneLabel('2026-10-06T22:30:00Z', NY)).toBe('EDT')
    expect(zoneLabel('2026-12-06T22:30:00Z', NY)).toBe('EST')
  })

  it('belongs to the day it falls on in that zone', () => {
    // 03:00Z on the 7th is still the evening of the 6th in New York. A grid
    // keyed on the UTC date puts this event on the wrong day.
    expect(dayKey('2026-10-07T03:00:00Z', NY)).toBe('2026-10-06')
    expect(dayKey('2026-10-07T03:00:00Z', 'UTC')).toBe('2026-10-07')
  })

  it('gives the day and month a card shows', () => {
    expect(dayOfMonth('2026-10-07T03:00:00Z', NY)).toBe('6')
    expect(monthAbbreviation('2026-10-07T03:00:00Z', NY)).toBe('OCT')
    // The leading zero is dropped: a card reads "6 OCT".
    expect(dayOfMonth('2026-10-09T04:00:00Z', NY)).toBe('9')
  })

  it('returns nothing for an unparseable instant rather than NaN on the page', () => {
    for (const fn of [dayKey, dayOfMonth, monthAbbreviation, clockTime, zoneLabel]) {
      expect(fn('not-a-date', NY)).toBe('')
    }
    expect(daysCovered('not-a-date', null, NY)).toEqual([])
  })
})

describe('the days an event covers', () => {
  it('is one day for an evening class', () => {
    expect(daysCovered('2026-10-06T22:30:00Z', '2026-10-07T02:00:00Z', NY)).toEqual(['2026-10-06'])
  })

  it('is one day when there is no stated end', () => {
    expect(daysCovered('2026-10-06T22:30:00Z', null, NY)).toEqual(['2026-10-06'])
  })

  it('includes the next day for something that runs past midnight', () => {
    // 10pm to 2am is two days on a grid, and a build night that goes long is
    // the normal case for this space rather than an exotic one.
    expect(daysCovered('2026-10-07T02:00:00Z', '2026-10-07T06:00:00Z', NY)).toEqual([
      '2026-10-06',
      '2026-10-07',
    ])
  })

  it('covers both days of a two-day all-day event, and not a third', () => {
    // HONK! is 9-10 October; the feed's DTEND is the 11th, exclusive. A naive
    // reading of the end instant puts a stray chip on the 11th.
    expect(daysCovered('2026-10-09T04:00:00Z', '2026-10-11T04:00:00Z', NY)).toEqual([
      '2026-10-09',
      '2026-10-10',
    ])
  })

  it('does not add a day for an event ending exactly at midnight', () => {
    expect(daysCovered('2026-10-06T22:00:00Z', '2026-10-07T04:00:00Z', NY)).toEqual(['2026-10-06'])
  })

  it('crosses a daylight-saving change without losing or doubling a day', () => {
    // 2026-11-01 is when the clocks go back in New York: that local day is 25
    // hours long, so fixed 24-hour arithmetic would skip or repeat a date.
    expect(daysCovered('2026-10-31T04:00:00Z', '2026-11-03T05:00:00Z', NY)).toEqual([
      '2026-10-31',
      '2026-11-01',
      '2026-11-02',
    ])
  })

  it('is bounded for an event with an absurd end', () => {
    const days = daysCovered('2026-01-01T05:00:00Z', '2036-01-01T05:00:00Z', NY)
    expect(days.length).toBeLessThanOrEqual(366)
    expect(days[0]).toBe('2026-01-01')
  })

  it('ignores an end that precedes its start', () => {
    expect(daysCovered('2026-10-06T22:30:00Z', '2026-10-01T00:00:00Z', NY)).toEqual(['2026-10-06'])
  })
})

describe('the grid a month is drawn on', () => {
  it('starts on the Sunday of the week containing the first', () => {
    // 1 October 2026 is a Thursday, so the first row starts on 27 September.
    const weeks = monthMatrix({ year: 2026, month: 10 })
    expect(weeks[0]?.map((d) => d.key)).toEqual([
      '2026-09-27',
      '2026-09-28',
      '2026-09-29',
      '2026-09-30',
      '2026-10-01',
      '2026-10-02',
      '2026-10-03',
    ])
    expect(weeks[0]?.map((d) => d.inMonth)).toEqual([false, false, false, false, true, true, true])
  })

  it('covers every day of the month exactly once', () => {
    for (const month of [1, 2, 6, 10, 12]) {
      const keys = monthMatrix({ year: 2026, month })
        .flat()
        .map((d) => d.key)
      const own = keys.filter((k) => k.startsWith(`2026-${String(month).padStart(2, '0')}`))
      const last = new Date(Date.UTC(2026, month, 0)).getUTCDate()
      expect(own, `month ${month}`).toHaveLength(last)
      expect(new Set(own).size, `month ${month}`).toBe(last)
    }
  })

  it('handles a leap February', () => {
    const own = monthMatrix({ year: 2028, month: 2 })
      .flat()
      .filter((d) => d.inMonth)
    expect(own).toHaveLength(29)
    expect(own.at(-1)?.key).toBe('2028-02-29')
  })

  it('draws only the weeks the month needs', () => {
    // February 2026 starts on a Sunday and has 28 days: exactly four rows, so
    // a fifth would be blank and the grid would change height for no reason.
    expect(monthMatrix({ year: 2026, month: 2 })).toHaveLength(4)
    expect(monthMatrix({ year: 2026, month: 10 })).toHaveLength(5)
    // August 2026 starts on a Saturday and has 31 days, which needs six.
    expect(monthMatrix({ year: 2026, month: 8 })).toHaveLength(6)
  })

  it('every row is a whole week', () => {
    for (const month of [1, 2, 8, 10]) {
      for (const row of monthMatrix({ year: 2026, month })) {
        expect(row).toHaveLength(7)
      }
    }
  })
})

describe('the month cursor', () => {
  it('steps forward and back', () => {
    expect(addMonths({ year: 2026, month: 10 }, 1)).toEqual({ year: 2026, month: 11 })
    expect(addMonths({ year: 2026, month: 10 }, -1)).toEqual({ year: 2026, month: 9 })
  })

  it('rolls the year over in both directions', () => {
    expect(addMonths({ year: 2026, month: 12 }, 1)).toEqual({ year: 2027, month: 1 })
    expect(addMonths({ year: 2026, month: 1 }, -1)).toEqual({ year: 2025, month: 12 })
    expect(addMonths({ year: 2026, month: 1 }, -13)).toEqual({ year: 2024, month: 12 })
  })

  it('labels a month without a zone shifting it', () => {
    expect(monthLabel({ year: 2026, month: 10 })).toBe('October 2026')
    expect(monthLabel({ year: 2026, month: 1 })).toBe('January 2026')
    expect(monthLabel({ year: 2026, month: 12 })).toBe('December 2026')
  })

  it('reads the month of an instant in the space’s zone', () => {
    // 2026-11-01T03:00Z is still 31 October in New York, so a view opened then
    // must open on October.
    expect(monthOf('2026-11-01T03:00:00Z', NY)).toEqual({ year: 2026, month: 10 })
    expect(monthOf('2026-11-01T03:00:00Z', 'UTC')).toEqual({ year: 2026, month: 11 })
  })

  it('builds a key from a cursor and a day', () => {
    expect(keyOf({ year: 2026, month: 10 }, 1)).toBe('2026-10-01')
    expect(keyOf({ year: 2026, month: 12 }, 31)).toBe('2026-12-31')
  })
})

describe('the length of a month', () => {
  it('knows the ordinary ones', () => {
    expect(lastDayOfMonth({ year: 2026, month: 1 })).toBe(31)
    expect(lastDayOfMonth({ year: 2026, month: 4 })).toBe(30)
    expect(lastDayOfMonth({ year: 2026, month: 12 })).toBe(31)
  })

  it('knows February', () => {
    expect(lastDayOfMonth({ year: 2026, month: 2 })).toBe(28)
    expect(lastDayOfMonth({ year: 2028, month: 2 })).toBe(29)
    // 2100 is not a leap year; a `% 4` rule says it is.
    expect(lastDayOfMonth({ year: 2100, month: 2 })).toBe(28)
    expect(lastDayOfMonth({ year: 2000, month: 2 })).toBe(29)
  })
})
