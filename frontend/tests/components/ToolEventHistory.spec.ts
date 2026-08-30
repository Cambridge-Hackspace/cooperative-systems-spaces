// Tier 2: ToolEventHistory.
//
// A read-only modal over one request, and the request handling is the finding.
// The component does this:
//
//     const response = await toolsApi.getToolEvents(props.tool.id)
//     events.value = response.data || []          // <- `success` is never read
//
// -- while `api.ts:591` makes `getToolEvents` swallow its own rejection and
// return `{ success: false, error, data: [] }`. Put together, every way this
// can fail lands on the same screen: an empty list, rendered as
// "No events recorded for this tool."
//
// So the modal does not merely fail quietly. It states something false. A
// member who lacks the staff role to read a tool's history is told the tool has
// no history, and so is anyone whose network dropped.
//
// The `v-else-if="error"` branch works; nothing can reach it. Both halves are
// asserted below.
//
// What this spec does NOT prove: that the server actually 403s a non-staff
// caller on this route. That is Tier 4's.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({ getToolEvents: vi.fn() }))
vi.mock('@/utils/api', () => ({ toolsApi: mocks }))

import ToolEventHistory from '@/components/ToolEventHistory.vue'
import { ToolStatus, type Tool, type ToolEvent } from '@/types/tools'

const TOOL = { id: 'tool-1', name: 'Lathe' } as unknown as Tool

function event(over: Partial<ToolEvent> = {}): ToolEvent {
  return {
    id: 'e1',
    tool_id: 'tool-1',
    event_type: 'status_change',
    created_at: '2026-01-14T09:30:00Z',
    ...over,
  }
}

beforeEach(() => {
  mocks.getToolEvents.mockReset()
  mocks.getToolEvents.mockResolvedValue({ success: true, data: [] })
})

async function history(events: ToolEvent[] = []) {
  mocks.getToolEvents.mockResolvedValue({ success: true, data: events })
  const w = mount(ToolEventHistory, { props: { tool: TOOL } })
  await flushPromises()
  return w
}

describe('the shell', () => {
  it('names the tool it is showing history for', async () => {
    expect((await history()).find('.modal-header h3').text()).toBe('Lathe - Event History')
  })

  it('closes from the overlay, the header button and Close', async () => {
    const w = await history()
    await w.find('.modal-overlay').trigger('click')
    await w.find('.close-btn').trigger('click')
    await w
      .findAll('button')
      .filter((b) => b.text().trim() === 'Close')[0]
      .trigger('click')
    expect(w.emitted('close')).toHaveLength(3)
  })

  it('does not close when the content is clicked', async () => {
    const w = await history()
    await w.find('.modal-content').trigger('click')
    expect(w.emitted('close')).toBeUndefined()
  })

  it('says it is loading while the request is in flight', async () => {
    mocks.getToolEvents.mockReturnValue(new Promise(() => {}))
    const w = mount(ToolEventHistory, { props: { tool: TOOL } })

    // `loading` starts false and is set inside `loadEvents`, which `onMounted`
    // calls after the first render -- so for exactly one tick the modal shows
    // "No events recorded for this tool." before the spinner. Not pinned as a
    // finding: it is a single frame, invisible in practice. Recorded because
    // the assertion below would otherwise look like it needed the tick for no
    // reason.
    expect(w.find('.no-events').exists()).toBe(true)
    await nextTick()
    expect(w.find('.loading').text()).toBe('Loading events...')
    expect(w.find('.no-events').exists()).toBe(false)
  })
})

describe('an event as rendered', () => {
  it('titles the event type in words and stamps it with the local time', async () => {
    const w = await history([event({ event_type: 'status_change' })])
    expect(w.find('.event-type').text()).toContain('Status Change')
    expect(w.find('.event-date').text()).toBe(new Date('2026-01-14T09:30:00Z').toLocaleString())
  })

  it('gives each known event type its own icon and its own class', async () => {
    // Exhaustive over the icon table, so a type added to the CSS or the API
    // without an icon shows up here rather than silently taking the fallback.
    const known = [
      ['created', '🆕'],
      ['status_change', '🔄'],
      ['maintenance', '🔧'],
      ['scan', '📱'],
      ['checkout', '📤'],
      ['return', '📥'],
      ['deleted', '🗑️'],
    ] as const

    const w = await history(known.map(([t], i) => event({ id: `e${i}`, event_type: t })))
    const items = w.findAll('.event-item')

    known.forEach(([type, icon], i) => {
      expect(items[i].find('.event-icon').text()).toBe(icon)
      expect(items[i].classes()).toContain(`event-${type}`)
    })
  })

  it('falls back to a generic icon for a type it does not know', async () => {
    const w = await history([event({ event_type: 'quarantined' })])
    expect(w.find('.event-icon').text()).toBe('📝')
    expect(w.find('.event-type').text()).toContain('Quarantined')
  })

  it('shows both sides of a status change with an arrow between them', async () => {
    const w = await history([
      event({ old_status: ToolStatus.InUse, new_status: ToolStatus.Maintenance }),
    ])
    expect(w.find('.old-status').text()).toBe('In Use')
    expect(w.find('.new-status').text()).toBe('Maintenance')
    expect(w.find('.arrow').exists()).toBe(true)
  })

  it('renders a readable label for every status the enum defines', async () => {
    // The expected labels are written out rather than recomputed from
    // `formatStatus`'s own regexes. A check derived from the thing it checks
    // agrees with itself no matter what either of them does.
    const EXPECTED: Record<ToolStatus, string> = {
      [ToolStatus.Idle]: 'Idle',
      [ToolStatus.InUse]: 'In Use',
      [ToolStatus.Maintenance]: 'Maintenance',
      [ToolStatus.Broken]: 'Broken',
      [ToolStatus.Repair]: 'Repair',
      [ToolStatus.Retired]: 'Retired',
    }
    // Exhaustive both ways: a status added to the enum has no entry here and
    // fails to compile, and one removed leaves an entry with no member.
    const all = Object.values(ToolStatus)
    expect(Object.keys(EXPECTED).sort()).toEqual([...all].sort())

    const w = await history(all.map((s, i) => event({ id: `e${i}`, new_status: s })))
    w.findAll('.new-status').forEach((el, i) => {
      expect(el.text(), `label for ${all[i]}`).toBe(EXPECTED[all[i]])
    })
  })

  it('omits the arrow when only one side is known', async () => {
    const w = await history([event({ new_status: ToolStatus.Idle })])
    expect(w.find('.arrow').exists()).toBe(false)
    expect(w.find('.old-status').exists()).toBe(false)
    expect(w.find('.new-status').text()).toBe('Idle')
  })

  it('omits the status row entirely when neither side is known', async () => {
    expect((await history([event()])).find('.status-change').exists()).toBe(false)
  })

  it('attributes the event when a username came back', async () => {
    const w = await history([event({ user_username: 'ada' })])
    expect(w.find('.event-user').text()).toBe('By: ada')
  })

  it('shows no attribution when none came back', async () => {
    expect((await history([event()])).find('.event-user').exists()).toBe(false)
  })

  it('surfaces the two metadata keys it knows about', async () => {
    const w = await history([event({ metadata: { scan_data: 'QR-77', checkout_duration: '2h' } })])
    expect(w.find('.scan-data').text()).toContain('QR-77')
    expect(w.find('.checkout-duration').text()).toContain('2h')
  })

  it('shows nothing for metadata keys it does not know about', async () => {
    // Recorded, not pinned as a defect: the template reads exactly two keys.
    // Anything else the server attaches is dropped, which is a display choice
    // rather than a bug -- but it does mean this modal is not a full audit
    // view, and a reader should not treat it as one.
    const w = await history([event({ metadata: { reason: 'blade replaced' } })])
    expect(w.text()).not.toContain('blade replaced')
  })

  it('shows notes when there are any', async () => {
    const w = await history([event({ notes: 'Belt slipping under load.' })])
    expect(w.find('.event-notes').text()).toContain('Belt slipping under load.')
  })
})

describe('when there is nothing to show', () => {
  it('says so for a tool that genuinely has no events', async () => {
    expect((await history([])).find('.no-events').text()).toBe('No events recorded for this tool.')
  })

  // FINDING, pinned. `events.value = response.data || []` never reads
  // `response.success`. `api.ts:591` catches its own rejection and returns
  // `{ success: false, error, data: [] }` -- so a 403, a 500 and a dropped
  // connection all arrive here as an empty array and are rendered as a
  // statement of fact about the tool.
  //
  // This is the shape below the one seen elsewhere on this branch. A blank
  // screen is unhelpful; this is confidently wrong.
  it('reports a refusal instead of claiming the tool has no events', async () => {
    mocks.getToolEvents.mockResolvedValue({
      success: false,
      error: 'Forbidden',
      data: [],
    })
    const w = mount(ToolEventHistory, { props: { tool: TOOL } })
    await flushPromises()

    expect(w.find('.error').text()).toBe('Forbidden')
    expect(w.find('.no-events').exists()).toBe(false)
  })
})

describe('the error branch', () => {
  // The template has one, and it works -- but nothing in production can reach
  // it, because `toolsApi.getToolEvents` never rejects. Asserted as a pair so
  // the dead branch is documented as dead rather than mistaken for coverage.
  it('renders on a rejection too', async () => {
    mocks.getToolEvents.mockRejectedValue(new Error('Network Error'))
    const w = mount(ToolEventHistory, { props: { tool: TOOL } })
    await flushPromises()

    expect(w.find('.error').text()).toBe('Network Error')
    expect(w.find('.no-events').exists()).toBe(false)
  })

  // FINDING, pinned. Even reached, it reads the wrong key. The server's
  // envelope is `{ success, data, message, error }` and every refusal fills
  // `error` (api/errors.rs:111 onwards); this catch reads
  // `err.response?.data?.message`, which is never populated, so the server's
  // own words are discarded in favor of the generic fallback.
  it("shows the server's own reason, from the key the envelope fills", async () => {
    mocks.getToolEvents.mockRejectedValue({
      response: { data: { success: false, error: 'Staff role required' } },
    })
    const w = mount(ToolEventHistory, { props: { tool: TOOL } })
    await flushPromises()

    expect(w.find('.error').text()).toBe('Staff role required')
  })

  it('stops loading whether the request resolved or rejected', async () => {
    mocks.getToolEvents.mockRejectedValue(new Error('Network Error'))
    const w = mount(ToolEventHistory, { props: { tool: TOOL } })
    await flushPromises()
    expect(w.find('.loading').exists()).toBe(false)
  })
})
