// The rung between Tier 2 and Tier 5, and the reason it exists.
//
// Every other component spec mocks `@/utils/api` wholesale. That is the right
// default -- it keeps a component test about the component -- but it means the
// api layer's own behavior is invisible to them, and `doorsApi` is wrapped in
// `withErrorGuard`, so what a component actually receives on a failure is
// decided there rather than in the double.
//
// The browser tier covers the seam. It also needs a full stack, a container
// engine and a Playwright image, none of which exist on the FreeBSD
// workstation, so it only runs in a reaper session or CI. That gap cost two
// round trips in a row on this branch, and both were the *same seam*:
//
//   1. A dropped connection showed "Network Error" -- axios's own developer
//      string -- because `envelopeError` preferred `e.message` over the
//      caller's fallback.
//   2. Once that was fixed, it showed "Door request failed", because
//      `withErrorGuard` supplied a generic message of its own, which shadowed
//      the specific one this component already had.
//
// Both are invisible to a spec that mocks the api and to a spec that tests the
// api without a component. So this file mounts the real component over the
// real `utils/api`, mocking only the axios transport underneath -- the same
// composition the browser tier exercises, minus the browser.
//
// WHAT THIS DOES NOT PROVE. Anything about CSS, layout, or a real network. A
// jsdom `.alert-error` that is present but invisible passes here and fails
// there. The browser tier remains the authority; this makes its two most
// expensive findings reproducible in a second on any machine.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const transport = vi.hoisted(() => ({
  get: vi.fn(),
  post: vi.fn(),
  put: vi.fn(),
  delete: vi.fn(),
}))

// Only axios. `@/utils/api` is deliberately real.
vi.mock('axios', () => ({
  default: {
    create: () => ({
      ...transport,
      interceptors: { request: { use: vi.fn() }, response: { use: vi.fn() } },
    }),
  },
}))

vi.mock('vue-router', () => ({
  useRoute: () => ({ params: { id: '00000000-0000-4000-8000-000000000001' } }),
}))

import DoorCheckinView from '@/views/DoorCheckinView.vue'

const stubs = { RouterLink: { template: '<a><slot /></a>' } }

const ok = (data: unknown) => ({ data: { success: true, data } })

const door = {
  id: '00000000-0000-4000-8000-000000000001',
  name: 'Front Door',
  location: null,
  enabled: true,
  you_are_authorized: true,
}

async function view() {
  const w = mount(DoorCheckinView, { global: { stubs } })
  await flushPromises()
  return w
}

describe('what a user sees when the connection drops', () => {
  beforeEach(() => {
    for (const fn of Object.values(transport)) fn.mockReset()
    // Silence the deliberate warn from envelopeError; asserted in
    // tests/unit/api-envelope.spec.ts rather than here.
    vi.spyOn(console, 'warn').mockImplementation(() => {})
  })

  it("shows the page's own words for a failed load, not axios's", async () => {
    transport.get.mockRejectedValue(new Error('Network Error'))
    const w = await view()

    const alert = w.find('.alert-error')
    expect(alert.exists()).toBe(true)
    expect(alert.text()).toBe('Failed to load door')
  })

  it("shows the page's own words for a failed check-in, not axios's", async () => {
    transport.get.mockResolvedValue(ok(door))
    const w = await view()

    transport.post.mockRejectedValue(new Error('Network Error'))
    await w.find('button').trigger('click')
    await flushPromises()

    expect(w.text()).toContain('Check-in failed')
  })

  it('never renders a string axios wrote, whatever the failure shape', async () => {
    // The general form, so neither of the two regressions above can come back
    // through a shape the two tests above happen not to use.
    const axiosProse = [
      new Error('Network Error'),
      new Error('timeout of 10000ms exceeded'),
      Object.assign(new Error('Request failed with status code 500'), {
        response: { data: {} },
      }),
    ]

    for (const failure of axiosProse) {
      transport.get.mockReset()
      transport.get.mockRejectedValue(failure)
      const w = await view()
      const text = w.find('.alert-error').text()

      expect(text, `axios prose reached the user: ${text}`).toBe('Failed to load door')
      w.unmount()
    }
  })

  it("still shows the server's own words when there are any", async () => {
    // The fallback must not swallow a real explanation. Both regressions were
    // fixed by narrowing what the api layer says; this is the boundary of that
    // narrowing.
    transport.get.mockRejectedValue({
      response: { data: { error: 'Door is not published' } },
    })
    const w = await view()

    expect(w.find('.alert-error').text()).toBe('Door is not published')
  })
})
