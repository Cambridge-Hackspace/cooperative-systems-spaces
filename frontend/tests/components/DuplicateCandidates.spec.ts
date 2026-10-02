// Tier 2: DuplicateCandidates (#38/#118).
//
// The report of account pairs that may be one person. What has teeth: each
// pair offers BOTH merge directions and opens the merge dialog with the right
// account as absorbed and the other as the only candidate -- a direction
// mix-up would delete the wrong account, and the dialog's own guard (every
// warning acknowledged) does not know which way round the administrator
// meant. After a merge the report reloads.
//
// What this does NOT prove: that the pairs are right. The server's
// `duplicates` module is unit-tested on the rules, and e2e/drivers/merge.mjs
// shows a real pair appearing before a merge and gone after it.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const mocks = vi.hoisted(() => ({
  listDuplicateCandidates: vi.fn(),
  previewMerge: vi.fn(),
  mergeUsers: vi.fn(),
}))
vi.mock('@/utils/api', () => ({ adminApi: mocks }))

import DuplicateCandidates from '@/components/DuplicateCandidates.vue'

const older = { id: 'o1', username: 'ed', email: 'e.klacza@x.invalid', full_name: 'Ed K' }
const newer = { id: 'n1', username: 'ed2', email: 'eklacza@x.invalid', full_name: 'Ed K' }
const PAIR = { a: older, b: newer, reasons: ['same_name', 'email_alias'] }

function mountReport() {
  return mount(DuplicateCandidates)
}
type Wrapper = ReturnType<typeof mountReport>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labeled ${JSON.stringify(label)}`)
  return b
}

beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset()
  mocks.listDuplicateCandidates.mockResolvedValue({ success: true, data: [PAIR] })
})

describe('DuplicateCandidates', () => {
  it('lists each pair with its reasons, older account first', async () => {
    const w = mountReport()
    await flushPromises()

    expect(mocks.listDuplicateCandidates).toHaveBeenCalledTimes(1)
    const row = w.find('[data-pair="o1:n1"]')
    expect(row.exists()).toBe(true)
    expect(row.findAll('td')[0].text()).toContain('ed')
    expect(row.findAll('[data-reason]').map((b) => b.attributes('data-reason'))).toEqual([
      'same_name',
      'email_alias',
    ])
  })

  it('says so when there is nothing to report', async () => {
    mocks.listDuplicateCandidates.mockResolvedValue({ success: true, data: [] })
    const w = mountReport()
    await flushPromises()
    expect(w.text()).toContain('No likely duplicates found')
    expect(w.find('[data-testid="duplicates"]').exists()).toBe(false)
  })

  it('opens the merge dialog the right way round for each direction', async () => {
    mocks.previewMerge.mockResolvedValue({ success: false, error: 'not needed here' })
    const w = mountReport()
    await flushPromises()

    await buttonNamed(w, 'Merge newer into older').trigger('click')
    // The dialog names the absorbed account in its heading, and the only
    // survivor candidate is the other one.
    expect(w.find('#merge-title').text()).toContain('ed2')
    expect(w.findAll('[data-candidate-id]').map((e) => e.attributes('data-candidate-id'))).toEqual([
      'o1',
    ])
    await buttonNamed(w, 'Cancel').trigger('click')
    expect(w.find('#merge-title').exists()).toBe(false)

    await buttonNamed(w, 'Merge older into newer').trigger('click')
    expect(w.find('#merge-title').text()).toContain('ed')
    expect(w.find('#merge-title').text()).not.toContain('ed2')
    expect(w.findAll('[data-candidate-id]').map((e) => e.attributes('data-candidate-id'))).toEqual([
      'n1',
    ])
  })

  it('reloads and tells the parent after a merge', async () => {
    mocks.previewMerge.mockResolvedValue({
      success: true,
      data: { survivor: older, absorbed: newer, moves: {}, warnings: [] },
    })
    mocks.mergeUsers.mockResolvedValue({
      success: true,
      data: { merge_id: 'm1', survivor_id: 'o1', absorbed_id: 'n1', moved: {}, warnings: [] },
    })
    const w = mountReport()
    await flushPromises()
    await buttonNamed(w, 'Merge newer into older').trigger('click')
    await w.find('[data-candidate-id="o1"]').trigger('click')
    await buttonNamed(w, 'Preview merge').trigger('click')
    await flushPromises()
    mocks.listDuplicateCandidates.mockResolvedValue({ success: true, data: [] })
    const commit = w.findAll('button').find((b) => b.text().startsWith('Merge and delete'))
    if (!commit) throw new Error('no commit button')
    await commit.trigger('click')
    await flushPromises()

    expect(mocks.mergeUsers).toHaveBeenCalledWith('o1', 'n1', [])
    expect(w.emitted('merged')?.[0]?.[0]).toBe('ed2 merged into ed.')
    expect(mocks.listDuplicateCandidates).toHaveBeenCalledTimes(2)
    expect(w.find('#merge-title').exists()).toBe(false)
  })

  it('surfaces a refused report rather than showing an empty one as clean', async () => {
    mocks.listDuplicateCandidates.mockResolvedValue({ success: false, error: 'Forbidden' })
    const w = mountReport()
    await flushPromises()
    expect(w.find('[role="alert"]').text()).toContain('Forbidden')
    expect(w.text()).not.toContain('No likely duplicates found')
    expect(w.emitted('error')?.[0]?.[0]).toBe('Forbidden')
  })
})
