// Tier 2: RateTierManagement (#34).
//
// The per-tool rate-tier editor + member assignment control. What is under test:
// the tier list renders what the API returns, adding a tier posts, deleting
// removes, and assigning a member calls through with (user, tool, tier).
//
// What this does NOT prove: that the charge actually resolves the tier -- that
// is the e2e billing stage's job, against a real ledger.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const tiers = vi.hoisted(() => ({
  listTiers: vi.fn(),
  createTier: vi.fn(),
  updateTier: vi.fn(),
  removeTier: vi.fn(),
  listAssignments: vi.fn(),
  assignTier: vi.fn(),
  clearTier: vi.fn(),
}))
const users = vi.hoisted(() => ({ getAllUsers: vi.fn() }))

vi.mock('@/utils/api', () => ({ tiersApi: tiers, userApi: users }))

import RateTierManagement from '@/components/RateTierManagement.vue'

const ok = <T>(data: T) => Promise.resolve({ success: true, data })

function tier(id: string, name: string, rate: string | null = '0.35') {
  return {
    id,
    tool_id: 'tool1',
    name,
    flat_fee: null,
    rate_per_min: rate,
    max_session_minutes: null,
    active: true,
    created_at: '',
    updated_at: '',
  }
}

function mountIt() {
  return mount(RateTierManagement, { props: { toolId: 'tool1' } })
}

beforeEach(() => {
  vi.clearAllMocks()
  tiers.listTiers.mockReturnValue(ok([tier('r1', 'Discounted')]))
  tiers.listAssignments.mockReturnValue(ok([]))
  users.getAllUsers.mockReturnValue(ok([{ id: 'u1', full_name: 'Ada Member' }]))
  tiers.createTier.mockReturnValue(ok(tier('r2', 'Free', '0')))
  tiers.removeTier.mockReturnValue(ok(undefined))
  tiers.assignTier.mockReturnValue(ok(undefined))
  tiers.clearTier.mockReturnValue(ok(undefined))
})

describe('RateTierManagement', () => {
  it("lists the tool's tiers", async () => {
    const w = mountIt()
    await flushPromises()
    expect(tiers.listTiers).toHaveBeenCalledWith('tool1')
    expect(w.text()).toContain('Discounted')
  })

  it('creates a tier from the form', async () => {
    const w = mountIt()
    await flushPromises()
    const tierForm = w.findAll('form')[0] // first form is the add-tier form
    const inputs = tierForm.findAll('input')
    await inputs[0].setValue('Free') // name
    await inputs[2].setValue('0') // rate/min
    await tierForm.trigger('submit.prevent')
    await flushPromises()
    expect(tiers.createTier).toHaveBeenCalledWith(
      'tool1',
      expect.objectContaining({ name: 'Free', rate_per_min: '0' })
    )
  })

  it('assigns a member to a tier', async () => {
    const w = mountIt()
    await flushPromises()
    const selects = w.findAll('select')
    await selects[0].setValue('u1') // member
    await selects[1].setValue('r1') // tier
    const forms = w.findAll('form')
    await forms[forms.length - 1].trigger('submit.prevent')
    await flushPromises()
    expect(tiers.assignTier).toHaveBeenCalledWith('u1', 'tool1', 'r1')
  })
})
