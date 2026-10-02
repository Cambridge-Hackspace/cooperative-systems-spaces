// Tier 6, `calendar`: the calendar endpoint's wiring, against a real server.
//
// WHAT THIS TIER CAN ANSWER, AND WHAT IT CANNOT.
//
// The stack's config has `[calendar] enabled = false` and no feed to subscribe
// to, so this driver cannot see an event. What it can see is everything
// between the wire and the service, which is exactly the part no unit test
// reaches:
//
//   * the window query is extracted at all, and a date that is not a date is
//     refused with the project's one error envelope and a 400 -- not a 422
//     from an extractor, not a 500, and not a silent empty list, which would
//     read to a caller as "nothing is on in October";
//   * the endpoint is still public, because it is on the home page of a site
//     most visitors are not signed in to;
//   * `/api/config/public` carries the space's timezone, which is what tells
//     the page which zone to render event times in. Without it the page falls
//     back to the viewer's own and #96 is half-fixed.
//
// WHAT IS NOT COVERED ANYWHERE AUTOMATED: fetching a real feed over HTTP and
// caching it. `server/src/calendar/ics.rs` holds fourteen components of the
// space's own published feed as a fixture and asserts what they expand to, so
// the parse is pinned against the real producer -- but nothing in CI points
// the service at an HTTP server and watches it fetch. A feed URL that starts
// answering with a login page is caught by the `BEGIN:VCALENDAR` check in the
// parser and logged; that path is tested in the unit suite, not here.

import { GET, assertEq, ok, main } from './lib.mjs'

/** The window query, as the month grid sends it. */
const GOOD_WINDOW = '?from=2026-10-01&to=2026-10-31'

const events = (query = '') => GET(`/api/calendar/events${query}`)

async function run() {
  // ----- the endpoint is public and answers a list -----------------------

  let res = await events()
  ok(
    'calendar/events-are-public',
    res.status === 200 && Array.isArray(res.json),
    `status ${res.status}, body ${String(res.text).slice(0, 80)}`
  )

  res = await events(GOOD_WINDOW)
  ok(
    'calendar/a-window-is-accepted',
    res.status === 200 && Array.isArray(res.json),
    `status ${res.status}, body ${String(res.text).slice(0, 80)}`
  )

  // Half-open windows are legitimate: the home page's panel sends neither
  // bound, and "from today to the end of the month" sends one.
  for (const [name, query] of [
    ['from-alone', '?from=2026-10-01'],
    ['to-alone', '?to=2036-10-31'],
  ]) {
    res = await events(query)
    assertEq(`calendar/${name}-is-accepted`, 200, res.status, query)
  }

  // ----- a bad window is refused, in the project's envelope --------------

  for (const [name, query, message] of [
    ['a-date-that-is-not-a-date', '?from=nonsense', 'dates must be YYYY-MM-DD'],
    ['a-month-that-does-not-exist', '?to=2026-13-01', 'dates must be YYYY-MM-DD'],
    ['a-backwards-window', '?from=2026-10-31&to=2026-10-01', '`to` must be after `from`'],
  ]) {
    res = await events(query)
    assertEq(`calendar/${name}-is-a-400`, 400, res.status, query)
    // The envelope, field by field: a 400 shaped differently from every other
    // error in the API is a client-side special case waiting to be written.
    ok(
      `calendar/${name}-uses-the-error-envelope`,
      res.json?.success === false && res.json?.error === message,
      String(res.text).slice(0, 140)
    )
  }

  // The refresh endpoint takes the same window and must refuse the same way.
  // It clears every cached feed before fetching, so a bad date that reached
  // the handler would have thrown the server's whole cache away first.
  res = await GET('/api/calendar/events/refresh?from=nonsense')
  ok(
    'calendar/refresh-refuses-a-bad-window-too',
    res.status === 400 && res.json?.error === 'dates must be YYYY-MM-DD',
    `status ${res.status}, body ${String(res.text).slice(0, 100)}`
  )

  // ----- the page is told which zone to render in ------------------------

  res = await GET('/api/config/public')
  const zone = res.json?.data?.calendar?.timezone
  ok(
    'calendar/public-config-names-the-timezone',
    res.status === 200 && typeof zone === 'string',
    `status ${res.status}, calendar ${JSON.stringify(res.json?.data?.calendar)}`
  )
  // Not merely present: a plausible IANA name, because the frontend hands it
  // straight to `Intl.DateTimeFormat`, where an empty string silently falls
  // back to the viewer's own zone -- the behaviour #96 exists to replace.
  ok(
    'calendar/the-timezone-is-a-usable-zone',
    typeof zone === 'string' && zone.length > 0 && !zone.includes(' '),
    `timezone ${JSON.stringify(zone)}`
  )
}

await main(run)
