// Tier 2: UserMergeModal (#118).
//
// The one rule with teeth: the Merge button is disabled until EVERY warning
// the preview returned is acknowledged, and the commit sends exactly those
// codes. The server refuses a commit whose acknowledged set differs from its
// current plan (409), so this proves the client can only ever send a complete
// set -- and that a refused commit drops back to the picker rather than
// retrying with a stale set.
//
// What this does NOT prove: that anything moves. e2e/drivers/merge.mjs does,
// against the real database.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const mocks = vi.hoisted(() => ({
  previewMerge: vi.fn(),
  mergeUsers: vi.fn(),
}))
vi.mock('@/utils/api', () => ({ adminApi: mocks }))

import UserMergeModal from '@/components/UserMergeModal.vue'
import type { MergePlan } from '@/types'

const absorbed = { id: 'a1', username: 'alice2', email: 'alice.two@x.invalid', full_name: 'Alice' }
const survivor = { id: 's1', username: 'alice', email: 'alice@x.invalid', full_name: 'Alice' }
const other = { id: 'o1', username: 'bob', email: 'bob@x.invalid', full_name: 'Bob' }

const PLAN: MergePlan = {
  survivor,
  absorbed,
  moves: { 'user_cards.user_id': 1, 'membership_ledger.user_id': 3 },
  warnings: [
    { code: 'absorbed_login_lost', detail: 'the username alice2 stops working' },
    { code: 'roles_raise_level', detail: 'level 1 -> 4' },
  ],
}

function mountModal() {
  return mount(UserMergeModal, {
    props: { absorbed, candidates: [absorbed, survivor, other] },
  })
}
type Wrapper = ReturnType<typeof mountModal>

function buttonNamed(w: Wrapper, startsWith: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim().startsWith(startsWith))
  if (!b) throw new Error(`no button starting with ${JSON.stringify(startsWith)}`)
  return b
}

async function toPlan(w: Wrapper) {
  await w.find('[data-candidate-id="s1"]').trigger('click')
  await buttonNamed(w, 'Preview merge').trigger('click')
  await flushPromises()
}

beforeEach(() => {
  mocks.previewMerge.mockReset()
  mocks.mergeUsers.mockReset()
  mocks.previewMerge.mockResolvedValue({ success: true, data: PLAN })
})

describe('UserMergeModal', () => {
  it('never offers the absorbed account as its own survivor, and filters by search', async () => {
    const w = mountModal()
    expect(w.find('[data-candidate-id="a1"]').exists()).toBe(false)
    expect(w.findAll('[data-candidate-id]')).toHaveLength(2)
    await w.find('#merge-survivor-search').setValue('bob@')
    expect(w.findAll('[data-candidate-id]').map((e) => e.attributes('data-candidate-id'))).toEqual([
      'o1',
    ])
  })

  it('previews against the chosen survivor and shows moves and warnings', async () => {
    const w = mountModal()
    expect(buttonNamed(w, 'Preview merge').attributes('disabled')).toBeDefined()
    await toPlan(w)

    expect(mocks.previewMerge).toHaveBeenCalledWith('s1', 'a1')
    expect(w.find('[data-testid="moves"]').text()).toContain('user_cards.user_id')
    expect(w.findAll('[data-warning-code]')).toHaveLength(2)
  })

  it('keeps Merge disabled until every warning is acknowledged, then sends exactly those codes', async () => {
    mocks.mergeUsers.mockResolvedValue({ success: true, data: { merge_id: 'm1', moved: {} } })
    const w = mountModal()
    await toPlan(w)

    const merge = () => buttonNamed(w, 'Merge and delete')
    expect(merge().attributes('disabled')).toBeDefined()
    await w.find('#ack-absorbed_login_lost').setValue(true)
    expect(merge().attributes('disabled')).toBeDefined()
    await w.find('#ack-roles_raise_level').setValue(true)
    expect(merge().attributes('disabled')).toBeUndefined()

    // Un-acknowledging one disables it again: the set must be complete at the
    // moment of the click, not merely once.
    await w.find('#ack-roles_raise_level').setValue(false)
    expect(merge().attributes('disabled')).toBeDefined()
    await w.find('#ack-roles_raise_level').setValue(true)

    await merge().trigger('click')
    await flushPromises()
    expect(mocks.mergeUsers).toHaveBeenCalledWith('s1', 'a1', [
      'absorbed_login_lost',
      'roles_raise_level',
    ])
    expect(w.emitted('merged')?.[0]?.[0]).toEqual({ merge_id: 'm1', moved: {} })
  })

  it('shows a refused commit and drops back to the picker rather than retrying', async () => {
    mocks.mergeUsers.mockResolvedValue({
      success: false,
      error:
        'every warning must be acknowledged before merging; the current set is: absorbed_login_lost, roles_raise_level, tier_differs',
    })
    const w = mountModal()
    await toPlan(w)
    await w.find('#ack-absorbed_login_lost').setValue(true)
    await w.find('#ack-roles_raise_level').setValue(true)
    await buttonNamed(w, 'Merge and delete').trigger('click')
    await flushPromises()

    expect(w.find('[role="alert"]').text()).toContain('tier_differs')
    expect(w.find('[data-testid="warnings"]').exists()).toBe(false)
    expect(w.emitted('merged')).toBeUndefined()
  })
})
