// Tier 2: PageNavigation.
//
// Unlike its neighbour SiteIndexContent, this one has all three states —
// loading, error, and content — so most of what is asserted here is that they
// stay distinct. The states are chained `v-if` / `v-else-if` / `v-else`, which
// is the arrangement where a reordering silently makes one unreachable.
//
// The `type` prop is the interesting part. One endpoint returns both
// navigations and the component picks a field by prop: `wiki_nav` or
// `site_nav`. Nothing else in the component distinguishes them, so a wrong
// field name shows the other section's pages with no error anywhere — the wiki
// listing the site's pages looks like a content problem, not a code one.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

import PageNavigation from '@/components/PageNavigation.vue'

const fetchMock = vi.fn()

beforeEach(() => {
  vi.stubGlobal('fetch', fetchMock)
  fetchMock.mockReset()
  vi.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => {
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

const NAV = {
  wiki_nav: [
    { title: 'Lathe', slug: 'lathe', path: 'lathe.md', children: [] },
    {
      title: 'Safety',
      slug: 'safety',
      path: 'safety.md',
      children: [{ title: 'PPE', slug: 'safety/ppe', path: 'safety/ppe.md', children: [] }],
    },
  ],
  site_nav: [{ title: 'Opening hours', slug: 'hours', path: 'hours.md', children: [] }],
}

function ok(body: unknown) {
  return Promise.resolve({ ok: true, statusText: 'OK', json: () => Promise.resolve(body) })
}

async function nav(props: Record<string, unknown> = {}) {
  const w = mount(PageNavigation, { props: { type: 'wiki', ...props } })
  await flushPromises()
  return w
}

describe('which navigation is shown', () => {
  it('shows the wiki pages for type=wiki', async () => {
    fetchMock.mockReturnValue(ok(NAV))
    const w = await nav({ type: 'wiki' })
    expect(w.text()).toContain('Lathe')
    expect(w.text()).not.toContain('Opening hours')
  })

  it('shows the site pages for type=site', async () => {
    // The two differ only by which field of one response is read. Asserting
    // both directions is what catches a swapped field name, which otherwise
    // presents as the wiki listing the site's pages and no error at all.
    fetchMock.mockReturnValue(ok(NAV))
    const w = await nav({ type: 'site' })
    expect(w.text()).toContain('Opening hours')
    expect(w.text()).not.toContain('Lathe')
  })
})

describe('the three states stay distinct', () => {
  // Not `async`: mount is synchronous and the fetch never settles, so there is
  // nothing to await. An idle `async` here is what `require-await` exists to
  // catch -- it reads like the assertions wait for something, and they do not.
  it('shows a spinner while the request is in flight', () => {
    fetchMock.mockReturnValue(new Promise(() => {}))
    const w = mount(PageNavigation, { props: { type: 'wiki' } })
    expect(w.find('.loading-state').exists()).toBe(true)
    expect(w.find('.nav-list').exists()).toBe(false)
  })

  it('shows the error and no navigation when the server refuses', async () => {
    fetchMock.mockResolvedValue({ ok: false, statusText: 'Bad Gateway' })
    const w = await nav()
    expect(w.find('.error-state').exists()).toBe(true)
    expect(w.text()).toContain('Bad Gateway')
    expect(w.find('.nav-list').exists()).toBe(false)
  })

  it('says so when there are no pages, rather than showing an empty box', async () => {
    // Distinct from the error state on purpose: "this space has no wiki pages"
    // and "the wiki could not be loaded" are different things to tell somebody.
    fetchMock.mockReturnValue(ok({ wiki_nav: [], site_nav: [] }))
    const w = await nav()
    expect(w.find('.empty-state').exists()).toBe(true)
    expect(w.text()).toContain('No pages available')
  })
})

describe('expanding a page with children', () => {
  it('hides children until expanded, and marks the arrow', async () => {
    fetchMock.mockReturnValue(ok(NAV))
    const w = await nav()

    expect(w.text()).not.toContain('PPE')
    expect(w.text()).toContain('▶')

    await w.find('.expand-button').trigger('click')
    expect(w.text()).toContain('PPE')
    expect(w.text()).toContain('▼')
  })

  it('collapses again, because the toggle has to work both ways', async () => {
    // `toggleExpanded` reassigns the Set to trigger reactivity. Mutating a Set
    // in place does not, so an expand that works and a collapse that does not
    // would be the symptom of dropping that reassignment.
    fetchMock.mockReturnValue(ok(NAV))
    const w = await nav()
    await w.find('.expand-button').trigger('click')
    await w.find('.expand-button').trigger('click')
    expect(w.text()).not.toContain('PPE')
  })

  it('offers no expander for a page with no children', async () => {
    fetchMock.mockReturnValue(ok({ wiki_nav: [NAV.wiki_nav[0]], site_nav: [] }))
    const w = await nav()
    expect(w.findAll('.expand-button')).toHaveLength(0)
  })
})

describe('selection', () => {
  it('emits the slug instead of navigating', async () => {
    // `@click.prevent`. The href is there so the link is real to a browser and
    // to a screen reader, but the component owns the navigation.
    fetchMock.mockReturnValue(ok(NAV))
    const w = await nav()
    await w.find('.nav-item').trigger('click')
    expect(w.emitted('select')?.[0]).toEqual(['lathe'])
  })

  it('marks the current page', async () => {
    fetchMock.mockReturnValue(ok(NAV))
    const w = await nav({ currentSlug: 'safety' })
    const marked = w.findAll('.nav-item').filter((a) => a.classes().includes('active'))
    expect(marked).toHaveLength(1)
    expect(marked[0].text()).toContain('Safety')
  })
})
