// Tier 2: the password reset request form.
//
// One property matters more than everything else on this screen: it must say
// the same thing whether or not the address has an account. The server is
// careful about that -- one shared message constant, identical status, a
// throttle that records unconditionally -- and all of it is undone if the view
// infers anything. A "no account with that address" state here would rebuild
// the enumeration oracle in the client.
//
// What this does NOT prove: that the server answers uniformly. That is
// asserted structurally in checks/tests/account_tokens_are_claimed_atomically.rs
// and end to end in the stack tier.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'

const mocks = vi.hoisted(() => ({ requestPasswordReset: vi.fn(), logout: vi.fn() }))

vi.mock('@/utils/api', () => ({
  accountApi: { requestPasswordReset: mocks.requestPasswordReset },
}))

import ForgotPasswordView from '@/views/ForgotPasswordView.vue'

const stubs = { 'router-link': { props: ['to'], template: '<a :href="to"><slot /></a>' } }

beforeEach(() => {
  setActivePinia(createPinia())
  for (const m of Object.values(mocks)) m.mockReset()
})

const mountView = () => mount(ForgotPasswordView, { global: { stubs } })

async function submitWith(address: string) {
  const w = mountView()
  await w.get('#forgot-email').setValue(address)
  await w.get('[data-test="submit"]').trigger('submit')
  await flushPromises()
  return w
}

describe('ForgotPasswordView', () => {
  it('labels its input, so a screen reader and the browser tier can find it', () => {
    // LoginView.vue documents why this matters: the browser tier addresses
    // fields by label, and both fields on the only form every user must
    // complete once had labels associated with nothing.
    const w = mountView()
    const label = w.findAll('label').find((l) => l.attributes('for') === 'forgot-email')
    expect(label, 'the email input needs a <label for> pointing at its id').toBeTruthy()
    expect(w.get('#forgot-email').attributes('type')).toBe('email')
  })

  it('sends the address to the reset endpoint', async () => {
    mocks.requestPasswordReset.mockResolvedValue({ success: true })
    await submitWith('ada@example.invalid')

    expect(mocks.requestPasswordReset).toHaveBeenCalledTimes(1)
    expect(mocks.requestPasswordReset).toHaveBeenCalledWith('ada@example.invalid')
  })

  it('says exactly the same thing for an address that has no account', async () => {
    // The server cannot tell us, and this view must not pretend to know. Both
    // calls are the success envelope because that is what the server returns on
    // both branches; what is asserted is that the rendered text is identical.
    mocks.requestPasswordReset.mockResolvedValue({ success: true })

    const known = await submitWith('ada@example.invalid')
    const unknown = await submitWith('nobody@example.invalid')

    const shown = (w: ReturnType<typeof mountView>) => w.get('[data-test="sent"]').text()
    expect(shown(known)).toBe(shown(unknown))
    expect(shown(known)).toMatch(/if an account exists/i)
  })

  it('hides the form once a link has been sent, so it is not submitted twice', async () => {
    mocks.requestPasswordReset.mockResolvedValue({ success: true })
    const w = await submitWith('ada@example.invalid')

    expect(w.find('[data-test="submit"]').exists()).toBe(false)
  })

  it('shows a deployment failure without implying anything about the address', async () => {
    // A 403 means recovery is switched off; a 429 means throttled. Both are
    // worth showing, and neither says whether an account exists.
    mocks.requestPasswordReset.mockResolvedValue({
      success: false,
      error: 'Password reset is not available on this instance',
    })
    const w = await submitWith('ada@example.invalid')

    expect(w.get('[data-test="error"]').text()).toContain('not available')
    expect(w.find('[data-test="sent"]').exists()).toBe(false)
  })
})
