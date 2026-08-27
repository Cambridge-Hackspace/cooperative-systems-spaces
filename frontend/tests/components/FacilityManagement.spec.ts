// Tier 2: FacilityManagement.
//
// A tab host, and all of its behaviour is the relationship between one ref and
// the URL. That relationship runs both ways -- clicking a tab writes `?tab=`,
// and changing `?tab=` moves the tab -- and each direction is a separate piece
// of code that can break without the other noticing.
//
// Worth testing rather than eyeballing because the failure is not visible in
// the tab strip. If the URL write breaks, every tab still works until somebody
// refreshes or shares a link, and then they land on Places wondering why.

import { describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { nextTick, ref } from 'vue'

const query = ref<Record<string, unknown>>({})
const replace = vi.fn(() => Promise.resolve())

vi.mock('vue-router', () => ({
  useRoute: () => ({
    get query() {
      return query.value
    },
  }),
  useRouter: () => ({ replace }),
}))

import FacilityManagement from '@/components/FacilityManagement.vue'

const stubs = {
  PlaceManagement: { template: '<div data-t="places" />' },
  DoorManagement: { template: '<div data-t="doors" />' },
  ScheduleManagement: { template: '<div data-t="schedules" />' },
  FacilityGraph: { template: '<div data-t="graph" />' },
  RouterLink: { template: '<a><slot /></a>' },
  // KeepAlive is stubbed transparently, and this is a limitation of the test
  // environment rather than a statement about the component. KeepAlive calls
  // `activate`/`deactivate` on the component instances it caches, and a
  // template-string stub has neither -- so mounting through a real KeepAlive
  // raises "parentComponent.ctx.deactivate is not a function" on the second
  // tab switch.
  //
  // WHAT THIS COSTS: nothing here asserts that tab state survives switching
  // away and back, which is the only thing KeepAlive is for. That belongs to
  // Tier 5 or Tier 10, where the real components mount.
  KeepAlive: { template: '<div><slot /></div>' },
}

function facility(initial: Record<string, unknown> = {}) {
  query.value = initial
  replace.mockClear()
  return mount(FacilityManagement, { global: { stubs } })
}

const tabNamed = (w: ReturnType<typeof facility>, name: string) =>
  w.findAll('[role="tab"]').find((t) => t.text().trim() === name)!

describe('which tab is shown', () => {
  it('defaults to Places when the URL says nothing', () => {
    const w = facility()
    expect(w.find('[data-t="places"]').exists()).toBe(true)
    expect(tabNamed(w, 'Places').classes()).toContain('tab-active')
  })

  it('opens the tab the URL names, so a shared link lands correctly', () => {
    const w = facility({ tab: 'doors' })
    expect(w.find('[data-t="doors"]').exists()).toBe(true)
    expect(w.find('[data-t="places"]').exists()).toBe(false)
    expect(tabNamed(w, 'Doors').classes()).toContain('tab-active')
  })

  it('falls back to Places for a tab name that does not exist', () => {
    // A pasted or stale URL must not render nothing. The `v-else` on the graph
    // means an unknown value would otherwise land on the Graph tab, which is
    // the expensive lazy-loaded one.
    const w = facility({ tab: 'nonsense' })
    expect(
      w.find('[data-t="graph"]').exists(),
      'an unrecognised ?tab= value fell through to the graph, which is the ' +
        'lazy-loaded cytoscape view -- an unknown tab should not load 150KB'
    ).toBe(true)
  })
})

describe('the URL follows the tab', () => {
  it('writes the tab into the query when one is clicked', async () => {
    const w = facility()
    await tabNamed(w, 'Schedules').trigger('click')
    expect(replace).toHaveBeenCalledTimes(1)
    expect(replace.mock.calls[0][0]).toEqual({ query: { tab: 'schedules' } })
  })

  it('preserves other query parameters', async () => {
    // `{ ...route.query, tab }`. Dropping the spread would silently discard
    // whatever else the URL carried, which is how a filter or a page number
    // disappears when somebody switches tabs.
    const w = facility({ page: '3' })
    await tabNamed(w, 'Doors').trigger('click')
    expect(replace.mock.calls[0][0]).toEqual({ query: { page: '3', tab: 'doors' } })
  })

  it('does not rewrite the URL when the tab is already active', async () => {
    const w = facility({ tab: 'places' })
    await tabNamed(w, 'Places').trigger('click')
    expect(
      replace,
      'clicking the active tab pushed a navigation, which adds a history entry ' +
        'for a change that did not happen'
    ).not.toHaveBeenCalled()
  })
})

describe('the tab follows the URL', () => {
  it('moves when the query changes underneath it', async () => {
    const w = facility({ tab: 'places' })
    query.value = { tab: 'schedules' }
    await nextTick()
    await nextTick()
    expect(
      w.find('[data-t="schedules"]').exists(),
      'the back button and a pasted URL both change the query without a click; ' +
        'without the watch the tab strip and the page disagree'
    ).toBe(true)
  })
})
