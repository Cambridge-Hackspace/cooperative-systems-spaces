import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * The audit log's event-type filter is a hand-written list of `<option>`s in
 * `AuditLogTable.vue`. The set of event types the server can actually write is
 * `AuditEventType::as_str` in `server/src/models.rs`. Nothing connects them.
 *
 * Two things can go wrong, and only one of them is visible from the UI:
 *
 *   - An option naming a type the server never writes filters to nothing. The
 *     admin picks it, gets an empty table, and reasonably concludes the events
 *     did not happen.
 *   - A type the server writes with no matching option cannot be isolated at
 *     all. That is the state today, for fifty-seven of the seventy-five.
 *
 *     (The "sixty-six" this sentence used to read was already wrong when it was
 *     written: the two profile-config variants had landed without it being
 *     updated, so the real total was sixty-eight.
 *     `training_documentation_acknowledged` made sixty-nine and the six
 *     transactional-email types make seventy-five. Each arrived with a matching
 *     option, so the ratchet has stayed at fifty-seven throughout.)
 *
 *     `training_documentation_acknowledged` is deliberately filterable. It is
 *     the record issue #2 exists to produce, and a record that cannot be
 *     isolated in the audit log is not usable as one.
 *
 * The first is asserted as a hard rule: every offered option must exist on the
 * server. The second is asserted as a ratchet, because closing it is a product
 * decision rather than a bug fix -- but it may not silently get worse, and if
 * it gets better this test says so and asks to be updated.
 *
 * `process.cwd()` rather than `new URL(..., import.meta.url)` -- under jsdom
 * the global `URL` resolves relative references against the document base, not
 * the file URL. vitest pins cwd to `test.root`, which is the frontend
 * directory. See tests/structure/role-hierarchy.spec.ts.
 */
const FRONTEND_ROOT = process.cwd()
const read = (rel: string) => readFileSync(join(FRONTEND_ROOT, rel), 'utf8')

/** Every `"snake_case"` string literal inside `impl AuditEventType`'s as_str. */
function serverEventTypes(): string[] {
  const source = read('../server/src/models.rs')
  const start = source.indexOf('impl AuditEventType')
  expect(start, 'impl AuditEventType not found in server/src/models.rs').toBeGreaterThan(-1)

  // The impl block ends at the first line that is exactly `}` at column zero.
  const rest = source.slice(start)
  const end = rest.search(/\n\}\n/)
  expect(end, 'could not find the end of impl AuditEventType').toBeGreaterThan(-1)

  const body = rest.slice(0, end)
  const found = [...body.matchAll(/=> "([a-z_]+)"/g)]
    .map((m) => m[1])
    .filter((v): v is string => v !== undefined)
  // Anti-vacuity: a regex that matched nothing would make every assertion below
  // pass while checking nothing at all.
  expect(found.length, 'no event types parsed out of AuditEventType::as_str').toBeGreaterThan(20)
  return [...new Set(found)].sort()
}

/** Every non-empty `<option value="...">` in the filter select. */
function offeredEventTypes(): string[] {
  const source = read('src/components/AuditLogTable.vue')
  const template = source.slice(0, source.indexOf('</template>'))
  const found = [...template.matchAll(/<option value="([a-z_]+)"/g)]
    .map((m) => m[1])
    .filter((v): v is string => v !== undefined)
  expect(found.length, 'no filter options parsed out of AuditLogTable.vue').toBeGreaterThan(0)
  return [...new Set(found)].sort()
}

describe('the audit filter against the server enum', () => {
  it('offers nothing the server cannot write', () => {
    const server = new Set(serverEventTypes())
    const bogus = offeredEventTypes().filter((t) => !server.has(t))

    expect(
      bogus,
      'these filter options name event types no server code path writes, so ' +
        'choosing one shows an empty table that looks like an absence of events'
    ).toEqual([])
  })

  // A ratchet, not a target. Recorded so the gap cannot widen unnoticed and
  // cannot narrow without somebody updating this number on purpose.
  it('leaves exactly the recorded number of event types unfilterable', () => {
    const offered = new Set(offeredEventTypes())
    const missing = serverEventTypes().filter((t) => !offered.has(t))

    expect(
      missing.length,
      `the filterable set changed. ${missing.length} of ${serverEventTypes().length} ` +
        'server event types have no option. If that went down, lower the number ' +
        'here and say so; if it went up, an event type was added without a way ' +
        'to filter for it'
      // 57 -> 62: the Groups.io module added five event types
      // (mailing_list_subscribe / _unsubscribe / _sync_add / _sync_remove and
      // user_email_change). They join the unfiltered set deliberately -- the
      // filter offers a curated subset, and these are diagnostic rather than
      // access-control events.
      // 62 -> 69: the membership module added seven more (membership_granted /
      // _revoked / _payment_recorded / _last_admin_protected and
      // subscription_started / _canceled / _payment_failed). Same rationale --
      // billing lifecycle records, not access-control filters.
      // 69 -> 71: metered tool billing added tool_usage_charged and
      // tool_session_abandoned. Same rationale -- billing records, not filters.
    ).toBe(71)
  })

  // Named separately because these are the ones that matter in an
  // access-control system, and a change to the count above should not be able
  // to quietly leave them out.
  it('cannot filter for any door event', () => {
    const offered = new Set(offeredEventTypes())
    const doorEvents = serverEventTypes().filter((t) => t.startsWith('door_'))

    expect(doorEvents.length, 'no door_* events found on the server').toBeGreaterThan(4)
    expect(
      doorEvents.filter((t) => offered.has(t)),
      'a door event is now filterable -- good; update this test and the count above'
    ).toEqual([])
  })

  it('cannot filter for any MFA or device event either', () => {
    const offered = new Set(offeredEventTypes())
    const security = serverEventTypes().filter(
      (t) => t.startsWith('mfa_') || t.startsWith('device_')
    )

    expect(security.length).toBeGreaterThan(4)
    expect(security.filter((t) => offered.has(t))).toEqual([])
  })
})
