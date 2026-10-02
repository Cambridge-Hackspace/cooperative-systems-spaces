// Tier 2: StripeCustomersCard (#150).
//
// A member can be several Stripe customers (#118). This card shows all of
// them with the one checkout and the portal would use marked, so an
// administrator looking at a merged member can see both and which is live.
// Read-only by design: unlinking is a Stripe-side act.
//
// What this does NOT prove: that the server's "current" pick is right. The
// e2e `stripe` stage's two-customer section asserts that against the real
// webhook flow.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const mocks = vi.hoisted(() => ({ listStripeCustomers: vi.fn() }))
vi.mock('@/utils/api', () => ({ userApi: mocks }))

import StripeCustomersCard from '@/components/StripeCustomersCard.vue'

const USER = 'u-1'
const live = {
  id: 'l1',
  customer_id: 'cus_live',
  subscription_id: 'sub_1',
  subscription_status: 'active',
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  current: true,
}
const old = {
  id: 'l2',
  customer_id: 'cus_old',
  subscription_id: null,
  subscription_status: 'canceled',
  created_at: '2025-06-01T00:00:00Z',
  updated_at: '2025-06-01T00:00:00Z',
  current: false,
}

beforeEach(() => {
  mocks.listStripeCustomers.mockReset()
})

describe('StripeCustomersCard', () => {
  it('lists every customer and marks the current one', async () => {
    mocks.listStripeCustomers.mockResolvedValue({ success: true, data: [old, live] })
    const w = mount(StripeCustomersCard, { props: { userId: USER } })
    await flushPromises()

    expect(mocks.listStripeCustomers).toHaveBeenCalledWith(USER)
    const rows = w.findAll('[data-customer]')
    expect(rows.map((r) => r.attributes('data-customer'))).toEqual(['cus_old', 'cus_live'])
    expect(w.find('[data-customer="cus_live"]').text()).toContain('Current')
    expect(w.find('[data-customer="cus_old"]').text()).not.toContain('Current')
    // A live subscription is marked as such; a canceled one is shown but not green.
    expect(w.find('[data-customer="cus_live"] .badge-success').exists()).toBe(true)
    expect(w.find('[data-customer="cus_old"]').text()).toContain('canceled')
  })

  it('says so when there is no customer yet', async () => {
    mocks.listStripeCustomers.mockResolvedValue({ success: true, data: [] })
    const w = mount(StripeCustomersCard, { props: { userId: USER } })
    await flushPromises()
    expect(w.text()).toContain('No Stripe customer yet')
    expect(w.find('[data-testid="stripe-customers"]').exists()).toBe(false)
  })

  it('shows a refusal instead of an empty list', async () => {
    mocks.listStripeCustomers.mockResolvedValue({ success: false, error: 'Forbidden' })
    const w = mount(StripeCustomersCard, { props: { userId: USER } })
    await flushPromises()
    expect(w.find('[role="alert"]').text()).toContain('Forbidden')
    expect(w.text()).not.toContain('No Stripe customer yet')
  })
})
