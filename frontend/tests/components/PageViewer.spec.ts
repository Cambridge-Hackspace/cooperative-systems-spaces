// Tier 2: PageViewer.
//
// Renders one wiki or site page. Two things are worth reading carefully.
//
// It calls `fetch` directly rather than going through `utils/api`, so it gets
// no base URL, no auth header, and none of the envelope handling every other
// component relies on -- and it interpolates the slug straight into the path
// without encoding it.
//
// And it renders the server's HTML with `v-html`, with the lint rule disabled
// inline. That is safe *only* because `server/src/pages.rs` renders markdown
// through comrak with `Options::default()`, whose `render.unsafe_` is false
// and which therefore strips raw HTML. Nothing in this component would notice
// if that changed, so the guarantee is asserted where it lives:
// `tests/structure/markdown-rendering.spec.ts`.
//
// What this spec does NOT prove: that the server escapes anything. This
// component does no sanitising of its own, which is asserted here as a fact
// about the component rather than a claim about the system.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

import PageViewer from '@/components/PageViewer.vue'

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

interface PageBody {
  title: string
  html_content: string
  slug: string
  relative_path: string
  repo_url?: string
  default_branch?: string
}

function body(over: Partial<PageBody> = {}): PageBody {
  return {
    title: 'Lathe safety',
    html_content: '<h1>Lathe safety</h1><p>Guards on, sleeves up.</p>',
    slug: 'lathe',
    relative_path: 'lathe.md',
    ...over,
  }
}

const ok = (b: PageBody) => ({ ok: true, status: 200, json: () => Promise.resolve(b) })

async function viewer(props: { type: 'wiki' | 'site'; slug?: string }, b = body()) {
  fetchMock.mockResolvedValue(ok(b))
  const w = mount(PageViewer, { props })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof viewer>>

const url = () => fetchMock.mock.calls[0][0] as string

describe('which page is fetched', () => {
  it('reads a wiki page from the wiki endpoint', async () => {
    await viewer({ type: 'wiki', slug: 'lathe' })
    expect(url()).toBe('/api/pages/wiki/lathe')
  })

  it('reads a site page from the site endpoint', async () => {
    await viewer({ type: 'site', slug: 'about' })
    expect(url()).toBe('/api/pages/page/about')
  })

  it('fetches nothing at all without a slug, and says so', async () => {
    const w = await viewer({ type: 'wiki' })
    expect(fetchMock).not.toHaveBeenCalled()
    expect(w.find('.empty-state').exists()).toBe(true)
  })

  // `fetchPage` also sets `page.value = null` before awaiting. That line
  // cannot be observed from outside: the `v-if` chain puts `loading` ahead of
  // `page`, so while a re-fetch is in flight the loading state renders whether
  // or not the old page is still in the ref, and a failure renders the error
  // state for the same reason. An equivalent mutant, recorded rather than
  // chased -- the observable claim is the one this test makes.
  it('re-fetches when the slug changes, and shows the new page', async () => {
    const w = await viewer({ type: 'wiki', slug: 'lathe' })
    expect(w.find('.page-content').text()).toContain('Lathe safety')

    fetchMock.mockResolvedValue(ok(body({ title: 'Bandsaw', slug: 'bandsaw' })))
    await w.setProps({ slug: 'bandsaw' })
    await flushPromises()

    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(w.find('.page-content').text()).toContain('Bandsaw')
  })

  it('drops the page when the slug is cleared', async () => {
    const w = await viewer({ type: 'wiki', slug: 'lathe' })
    await w.setProps({ slug: undefined })
    await flushPromises()

    expect(w.find('.page-content').exists()).toBe(false)
    expect(w.find('.empty-state').exists()).toBe(true)
  })

  // FINDING, pinned. The slug is interpolated into the path with no
  // `encodeURIComponent`. A slug containing `?` or `#` truncates the request,
  // one containing `..` walks up the API path, and one containing a space
  // produces a malformed URL. Slugs come from `slug_from_path_static` on the
  // server today, so this is reachable only by whoever can name a file in the
  // wiki repository -- which, in a hackspace running a public wiki, is a
  // larger set than it sounds.
  it('does not encode the slug it interpolates into the path', async () => {
    await viewer({ type: 'wiki', slug: '../../admin/users' })
    expect(
      url(),
      'the slug is now encoded -- if `encodeURIComponent` was added, this test ' +
        'should assert the encoded form instead'
    ).toBe('/api/pages/wiki/../../admin/users')

    fetchMock.mockClear()
    fetchMock.mockResolvedValue(ok(body()))
    const w = mount(PageViewer, { props: { type: 'wiki', slug: 'a?b#c' } })
    await flushPromises()
    expect((fetchMock.mock.calls[0][0] as string).endsWith('a?b#c')).toBe(true)
    w.unmount()
  })

  // Recorded rather than pinned as a defect: this is the only component in the
  // tier-2 inventory that bypasses `utils/api` entirely. It therefore sends no
  // Authorization header and gets none of the envelope handling, which is why
  // its error path deals in `response.statusText` rather than the server's
  // error body. A `checks/` case for raw `fetch` outside `utils/api` is the
  // right home for the general rule.
  it('sends a bare fetch with no options at all', async () => {
    await viewer({ type: 'wiki', slug: 'lathe' })
    expect(fetchMock.mock.calls[0]).toHaveLength(1)
  })
})

describe('the three states', () => {
  it('shows a spinner while the request is in flight', async () => {
    fetchMock.mockReturnValue(new Promise(() => {}))
    const w = mount(PageViewer, { props: { type: 'wiki', slug: 'lathe' } })
    await flushPromises()

    expect(w.find('.loading-state').exists()).toBe(true)
    expect(w.find('.page-content').exists()).toBe(false)
  })

  it('names a missing page rather than restating the status', async () => {
    fetchMock.mockResolvedValue({ ok: false, status: 404, statusText: 'Not Found' })
    const w = mount(PageViewer, { props: { type: 'wiki', slug: 'nope' } })
    await flushPromises()

    expect(w.find('.error-state').text()).toContain('Page not found')
  })

  it('restates the status for anything else', async () => {
    fetchMock.mockResolvedValue({ ok: false, status: 401, statusText: 'Unauthorized' })
    const w = mount(PageViewer, { props: { type: 'wiki', slug: 'lathe' } })
    await flushPromises()

    expect(w.find('.error-state').text()).toContain('Failed to fetch page: Unauthorized')
  })

  it('reports a transport failure', async () => {
    fetchMock.mockRejectedValue(new Error('Network Error'))
    const w = mount(PageViewer, { props: { type: 'wiki', slug: 'lathe' } })
    await flushPromises()

    expect(w.find('.error-state').text()).toContain('Network Error')
  })

  it('offers a way back from the error state', async () => {
    fetchMock.mockResolvedValue({ ok: false, status: 404, statusText: 'Not Found' })
    const w = mount(PageViewer, { props: { type: 'wiki', slug: 'nope' } })
    await flushPromises()

    await w.find('.back-btn').trigger('click')
    expect(w.emitted('back')).toHaveLength(1)
  })

  it('clears a previous error when a new slug is requested', async () => {
    fetchMock.mockResolvedValue({ ok: false, status: 404, statusText: 'Not Found' })
    const w = mount(PageViewer, { props: { type: 'wiki', slug: 'nope' } })
    await flushPromises()
    expect(w.find('.error-state').exists()).toBe(true)

    fetchMock.mockResolvedValue(ok(body()))
    await w.setProps({ slug: 'lathe' })
    await flushPromises()
    expect(w.find('.error-state').exists()).toBe(false)
  })
})

describe('the rendered page', () => {
  it('renders the server HTML as markup, not as text', async () => {
    const w = await viewer({ type: 'wiki', slug: 'lathe' })
    expect(w.find('.page-body h1').text()).toBe('Lathe safety')
    expect(w.find('.page-body p').text()).toBe('Guards on, sleeves up.')
  })

  // Asserted as a fact about this component, not as a claim about the system:
  // whatever HTML arrives is inserted verbatim. There is no sanitising step
  // here, so the safety of this line rests entirely on the server rendering
  // markdown with comrak's raw-HTML passthrough off. That guarantee is checked
  // in tests/structure/markdown-rendering.spec.ts.
  it('inserts whatever HTML it is given, with no sanitising of its own', async () => {
    const w = await viewer(
      { type: 'wiki', slug: 'lathe' },
      body({ html_content: '<p onclick="x">hi</p><em>emphasis</em>' })
    )
    expect(w.find('.page-body').html()).toContain('onclick="x"')
    expect(w.find('.page-body em').exists()).toBe(true)
  })

  it('shows the page path', async () => {
    const w = await viewer(
      { type: 'wiki', slug: 'lathe' },
      body({ relative_path: 'shop/lathe.md' })
    )
    expect(w.find('.page-path').text()).toContain('shop/lathe.md')
  })
})

describe('the edit link', () => {
  const withRepo = (repo_url: string, over: Partial<PageBody> = {}) =>
    viewer({ type: 'wiki', slug: 'lathe' }, body({ repo_url, relative_path: 'lathe.md', ...over }))

  const href = (w: Wrapper) => w.find('.edit-link').attributes('href')

  it('is absent when the server sends no repository', async () => {
    const w = await viewer({ type: 'wiki', slug: 'lathe' })
    expect(w.find('.edit-link-container').exists()).toBe(false)
  })

  it('builds a GitHub edit URL', async () => {
    const w = await withRepo('https://github.com/space/wiki.git')
    expect(href(w)).toBe('https://github.com/space/wiki/edit/main/lathe.md')
    expect(w.find('.edit-link').text()).toContain('GitHub')
  })

  it('builds a GitLab edit URL, which uses a different path shape', async () => {
    const w = await withRepo('https://gitlab.com/space/wiki')
    expect(href(w)).toBe('https://gitlab.com/space/wiki/-/edit/main/lathe.md')
    expect(w.find('.edit-link').text()).toContain('GitLab')
  })

  it('builds a Gitea edit URL', async () => {
    const w = await withRepo('https://gitea.example.org/space/wiki')
    expect(href(w)).toBe('https://gitea.example.org/space/wiki/_edit/main/lathe.md')
    expect(w.find('.edit-link').text()).toContain('Gitea')
  })

  it('treats Codeberg as Gitea-shaped, because Forgejo is', async () => {
    const w = await withRepo('https://codeberg.org/space/wiki')
    expect(href(w)).toBe('https://codeberg.org/space/wiki/_edit/main/lathe.md')
    expect(w.find('.edit-link').text()).toContain('Codeberg')
  })

  it('honours a non-default branch', async () => {
    const w = await withRepo('https://github.com/space/wiki', { default_branch: 'trunk' })
    expect(href(w)).toBe('https://github.com/space/wiki/edit/trunk/lathe.md')
  })

  it('falls back to main when the server names no branch', async () => {
    const w = await withRepo('https://github.com/space/wiki')
    expect(href(w)).toContain('/edit/main/')
  })

  // Recorded: for a host it does not recognize the link goes to the repository
  // root rather than the file, and calls itself "Edit on Git". It is a
  // deliberate fallback rather than a broken link, but the label does not say
  // that the destination is different in kind.
  it('links to the repository root, still labeled as an edit, for an unknown host', async () => {
    const w = await withRepo('https://git.example.org/space/wiki.git')
    expect(href(w)).toBe('https://git.example.org/space/wiki')
    expect(w.find('.edit-link').text()).toContain('Git')
  })

  it('is absent when the server sends a repository but no path', async () => {
    const w = await viewer(
      { type: 'wiki', slug: 'lathe' },
      body({ repo_url: 'https://github.com/space/wiki', relative_path: '' })
    )
    expect(w.find('.edit-link-container').exists()).toBe(false)
  })
})
