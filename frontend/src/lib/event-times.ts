/**
 * Formatting calendar event instants in the space's timezone.
 *
 * Every function here takes the zone as an argument and defaults to the
 * viewer's own. That default is the behaviour the calendar had before, and it
 * is wrong for a calendar of events in one building: an all-day event pinned
 * to local midnight renders as the previous day for a viewer an hour west, and
 * a 6:30pm class reads as 3:30pm to somebody travelling. The views pass
 * `configStore.siteTimezone()`; the argument exists so these stay pure
 * functions a test can pin without a store.
 *
 * `lib/dates.ts` is the other half of this subject and deliberately separate:
 * those helpers are about `<input type="date">`, which works in the viewer's
 * zone and must keep doing so.
 */

/** Parts of an instant, in a given zone. */
function parts(iso: string, timeZone?: string): Record<string, string> {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return {}
  const formatter = new Intl.DateTimeFormat('en-US', {
    timeZone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
    weekday: 'short',
    timeZoneName: 'short',
  })
  const found: Record<string, string> = {}
  for (const part of formatter.formatToParts(date)) found[part.type] = part.value
  return found
}

/** `YYYY-MM-DD` for the day an instant falls on, in the given zone. */
export function dayKey(iso: string, timeZone?: string): string {
  const p = parts(iso, timeZone)
  if (!p.year) return ''
  return `${p.year}-${p.month}-${p.day}`
}

/** Day of the month, as shown on a card: `12`. */
export function dayOfMonth(iso: string, timeZone?: string): string {
  // Without the leading zero: a card reads "12 OCT", not "012".
  return (parts(iso, timeZone).day ?? '').replace(/^0/, '')
}

/** Abbreviated month, as shown on a card: `OCT`. */
export function monthAbbreviation(iso: string, timeZone?: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return ''
  return new Intl.DateTimeFormat('en-US', { timeZone, month: 'short' }).format(date).toUpperCase()
}

/** Clock time, as shown on a card: `6:30 PM`. */
export function clockTime(iso: string, timeZone?: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return ''
  return new Intl.DateTimeFormat('en-US', {
    timeZone,
    hour: 'numeric',
    minute: '2-digit',
    hour12: true,
  }).format(date)
}

/**
 * The zone's short name at that instant: `EDT`, `EST`, `GMT+1`.
 *
 * Shown once per event rather than per time, and it is not decoration: "7:00
 * PM" with no zone is ambiguous to exactly the reader who most needs it, and
 * daylight saving means the label cannot be a constant.
 */
export function zoneLabel(iso: string, timeZone?: string): string {
  return parts(iso, timeZone).timeZoneName ?? ''
}

/**
 * The days an event covers, as `YYYY-MM-DD` keys in the given zone.
 *
 * A grid needs an event to appear on every day it touches: HONK! runs for two
 * days and a Friday-night build session can cross midnight. The end instant is
 * exclusive -- an all-day event ends at the next local midnight, and an event
 * ending exactly at midnight belongs to the day before it.
 */
export function daysCovered(
  start: string,
  end: string | null | undefined,
  timeZone?: string
): string[] {
  const startMs = new Date(start).getTime()
  if (Number.isNaN(startMs)) return []

  const days: string[] = []
  const push = (ms: number) => {
    const key = dayKey(new Date(ms).toISOString(), timeZone)
    if (key && !days.includes(key)) days.push(key)
  }
  push(startMs)

  const endMs = end ? new Date(end).getTime() : NaN
  if (!Number.isNaN(endMs) && endMs > startMs) {
    // Twelve-hour steps for the days in between. Half a day cannot step over
    // a whole one (the shortest local day is 23 hours), and re-deriving the
    // key each time is what makes this correct across a daylight-saving
    // change rather than assuming every day is 86,400 seconds long.
    const HALF_DAY = 43_200_000
    for (let cursor = startMs + HALF_DAY; cursor < endMs; cursor += HALF_DAY) {
      push(cursor)
      // A guard against a feed with an end a decade after its start.
      if (days.length >= 366) break
    }
    // The last instant the event is actually running. Using the end itself
    // would add a day to every all-day event, whose end is the *next* local
    // midnight, and to every meeting that finishes at midnight exactly.
    push(endMs - 1)
  }

  // The cap is applied here rather than only inside the loop, so that the
  // final day cannot push the total one over it.
  return days.sort().slice(0, 366)
}

/** A year and a 1-based month: the cursor a month view is looking at. */
export interface YearMonth {
  year: number
  /** 1-12. One-based, like the `YYYY-MM-DD` keys everything else here uses. */
  month: number
}

/** The month an instant falls in, in the given zone. */
export function monthOf(iso: string, timeZone?: string): YearMonth {
  const key = dayKey(iso, timeZone)
  return { year: Number(key.slice(0, 4)), month: Number(key.slice(5, 7)) }
}

/** Today, as a `YYYY-MM-DD` key in the given zone. */
export function todayKey(timeZone?: string): string {
  return dayKey(new Date().toISOString(), timeZone)
}

/** Step a month cursor, rolling the year over. */
export function addMonths({ year, month }: YearMonth, delta: number): YearMonth {
  // Zero-based arithmetic, then back: `month + delta` on a 1-based value needs
  // a correction for every negative result, and getting it wrong is a December
  // that becomes month 0.
  const zeroBased = year * 12 + (month - 1) + delta
  return { year: Math.floor(zeroBased / 12), month: (zeroBased % 12) + 1 }
}

/** `October 2026`, for the heading above a grid. */
export function monthLabel({ year, month }: YearMonth): string {
  return new Intl.DateTimeFormat('en-US', {
    timeZone: 'UTC',
    month: 'long',
    year: 'numeric',
    // Formatted from a UTC noon so the zone can never shift the month.
  }).format(new Date(Date.UTC(year, month - 1, 15, 12)))
}

/** `YYYY-MM-DD` for a year, 1-based month and day. */
export function keyOf({ year, month }: YearMonth, day: number): string {
  return `${String(year).padStart(4, '0')}-${String(month).padStart(2, '0')}-${String(day).padStart(2, '0')}`
}

/**
 * The grid a month is drawn on: whole weeks, Sunday first, with the days of
 * the neighbouring months that share those weeks.
 *
 * Built from `Date.UTC` arithmetic rather than from any zone: which dates make
 * up October is not a question about timezones, and asking it in one is how a
 * grid ends up with 1 October in the row above where it belongs. The events
 * are the part that needs a zone, and they are keyed by `dayKey`.
 */
export function monthMatrix(cursor: YearMonth): { key: string; inMonth: boolean }[][] {
  const firstOfMonth = new Date(Date.UTC(cursor.year, cursor.month - 1, 1))
  const lead = firstOfMonth.getUTCDay() // 0 = Sunday
  const start = Date.UTC(cursor.year, cursor.month - 1, 1 - lead)

  const weeks: { key: string; inMonth: boolean }[][] = []
  for (let week = 0; week < 6; week += 1) {
    const row: { key: string; inMonth: boolean }[] = []
    for (let day = 0; day < 7; day += 1) {
      const at = new Date(start + (week * 7 + day) * 86_400_000)
      row.push({
        key: `${at.getUTCFullYear()}-${String(at.getUTCMonth() + 1).padStart(2, '0')}-${String(
          at.getUTCDate()
        ).padStart(2, '0')}`,
        inMonth: at.getUTCMonth() === cursor.month - 1 && at.getUTCFullYear() === cursor.year,
      })
    }
    weeks.push(row)
    // A sixth row exists only when the month needs it: February starting on a
    // Sunday fills exactly four, and drawing empty rows leaves the grid
    // jumping height as you page through the year.
    const next = new Date(start + (week + 1) * 7 * 86_400_000)
    if (next.getUTCMonth() !== cursor.month - 1 || next.getUTCFullYear() !== cursor.year) break
  }
  return weeks
}

/** How many days a month has. */
export function lastDayOfMonth({ year, month }: YearMonth): number {
  // Day 0 of the *next* month is the last day of this one, which is the one
  // piece of date arithmetic that needs no table of month lengths and no leap
  // year rule.
  return new Date(Date.UTC(year, month, 0)).getUTCDate()
}
