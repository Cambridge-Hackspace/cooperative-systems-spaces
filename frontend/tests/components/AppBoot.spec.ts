// Tier 2: the boot sequence, and the overlay that gates every click.
//
// `App.vue`'s `onMounted` sets `globalLoading` true, awaits
// `Promise.all([authStore.initialize(), configStore.fetchConfig()])`, and
// clears it in a `finally`. While it is true the template renders
//
//     <div class="fixed inset-0 bg-black/50 ... z-50">
//
// which covers the entire viewport and intercepts every pointer event. That is
// the correct way to render a blocking load — and it means the *only* thing
// standing between a working application and a completely unusable one is that
// `finally` running.
//
// This file exists because the browser tier found the overlay still up after
// thirty seconds, on every page, with the form behind it filled in and
// unclickable. That is the highest-consequence failure mode in the frontend:
// not a wrong number or a missing message, but an application that renders
// perfectly and accepts no input at all.
//
// jsdom cannot tell you the overlay intercepts clicks — it runs no layout. What
// it can tell you is whether the overlay is still in the DOM, which is the same
// question one step earlier and costs a millisecond instead of a session.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createTestingPinia } from '@pinia/testing'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  post: vi.fn(),
  put: vi.fn(),
  patch: vi.fn(),
  delete: vi.fn(),
}))

vi.mock('@/utils/api', () => ({
  apiClient: mocks,
  // Every named export App.vue's tree might reach. A missing one is a module
  // error that reads like a component bug.
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
    useRouter: () => ({ push: vi.fn(), replace: vi.fn(), currentRoute: { value: {} } }),
  }
})

import App from '@/App.vue'

const OVERLAY = '.fixed.inset-0'

const stubs = {
  RouterLink: { template: '<a><slot /></a>' },
  RouterView: { template: '<div />' },
}

function mountApp() {
  return mount(App, {
    global: {
      plugins: [createTestingPinia({ createSpy: vi.fn, stubActions: false })],
      stubs,
      // App.vue's template reads `$route.name` directly for the nav's active
      // state. Without a router installed that is an undefined property, which
      // this suite treats as a failure -- correctly, since it is the same class
      // of thing as an unresolved component.
      mocks: { $route: { name: 'home', path: '/' } },
    },
  })
}

beforeEach(() => {
  localStorage.clear()
  for (const m of Object.values(mocks)) m.mockReset()
  // The one call the boot sequence makes.
  // `pages` included: App.vue's nav computes `shouldShowWikiInNav()` during
  // render, which reads `config.pages`. A fixture without it throws inside a
  // computed -- which surfaces as an unrelated-looking render error, not as
  // "your fixture is incomplete".
  // The shape `PublicConfig` declares. A fixture that invents one tests the
  // component against a server that does not exist -- which is how the browser
  // tier spent two cycles on a fake whose config had no `pages` block.
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

afterEach(() => {
  localStorage.clear()
})

describe('a failed config load is not silent', () => {
  it('records the failure somewhere the user could be told about', async () => {
    // A FINDING, recorded as an assertion on the current behaviour.
    //
    // `configStore.fetchConfig` has its own try/catch and does **not** rethrow:
    // it writes the message into `configStore.error` and returns normally. So
    // `Promise.all` in App.vue's onMounted resolves, its `catch` never runs, and
    // the "Initialization Error" notification it exists to raise is unreachable
    // for the one failure most likely to happen at boot.
    //
    // The application then runs with `config` at its defaults: no site name, no
    // page links, features gated off -- and nothing anywhere says the
    // configuration failed to load. It looks like an administrator has not set
    // anything up.
    //
    // The browser tier found it by arming a 500 on /config/public and waiting
    // for a notification that never came.
    mocks.get.mockRejectedValue(new Error('Network Error'))
    const wrapper = mountApp()
    await flushPromises()
    await nextTick()

    expect(
      wrapper.text(),
      'PINNED FINDING: no notification is shown when the boot configuration ' +
        'fails to load, because fetchConfig swallows the error rather than ' +
        'rethrowing it. If this assertion fails, somebody fixed it -- delete ' +
        'the assertion. See TESTING.md, "Known defects".'
    ).not.toContain('Initialization Error')
  })
})

describe('the loading overlay', () => {
  it('is up while the boot requests are in flight', async () => {
    mocks.get.mockReturnValue(new Promise(() => {}))
    const wrapper = mountApp()
    await nextTick()
    expect(wrapper.find(OVERLAY).exists()).toBe(true)
  })

  it('is gone once they resolve', async () => {
    // The assertion the browser tier failed on, one layer down. If this fails,
    // the application renders and accepts no input.
    const wrapper = mountApp()
    await flushPromises()
    await nextTick()

    expect(
      wrapper.find(OVERLAY).exists(),
      'the full-screen loading overlay is still in the DOM after boot. It has ' +
        '`fixed inset-0 ... z-50`, so it covers the viewport and swallows every ' +
        'click: the application renders correctly and is completely unusable.'
    ).toBe(false)
  })

  it('is gone even when a boot request fails', async () => {
    // The `finally` is what clears it, so a rejection must not strand it. This
    // is the case a person actually hits -- the server is down, and the app has
    // to stay usable enough to say so.
    mocks.get.mockRejectedValue(new Error('Network Error'))
    const wrapper = mountApp()
    await flushPromises()
    await nextTick()

    expect(
      wrapper.find(OVERLAY).exists(),
      'a failed boot request leaves the overlay up, so the user cannot even ' +
        'reach the login form to retry'
    ).toBe(false)
  })

  it('is gone when the config response is malformed', async () => {
    mocks.get.mockResolvedValue({ success: true, data: null })
    const wrapper = mountApp()
    await flushPromises()
    await nextTick()
    expect(wrapper.find(OVERLAY).exists()).toBe(false)
  })

  it('is gone when a stale token makes /auth/me fail', async () => {
    // `token` is read from localStorage at store construction, so a leftover
    // token sends `initialize()` down a path that talks to the server. If that
    // path can hang or throw past the `finally`, every returning user with an
    // expired session gets an unusable page.
    localStorage.setItem('css_token', 'stale')
    mocks.get.mockRejectedValue(new Error('Unauthorized'))
    const wrapper = mountApp()
    await flushPromises()
    await nextTick()
    expect(wrapper.find(OVERLAY).exists()).toBe(false)
  })
})
