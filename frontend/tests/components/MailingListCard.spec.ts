// Tier 2: MailingListCard.
//
// The member's own view of their Groups.io mailing-list subscription. It reads
// GET /groupsio/subscription on mount and writes PUT on toggle, through the
// typed apiClient (envelope handling applies, so failures surface as the
// envelope's `error` or a thrown message).
//
// What this spec does NOT prove: that the endpoints answer, or that the module
// is gated -- ProfileView decides whether to render this at all (Tier 2 in its
// own spec), and the server tier owns the endpoint behavior.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  put: vi.fn(),
}))
vi.mock('@/utils/api', () => ({ apiClient: mocks }))

import MailingListCard from '@/components/MailingListCard.vue'

function ok<T>(data: T) {
  return { success: true, data }
}

beforeEach(() => {
  mocks.get.mockReset()
  mocks.put.mockReset()
})

describe('MailingListCard', () => {
  it('reflects a verified, subscribed member: toggle on, no verification note', async () => {
    mocks.get.mockResolvedValue(ok({ subscribed: true, email_verified: true }))
    const w = mount(MailingListCard)
    await flushPromises()

    expect(mocks.get).toHaveBeenCalledWith('/groupsio/subscription')
    const toggle = w.find('.mailing-list-toggle').element as HTMLInputElement
    expect(toggle.checked).toBe(true)
    expect(w.text()).toContain('Subscribed')
    expect(w.find('[role="note"]').exists()).toBe(false)
  })

  it('warns when the address is not verified yet', async () => {
    mocks.get.mockResolvedValue(ok({ subscribed: true, email_verified: false }))
    const w = mount(MailingListCard)
    await flushPromises()

    const note = w.find('[role="note"]')
    expect(note.exists()).toBe(true)
    expect(note.text()).toContain('not verified')
  })

  it('unsubscribing PUTs subscribed:false and updates the label', async () => {
    mocks.get.mockResolvedValue(ok({ subscribed: true, email_verified: true }))
    mocks.put.mockResolvedValue(ok({ subscribed: false, email_verified: true }))
    const w = mount(MailingListCard)
    await flushPromises()

    await w.find('.mailing-list-toggle').setValue(false)
    await flushPromises()

    expect(mocks.put).toHaveBeenCalledWith('/groupsio/subscription', { subscribed: false })
    expect(w.text()).toContain('Not subscribed')
    const toggle = w.find('.mailing-list-toggle').element as HTMLInputElement
    expect(toggle.checked).toBe(false)
  })

  it('rolls the toggle back and shows an error when the write fails', async () => {
    mocks.get.mockResolvedValue(ok({ subscribed: true, email_verified: true }))
    mocks.put.mockResolvedValue({ success: false, error: 'boom' })
    const w = mount(MailingListCard)
    await flushPromises()

    await w.find('.mailing-list-toggle').setValue(false)
    await flushPromises()

    expect(w.find('[role="alert"]').text()).toContain('boom')
    // The visible state returns to the server's last-confirmed value.
    const toggle = w.find('.mailing-list-toggle').element as HTMLInputElement
    expect(toggle.checked).toBe(true)
    expect(w.text()).toContain('Subscribed')
  })
})
