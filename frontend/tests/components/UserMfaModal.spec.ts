// Tier 2: UserMfaModal (#151).
//
// The administrator's per-factor view. What has teeth: each row's Remove
// targets THAT factor's id on the right endpoint (an authenticator and a
// security key have different routes and a mix-up removes the wrong thing),
// the roster is told the resulting enrolment state, and "reset everything"
// stays the separate, coarser act it always was.
//
// What this does NOT prove: that the member's other factor still logs them
// in afterwards. e2e/drivers/mfa.mjs does, against the real verifier.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const mocks = vi.hoisted(() => ({
  listUserMfa: vi.fn(),
  removeUserTotp: vi.fn(),
  removeUserWebauthn: vi.fn(),
  resetUserMfa: vi.fn(),
}))
vi.mock('@/utils/api', () => ({ adminApi: mocks }))

import UserMfaModal from '@/components/UserMfaModal.vue'

const VIEW = {
  totp: [
    {
      id: 't1',
      label: 'Phone',
      created_at: '2026-01-01T00:00:00Z',
      confirmed_at: '2026-01-01T00:01:00Z',
    },
    {
      id: 't2',
      label: 'Desktop',
      created_at: '2026-02-01T00:00:00Z',
      confirmed_at: '2026-02-01T00:01:00Z',
    },
  ],
  webauthn: [
    { id: 'k1', label: 'Yubikey', created_at: '2026-03-01T00:00:00Z', last_used_at: null },
  ],
  recovery_codes_remaining: 9,
  mfa_enrolled_at: '2026-01-01T00:01:00Z',
}

function mountModal() {
  return mount(UserMfaModal, { props: { userId: 'u-1', username: 'alice' } })
}

let confirmResult = true
beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset()
  mocks.listUserMfa.mockResolvedValue({ success: true, data: VIEW })
  confirmResult = true
  vi.stubGlobal(
    'confirm',
    vi.fn(() => confirmResult)
  )
})

describe('UserMfaModal', () => {
  it('lists every factor by label with the recovery-code count', async () => {
    const w = mountModal()
    await flushPromises()
    expect(mocks.listUserMfa).toHaveBeenCalledWith('u-1')
    expect(w.findAll('[data-totp-id]').map((r) => r.text())).toEqual([
      expect.stringContaining('Phone'),
      expect.stringContaining('Desktop'),
    ])
    expect(w.find('[data-webauthn-id="k1"]').text()).toContain('Yubikey')
    expect(w.text()).toContain('Unused recovery codes: 9')
  })

  it('removes exactly the authenticator whose row was clicked, and reports enrolment', async () => {
    mocks.removeUserTotp.mockResolvedValue({
      success: true,
      data: { ...VIEW, totp: [VIEW.totp[0]] },
    })
    const w = mountModal()
    await flushPromises()
    await w.find('[data-totp-id="t2"] button').trigger('click')
    await flushPromises()

    expect(mocks.removeUserTotp).toHaveBeenCalledWith('u-1', 't2')
    expect(mocks.removeUserWebauthn).not.toHaveBeenCalled()
    expect(w.findAll('[data-totp-id]')).toHaveLength(1)
    expect(w.emitted('changed')?.[0]?.[0]).toBe(true)
  })

  it('removes a security key through its own endpoint', async () => {
    mocks.removeUserWebauthn.mockResolvedValue({
      success: true,
      data: { ...VIEW, webauthn: [] },
    })
    const w = mountModal()
    await flushPromises()
    await w.find('[data-webauthn-id="k1"] button').trigger('click')
    await flushPromises()

    expect(mocks.removeUserWebauthn).toHaveBeenCalledWith('u-1', 'k1')
    expect(mocks.removeUserTotp).not.toHaveBeenCalled()
  })

  it('does nothing when the confirmation is declined', async () => {
    confirmResult = false
    const w = mountModal()
    await flushPromises()
    await w.find('[data-totp-id="t1"] button').trigger('click')
    await flushPromises()
    expect(mocks.removeUserTotp).not.toHaveBeenCalled()
  })

  it('reports an un-enrolled state after the last factor goes', async () => {
    mocks.removeUserTotp.mockResolvedValue({
      success: true,
      data: { totp: [], webauthn: [], recovery_codes_remaining: 9, mfa_enrolled_at: null },
    })
    const w = mountModal()
    await flushPromises()
    await w.find('[data-totp-id="t1"] button').trigger('click')
    await flushPromises()
    expect(w.emitted('changed')?.at(-1)?.[0]).toBe(false)
  })

  it('keeps reset-everything as a separate act that reloads', async () => {
    mocks.resetUserMfa.mockResolvedValue({ success: true })
    const w = mountModal()
    await flushPromises()
    const reset = w.findAll('button').find((b) => b.text() === 'Reset everything')
    if (!reset) throw new Error('no reset button')
    await reset.trigger('click')
    await flushPromises()
    expect(mocks.resetUserMfa).toHaveBeenCalledWith('u-1')
    expect(mocks.listUserMfa).toHaveBeenCalledTimes(2)
    expect(w.emitted('changed')?.at(-1)?.[0]).toBe(false)
  })
})
