// Tier 2: EmailAddressesCard (#118).
//
// A user's addresses: list, add, remove, promote, resend -- through the typed
// apiClient against /users/{id}/emails. The one rule with teeth here is the
// self-service credential rule: when the viewer manages their OWN addresses
// the current password is sent with an add or a promotion, and when a manager
// acts on someone else it is not. The server enforces it (#120/#2); this
// proves the client sends what the server will ask for, in both modes.
//
// What this spec does NOT prove: that the server refuses an add without the
// password, or that the primary cannot be removed. e2e/drivers/emails.mjs
// carries those against the real stack.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  post: vi.fn(),
  put: vi.fn(),
  delete: vi.fn(),
}))
vi.mock('@/utils/api', () => ({ apiClient: mocks }))

import EmailAddressesCard from '@/components/EmailAddressesCard.vue'

function ok<T>(data: T, message?: string) {
  return { success: true, data, message }
}

const USER = '11111111-1111-4111-8111-111111111111'
const primary = {
  id: 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',
  user_id: USER,
  email: 'one@example.invalid',
  is_primary: true,
  verified_at: '2026-01-01T00:00:00Z',
  created_at: '2026-01-01T00:00:00Z',
}
const confirmedSecondary = {
  id: 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb',
  user_id: USER,
  email: 'two@example.invalid',
  is_primary: false,
  verified_at: '2026-01-02T00:00:00Z',
  created_at: '2026-01-02T00:00:00Z',
}
const pendingSecondary = {
  id: 'cccccccc-cccc-4ccc-8ccc-cccccccccccc',
  user_id: USER,
  email: 'three@example.invalid',
  is_primary: false,
  verified_at: null,
  created_at: '2026-01-03T00:00:00Z',
}

beforeEach(() => {
  mocks.get.mockReset()
  mocks.post.mockReset()
  mocks.put.mockReset()
  mocks.delete.mockReset()
  mocks.get.mockResolvedValue(ok([primary, confirmedSecondary, pendingSecondary]))
})

/**
 * Mounting in one place so `Wrapper` is the concrete wrapper type rather than
 * the unbound `VueWrapper<any, any>` a bare `ReturnType<typeof mount>` gives,
 * which would type every helper argument as `any`.
 */
function mountCard(self: boolean) {
  return mount(EmailAddressesCard, { props: { userId: USER, self } })
}
type Wrapper = ReturnType<typeof mountCard>

function row(w: Wrapper, id: string) {
  return w.find(`[data-email-id="${id}"]`)
}

describe('EmailAddressesCard', () => {
  it('lists every address with its primary and confirmation state', async () => {
    const w = mountCard(true)
    await flushPromises()

    expect(mocks.get).toHaveBeenCalledWith(`/users/${USER}/emails`)
    expect(row(w, primary.id).text()).toContain('Primary')
    expect(row(w, primary.id).text()).toContain('Confirmed')
    expect(row(w, pendingSecondary.id).text()).toContain('Unconfirmed')
  })

  it('offers the actions each row can actually take', async () => {
    const w = mountCard(true)
    await flushPromises()

    // The primary can be neither removed nor promoted.
    expect(row(w, primary.id).findAll('button')).toHaveLength(0)
    // A confirmed secondary can be promoted or removed, not resent.
    const confirmed = row(w, confirmedSecondary.id)
      .findAll('button')
      .map((b) => b.text())
    expect(confirmed).toEqual(['Make primary', 'Remove'])
    // An unconfirmed secondary can be resent or removed, not promoted.
    const pending = row(w, pendingSecondary.id)
      .findAll('button')
      .map((b) => b.text())
    expect(pending).toEqual(['Resend link', 'Remove'])
  })

  it('sends the current password with a self-service add, and reloads', async () => {
    mocks.post.mockResolvedValue(ok(pendingSecondary, 'Address added'))
    const w = mountCard(true)
    await flushPromises()

    await w.find('input[type="email"]').setValue('  three@example.invalid ')
    await w.find('input[type="password"]').setValue('hunter2hunter2')
    await w.find('form').trigger('submit.prevent')
    await flushPromises()

    expect(mocks.post).toHaveBeenCalledWith(`/users/${USER}/emails`, {
      email: 'three@example.invalid',
      current_password: 'hunter2hunter2',
    })
    // Reloaded after the write: one GET on mount, one after the add.
    expect(mocks.get).toHaveBeenCalledTimes(2)
    expect(w.find('[role="status"]').text()).toContain('Address added')
  })

  it('does not ask a manager for a password, and sends none', async () => {
    mocks.post.mockResolvedValue(ok(pendingSecondary))
    const w = mountCard(false)
    await flushPromises()

    expect(w.find('input[type="password"]').exists()).toBe(false)
    await w.find('input[type="email"]').setValue('three@example.invalid')
    await w.find('form').trigger('submit.prevent')
    await flushPromises()

    expect(mocks.post).toHaveBeenCalledWith(`/users/${USER}/emails`, {
      email: 'three@example.invalid',
    })
  })

  it('promotes with the password in self mode and removes by row id', async () => {
    mocks.put.mockResolvedValue(ok([confirmedSecondary, primary]))
    mocks.delete.mockResolvedValue(ok(null))
    const w = mountCard(true)
    await flushPromises()

    await w.find('input[type="password"]').setValue('hunter2hunter2')
    await row(w, confirmedSecondary.id).findAll('button')[0].trigger('click')
    await flushPromises()
    expect(mocks.put).toHaveBeenCalledWith(
      `/users/${USER}/emails/${confirmedSecondary.id}/primary`,
      {
        current_password: 'hunter2hunter2',
      }
    )

    await row(w, pendingSecondary.id).findAll('button')[1].trigger('click')
    await flushPromises()
    expect(mocks.delete).toHaveBeenCalledWith(`/users/${USER}/emails/${pendingSecondary.id}`)
  })

  it('surfaces a refusal from the envelope rather than swallowing it', async () => {
    mocks.post.mockResolvedValue({ success: false, error: 'Email already exists' })
    const w = mountCard(false)
    await flushPromises()

    await w.find('input[type="email"]').setValue('one@example.invalid')
    await w.find('form').trigger('submit.prevent')
    await flushPromises()

    expect(w.find('[role="alert"]').text()).toContain('Email already exists')
  })
})
