// Tier 1: `envelopeError`.
//
// The forty-two inline catches in `utils/api.ts` all did the same thing and all
// did it wrong: `error.message`, which for axios is the status restated
// ("Request failed with status code 409"), while the server's own explanation
// sat unread in `error.response.data.error`. Every component that showed a
// failure to a user showed the wrong half of it -- StartTrainingModal,
// EditTrainerModal, CompleteTrainingModal and DeviceManagement each have a
// pinned spec saying so.
//
// This is that logic in one place, so it is worth testing in one place. The
// component specs cannot cover it: they mock `@/utils/api` wholesale, so the
// whole suite passed with the helper untested, which is how this file came to
// be written.
//
// What this does NOT prove: that any caller reads `.success`. The helper still
// resolves rather than rejects, so a caller that ignores the flag still cannot
// tell a refusal from a success. That is fixed per component, not here.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { envelopeError } from '@/utils/api'

const axiosish = (status: number, body?: unknown) =>
  Object.assign(new Error(`Request failed with status code ${status}`), {
    response: { data: body },
  })

describe('which message reaches the user', () => {
  it("prefers the server's own words over the axios prose", () => {
    const r = envelopeError(axiosish(409, { error: 'That barcode is already in use' }), 'fallback')
    expect(r).toEqual({ success: false, error: 'That barcode is already in use' })
  })

  it('accepts `message` as well, because some bodies use that key', () => {
    const r = envelopeError(axiosish(400, { message: 'expires_at: premature end of input' }), 'x')
    expect(r.error).toBe('expires_at: premature end of input')
  })

  it('prefers `error` when a body carries both', () => {
    const r = envelopeError(axiosish(409, { error: 'the real one', message: 'the other' }), 'x')
    expect(r.error).toBe('the real one')
  })

  it('falls back to the axios message when the body says nothing', () => {
    expect(envelopeError(axiosish(500, {}), 'fallback').error).toBe(
      'Request failed with status code 500'
    )
  })

  it('falls back to the axios message when there is no response at all', () => {
    // The transport-failure shape: no `response`, which is the branch
    // DoorCheckinView's fix (92afb4c) exists for.
    expect(envelopeError(new Error('Network Error'), 'fallback').error).toBe('Network Error')
  })

  it('falls back to the caller-supplied text when there is nothing else', () => {
    expect(envelopeError({}, 'Failed to create tool').error).toBe('Failed to create tool')
    expect(envelopeError(undefined, 'Failed to create tool').error).toBe('Failed to create tool')
    expect(envelopeError(null, 'Failed to create tool').error).toBe('Failed to create tool')
  })

  it('ignores an empty or whitespace-only server message rather than showing it', () => {
    // A blank alert is worse than a generic one: it looks like the UI broke.
    expect(envelopeError(axiosish(500, { error: '' }), 'fallback').error).toBe(
      'Request failed with status code 500'
    )
    expect(envelopeError(axiosish(500, { error: '   ' }), 'fallback').error).toBe(
      'Request failed with status code 500'
    )
  })

  it('ignores a non-string server message', () => {
    // Postgres and serde have both been seen to put structured detail here.
    expect(envelopeError(axiosish(400, { error: { detail: 'nested' } }), 'fallback').error).toBe(
      'Request failed with status code 400'
    )
  })

  it('always reports failure, whatever it was given', () => {
    for (const input of [new Error('x'), {}, null, undefined, 'a string', 42]) {
      expect(envelopeError(input, 'fallback').success).toBe(false)
    }
  })
})

// ---------------------------------------------------------------------------
// `withErrorGuard`, and why these assertions moved here from the component.
//
// `DoorCheckinView` used to catch its own rejections. It no longer does: the
// guard wraps every method on `doorsApi`, so the guarantee is uniform and a
// method added tomorrow gets it without anybody remembering. That is broader
// than a per-call-site `try`, and it is the version kept.
//
// The cost is that the guarantee left the component's own module, and the
// component specs mock `@/utils/api` wholesale -- so a double that resolves
// envelopes proves nothing about what happens when axios rejects. These tests
// exercise the real `doorsApi` over a rejecting transport, which is the only
// place that question can honestly be asked.
//
// Mutation check: delete the `withErrorGuard(` wrapper around `doorsApi` and
// every test below fails with an unhandled rejection.
//
// What this does NOT prove: that the guard is applied to any other api object.
// It is not -- `doorsApi` is the only one wrapped today; the rest still catch
// inline through `envelopeError`.

// `vi.hoisted`, because `vi.mock` is lifted above every `const` in the file
// and `utils/api.ts` calls `axios.create()` at module scope.
const transport = vi.hoisted(() => ({
  get: vi.fn(),
  post: vi.fn(),
  put: vi.fn(),
  delete: vi.fn(),
}))

vi.mock('axios', () => ({
  default: {
    create: () => ({
      ...transport,
      interceptors: {
        request: { use: vi.fn() },
        response: { use: vi.fn() },
      },
    }),
  },
}))

describe('doorsApi never rejects, however the transport failed', () => {
  beforeEach(() => {
    for (const fn of Object.values(transport)) fn.mockReset()
  })

  it("reports the server's own words when the body carries them", async () => {
    const { doorsApi } = await import('@/utils/api')
    transport.get.mockRejectedValue(axiosish(403, { error: 'Door is not published' }))
    await expect(doorsApi.info('d1')).resolves.toEqual({
      success: false,
      error: 'Door is not published',
    })
  })

  it('reports the fallback when there is no response at all', async () => {
    // **The branch 92afb4c added.** A network failure, a DNS failure, a
    // cancelled request: `e.response` is undefined, so without a fallback the
    // alert renders empty -- a red box with no text, in a corridor, on
    // somebody's phone.
    //
    // Only a transport-level rejection reaches it. Injecting a 500 does not:
    // axios attaches a response to those, so a suite that only injects HTTP
    // errors reports this line as covered while never running it.
    const { doorsApi } = await import('@/utils/api')
    transport.get.mockRejectedValue(new Error('Network Error'))
    const r = await doorsApi.info('d1')
    expect(r.success).toBe(false)
    expect(r.error?.length ?? 0).toBeGreaterThan(0)
  })

  it('guards the action, not only the load', async () => {
    // Silence after pressing the button is indistinguishable from success to
    // somebody standing at a door that did not open.
    const { doorsApi } = await import('@/utils/api')
    transport.post.mockRejectedValue(new Error('Network Error'))
    const r = await doorsApi.checkin('d1')
    expect(r.success).toBe(false)
    expect(r.error?.length ?? 0).toBeGreaterThan(0)
  })

  it('guards every method the object exposes, not a remembered few', async () => {
    // The whole reason the guard is better than the `try` it replaced. If a
    // method is added to `doorsApi` outside the wrapper, this fails.
    const { doorsApi } = await import('@/utils/api')
    for (const fn of Object.values(transport)) fn.mockRejectedValue(new Error('Network Error'))
    // Typed at the entries, not per call: the methods have different arities,
    // and every argument is ignored anyway since the transport rejects before
    // any of them is read.
    type AnyCall = (...a: unknown[]) => Promise<{ success: boolean }>
    for (const [name, method] of Object.entries(doorsApi) as [string, AnyCall][]) {
      const r = await method('d1', {}, {}, {})
      expect(r, `doorsApi.${name} rejected instead of returning an envelope`).toMatchObject({
        success: false,
      })
    }
  })
})
