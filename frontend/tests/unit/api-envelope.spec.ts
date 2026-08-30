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

import { describe, expect, it } from 'vitest'
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
