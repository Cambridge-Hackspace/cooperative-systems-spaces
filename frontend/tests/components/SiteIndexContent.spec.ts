// Tier 2: SiteIndexContent.
//
// Two hundred lines, most of them CSS, and one branch worth pinning: there
// isn't one for failure. The template is `v-if="content"`, `v-else-if="loading"`
// and nothing else, so a fetch that fails renders an empty div — no message, no
// retry, no trace that anything was attempted.
//
// It also reaches for `fetch` directly rather than `apiClient`, which is why it
// gets no interceptor, no base URL and no shared error handling. That is not
// asserted here — it is a structural claim and belongs to a check that reads
// the source — but it is the reason the failure path is missing rather than
// merely broken.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

import SiteIndexContent from '@/components/SiteIndexContent.vue'

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

function ok(body: unknown) {
  return Promise.resolve({ ok: true, json: () => Promise.resolve(body) })
}

const INDEX = {
  title: 'Welcome',
  html_content: '<h1>Welcome to the space</h1>',
  slug: 'index',
  relative_path: 'index.md',
}

describe('when the index loads', () => {
  it('renders the server-rendered HTML', async () => {
    fetchMock.mockReturnValue(ok(INDEX))
    const w = mount(SiteIndexContent)
    await flushPromises()

    expect(w.find('.markdown-content').html()).toContain('Welcome to the space')
  })

  it('asks for the index page and nothing else', async () => {
    fetchMock.mockReturnValue(ok(INDEX))
    mount(SiteIndexContent)
    await flushPromises()

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock.mock.calls[0][0]).toBe('/api/pages/page/index')
  })
})

describe('findings: a failed load is silent', () => {
  it('renders nothing at all when the request fails', async () => {
    // A PINNED FINDING, not a passing behaviour.
    //
    // The template has no failure branch. `content` stays null and `loading`
    // returns to false, so both conditions are false and the component renders
    // an empty div. A member sees a blank panel where the site index should be
    // and has no way to tell whether the space has no index page, the server is
    // down, or the page failed to load.
    //
    // The error goes to console.error, which nobody is reading.
    //
    // If this assertion fails, somebody added a failure state -- delete it and
    // assert the message instead. See TESTING.md, "Known defects".
    fetchMock.mockRejectedValue(new Error('Network error'))
    const w = mount(SiteIndexContent)
    await flushPromises()

    expect(
      w.text().trim(),
      'PINNED FINDING: a failed site-index fetch renders nothing -- no error, ' +
        'no retry, indistinguishable from a space that has no index page'
    ).toBe('')
  })

  it('renders nothing when the server refuses, too', async () => {
    // `if (response.ok)` with no else. A 500 takes the same silent path as a
    // network failure, and does not even reach the catch.
    fetchMock.mockResolvedValue({ ok: false, status: 500, json: () => Promise.resolve({}) })
    const w = mount(SiteIndexContent)
    await flushPromises()
    expect(w.text().trim()).toBe('')
  })
})
