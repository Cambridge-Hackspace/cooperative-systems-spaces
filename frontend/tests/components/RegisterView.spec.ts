// #120 (XSS): the terms-of-service text on the registration form.
//
// `terms_of_service_md` comes from /api/config/registration, which serves the
// server admin's config.toml. It was rendered with `v-html`, so any markup in
// that value became live DOM -- an <img onerror> or <script> the admin (or
// anyone who could influence that config) placed there would execute in every
// registrant's browser. The field is markdown and nothing converts it, so
// `v-html` bought no rendering either: markdown syntax showed literally under it
// exactly as it does under text interpolation. The fix renders it as text.
//
// The oracle is two-sided: the payload's characters must appear (it is still
// shown to the user) AND no element from the payload may exist in the DOM
// (it must not be parsed as HTML). Restoring `v-html` fails the second half --
// `find('img')` would then find the injected node.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'

import RegisterView from '@/views/RegisterView.vue'

const fetchMock = vi.fn()

const PAYLOAD = '<img src=x onerror="window.__xss_fired = true"><b>bold</b>'

function registrationConfig(overrides: Record<string, unknown> = {}) {
  return Promise.resolve({
    ok: true,
    json: () =>
      Promise.resolve({
        data: {
          registration_challenge: {
            enabled: false,
            hint: '',
            throttle_enabled: false,
            terms_of_service_checkbox: true,
            terms_of_service_md: PAYLOAD,
            recaptcha_enabled: false,
            recaptcha_site_key: '',
            ...overrides,
          },
        },
      }),
  })
}

function mountView() {
  return mount(RegisterView, {
    global: {
      stubs: { RouterLink: { template: '<a><slot /></a>' } },
    },
  })
}

beforeEach(() => {
  setActivePinia(createPinia())
  vi.stubGlobal('fetch', fetchMock)
  fetchMock.mockReset()
  ;(window as unknown as { __xss_fired?: boolean }).__xss_fired = undefined
})

afterEach(() => {
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('terms-of-service rendering', () => {
  it('shows the admin-set terms text to the registrant', async () => {
    fetchMock.mockReturnValue(registrationConfig())
    const w = mountView()
    await flushPromises()

    // Rendered as text: the literal characters are present so nothing is lost.
    expect(w.text()).toContain(PAYLOAD)
  })

  it('does not turn the terms markup into live DOM', async () => {
    fetchMock.mockReturnValue(registrationConfig())
    const w = mountView()
    await flushPromises()

    expect(
      w.find('img').exists(),
      'terms_of_service_md must be text, not HTML: an injected <img> in the ' +
        'admin config was parsed into the registration form'
    ).toBe(false)
    expect(w.find('b').exists()).toBe(false)
    expect((window as unknown as { __xss_fired?: boolean }).__xss_fired).toBeUndefined()
  })
})
