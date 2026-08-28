// Tier 2: ThemePicker.
//
// The theme LIST is already pinned by tests/structure/themes.spec.ts, which
// asserts the same set and the same order across tailwind.config.js and this
// component. This file is about behaviour instead: grouping, the guard against
// pointless writes, applying the theme to the document, and the failure path —
// which is missing.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createTestingPinia } from '@pinia/testing'
import { flushPromises, mount } from '@vue/test-utils'

// `vi.hoisted` and a direct reference, not a forwarding arrow. `vi.mock` is
// hoisted above every top-level `const`, so a factory that closes over an
// ordinary binding throws "Cannot access 'patch' before initialization" from
// inside the *component's* import -- and a forwarding `(...a) => patch(...a)`
// wrapper, the other way to defer it, returns `any`.
const mocks = vi.hoisted(() => ({ patch: vi.fn() }))
const patch = mocks.patch
vi.mock('@/utils/api', () => ({ apiClient: { patch: mocks.patch } }))

import ThemePicker from '@/components/ThemePicker.vue'
import { useAuthStore } from '@/stores/auth'

beforeEach(() => {
  patch.mockReset()
  vi.useFakeTimers()
  vi.spyOn(console, 'error').mockImplementation(() => {})
  document.documentElement.removeAttribute('data-theme')
})
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

function picker(user: Record<string, unknown> | null = { id: 'u1', meta: { theme: 'forest' } }) {
  const w = mount(ThemePicker, {
    global: { plugins: [createTestingPinia({ createSpy: vi.fn, stubActions: false })] },
  })
  const store = useAuthStore()
  ;(store as unknown as { user: unknown }).user = user
  return w
}

// Exact match on the trimmed label, not `includes`. 'Light' is a substring of
// 'CSS Light', so a loose match silently clicks the wrong theme and the test
// then asserts something true about a button nobody meant to press.
const findButton = (w: ReturnType<typeof picker>, label: string) =>
  w.findAll('button').find((b) => b.text().trim() === label)

// Throws rather than returning undefined. Under the non-strict base tsconfig a
// `!` at the call site is a no-op the linter correctly rejects, and without one
// a missing button surfaces as "Cannot read properties of undefined (reading
// 'trigger')" instead of naming the label it looked for.
function buttonFor(w: ReturnType<typeof picker>, label: string) {
  const b = findButton(w, label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}

describe('how themes are grouped', () => {
  it('renders one section per group, in the declared order', async () => {
    // `order = ['CSS', 'NEIAM', 'DAISY']` is applied deliberately rather than
    // relying on Map insertion order. Asserted through the sections and their
    // contents, because the names themselves are never shown -- see the finding
    // below.
    const w = picker()
    await flushPromises()
    const sections = w.findAll('.space-y-4')
    expect(sections).toHaveLength(3)
    expect(sections[0].text()).toContain('CSS Light')
    expect(sections[1].text()).toContain('After Dark')
    expect(sections[2].text()).toContain('Lo-Fi')
  })

  it('never shows the group names it went to the trouble of computing', async () => {
    // A PINNED FINDING, not a passing behaviour.
    //
    // `themeGroups` builds named groups and orders them CSS, NEIAM, DAISY. The
    // template renders that name into
    //
    //     <h3 class="text-lg font-semibold text-base-content/70"></h3>
    //
    // which is empty. So the user sees three unlabelled clusters of buttons
    // with no indication of what separates them, and the ordering work is
    // invisible. An empty <h3> is also a heading with no accessible name, which
    // a screen reader announces as a heading and then says nothing.
    //
    // If this fails, somebody filled the heading in -- delete it and assert the
    // names are shown.
    const w = picker()
    await flushPromises()
    const headings = w.findAll('h3')
    expect(headings.length).toBeGreaterThan(0)
    expect(
      headings.every((h) => h.text().trim() === ''),
      'PINNED FINDING: the group headings are empty, so CSS/NEIAM/DAISY are ' +
        'computed and never shown'
    ).toBe(true)
  })

  it('offers every theme it declares', async () => {
    const w = picker()
    await flushPromises()
    for (const label of ['CSS Light', 'After Dark', 'Lo-Fi', 'Corporate']) {
      expect(findButton(w, label), `${label} is missing from the picker`).toBeTruthy()
    }
  })
})

describe('choosing a theme', () => {
  it('writes it for the user and applies it to the document', async () => {
    patch.mockResolvedValue({ success: true, data: { id: 'u1', meta: { theme: 'sky' } } })
    const w = picker()
    await flushPromises()

    await buttonFor(w, 'Sky').trigger('click')
    await flushPromises()

    expect(patch).toHaveBeenCalledWith('/users/u1/theme', { theme: 'sky' })
    expect(
      document.documentElement.getAttribute('data-theme'),
      'the theme must be applied to the document immediately; waiting for a ' +
        'reload would make the picker look broken'
    ).toBe('sky')
  })

  it('does nothing when the chosen theme is already current', async () => {
    // `theme === currentTheme.value` returns early. Without it every click
    // writes to the server, and a user clicking their own theme gets a network
    // round-trip and a success banner for a change that did not happen.
    const w = picker({ id: 'u1', meta: { theme: 'forest' } })
    await flushPromises()
    await buttonFor(w, 'Forest').trigger('click')
    await flushPromises()
    expect(patch).not.toHaveBeenCalled()
  })

  it('defaults to light when the user has no theme recorded', async () => {
    const w = picker({ id: 'u1', meta: null })
    await flushPromises()
    // Light is current, so clicking it is the no-op case.
    await buttonFor(w, 'Light').trigger('click')
    await flushPromises()
    expect(patch).not.toHaveBeenCalled()
  })
})

describe('messages', () => {
  it('confirms the change, then clears itself', async () => {
    patch.mockResolvedValue({ success: true, data: { id: 'u1', meta: { theme: 'sky' } } })
    const w = picker()
    await flushPromises()
    await buttonFor(w, 'Sky').trigger('click')
    await flushPromises()

    expect(w.text()).toContain('Theme changed to sky')
    vi.advanceTimersByTime(3000)
    await flushPromises()
    expect(w.text()).not.toContain('Theme changed to sky')
  })

  it('reports a thrown failure and clears it later', async () => {
    patch.mockRejectedValue(new Error('Network down'))
    const w = picker()
    await flushPromises()
    await buttonFor(w, 'Sky').trigger('click')
    await flushPromises()

    expect(w.text()).toContain('Network down')
    vi.advanceTimersByTime(5000)
    await flushPromises()
    expect(w.text()).not.toContain('Network down')
  })
})

describe('findings: a refused write says nothing', () => {
  it('shows no message at all when the server answers success: false', async () => {
    // A PINNED FINDING, not a passing behaviour.
    //
    // `if (response.success) { ... }` has no else. A server that refuses the
    // change -- a role that may not set another user's theme, a validation
    // failure -- returns `success: false` without throwing, so the catch never
    // runs. The user sees the button they clicked do nothing: no confirmation,
    // no error, and the theme unchanged.
    //
    // This is the fourth component with this shape (AppBoot, SiteIndexContent,
    // the audit logger, this). Recorded in TESTING.md as a pattern rather than
    // four separate defects.
    //
    // If this fails, an else branch was added -- delete it and assert the
    // message instead.
    patch.mockResolvedValue({ success: false, error: 'Not permitted' })
    const w = picker()
    await flushPromises()
    await buttonFor(w, 'Sky').trigger('click')
    await flushPromises()

    expect(
      w.text().includes('Not permitted'),
      'PINNED FINDING: a refused theme change is silent -- the server said ' +
        '"Not permitted" and the user is shown nothing'
    ).toBe(false)
    expect(document.documentElement.getAttribute('data-theme')).not.toBe('sky')
  })
})
