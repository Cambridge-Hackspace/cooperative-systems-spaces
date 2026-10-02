// Tier 2: AlertFeedView (#87).
//
// The alert feed is permission-gated, not role-gated, and the view must
// mirror that: it renders for a user holding `alerts.view` whatever their
// role, offers Acknowledge only to a holder of `alerts.acknowledge`, and
// sends the filters it shows. The server enforces both keys; this proves the
// client asks the right questions and never offers an action the server will
// refuse.
//
// What this does NOT prove: that the server classifies anything, or that the
// grant reaches the user. e2e/drivers/alerts.mjs carries those.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createTestingPinia } from '@pinia/testing'
import { flushPromises, mount } from '@vue/test-utils'

const mocks = vi.hoisted(() => ({
  list: vi.fn(),
  summary: vi.fn(),
  classification: vi.fn(),
  acknowledge: vi.fn(),
}))
vi.mock('@/utils/api', () => ({ alertsApi: mocks }))

import AlertFeedView from '@/views/AlertFeedView.vue'
import { useAuthStore } from '@/stores/auth'

const stubs = { 'router-link': { props: ['to'], template: '<a><slot /></a>' } }

const ALERT = {
  id: 'al-1',
  event_type: 'failed_login_attempt',
  category: 'user',
  severity: 'notice',
  user_id: null,
  actor_id: null,
  event_data: { username: 'mallory' },
  created_at: '2026-10-02T10:00:00Z',
  acknowledgement: null,
}

function mountFeed(permissions: string[]) {
  const pinia = createTestingPinia({ createSpy: vi.fn, stubActions: false })
  const auth = useAuthStore(pinia)
  auth.permissions = permissions
  return mount(AlertFeedView, { global: { plugins: [pinia], stubs } })
}

beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset()
  mocks.classification.mockResolvedValue({
    success: true,
    data: {
      categories: ['user', 'door', 'bypass'],
      severities: ['info', 'notice', 'warning', 'critical'],
      events: {},
    },
  })
  mocks.list.mockResolvedValue({
    success: true,
    data: { items: [ALERT], page: 1, per_page: 50, total: 1, total_pages: 1 },
  })
  vi.stubGlobal(
    'prompt',
    vi.fn(() => 'looked into it')
  )
})

describe('AlertFeedView', () => {
  it('refuses to render the feed without alerts.view, and never asks the server', async () => {
    const w = mountFeed([])
    await flushPromises()
    expect(w.find('[role="alert"]').text()).toContain('alerts.view')
    expect(mocks.list).not.toHaveBeenCalled()
  })

  it('asks for unacknowledged notice-and-above by default and lists what comes back', async () => {
    const w = mountFeed(['alerts.view'])
    await flushPromises()
    expect(mocks.list).toHaveBeenCalledWith({
      page: 1,
      per_page: 50,
      min_severity: 'notice',
      category: undefined,
      acknowledged: false,
    })
    const row = w.find('[data-alert-id="al-1"]')
    expect(row.exists()).toBe(true)
    expect(row.attributes('data-severity')).toBe('notice')
    expect(w.find('[data-testid="total"]').text()).toContain('1 matching')
    // The severity picker never offers "info": that is the audit trail, not the feed.
    const offered = w.findAll('#alert-severity option').map((o) => o.attributes('value'))
    expect(offered).toEqual(['notice', 'warning', 'critical'])
  })

  it('offers Acknowledge only with alerts.acknowledge', async () => {
    const viewer = mountFeed(['alerts.view'])
    await flushPromises()
    expect(viewer.findAll('button').map((b) => b.text())).not.toContain('Acknowledge')

    const acker = mountFeed(['alerts.view', 'alerts.acknowledge'])
    await flushPromises()
    expect(acker.findAll('button').map((b) => b.text())).toContain('Acknowledge')
  })

  it('acknowledges with the note and reloads', async () => {
    mocks.acknowledge.mockResolvedValue({ success: true, data: { ...ALERT, acknowledgement: {} } })
    const w = mountFeed(['alerts.view', 'alerts.acknowledge'])
    await flushPromises()
    await w.find('[data-alert-id="al-1"] button').trigger('click')
    await flushPromises()
    expect(mocks.acknowledge).toHaveBeenCalledWith('al-1', 'looked into it')
    expect(mocks.list).toHaveBeenCalledTimes(2)
  })

  it('sends a changed filter', async () => {
    const w = mountFeed(['alerts.view'])
    await flushPromises()
    await w.find('#alert-severity').setValue('critical')
    await w.find('#alert-category').setValue('bypass')
    await w.find('#alert-ack').setValue('all')
    await flushPromises()
    expect(mocks.list).toHaveBeenLastCalledWith({
      page: 1,
      per_page: 50,
      min_severity: 'critical',
      category: 'bypass',
      acknowledged: undefined,
    })
  })
})
