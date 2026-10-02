// Tier 2: component conformance for CalendarEvents.
//
// Two things make this component worth exact assertions.
//
// FIRST, ITS INPUT IS NOT THE SPACE'S. The events come from third-party iCal
// feeds — `server/src/calendar.rs` fetches whatever `source.ical_link` points
// at. The description used to be rendered with `v-html`, which meant anybody
// who could put an event in a subscribed calendar could put markup, and
// therefore script, into every viewer's page.
//
// #96 split that field in two, because interpolating it was not free either:
// Google Calendar puts HTML in a DESCRIPTION, so members were reading tags and
// losing the links inside them. The component now renders `description_html`
// — which the server built by reducing the feed's markup to Markdown and
// re-rendering it with raw HTML off — and still never renders `description` as
// markup. Both halves are asserted below, the second against the hostile
// corpus the rest of the suite uses rather than one hand-written payload.
//
// WHAT THE FIRST HALF DOES NOT PROVE: that `description_html` is safe. It is
// v-html'd here deliberately, so this component cannot be the thing that
// decides. `server/src/calendar/description.rs` carries that claim, in tests
// that feed it script tags and `javascript:` destinations, and
// `tests/structure/markdown-rendering.spec.ts` asserts nobody has switched the
// renderer's passthrough on.
//
// SECOND, IT HAS FOUR MUTUALLY EXCLUSIVE STATES chained on a v-if. Loading,
// error, empty and populated. Three of the four are states a person only sees
// when something is wrong, which is precisely when nobody is looking at them in
// development.
//
// WHAT THIS DOES NOT PROVE. That the feed is fetched from the right URL in
// production — `fetch('/api/calendar/events')` is a same-origin relative path
// and the fake API tier is what exercises it against a real server. And nothing
// about the fifteen-minute refresh interval other than that it is cleared on
// unmount; an interval that fires is a Tier 5 concern.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

import CalendarEvents from '@/components/CalendarEvents.vue'
import corpus from '../../../e2e/corpus/hostile.json'

interface Event {
  title: string
  description?: string
  description_html?: string
  start: string
  end?: string
  location?: string
  calendar_name: string
  calendar_color: string
  all_day: boolean
}

function event(overrides: Partial<Event> = {}): Event {
  return {
    title: 'Open Shop Night',
    start: '2026-03-12T18:00:00Z',
    end: '2026-03-12T21:00:00Z',
    calendar_name: 'Main',
    calendar_color: '#3788d8',
    all_day: false,
    ...overrides,
  }
}

/** Mount with `fetch` answering once with `events`, and wait for the render. */
async function mountWith(events: Event[] | { status: number }, timezone?: string) {
  // The parameter is declared even though the body ignores it: `vi.fn(async () => ..)`
  // gives the mock an empty argument tuple, so `calls[n][0]` is a type error --
  // and vue-tsc checks this directory as part of `npm run build`. Naming it also
  // makes the URL assertion below express what it is asserting.
  // The URL parameter is declared but unused here: it exists so the mock's
  // call tuple has an element the URL assertions can read. `void url` is the
  // idiom the lint accepts for that, and it says so out loud.
  const fetchMock = vi.fn((url: string): Promise<Response> => {
    void url
    const body = Array.isArray(events)
      ? { ok: true, statusText: 'OK', json: () => Promise.resolve(events) }
      : { ok: false, statusText: 'Service Unavailable' }
    return Promise.resolve(body as unknown as Response)
  })
  vi.stubGlobal('fetch', fetchMock)

  const wrapper = mount(CalendarEvents, { props: { timezone } })
  await flushPromises()
  return { wrapper, fetchMock }
}

beforeEach(() => {
  vi.useFakeTimers()
  // Console noise from the error paths is expected and is not the subject.
  vi.spyOn(console, 'error').mockImplementation(() => {})
})

afterEach(() => {
  vi.useRealTimers()
})

describe('the description is text, never markup', () => {
  // The regression test for the v-html fix. Driven from the shared corpus so
  // that a payload added for the fuzz tier is automatically tried here too --
  // one list, both languages, which is the whole reason the corpus is JSON.
  const dangerous = corpus.strings
    .filter((s) => typeof s.v === 'string' && /[<>{}]/.test(s.v))
    .map((s) => [s.why, s.v] as const)

  it.each(dangerous)('renders %s as characters', async (_why, payload) => {
    const { wrapper } = await mountWith([event({ description: payload })])
    const description = wrapper.find('.event-description')

    expect(description.exists()).toBe(true)
    // The text is present in full...
    expect(description.text()).toBe(payload)
    // ...and produced no elements. `element.children` is the assertion that
    // matters: `innerHTML` would contain an escaped `&lt;script&gt;` either
    // way, and only the parsed tree distinguishes text from markup.
    expect(description.element.children.length).toBe(0)
    expect(wrapper.find('.event-description script').exists()).toBe(false)
    expect(wrapper.find('.event-description img').exists()).toBe(false)
  })

  it('escapes markup in the title and location as well', async () => {
    // Same feed, same trust level. Interpolating one and not the others would
    // be an accident of which field somebody happened to look at.
    const payload = '<img src=x onerror=alert(1)>'
    const { wrapper } = await mountWith([
      event({ title: payload, location: payload, calendar_name: payload }),
    ])
    expect(wrapper.findAll('img')).toHaveLength(0)
    expect(wrapper.find('.event-title').text()).toBe(payload)
  })

  it('renders no description element when the event has none', async () => {
    const { wrapper } = await mountWith([event()])
    expect(wrapper.find('.event-description').exists()).toBe(false)
  })

  it('prefers the rendered form and does not also show the raw one', async () => {
    // Both fields arrive on every event the current server produces. Showing
    // both would print the feed's tags underneath the rendered copy.
    const { wrapper } = await mountWith([
      event({
        description: 'Review <a href="https://example.org/pre">the reading</a> first.',
        description_html: '<p>Review <a href="https://example.org/pre">the reading</a> first.</p>',
      }),
    ])
    const descriptions = wrapper.findAll('.event-description')
    expect(descriptions).toHaveLength(1)
    const link = descriptions[0]?.find('a')
    expect(link?.attributes('href')).toBe('https://example.org/pre')
    expect(descriptions[0]?.text()).not.toContain('<a href')
  })

  it('renders the markup in the rendered form, which is the point of it', async () => {
    const { wrapper } = await mountWith([
      event({ description_html: '<p>one<br>two</p><ul><li>bring wood</li></ul>' }),
    ])
    const description = wrapper.find('.event-description')
    expect(description.find('li').text()).toBe('bring wood')
    expect(description.findAll('br')).toHaveLength(1)
  })
})

describe('the four states', () => {
  it('shows the spinner before the first response arrives, and nothing else', () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => new Promise(() => {}))
    )
    const wrapper = mount(CalendarEvents)

    expect(wrapper.find('.loading-state').exists()).toBe(true)
    expect(wrapper.find('.error-state').exists()).toBe(false)
    expect(wrapper.find('.empty-state').exists()).toBe(false)
    expect(wrapper.find('.events-list').exists()).toBe(false)
    // The refresh button is hidden while loading -- pressing it would start a
    // second request against a component that has not finished its first.
    expect(wrapper.find('.refresh-btn').exists()).toBe(false)
  })

  it('shows the empty state, not an empty list, for a feed with no events', async () => {
    const { wrapper } = await mountWith([])
    expect(wrapper.find('.empty-state').text()).toBe('No upcoming events scheduled')
    expect(wrapper.find('.events-list').exists()).toBe(false)
    expect(wrapper.find('.loading-state').exists()).toBe(false)
  })

  it('shows the failure and offers a retry', async () => {
    const { wrapper } = await mountWith({ status: 503 })
    const error = wrapper.find('.error-state')

    expect(error.exists()).toBe(true)
    // The statusText is included, because "could not load events" with no
    // reason is a message nobody can act on.
    expect(error.text()).toContain('Service Unavailable')
    expect(wrapper.find('.retry-btn').text()).toBe('Try Again')
    expect(wrapper.find('.events-list').exists()).toBe(false)
  })

  it('clears a previous error when a retry succeeds', async () => {
    // A stale error banner above a freshly loaded list is worse than either on
    // its own: the reader cannot tell which one is current.
    const responses = [
      { ok: false, statusText: 'Bad Gateway' },
      { ok: true, statusText: 'OK', json: () => Promise.resolve([event()]) },
    ]
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve(responses.shift() as unknown as Response))
    )

    const wrapper = mount(CalendarEvents)
    await flushPromises()
    expect(wrapper.find('.error-state').exists()).toBe(true)

    await wrapper.find('.retry-btn').trigger('click')
    await flushPromises()

    expect(wrapper.find('.error-state').exists()).toBe(false)
    expect(wrapper.findAll('.event-card')).toHaveLength(1)
  })

  it('never shows two states at once', async () => {
    for (const payload of [[], [event()], { status: 500 }] as const) {
      const { wrapper } = await mountWith(payload as Event[] | { status: number })
      const shown = ['.loading-state', '.error-state', '.empty-state', '.events-list'].filter((s) =>
        wrapper.find(s).exists()
      )
      expect(shown, JSON.stringify(payload)).toHaveLength(1)
    }
  })
})

describe('the event list', () => {
  it('renders one card per event, in the order given', async () => {
    const { wrapper } = await mountWith([
      event({ title: 'First' }),
      event({ title: 'Second' }),
      event({ title: 'Third' }),
    ])
    const titles = wrapper.findAll('.event-title').map((t) => t.text())
    expect(titles).toEqual(['First', 'Second', 'Third'])
  })

  it('carries the calendar color onto the card and the tag', async () => {
    const { wrapper } = await mountWith([
      event({ calendar_color: '#ff5733', calendar_name: 'Shop' }),
    ])
    const card = wrapper.find('.event-card')
    expect(card.attributes('style')).toContain('border-left-color: rgb(255, 87, 51)')
    const tag = wrapper.find('.event-calendar-tag')
    expect(tag.text()).toBe('Shop')
    expect(tag.attributes('style')).toContain('background-color: rgb(255, 87, 51)')
  })

  it('shows All Day instead of a time range for an all-day event', async () => {
    const { wrapper } = await mountWith([event({ all_day: true })])
    expect(wrapper.find('.event-time').text()).toBe('🕐 All Day')
  })

  it('shows a start with no end when the event has no end', async () => {
    const { wrapper } = await mountWith([event({ end: undefined })])
    expect(wrapper.find('.event-time').text()).not.toContain('-')
  })

  it('renders no location element when there is no location', async () => {
    const { wrapper } = await mountWith([event()])
    expect(wrapper.find('.event-location').exists()).toBe(false)
  })

  it('puts the location in the title attribute so a long one is readable', async () => {
    const { wrapper } = await mountWith([event({ location: 'Bay 3, behind the metal shop' })])
    const location = wrapper.find('.event-location')
    expect(location.attributes('title')).toBe('Bay 3, behind the metal shop')
  })
})

describe("times are the space's, not the viewer's", () => {
  // The live defect: `DTSTART;TZID=America/New_York:20261006T183000` was read
  // as UTC by the server and then rendered in the browser's zone, so a 6:30pm
  // class showed as 2:30pm. The server half is fixed in
  // server/src/calendar/ics.rs; this is the other half.
  const NY = 'America/New_York'

  it('renders the clock time in the zone it is given', async () => {
    const { wrapper } = await mountWith(
      [event({ start: '2026-10-06T22:30:00Z', end: '2026-10-07T02:00:00Z' })],
      NY
    )
    const time = wrapper.find('.event-time').text()
    expect(time).toContain('6:30 PM')
    expect(time).toContain('10:00 PM')
    // And says which zone, because "6:30 PM" alone is ambiguous to the reader
    // who is not in the building.
    expect(time).toContain('EDT')
  })

  it('dates the card in that zone too', async () => {
    // 03:00Z on the 7th is the evening of the 6th in New York. A card dated
    // from the UTC instant puts the event on the wrong day of the month.
    const { wrapper } = await mountWith([event({ start: '2026-10-07T03:00:00Z' })], NY)
    expect(wrapper.find('.event-day').text()).toBe('6')
    expect(wrapper.find('.event-month').text()).toBe('OCT')
  })

  it('dates an all-day event on its own day', async () => {
    // An all-day event is local midnight: 2026-10-09T04:00Z is the 9th in New
    // York and the 9th is what it must say. Pinned to UTC midnight -- what the
    // server used to send -- this card read "8".
    const { wrapper } = await mountWith(
      [event({ start: '2026-10-09T04:00:00Z', end: '2026-10-10T04:00:00Z', all_day: true })],
      NY
    )
    expect(wrapper.find('.event-day').text()).toBe('9')
    expect(wrapper.find('.event-time').text()).toBe('🕐 All Day')
  })
})

describe('the refresh', () => {
  it('calls the refresh endpoint rather than the plain one', async () => {
    // These are different URLs on the server: one reads the cache, the other
    // re-fetches every subscribed feed. Calling the wrong one makes the button
    // look like it works and change nothing.
    const { wrapper, fetchMock } = await mountWith([event()])
    await wrapper.find('.refresh-btn').trigger('click')
    await flushPromises()

    expect(fetchMock.mock.calls.map((c) => c[0])).toEqual([
      '/api/calendar/events',
      '/api/calendar/events/refresh',
    ])
  })

  it('replaces the list rather than appending to it', async () => {
    const responses = [
      { ok: true, statusText: 'OK', json: () => Promise.resolve([event({ title: 'Old' })]) },
      { ok: true, statusText: 'OK', json: () => Promise.resolve([event({ title: 'New' })]) },
    ]
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve(responses.shift() as unknown as Response))
    )
    const wrapper = mount(CalendarEvents)
    await flushPromises()

    await wrapper.find('.refresh-btn').trigger('click')
    await flushPromises()

    expect(wrapper.findAll('.event-card')).toHaveLength(1)
    expect(wrapper.find('.event-title').text()).toBe('New')
  })

  it('stops the auto-refresh timer when the component goes away', async () => {
    // An interval left running after unmount keeps fetching for the life of the
    // page, and on a route the user has left.
    const clear = vi.spyOn(globalThis, 'clearInterval')
    const { wrapper } = await mountWith([event()])
    wrapper.unmount()
    expect(clear).toHaveBeenCalled()
  })

  it('does not blank the list while a background refresh is in flight', async () => {
    // The silent path passes `silent = true` so `loading` stays false. Without
    // that, the whole list is replaced by a spinner every fifteen minutes while
    // somebody is reading it.
    const { wrapper } = await mountWith([event({ title: 'Visible' })])

    vi.stubGlobal(
      'fetch',
      vi.fn(() => new Promise(() => {}))
    )
    vi.advanceTimersByTime(15 * 60 * 1000)
    await flushPromises()

    expect(wrapper.find('.loading-state').exists()).toBe(false)
    expect(wrapper.find('.event-title').text()).toBe('Visible')
  })
})
