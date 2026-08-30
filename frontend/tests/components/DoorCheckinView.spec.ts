// Tier 2: conformance for DoorCheckinView.
//
// This is the phone-first QR flow: somebody scans a code on a door, the page
// tells them whether they may come in, and a button energises a relay. It is
// the highest-consequence screen in the application and the one most likely to
// be looked at once, in a corridor, by somebody who cannot debug it.
//
// It is also the subject of `92afb4c`, and the shape of that fix is what this
// file is built around. The error handler reads
// `e?.response?.data?.error || 'Failed to load door'`, and the fallback branch
// is only reachable when `e.response` is undefined — which happens on a
// *transport* failure, not on a 500. A suite that only ever injects HTTP error
// responses never executes it, which is exactly how it came to be missing.
//
// So the failure tests here inject two different shapes deliberately: a
// rejection carrying a response, and a rejection carrying none.
//
// WHAT THIS DOES NOT PROVE. That the button actually opens a door. The relay is
// on the other side of MQTT and an edge device, and the only tier that can see
// it is the stack battery's `doors` stage, which asserts the
// `door_access_events` row and the message the edge received alongside the HTTP
// response. Here the claim is narrower and still worth making: that the button
// is offered exactly when the server said the person is authorized, and never
// otherwise.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

type Envelope = Promise<{ success: boolean; error?: string; data?: unknown }>

// `vi.hoisted`, because `vi.mock` runs before every `const` in this file. See
// the same note in RosterTable.spec.ts: the failure surfaces inside the
// component's own import and reads like an application problem.
const mocks = vi.hoisted(() => ({ info: vi.fn(), checkin: vi.fn() }))

vi.mock('@/utils/api', () => ({
  doorsApi: { info: mocks.info, checkin: mocks.checkin },
  // The module also exports apiClient; anything importing it in this tree gets
  // a stub rather than an axios instance that would try to reach the network.
  apiClient: {},
}))

const info = mocks.info as unknown as ReturnType<typeof vi.fn<(id: string) => Envelope>>
const checkin = mocks.checkin as unknown as ReturnType<typeof vi.fn<(id: string) => Envelope>>

vi.mock('vue-router', () => ({
  useRoute: () => ({ params: { id: '00000000-0000-4000-8000-000000000001' } }),
}))

import DoorCheckinView from '@/views/DoorCheckinView.vue'

const stubs = { RouterLink: { template: '<a><slot /></a>' } }

function door(overrides: Record<string, unknown> = {}) {
  return {
    id: '00000000-0000-4000-8000-000000000001',
    name: 'Front Door',
    location: null,
    enabled: true,
    you_are_authorized: true,
    reason: null,
    ...overrides,
  }
}

async function mountView() {
  const wrapper = mount(DoorCheckinView, { global: { stubs } })
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(new Date('2026-03-12T18:00:00Z'))
  info.mockReset()
  checkin.mockReset()
  info.mockResolvedValue({ success: true, data: door() })
  checkin.mockResolvedValue({ success: true, data: { unlocked: true, reason: null } })
})

afterEach(() => {
  vi.useRealTimers()
})

describe('the four states of the door', () => {
  it('shows a spinner and nothing else until the door loads', () => {
    info.mockReturnValue(new Promise(() => {}))
    const wrapper = mount(DoorCheckinView, { global: { stubs } })

    expect(wrapper.find('.loading-spinner').exists()).toBe(true)
    expect(wrapper.find('button').exists()).toBe(false)
    expect(wrapper.find('.alert').exists()).toBe(false)
  })

  it('offers the unlock button to somebody who is authorized', async () => {
    const wrapper = await mountView()
    expect(wrapper.find('.alert-success').text()).toBe('You are authorized.')
    expect(wrapper.find('button').attributes('disabled')).toBeUndefined()
    expect(wrapper.find('button').text()).toContain("I'm here")
  })

  it('disables the button and says why when the person is not authorized', async () => {
    info.mockResolvedValue({
      success: true,
      data: door({ you_are_authorized: false, reason: 'Outside opening hours' }),
    })
    const wrapper = await mountView()

    expect(wrapper.find('.alert-error').text()).toContain('Not authorized')
    // The server's reason, verbatim. "No matching access rule" when the real
    // reason was a schedule sends somebody to the wrong person for help.
    expect(wrapper.find('.alert-error').text()).toContain('Outside opening hours')
    expect(wrapper.find('button').attributes('disabled')).toBeDefined()
  })

  it('falls back to a generic reason rather than showing nothing', async () => {
    info.mockResolvedValue({
      success: true,
      data: door({ you_are_authorized: false, reason: null }),
    })
    const wrapper = await mountView()
    expect(wrapper.find('.alert-error').text()).toContain('No matching access rule.')
  })

  it('reports a disabled door as disabled, whatever the authorization says', async () => {
    // The precedence matters. Telling somebody they are authorized for a door
    // that is switched off sends them to stand in front of it.
    const wrapper = await mountViewWith(door({ enabled: false, you_are_authorized: true }))
    expect(wrapper.find('.alert-warning').text()).toBe('This door is currently disabled.')
    expect(wrapper.find('.alert-success').exists()).toBe(false)
    expect(wrapper.find('button').attributes('disabled')).toBeDefined()
  })

  it('shows the door name and location it was given', async () => {
    const wrapper = await mountViewWith(door({ name: 'Metal Shop', location: 'Bay 3' }))
    expect(wrapper.find('.card-title').text()).toBe('Metal Shop')
    expect(wrapper.text()).toContain('Bay 3')
  })

  it('renders no location line when the door has none', async () => {
    const wrapper = await mountView()
    expect(wrapper.find('.text-base-content\\/70').exists()).toBe(false)
  })
})

describe('the load failure paths, both of them', () => {
  it('shows the server error when the envelope reports failure', async () => {
    info.mockResolvedValue({ success: false, error: 'Door not found' })
    const wrapper = await mountView()
    expect(wrapper.find('.alert-error').text()).toBe('Door not found')
    expect(wrapper.find('button').exists()).toBe(false)
  })

  // `doorsApi` is wrapped in `withErrorGuard`, so its contract is that it
  // resolves an envelope and never rejects — including for a transport
  // failure, which is the branch 92afb4c added. That guarantee lives in
  // `utils/api.ts` now rather than in this component's own `try`, so it is
  // asserted in `tests/unit/api-envelope.spec.ts` against the real `doorsApi`
  // over a rejecting transport. Asserting it here would only exercise this
  // file's own mock.
  //
  // What is left for this component is the half it still owns: showing the
  // envelope's message, and having a message to show when the envelope
  // carries none.

  it("shows the server's own words when the load is refused", async () => {
    info.mockResolvedValue({ success: false, error: 'Door is not published' })
    const wrapper = await mountView()
    expect(wrapper.find('.alert-error').text()).toBe('Door is not published')
  })

  it('shows a fallback rather than an empty red box when the envelope has no message', async () => {
    // A red alert with no text, in a corridor, on somebody's phone.
    info.mockResolvedValue({ success: false })
    const wrapper = await mountView()
    expect(wrapper.find('.alert-error').text()).toBe('Failed to load door')
  })

  it('stops the spinner however the load failed', async () => {
    for (const envelope of [
      { success: false, error: 'nope' },
      { success: false },
      { success: true, data: undefined },
    ]) {
      info.mockReset()
      info.mockResolvedValue(envelope)
      const wrapper = await mountView()
      expect(wrapper.find('.loading-spinner').exists()).toBe(false)
    }
  })
})

describe('the unlock', () => {
  it('sends the door id from the route', async () => {
    const wrapper = await mountView()
    await wrapper.find('button').trigger('click')
    await flushPromises()
    expect(checkin).toHaveBeenCalledWith('00000000-0000-4000-8000-000000000001')
  })

  it('reports the unlock', async () => {
    const wrapper = await mountView()
    await wrapper.find('button').trigger('click')
    await flushPromises()

    const alerts = wrapper.findAll('.alert-success')
    expect(alerts.some((a) => a.text().includes('Door unlocked'))).toBe(true)
  })

  it('reports a refusal as a refusal, with the reason', async () => {
    checkin.mockResolvedValue({ success: true, data: { unlocked: false, reason: 'Relay offline' } })
    const wrapper = await mountView()
    await wrapper.find('button').trigger('click')
    await flushPromises()

    const alert = wrapper.findAll('.alert-error').find((a) => a.text().includes('Did not unlock'))
    expect(alert, 'a refusal must not be presented as a success').toBeTruthy()
    expect(alert?.text()).toContain('Relay offline')
  })

  it('surfaces a failed unlock rather than appearing to have worked', async () => {
    // The other half of the 92afb4c shape, on the action rather than the load.
    // Silence after pressing the button is indistinguishable from success to
    // somebody standing at a door that did not open. An envelope with no
    // message is the case that used to render nothing at all.
    checkin.mockResolvedValue({ success: false })
    const wrapper = await mountView()
    await wrapper.find('button').trigger('click')
    await flushPromises()

    const alert = wrapper.findAll('.alert-error').find((a) => a.text().includes('Did not unlock'))
    expect(alert?.text()).toContain('Check-in failed')
  })

  it('never calls the endpoint for somebody who is not authorized', async () => {
    // The button is disabled, but `checkin` guards again. Belt and braces is
    // right here: a disabled attribute is a hint to a browser, and this
    // endpoint opens a door.
    const wrapper = await mountViewWith(door({ you_are_authorized: false }))
    await wrapper.find('button').trigger('click')
    await flushPromises()
    expect(checkin).not.toHaveBeenCalled()
  })

  it('refuses a second press within ten seconds and says so', async () => {
    // Local debounce, so one person with a phone cannot cycle the relay.
    const wrapper = await mountView()

    await wrapper.find('button').trigger('click')
    await flushPromises()
    expect(checkin).toHaveBeenCalledTimes(1)

    vi.setSystemTime(new Date('2026-03-12T18:00:05Z'))
    await wrapper.find('button').trigger('click')
    await flushPromises()

    expect(checkin).toHaveBeenCalledTimes(1)
    expect(wrapper.text()).toContain('Please wait a few seconds')
  })

  it('allows another attempt once the interval has passed', async () => {
    // The debounce must expire. A guard that never releases turns one failed
    // attempt into a door somebody cannot open until they reload the page --
    // and they have no reason to think reloading would help.
    const wrapper = await mountView()

    await wrapper.find('button').trigger('click')
    await flushPromises()

    vi.setSystemTime(new Date('2026-03-12T18:00:11Z'))
    await wrapper.find('button').trigger('click')
    await flushPromises()

    expect(checkin).toHaveBeenCalledTimes(2)
  })

  it('clears the previous result before the next attempt', async () => {
    // A stale "Door unlocked" above a fresh attempt in flight is the worst
    // possible reading of the screen.
    checkin.mockResolvedValue({ success: true, data: { unlocked: true, reason: null } })
    const wrapper = await mountView()
    await wrapper.find('button').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('Door unlocked')

    vi.setSystemTime(new Date('2026-03-12T18:00:11Z'))
    let resolve: (v: unknown) => void = () => {}
    checkin.mockReturnValue(new Promise((r) => (resolve = r)))
    await wrapper.find('button').trigger('click')

    expect(wrapper.text()).not.toContain('Door unlocked')
    resolve({ success: true, data: { unlocked: true, reason: null } })
  })
})

async function mountViewWith(data: ReturnType<typeof door>) {
  info.mockResolvedValue({ success: true, data })
  return mountView()
}
