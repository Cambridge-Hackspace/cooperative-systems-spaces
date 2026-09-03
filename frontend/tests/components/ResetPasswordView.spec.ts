// Tier 2: choosing a new password from an emailed link.
//
// The load-bearing test in this file is the last one. The server answers 400 --
// never 401 -- for an unknown, expired or already-spent token, because
// `utils/api.ts` calls `authStore.logout()` on any 401 from any endpoint. A
// signed-in user who opens a stale reset link must see "this link expired", not
// find themselves silently signed out. That rule is asserted structurally in
// checks/tests/account_tokens_are_claimed_atomically.rs; this asserts the
// client half, that a 400 leaves the session alone.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'

const mocks = vi.hoisted(() => ({ consumePasswordReset: vi.fn(), logout: vi.fn() }))

vi.mock('@/utils/api', () => ({
  accountApi: { consumePasswordReset: mocks.consumePasswordReset },
}))

const route = { query: {} as Record<string, string> }
vi.mock('vue-router', () => ({ useRoute: () => route }))

import ResetPasswordView from '@/views/ResetPasswordView.vue'
import { useAuthStore } from '@/stores/auth'

const stubs = { 'router-link': { props: ['to'], template: '<a :href="to"><slot /></a>' } }

beforeEach(() => {
  setActivePinia(createPinia())
  for (const m of Object.values(mocks)) m.mockReset()
  route.query = { token: 'tok-abc' }
})

const mountView = () => mount(ResetPasswordView, { global: { stubs } })

async function fill(w: ReturnType<typeof mountView>, pw: string, confirm: string) {
  await w.get('#reset-password').setValue(pw)
  await w.get('#reset-confirm').setValue(confirm)
}

describe('ResetPasswordView', () => {
  it('sends the token from the query alongside the new password', async () => {
    mocks.consumePasswordReset.mockResolvedValue({ success: true })
    const w = mountView()
    await fill(w, 'correct-horse', 'correct-horse')
    await w.get('[data-test="submit"]').trigger('submit')
    await flushPromises()

    expect(mocks.consumePasswordReset).toHaveBeenCalledWith('tok-abc', 'correct-horse')
    expect(w.find('[data-test="done"]').exists()).toBe(true)
  })

  it('says so when the link carries no token at all', async () => {
    // A truncated link is a real failure mode -- mail clients wrap URLs. The
    // page has to say that rather than render a form that cannot work.
    route.query = {}
    const w = mountView()
    await flushPromises()

    expect(w.find('[data-test="no-token"]').exists()).toBe(true)
    expect(w.find('[data-test="submit"]').exists()).toBe(false)
  })

  it('refuses to submit two passwords that do not match', async () => {
    const w = mountView()
    await fill(w, 'correct-horse', 'correct-hors')

    expect(w.find('[data-test="mismatch"]').exists()).toBe(true)
    // Asserted on the attribute rather than by trying to submit: vue-test-utils
    // declines to fire events on a disabled element, so a `trigger` that did
    // nothing would pass whether or not the guard existed.
    expect(w.get('[data-test="submit"]').attributes('disabled')).toBeDefined()
    expect(mocks.consumePasswordReset).not.toHaveBeenCalled()
  })

  it('reports an expired link and leaves the session alone', async () => {
    // The whole reason the server answers 400 here. If it answered 401, the
    // API client's interceptor would log the user out, and a stale link would
    // present as a mysterious session expiry.
    const auth = useAuthStore()
    const logout = vi.spyOn(auth, 'logout')

    mocks.consumePasswordReset.mockResolvedValue({
      success: false,
      error: 'This password reset link is invalid or has expired. Request a new one.',
    })

    const w = mountView()
    await fill(w, 'correct-horse', 'correct-horse')
    await w.get('[data-test="submit"]').trigger('submit')
    await flushPromises()

    expect(w.get('[data-test="error"]').text()).toMatch(/invalid or has expired/i)
    expect(w.find('[data-test="done"]').exists()).toBe(false)
    expect(
      logout,
      'a rejected reset token must not end the session -- that is why the server ' +
        'answers 400 rather than 401'
    ).not.toHaveBeenCalled()
  })
})
