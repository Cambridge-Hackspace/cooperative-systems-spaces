/**
 * Calendar dates in the user's timezone, not in UTC.
 *
 * `new Date().toISOString().split('T')[0]` was written in five components, and
 * it is the date in *UTC*. West of UTC the two disagree for the last hours of
 * every day: at eight in the evening in Chicago it is already tomorrow in
 * London, so the date pickers floored at "today" refused today, and
 * RecordTrainingModal handed an instructor recording an evening session
 * tomorrow's date -- with the ceiling moved to match, so nothing objected.
 *
 * These use the local getters instead. `<input type="date">` and
 * `<input type="datetime-local">` both work in the user's own timezone, so a
 * value built this way round-trips through the control unchanged, which is not
 * true of a UTC one.
 *
 * What these are NOT for: anything sent to a field the server declares as a
 * timestamp. A `DateTime<Utc>` wants RFC 3339 and `toISOString()` is right for
 * it. These are for the fields that genuinely are calendar dates --
 * `training_date` is a `NaiveDate` -- and for the `min`/`max` attributes of a
 * date control, which are compared against what the user sees.
 */

const pad = (n: number): string => String(n).padStart(2, '0')

/** `YYYY-MM-DD` for a date, in the local timezone. */
export function localDate(d: Date = new Date()): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
}

/** `YYYY-MM-DDTHH:MM` for a `datetime-local` control, in the local timezone. */
export function localDateTime(d: Date = new Date()): string {
  return `${localDate(d)}T${pad(d.getHours())}:${pad(d.getMinutes())}`
}

/** The local calendar date of an RFC-3339 instant, or `''` when there is none. */
export function localDateOf(iso: string | null | undefined): string {
  if (!iso) return ''
  const d = new Date(iso)
  return Number.isNaN(d.getTime()) ? '' : localDate(d)
}

/**
 * The UTC calendar date of an RFC-3339 instant, or `''` when there is none.
 *
 * For reading a stored value back into a date control, where the stored value
 * is a *timestamp* but the thing it represents is a *date somebody picked*.
 *
 * A trainer expiry chosen as "1 March" is stored as `2026-03-01T00:00:00Z`.
 * Rendering that instant in the local timezone shows 28 February to anyone west
 * of UTC -- technically true and not what was picked, and it walks backwards a
 * day every time the form is opened and saved. The date component of the stored
 * instant is the answer here, because it is the answer that went in.
 *
 * `localDate` is for "now", where the user's own day is the only sensible
 * meaning. The two are different questions and this file answers both rather
 * than picking one.
 */
export function utcDateOf(iso: string | null | undefined): string {
  if (!iso) return ''
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return ''
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`
}
