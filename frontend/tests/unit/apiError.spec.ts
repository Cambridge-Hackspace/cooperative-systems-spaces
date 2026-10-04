import { describe, it, expect } from 'vitest'
import { apiErrorMessage } from '@/utils/apiError'

/**
 * The defect this exists for: a tool edit that failed returned
 * "Request failed with status code 422" and nothing else, because the only
 * reader was `err.response.data.error` and a deserialization rejection carries
 * a plain-text body instead of our envelope. The operator could not say which
 * field was wrong, so the real cause stayed unidentified.
 */
describe('apiErrorMessage', () => {
  it('prefers the API envelope, which is what a handler-level refusal carries', () => {
    const err = { response: { status: 409, data: { success: false, error: 'Name already taken' } } }
    expect(apiErrorMessage(err, 'fallback')).toBe('Name already taken')
  })

  it('surfaces a plain-text framework rejection with its status', () => {
    const err = {
      response: {
        status: 422,
        data: 'Failed to deserialize the JSON body into the target type: purchase_price: invalid type: string "12.00", expected a number',
      },
      message: 'Request failed with status code 422',
    }
    const msg = apiErrorMessage(err, 'fallback')
    expect(msg).toContain('422')
    expect(msg).toContain('purchase_price')
    // The bare axios message must NOT win here -- that is the whole defect.
    expect(msg).not.toBe('Request failed with status code 422')
  })

  it('falls back to the transport message when there is no body at all', () => {
    const err = { message: 'Network Error' }
    expect(apiErrorMessage(err, 'fallback')).toBe('Network Error')
  })

  it('uses the caller fallback when there is nothing to report', () => {
    expect(apiErrorMessage({}, 'Failed to update tool')).toBe('Failed to update tool')
    expect(apiErrorMessage(undefined, 'Failed to update tool')).toBe('Failed to update tool')
  })

  it('does not mistake an empty envelope field for a message', () => {
    const err = { response: { status: 500, data: { error: '   ' } }, message: 'Server Error' }
    expect(apiErrorMessage(err, 'fallback')).toBe('Server Error')
  })

  it('reads `message` from the envelope when `error` is absent', () => {
    const err = { response: { status: 400, data: { message: 'tool_id is required' } } }
    expect(apiErrorMessage(err, 'fallback')).toBe('tool_id is required')
  })
})
