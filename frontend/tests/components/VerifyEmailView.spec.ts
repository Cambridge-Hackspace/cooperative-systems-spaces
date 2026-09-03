// Tier 2: confirming an address from an emailed link.
//
// The interesting case is the failing one. A confirmation link that has expired
// leaves an account that cannot sign in -- `require_email_verification` refuses
// the login -- and cannot confirm, because the link is spent. Without a resend
// affordance on this page that account is dead until an administrator edits the
// database, so the failure path is tested at least as carefully as the success.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'

const mocks = vi.hoisted(() => ({ verifyEmail: vi.fn(), resendVerification: vi.fn() }))

vi.mock('@/utils/api', () => ({
  accountApi: { verifyEmail: mocks.verifyEmail, resendVerification: mocks.resendVerification },
}))

const route = { query: {} as Record<string, string> }
vi.mock('vue-router', () => ({ useRoute: () => route }))

import VerifyEmailView from '@/views/VerifyEmailView.vue'

const stubs = { 'router-link': { props: ['to'], template: '<a :href="to"><slot /></a>' } }

beforeEach(() => {
  setActivePinia(createPinia())
  for (const m of Object.values(mocks)) m.mockReset()
  route.query = { token: 'tok-abc' }
})

const mountView = () => mount(VerifyEmailView, { global: { stubs } })

describe('VerifyEmailView', () => {
  it('spends the token on arrival, without waiting to be asked', async () => {
    // The link in the mail is the whole interaction. A page that then asked the
    // user to press "confirm" would be asking them to do again what they just
    // did by clicking.
    mocks.verifyEmail.mockResolvedValue({ success: true })
    const w = mountView()
    await flushPromises()

    expect(mocks.verifyEmail).toHaveBeenCalledWith('tok-abc')
    expect(w.find('[data-test="verified"]').exists()).toBe(true)
    expect(w.find('[data-test="resend"]').exists()).toBe(false)
  })

  it('offers a new link when the old one is spent or expired', async () => {
    mocks.verifyEmail.mockResolvedValue({
      success: false,
      error: 'This confirmation link is invalid or has expired. Ask for a new one.',
    })
    const w = mountView()
    await flushPromises()

    expect(w.get('[data-test="error"]').text()).toMatch(/invalid or has expired/i)
    expect(
      w.find('[data-test="resend"]').exists(),
      'without this control the account is unreachable: it cannot sign in and ' + 'cannot confirm'
    ).toBe(true)
  })

  it('offers the resend form when the link carried no token', async () => {
    route.query = {}
    const w = mountView()
    await flushPromises()

    expect(mocks.verifyEmail).not.toHaveBeenCalled()
    expect(w.find('[data-test="resend"]').exists()).toBe(true)
  })

  it('answers a resend the same way whatever the address is', async () => {
    // Same uniform answer as the reset request, for the same reason: an
    // already-confirmed address, an unknown one and a fresh send are not told
    // apart.
    mocks.verifyEmail.mockResolvedValue({ success: false, error: 'expired' })
    mocks.resendVerification.mockResolvedValue({ success: true })

    const w = mountView()
    await flushPromises()
    await w.get('#verify-email').setValue('ada@example.invalid')
    await w.get('[data-test="resend"]').trigger('submit')
    await flushPromises()

    expect(mocks.resendVerification).toHaveBeenCalledWith('ada@example.invalid')
    expect(w.get('[data-test="resent"]').text()).toMatch(/if that address needs confirming/i)
  })
})
