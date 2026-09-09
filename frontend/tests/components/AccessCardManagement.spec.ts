import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

// The component talks to the backend through `apiClient` (get/post). Mock just
// those two so each test drives the component from stubbed responses.
const mocks = vi.hoisted(() => ({ get: vi.fn(), post: vi.fn() }))
vi.mock('@/utils/api', () => ({ apiClient: mocks }))

import AccessCardManagement from '@/components/AccessCardManagement.vue'
import { CardStatus } from '@/types'
import type { Card } from '@/types'

const card = (o: Partial<Card> = {}): Card => ({
  id: o.id ?? 'c1',
  user_id: 'u1',
  code: o.code ?? '2A-9E-7B-92',
  status: o.status ?? CardStatus.Active,
  last_used_at: o.last_used_at ?? null,
  issued_at: '2026-01-01T00:00:00Z',
  disabled_at: null,
  released_at: null,
  disabled_reason: o.disabled_reason ?? null,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
})

beforeEach(() => {
  mocks.get.mockReset()
  mocks.post.mockReset()
})

describe('AccessCardManagement', () => {
  it('lists the member’s cards for the given user', async () => {
    mocks.get.mockResolvedValue({ success: true, data: [card({ code: 'AA-BB-CC' })] })
    const w = mount(AccessCardManagement, { props: { userId: 'u1' } })
    await flushPromises()
    expect(mocks.get).toHaveBeenCalledWith('/cards/user/u1')
    expect(w.text()).toContain('AA-BB-CC')
  })

  it('issues a card, then reloads the list', async () => {
    mocks.get.mockResolvedValue({ success: true, data: [] })
    mocks.post.mockResolvedValue({ success: true, data: card() })
    const w = mount(AccessCardManagement, { props: { userId: 'u1' } })
    await flushPromises()

    await w.find('input').setValue('NEW-CODE')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(mocks.post).toHaveBeenCalledWith('/cards/user/u1', { code: 'NEW-CODE' })
    // one initial load + one after issuing
    expect(mocks.get).toHaveBeenCalledTimes(2)
  })

  it('offers Release but not Disable for an already-disabled card', async () => {
    mocks.get.mockResolvedValue({
      success: true,
      data: [card({ status: CardStatus.Disabled })],
    })
    const w = mount(AccessCardManagement, { props: { userId: 'u1' } })
    await flushPromises()
    const actions = w.findAll('tbody button').map((b) => b.text())
    expect(actions).toContain('Release')
    expect(actions).not.toContain('Disable')
  })

  it('surfaces a load error instead of failing silently', async () => {
    mocks.get.mockResolvedValue({ success: false, error: 'Admin role required' })
    const w = mount(AccessCardManagement, { props: { userId: 'u1' } })
    await flushPromises()
    expect(w.text()).toContain('Admin role required')
  })
})
