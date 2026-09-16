// The navigation tree, as #81 left it.
//
// The server used to drop any folder that had no same-named page at the root,
// and this component could only render two levels anyway. Between them, a wiki
// of 67 pages showed 7 in the sidebar -- every page still served, none of them
// findable -- and the administrator who pressed Refresh was told "67 pages
// loaded" while nothing on screen changed.
//
// The backend half is covered by `css_lib::nav`. What is asserted here is the
// half that lives in the browser: that a folder without a page is a control
// rather than a broken link, that nesting renders past the second level, and
// that sibling folders stay distinct even though neither has a slug.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

import PageNavigation from '@/components/PageNavigation.vue'
import { ancestorKeys, keyOf, type NavItem } from '@/types/nav'

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

const group = (title: string, path: string, children: NavItem[]): NavItem => ({
  title,
  slug: '',
  path,
  children,
})
const leaf = (title: string, slug: string): NavItem => ({
  title,
  slug,
  path: `${slug}.md`,
  children: [],
})

// The live wiki's shape: folders with no page of their own, three deep.
const TREE: NavItem[] = [
  group('Tools', 'TOOLS', [
    group('Lasers', 'TOOLS/LASERS', [leaf('Muse', 'tools/lasers/muse')]),
    leaf('Lathe', 'tools/lathe'),
  ]),
  group('Policies', 'POLICIES', [leaf('Code of Conduct', 'policies/code-of-conduct')]),
  leaf('Index', 'index'),
]

function ok(body: unknown) {
  return Promise.resolve({ ok: true, statusText: 'OK', json: () => Promise.resolve(body) })
}

async function nav(props: Record<string, unknown> = {}) {
  fetchMock.mockReturnValue(ok({ wiki_nav: TREE, site_nav: [] }))
  const w = mount(PageNavigation, { props: { type: 'wiki', baseUrl: '/wiki', ...props } })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof nav>>

// Fail loudly when a node is missing rather than reaching for `!`. Most of the
// assertions below are about something being *shown*, so a lookup that quietly
// yielded undefined would take the test with it.
function groupNamed(w: Wrapper, title: string) {
  const b = w.findAll('button.nav-group').find((x) => x.find('.nav-title').text() === title)
  if (!b) throw new Error(`no group header titled "${title}"`)
  return b
}

function linkNamed(w: Wrapper, title: string) {
  const a = w.findAll('a.nav-item').find((x) => x.find('.nav-title').text() === title)
  if (!a) throw new Error(`no page link titled "${title}"`)
  return a
}

describe('a folder with no page of its own', () => {
  it('is rendered as a control, not as a link', async () => {
    // Two oracles, because either alone can pass on a broken component: there
    // must BE a button for the group, and there must be NO anchor pointing at
    // the empty slug. A component that rendered both would satisfy the first.
    const w = await nav()

    const groupTitles = w.findAll('button.nav-group').map((b) => b.find('.nav-title').text())
    expect(groupTitles).toContain('Tools')

    const hrefs = w.findAll('a.nav-item').map((a) => a.attributes('href'))
    expect(hrefs).not.toContain('/wiki/')
    expect(hrefs).not.toContain('/wiki/undefined')
  })

  it('opens and closes on click', async () => {
    const w = await nav()
    expect(w.text()).not.toContain('Lathe')

    await groupNamed(w, 'Tools').trigger('click')
    expect(w.text()).toContain('Lathe')

    await groupNamed(w, 'Tools').trigger('click')
    expect(w.text()).not.toContain('Lathe')
  })

  it('reports its open state to assistive technology', async () => {
    const w = await nav()
    expect(groupNamed(w, 'Tools').attributes('aria-expanded')).toBe('false')
    await groupNamed(w, 'Tools').trigger('click')
    expect(groupNamed(w, 'Tools').attributes('aria-expanded')).toBe('true')
  })
})

describe('nesting past the second level', () => {
  it('renders a grandchild page', async () => {
    // The old template hard-coded item -> children and stopped. `Muse` lives at
    // Tools > Lasers > Muse, so it could not be shown at all.
    const w = await nav()
    await groupNamed(w, 'Tools').trigger('click')
    await groupNamed(w, 'Lasers').trigger('click')

    expect(linkNamed(w, 'Muse').attributes('href')).toBe('/wiki/tools/lasers/muse')
  })
})

describe('sibling folders stay distinct', () => {
  it('opening one group does not open the other', async () => {
    // Both groups have an empty slug. Keying or tracking expansion on the slug
    // makes them the same node: opening Tools would open Policies as well, and
    // Vue would reuse one's DOM for the other.
    const w = await nav()
    await groupNamed(w, 'Policies').trigger('click')

    expect(w.text()).toContain('Code of Conduct')
    expect(w.text()).not.toContain('Lathe')
  })
})

describe('the branch you are already in', () => {
  it('is open on arrival', async () => {
    const w = await nav({ currentSlug: 'tools/lasers/muse' })
    expect(linkNamed(w, 'Muse').classes()).toContain('active')
  })

  it('is the only branch opened', async () => {
    const w = await nav({ currentSlug: 'tools/lasers/muse' })
    expect(w.text()).not.toContain('Code of Conduct')
  })

  it('leaves everything closed when no page is current', async () => {
    const w = await nav()
    expect(w.text()).not.toContain('Muse')
    expect(w.text()).not.toContain('Lathe')
  })
  it('opens when the reader moves to a page in another branch', async () => {
    // The sidebar is mounted once and the slug changes under it, so this needs
    // a watcher rather than only the arrival path. A link in the body of one
    // page can land you in a branch the sidebar has never opened.
    const w = await nav()
    expect(w.text()).not.toContain('Muse')

    await w.setProps({ currentSlug: 'tools/lasers/muse' })
    await flushPromises()

    expect(linkNamed(w, 'Muse').classes()).toContain('active')
  })

  it('does not close branches the reader opened by hand', async () => {
    // Revealing the current branch must *add* to what is open, not replace it.
    // Assigning the ancestor set wholesale reads as equivalent and quietly
    // collapses everything else the reader was using.
    const w = await nav()
    await groupNamed(w, 'Policies').trigger('click')
    expect(w.text()).toContain('Code of Conduct')

    await w.setProps({ currentSlug: 'tools/lasers/muse' })
    await flushPromises()

    expect(linkNamed(w, 'Muse').classes()).toContain('active')
    expect(w.text()).toContain('Code of Conduct')
  })
})

describe('ancestorKeys', () => {
  it('names every node from the root down to the page', () => {
    expect(ancestorKeys(TREE, 'tools/lasers/muse')).toEqual([
      'TOOLS',
      'TOOLS/LASERS',
      'tools/lasers/muse',
    ])
  })

  it('is empty for a page that is not in the tree', () => {
    expect(ancestorKeys(TREE, 'nope/not/here')).toEqual([])
  })

  it('is empty when nothing is current', () => {
    expect(ancestorKeys(TREE, undefined)).toEqual([])
  })
})

describe('keyOf', () => {
  it('prefers the slug, which is unique per page', () => {
    expect(keyOf(leaf('Lathe', 'tools/lathe'))).toBe('tools/lathe')
  })

  it('falls back to the path for a group, which has no slug', () => {
    expect(keyOf(group('Tools', 'TOOLS', []))).toBe('TOOLS')
  })

  it('falls back to the title when a group has no path either', () => {
    // The server derives a group's path from a page beneath it, so this needs
    // an empty folder to happen -- but an empty string key is shared by every
    // such node, and a shared key is a rendering bug rather than a blank entry.
    expect(keyOf({ title: 'Orphan', slug: '', path: '', children: [] })).toBe('Orphan')
  })
})
