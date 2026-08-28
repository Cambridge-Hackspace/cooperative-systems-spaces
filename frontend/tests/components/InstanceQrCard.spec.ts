// Tier 2: InstanceQrCard.
//
// The component renders an image, so almost nothing about it is visible to a
// test — a QR code is a data URL either way. What matters is what went INTO it,
// and that is a decision the code documents:
//
//   Encode the full payload JSON (not just the URL) so client onboarding flows
//   can read the display name without a second round-trip.
//
// Encoding `p.url` instead would produce a perfectly good QR code, scannable,
// pointing at the right instance, and would break onboarding in a way no
// screenshot and no visual review would catch. That is the assertion this file
// exists for; the rest is state handling.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

// `vi.hoisted` and direct references, not forwarding arrows. `vi.mock` is
// hoisted above every top-level `const`, so a factory that closes over an
// ordinary binding throws "Cannot access 'get' before initialization" from
// inside the *component's* import -- and a forwarding `(...a) => get(...a)`
// wrapper, the other way to defer it, returns `any`.
const mocks = vi.hoisted(() => ({ toDataURL: vi.fn(), get: vi.fn() }))
const toDataURL = mocks.toDataURL
const get = mocks.get

vi.mock('qrcode', () => ({ default: { toDataURL: mocks.toDataURL } }))
vi.mock('@/utils/api', () => ({ apiClient: { get: mocks.get } }))

import InstanceQrCard from '@/components/InstanceQrCard.vue'

const PAYLOAD = { v: 1, url: 'https://space.example.org', name: 'Cambridge Hackspace' }

beforeEach(() => {
  get.mockReset()
  toDataURL.mockReset()
  toDataURL.mockResolvedValue('data:image/png;base64,QR')
})
afterEach(() => vi.restoreAllMocks())

async function card() {
  const w = mount(InstanceQrCard)
  await flushPromises()
  await flushPromises() // the watch on payload renders the QR a tick later
  return w
}

describe('what is encoded in the code', () => {
  it('encodes the whole payload, not just the URL', async () => {
    get.mockResolvedValue({ success: true, data: PAYLOAD })
    await card()

    expect(toDataURL).toHaveBeenCalledTimes(1)
    const encoded = toDataURL.mock.calls[0][0] as string
    const decoded: unknown = JSON.parse(encoded)

    expect(
      decoded,
      'the code must carry the whole payload: an onboarding client reads the ' +
        'display name from it, and encoding only the URL would still scan, ' +
        'still point at the right instance, and still be wrong'
    ).toEqual(PAYLOAD)
  })

  it('re-renders nothing when there is no payload', async () => {
    get.mockResolvedValue({ success: false, error: 'nope' })
    await card()
    expect(toDataURL).not.toHaveBeenCalled()
  })
})

describe('the three states', () => {
  it('shows a spinner while loading', () => {
    get.mockReturnValue(new Promise(() => {}))
    const w = mount(InstanceQrCard)
    expect(w.text()).toContain('Loading')
    expect(w.find('img').exists()).toBe(false)
  })

  it('shows the instance name and URL beside the code', async () => {
    get.mockResolvedValue({ success: true, data: PAYLOAD })
    const w = await card()
    expect(w.find('img').attributes('src')).toBe('data:image/png;base64,QR')
    expect(w.text()).toContain('Cambridge Hackspace')
    expect(w.text()).toContain('https://space.example.org')
  })

  it('reports the server’s own message when the request fails', async () => {
    // `resp.error || 'Failed to fetch instance QR'`. The server's message is
    // preferred because it can say something specific; the fallback exists for
    // a transport failure that carries none.
    get.mockResolvedValue({ success: false, error: 'Instance not configured' })
    const w = await card()
    expect(w.text()).toContain('Instance not configured')
    expect(w.find('img').exists()).toBe(false)
  })

  it('falls back to its own message when the failure carries none', async () => {
    get.mockRejectedValue(new Error(''))
    const w = await card()
    expect(w.text()).toContain('Failed to fetch instance QR')
  })

  it('reports a QR rendering failure rather than showing an empty frame', async () => {
    // The fetch succeeded and the encode did not. Without this branch the
    // component would show the name and URL beside a blank square, which reads
    // as a broken image rather than an error.
    get.mockResolvedValue({ success: true, data: PAYLOAD })
    toDataURL.mockRejectedValue(new Error('canvas unavailable'))
    const w = await card()
    expect(w.text()).toContain('canvas unavailable')
  })
})

describe('the alt text', () => {
  it('names what the image is, for anyone who cannot see it', async () => {
    get.mockResolvedValue({ success: true, data: PAYLOAD })
    const w = await card()
    expect(w.find('img').attributes('alt')).toBe('Instance QR code')
  })
})
