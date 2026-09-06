// Tier 2: GroupsioManagement (admin).
//
// The admin view of the Groups.io sync: status, recent runs, and a manual
// reconcile. Goes through the typed apiClient. The router gate (admin-only) and
// the endpoint behavior are owned elsewhere.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount, RouterLinkStub } from '@vue/test-utils'

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  post: vi.fn(),
}))
vi.mock('@/utils/api', () => ({ apiClient: mocks }))

import GroupsioManagement from '@/components/GroupsioManagement.vue'

function mountView() {
  return mount(GroupsioManagement, {
    global: { stubs: { 'router-link': RouterLinkStub } },
  })
}

function ok<T>(data: T) {
  return { success: true, data }
}

function status(over: Record<string, unknown> = {}) {
  return {
    enabled: true,
    intended_count: 42,
    recent_runs: [
      {
        id: 'r1',
        started_at: '2026-09-05T10:00:00Z',
        finished_at: '2026-09-05T10:00:01Z',
        added: 2,
        removed: 1,
        opted_out: 0,
        ok: true,
        error: null,
      },
    ],
    ...over,
  }
}

beforeEach(() => {
  mocks.get.mockReset()
  mocks.post.mockReset()
})

describe('GroupsioManagement', () => {
  it('shows the intended count and a recent run', async () => {
    mocks.get.mockResolvedValue(ok(status()))
    const w = mountView()
    await flushPromises()

    expect(mocks.get).toHaveBeenCalledWith('/admin/groupsio/status')
    expect(w.text()).toContain('42')
    expect(w.text()).toContain('ok')
  })

  it('reconcile posts then refreshes status', async () => {
    mocks.get.mockResolvedValue(ok(status()))
    mocks.post.mockResolvedValue(ok({ added: 0, removed: 0, opted_out: 0, ok: true, error: null }))
    const w = mountView()
    await flushPromises()

    mocks.get.mockClear()
    await w.find('.reconcile-now').trigger('click')
    await flushPromises()

    expect(mocks.post).toHaveBeenCalledWith('/admin/groupsio/reconcile')
    // Status is refetched after a reconcile so the run list updates.
    expect(mocks.get).toHaveBeenCalledWith('/admin/groupsio/status')
  })

  it('surfaces a failed reconcile outcome', async () => {
    mocks.get.mockResolvedValue(ok(status()))
    mocks.post.mockResolvedValue(
      ok({ added: 0, removed: 0, opted_out: 0, ok: false, error: 'get_members: boom' })
    )
    const w = mountView()
    await flushPromises()

    await w.find('.reconcile-now').trigger('click')
    await flushPromises()

    expect(w.find('[role="alert"]').text()).toContain('boom')
  })

  it('shows an error when the status load fails', async () => {
    mocks.get.mockResolvedValue({ success: false, error: 'nope' })
    const w = mountView()
    await flushPromises()

    expect(w.find('[role="alert"]').text()).toContain('nope')
  })
})
