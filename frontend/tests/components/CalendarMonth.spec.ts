// Tier 2: the month grid (#96, "should be calendar-shaped anyhow").
//
// What has teeth here is everything that depends on a date boundary, because
// every one of those is a place an event lands in the wrong cell and nobody
// notices until somebody turns up on the wrong evening:
//
//   * which day a cell is, and which day an event belongs to, are both read in
//     the SPACE's zone -- not the viewer's and not UTC;
//   * an event that spans days appears in every cell it touches;
//   * the request covers the whole visible grid, including the days of the
//     neighbouring months in the first and last rows.
//
// The grid arithmetic itself (which dates make up a month, how many rows) is
// tested in tests/unit/event-times.spec.ts against the pure functions. This
// file tests what the component does with them.
//
// WHAT THIS DOES NOT PROVE: that `description_html` is safe to render. It is
// v-html'd deliberately; `server/src/calendar/description.rs` is what makes
// that defensible, and tests/structure/markdown-rendering.spec.ts asserts
// nobody has turned the renderer's passthrough on.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

import CalendarMonth from '@/components/CalendarMonth.vue'

const NY = 'America/New_York'

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
    title: 'Open Project Night',
    start: '2026-10-06T22:30:00Z',
    end: '2026-10-07T02:00:00Z',
    calendar_name: 'Main',
    calendar_color: '#3788d8',
    all_day: false,
    ...overrides,
  }
}

/**
 * Mount with the clock fixed inside October 2026 and `fetch` answering with
 * `events`.
 *
 * The clock is fixed because "which month opens" and "which cell is today" are
 * assertions about today, and a test whose expectations depend on the day it
 * runs is a test that fails on its own one morning.
 */
async function mountWith(events: Event[] | { status: number }) {
  const fetchMock = vi.fn((url: string): Promise<Response> => {
    void url
    const body = Array.isArray(events)
      ? { ok: true, statusText: 'OK', json: () => Promise.resolve(events) }
      : { ok: false, statusText: 'Service Unavailable' }
    return Promise.resolve(body as unknown as Response)
  })
  vi.stubGlobal('fetch', fetchMock)
  const wrapper = mount(CalendarMonth, { props: { timezone: NY } })
  await flushPromises()
  return { wrapper, fetchMock }
}

beforeEach(() => {
  // 14 October 2026, 18:00 in New York.
  vi.useFakeTimers()
  vi.setSystemTime(new Date('2026-10-14T22:00:00Z'))
})

afterEach(() => {
  vi.useRealTimers()
})

describe('the grid it opens on', () => {
  it('opens on the month that contains today, in the space’s zone', async () => {
    const { wrapper } = await mountWith([])
    expect(wrapper.get('[data-testid="month-title"]').text()).toBe('October 2026')
  })

  it('opens on the previous month when today is the small hours of the first', async () => {
    // 2026-11-01T03:00Z is still 31 October in New York. Opening on November
    // would hide the evening the member is about to walk into.
    vi.setSystemTime(new Date('2026-11-01T03:00:00Z'))
    const { wrapper } = await mountWith([])
    expect(wrapper.get('[data-testid="month-title"]').text()).toBe('October 2026')
  })

  it('draws whole weeks, including the neighbouring days', async () => {
    const { wrapper } = await mountWith([])
    const cells = wrapper.findAll('[data-day]')
    expect(cells).toHaveLength(35) // five rows for October 2026
    expect(cells[0]?.attributes('data-day')).toBe('2026-09-27')
    expect(cells[0]?.classes()).toContain('outside')
    expect(cells.at(-1)?.attributes('data-day')).toBe('2026-10-31')
  })

  it('marks today and selects it', async () => {
    const { wrapper } = await mountWith([])
    const today = wrapper.get('[data-day="2026-10-14"]')
    expect(today.classes()).toContain('today')
    expect(today.classes()).toContain('selected')
    expect(wrapper.get('[data-testid="day-detail"]').text()).toContain(
      'Wednesday, October 14, 2026'
    )
  })

  it('asks for every day the grid shows, not just the month', async () => {
    // A request for 1-31 October leaves the first row's September cells and
    // any trailing November cells blank while showing them.
    const { fetchMock } = await mountWith([])
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/calendar/events?from=2026-09-27&to=2026-10-31')
  })
})

describe('where an event lands', () => {
  it('puts an evening event on the day it is in the space’s zone', async () => {
    // 22:30Z on the 6th is 6:30pm on the 6th in New York. Read in UTC this
    // event is still the 6th; read in UTC the one below is not.
    const { wrapper } = await mountWith([event()])
    expect(wrapper.get('[data-day="2026-10-06"]').text()).toContain('Open Project Night')
    expect(wrapper.get('[data-day="2026-10-07"]').text()).not.toContain('Open Project Night')
  })

  it('does not push a late event onto the next day', async () => {
    // 2026-10-10T01:00Z is 9:00pm on the 9th in New York. A grid keyed on the
    // UTC date puts this on the 10th.
    const { wrapper } = await mountWith([
      event({ title: 'Late build', start: '2026-10-10T01:00:00Z', end: '2026-10-10T03:00:00Z' }),
    ])
    expect(wrapper.get('[data-day="2026-10-09"]').text()).toContain('Late build')
    expect(wrapper.get('[data-day="2026-10-10"]').text()).not.toContain('Late build')
  })

  it('spans an event that runs past midnight across both days', async () => {
    const { wrapper } = await mountWith([
      event({ title: 'All nighter', start: '2026-10-10T02:00:00Z', end: '2026-10-10T06:00:00Z' }),
    ])
    expect(wrapper.get('[data-day="2026-10-09"]').text()).toContain('All nighter')
    expect(wrapper.get('[data-day="2026-10-10"]').text()).toContain('All nighter')
  })

  it('spans a two-day all-day event across exactly its two days', async () => {
    // HONK! in the live feed: starts the 9th, DTEND the 11th (exclusive).
    const { wrapper } = await mountWith([
      event({
        title: 'HONK!',
        start: '2026-10-09T04:00:00Z',
        end: '2026-10-11T04:00:00Z',
        all_day: true,
      }),
    ])
    expect(wrapper.get('[data-day="2026-10-09"]').text()).toContain('HONK!')
    expect(wrapper.get('[data-day="2026-10-10"]').text()).toContain('HONK!')
    expect(wrapper.get('[data-day="2026-10-11"]').text()).not.toContain('HONK!')
  })

  it('shows a chip time for a timed event and none for an all-day one', async () => {
    const { wrapper } = await mountWith([
      event(),
      event({ title: 'Festival', start: '2026-10-06T04:00:00Z', all_day: true }),
    ])
    const cell = wrapper.get('[data-day="2026-10-06"]')
    expect(cell.text()).toContain('6:30 PM')
    const chips = cell.findAll('.chip')
    // All-day first, and it carries no time.
    expect(chips[0]?.text()).toBe('Festival')
  })

  it('caps the chips in a cell and says how many are hidden', async () => {
    // Five events on one day, as valid instants: `2026-10-06T24:00:00Z` is not
    // a date, and a fixture that quietly produces fewer events than it looks
    // like is how a cap test ends up asserting the wrong number.
    const many = [17, 18, 19, 20, 21].map((hour) =>
      event({ title: `Event ${hour}`, start: `2026-10-06T${hour}:00:00Z`, end: undefined })
    )
    const { wrapper } = await mountWith(many)
    const cell = wrapper.get('[data-day="2026-10-06"]')
    expect(cell.findAll('.chip')).toHaveLength(3)
    expect(cell.get('.chip-more').text()).toBe('+2 more')
  })
})

describe('the day panel', () => {
  it('lists the selected day’s events with times in the space’s zone', async () => {
    const { wrapper } = await mountWith([event()])
    await wrapper.get('[data-day="2026-10-06"]').trigger('click')
    const detail = wrapper.get('[data-testid="day-detail"]')
    expect(detail.text()).toContain('Tuesday, October 6, 2026')
    expect(detail.text()).toContain('6:30 PM')
    expect(detail.text()).toContain('10:00 PM')
    expect(detail.text()).toContain('EDT')
  })

  it('says so when a day has nothing on it', async () => {
    const { wrapper } = await mountWith([event()])
    await wrapper.get('[data-day="2026-10-07"]').trigger('click')
    expect(wrapper.get('[data-testid="day-detail"]').text()).toContain('Nothing scheduled.')
  })

  it('renders the server-rendered description as markup', async () => {
    const { wrapper } = await mountWith([
      event({
        description_html: '<p>Review <a href="https://example.org/pre">the reading</a></p>',
      }),
    ])
    await wrapper.get('[data-day="2026-10-06"]').trigger('click')
    const link = wrapper.get('[data-testid="day-detail"]').find('a')
    expect(link.attributes('href')).toBe('https://example.org/pre')
  })

  it('never renders a raw feed description as markup', async () => {
    // The fallback path, for a server that sends no rendered form. Interpolated
    // -- `children.length` is the assertion that distinguishes text from
    // markup, where innerHTML would contain an escaped copy either way.
    const { wrapper } = await mountWith([
      event({ description: '<img src=x onerror=alert(1)>and text' }),
    ])
    await wrapper.get('[data-day="2026-10-06"]').trigger('click')
    const description = wrapper.get('[data-testid="day-detail"]').get('.detail-description')
    expect(description.element.children.length).toBe(0)
    expect(description.text()).toBe('<img src=x onerror=alert(1)>and text')
    expect(wrapper.findAll('img')).toHaveLength(0)
  })

  it('escapes markup in a title and a location as well', async () => {
    const payload = '<img src=x onerror=alert(1)>'
    const { wrapper } = await mountWith([event({ title: payload, location: payload })])
    await wrapper.get('[data-day="2026-10-06"]').trigger('click')
    expect(wrapper.findAll('img')).toHaveLength(0)
    expect(wrapper.get('.detail-event-title').text()).toBe(payload)
  })
})

describe('paging', () => {
  it('steps to the next month and re-asks for that grid', async () => {
    const { wrapper, fetchMock } = await mountWith([])
    await wrapper.get('[aria-label="Next month"]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="month-title"]').text()).toBe('November 2026')
    expect(fetchMock.mock.calls.at(-1)?.[0]).toBe(
      '/api/calendar/events?from=2026-11-01&to=2026-12-05'
    )
  })

  it('steps back across a year boundary', async () => {
    const { wrapper } = await mountWith([])
    for (let i = 0; i < 10; i += 1) {
      await wrapper.get('[aria-label="Previous month"]').trigger('click')
      await flushPromises()
    }
    expect(wrapper.get('[data-testid="month-title"]').text()).toBe('December 2025')
  })

  it('comes back to today', async () => {
    const { wrapper } = await mountWith([])
    await wrapper.get('[aria-label="Next month"]').trigger('click')
    await flushPromises()
    await wrapper.get('.today-btn').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="month-title"]').text()).toBe('October 2026')
    expect(wrapper.get('[data-day="2026-10-14"]').classes()).toContain('selected')
  })
})

describe('failure', () => {
  it('shows the reason and offers a retry', async () => {
    const { wrapper } = await mountWith({ status: 503 })
    const error = wrapper.get('[role="alert"]')
    expect(error.text()).toContain('Service Unavailable')
    expect(wrapper.find('.retry-btn').exists()).toBe(true)
    // The grid is still drawn: a month with no events in it is a usable
    // calendar, and an error that blanks the page tells the reader less.
    expect(wrapper.findAll('[data-day]').length).toBeGreaterThan(0)
  })

  it('clears the error when a retry succeeds', async () => {
    const responses = [
      { ok: false, statusText: 'Bad Gateway' },
      { ok: true, statusText: 'OK', json: () => Promise.resolve([event()]) },
    ]
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve(responses.shift() as unknown as Response))
    )
    const wrapper = mount(CalendarMonth, { props: { timezone: NY } })
    await flushPromises()
    expect(wrapper.find('[role="alert"]').exists()).toBe(true)

    await wrapper.get('.retry-btn').trigger('click')
    await flushPromises()

    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
    expect(wrapper.get('[data-day="2026-10-06"]').text()).toContain('Open Project Night')
  })

  it('drops the previous month’s events rather than showing them on this one', async () => {
    // A failed reload that left the old list in place would show October's
    // events on November's grid, which is worse than showing none.
    const responses = [
      { ok: true, statusText: 'OK', json: () => Promise.resolve([event()]) },
      { ok: false, statusText: 'Gone' },
    ]
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve(responses.shift() as unknown as Response))
    )
    const wrapper = mount(CalendarMonth, { props: { timezone: NY } })
    await flushPromises()
    expect(wrapper.get('[data-day="2026-10-06"]').text()).toContain('Open Project Night')

    await wrapper.get('[aria-label="Next month"]').trigger('click')
    await flushPromises()

    expect(wrapper.text()).not.toContain('Open Project Night')
  })
})
