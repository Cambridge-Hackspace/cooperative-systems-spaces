// Tier 2: nav menus close when you are done with them (#82).
//
// The nav opens menus two different ways and neither closed on its own:
//
//   * `<details><summary>` (the Admin submenu) stays open until something
//     clears its `open` attribute. Nothing did, so clicking a link inside left
//     the menu hanging over the page you had just navigated to, and clicking
//     elsewhere did nothing -- `<details>` has no native "click outside".
//   * daisyUI's focus-driven `.dropdown` (the mobile menu, the avatar menu) is
//     open while focus is inside it, and clicking a link within does not
//     reliably move focus out.
//
// This mounts the REAL App.vue and drives real DOM events at it, rather than
// re-implementing the handler in the test. A test holding its own copy of the
// logic passes whatever App.vue later does, which for a fix like this -- one
// small function, easy to "tidy" away -- is the failure mode most worth
// avoiding.
//
// What this does NOT prove: that daisyUI's CSS visually hides a blurred
// dropdown. That is the framework's behaviour; ours is clearing `open` and
// dropping focus, and that is what is asserted.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createTestingPinia } from '@pinia/testing'
import { flushPromises, mount } from '@vue/test-utils'

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  post: vi.fn(),
  put: vi.fn(),
  patch: vi.fn(),
  delete: vi.fn(),
}))

vi.mock('@/utils/api', () => ({
  apiClient: mocks,
  userApi: {},
  adminApi: {},
  profileApi: {},
  doorsApi: {},
  trainingApi: {},
  toolsApi: {},
}))

vi.mock('vue-router', async () => {
  const actual = await vi.importActual<Record<string, unknown>>('vue-router')
  return {
    ...actual,
    useRoute: () => ({ name: 'home', path: '/', params: {}, meta: {} }),
    useRouter: () => ({
      push: vi.fn(),
      replace: vi.fn(),
      currentRoute: { value: {} },
      // App.vue registers its dismissal on afterEach; capturing it lets the
      // "navigation closes the menu" case fire a real navigation.
      afterEach: (fn: () => void) => {
        afterEachHooks.push(fn)
      },
    }),
  }
})

let afterEachHooks: Array<() => void> = []

import App from '@/App.vue'

const stubs = {
  RouterLink: { template: '<a><slot /></a>' },
  RouterView: { template: '<div />' },
}

function mountApp() {
  return mount(App, {
    attachTo: document.body, // real focus/blur needs a document-attached tree
    global: {
      plugins: [createTestingPinia({ createSpy: vi.fn, stubActions: false })],
      stubs,
      mocks: { $route: { name: 'home', path: '/' } },
    },
  })
}

/** Put an open `<details>` menu in the nav, as the Admin submenu would be. */
function openAdminSubmenu(): HTMLDetailsElement {
  const nav = document.querySelector('nav')
  if (!nav) throw new Error('App.vue rendered no <nav>')
  const details = document.createElement('details')
  details.setAttribute('open', '')
  details.innerHTML = '<summary>Admin</summary><ul><li><a id="admin-link">Dashboard</a></li></ul>'
  nav.appendChild(details)
  return details
}

beforeEach(() => {
  localStorage.clear()
  afterEachHooks = []
  document.body.innerHTML = ''
  for (const m of Object.values(mocks)) m.mockReset()
  mocks.get.mockResolvedValue({
    success: true,
    data: {
      site: { site_name: 'Test Space' },
      pages: { wiki_enabled: false, wiki_link: 'None', site_enabled: false, site_link: 'None' },
      tools: { tool_categories: [] },
      registration_challenge: {
        enabled: false,
        hint: '',
        throttle_enabled: false,
        terms_of_service_checkbox: false,
        terms_of_service_md: '',
        recaptcha_enabled: false,
        recaptcha_site_key: '',
      },
    },
  })
})

describe('nav menu dismissal (#82)', () => {
  it('the fixture really does start with a menu open', async () => {
    // Anti-vacuity: every assertion below is "it closed", which passes
    // trivially against a menu that was never open in the first place.
    const w = mountApp()
    await flushPromises()
    const details = openAdminSubmenu()
    expect(details.hasAttribute('open')).toBe(true)
    w.unmount()
  })

  it('a click elsewhere on the page closes an open submenu', async () => {
    const w = mountApp()
    await flushPromises()
    const details = openAdminSubmenu()

    document.body.dispatchEvent(new MouseEvent('click', { bubbles: true }))

    expect(details.hasAttribute('open')).toBe(false)
    w.unmount()
  })

  it('a click inside the submenu leaves it open', async () => {
    // Using a menu is not dismissing it. Closing on every click would make a
    // submenu impossible to click through.
    const w = mountApp()
    await flushPromises()
    const details = openAdminSubmenu()

    const link = document.getElementById('admin-link')
    // Not `link?.dispatchEvent` -- if the fixture ever loses this anchor the
    // click silently never happens, the submenu stays open because nothing
    // touched it, and the assertion below passes for the wrong reason.
    if (!link) throw new Error('fixture is missing #admin-link')
    link.dispatchEvent(new MouseEvent('click', { bubbles: true }))

    expect(details.hasAttribute('open')).toBe(true)
    w.unmount()
  })

  it('Escape closes an open submenu', async () => {
    const w = mountApp()
    await flushPromises()
    const details = openAdminSubmenu()

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))

    expect(details.hasAttribute('open')).toBe(false)
    w.unmount()
  })

  it('navigating closes an open submenu', async () => {
    // The original complaint: click a link in the menu, arrive at the new page,
    // and the menu is still sitting on top of it.
    const w = mountApp()
    await flushPromises()
    const details = openAdminSubmenu()
    expect(afterEachHooks.length).toBeGreaterThan(0)

    afterEachHooks.forEach((fn) => fn())

    expect(details.hasAttribute('open')).toBe(false)
    w.unmount()
  })

  it('does not steal focus from something outside the nav', async () => {
    // Blurring indiscriminately would yank focus out of whatever the user is
    // typing in, which is a worse bug than the one being fixed.
    const w = mountApp()
    await flushPromises()
    const input = document.createElement('input')
    document.body.appendChild(input)
    input.focus()
    expect(document.activeElement).toBe(input)

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))

    expect(document.activeElement).toBe(input)
    w.unmount()
  })

  it('stops listening once the app is unmounted', async () => {
    // A listener that outlives its component is a leak, and in a test suite it
    // is also cross-test interference.
    const w = mountApp()
    await flushPromises()
    w.unmount()

    const orphan = document.createElement('details')
    orphan.setAttribute('open', '')
    const nav = document.createElement('nav')
    nav.appendChild(orphan)
    document.body.appendChild(nav)

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))

    expect(orphan.hasAttribute('open')).toBe(true)
  })
})
