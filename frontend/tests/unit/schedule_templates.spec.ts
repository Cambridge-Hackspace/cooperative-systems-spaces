import { describe, expect, it } from 'vitest'
import {
  ALL_DAYS,
  SCHEDULE_TEMPLATES,
  WEEKDAYS,
  WEEKEND,
  type ScheduleTemplate,
} from '@/components/schedule_templates'
import type { DayOfWeek } from '@/types'

function template(id: string): ScheduleTemplate {
  const found = SCHEDULE_TEMPLATES.find((t) => t.id === id)
  if (!found) throw new Error(`no template with id ${id}; ids are ${ids().join(', ')}`)
  return found
}

const ids = () => SCHEDULE_TEMPLATES.map((t) => t.id)

describe('day constants', () => {
  // Written out verbatim rather than derived from the constants themselves. A
  // check computed from the thing it checks agrees with itself no matter what,
  // and these seven strings are a wire vocabulary: `schedules::DayOfWeek` on
  // the server is `#[serde(rename_all = "lowercase")]` over Mon..Sun, so a
  // typo here produces a schedule the server rejects at the API boundary.
  const EXPECTED: DayOfWeek[] = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun']

  it('lists every day once, in render order', () => {
    expect(ALL_DAYS).toEqual(EXPECTED)
  })

  it('partitions the week into weekdays and weekend with nothing lost or shared', () => {
    expect([...WEEKDAYS, ...WEEKEND].sort()).toEqual([...EXPECTED].sort())
    expect(WEEKDAYS.filter((d) => WEEKEND.includes(d))).toEqual([])
  })
})

describe('templates', () => {
  it('have unique ids', () => {
    expect(new Set(ids()).size).toBe(ids().length)
  })

  it('all produce intervals whose end is strictly after their start', () => {
    // `schedules::validate` on the server rejects `end <= start` outright, so a
    // template that produced one would be a preset that cannot be saved.
    for (const t of SCHEDULE_TEMPLATES) {
      for (const iv of t.build()) {
        expect(iv.end > iv.start, `${t.id}: ${iv.day} ${iv.start}-${iv.end}`).toBe(true)
      }
    }
  })

  it('all produce well-formed HH:MM on a known day', () => {
    for (const t of SCHEDULE_TEMPLATES) {
      for (const iv of t.build()) {
        expect(iv.start).toMatch(/^([01]\d|2[0-3]):[0-5]\d$/)
        expect(iv.end).toMatch(/^([01]\d|2[0-3]):[0-5]\d$/)
        expect(ALL_DAYS).toContain(iv.day)
      }
    }
  })

  it('build fresh arrays, so editing one applied template cannot mutate the preset', () => {
    const first = template('weekday-9-5').build()
    first[0]!.start = '06:00'
    expect(template('weekday-9-5').build()[0]!.start).toBe('09:00')
  })

  it('weekday-9-5 is five intervals, 09:00 to 17:00', () => {
    const built = template('weekday-9-5').build()
    expect(built).toHaveLength(5)
    expect(built.map((i) => i.day)).toEqual(WEEKDAYS)
    expect(new Set(built.map((i) => `${i.start}-${i.end}`))).toEqual(new Set(['09:00-17:00']))
  })

  it('weekends is two intervals covering Saturday and Sunday', () => {
    expect(template('weekends').build().map((i) => i.day)).toEqual(WEEKEND)
  })
})

describe('the 24/7 template', () => {
  it('covers all seven days', () => {
    const built = template('24-7').build()
    expect(built).toHaveLength(7)
    expect(built.map((i) => i.day)).toEqual(ALL_DAYS)
  })

  // This pins a real gap rather than asserting the template is correct.
  //
  // The server matches an interval as `start <= now < end` — the end is
  // exclusive, which `schedules.rs`'s own `matches_handles_weekday_boundaries`
  // test establishes. So a template that ends at 23:59 is *closed* from
  // 23:59:00 until midnight: a door or tool on a "24 / 7" schedule is shut for
  // sixty seconds every night.
  //
  // This is not fixable in this file. The interval is `HH:MM` parsed into a
  // `NaiveTime`, so the end of a day cannot be written down: `24:00` does not
  // parse and `00:00` would be `end <= start` and rejected by `validate`. The
  // fix belongs in the server's interval model — an explicit
  // "to end of day" representation, or an inclusive end for the final minute.
  //
  // Asserted as-is so the gap is recorded and cannot widen unnoticed, and so
  // that whoever fixes the model has a test that tells them this file needs
  // changing too.
  it('is closed for the final minute of each day, which is a defect in the interval model', () => {
    const built = template('24-7').build()
    expect(new Set(built.map((i) => `${i.start}-${i.end}`))).toEqual(new Set(['00:00-23:59']))

    // The gap, stated as the number it is.
    const endMinutes = 23 * 60 + 59
    const minutesInADay = 24 * 60
    expect(minutesInADay - endMinutes).toBe(1)
  })
})
