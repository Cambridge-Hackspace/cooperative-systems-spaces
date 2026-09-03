// Tier 2: AuditLogTable.
//
// The admin's view of the audit trail. Two of its three problems come from one
// line, and the line is a guess at something the server never sends:
//
//     auditLogs.value = Array.isArray(response.data) ? response.data : []
//     totalLogs.value = auditLogs.value.length
//     totalPages.value = Math.max(1, Math.ceil(totalLogs.value / perPage))
//
// `GET /admin/audit-logs` answers `ApiResponse<Vec<AuditLog>>` -- a bare page
// of rows with no total (api/admin.rs:340). So `totalLogs` is the length of the
// page in hand, and since `perPage` is 50 and the server returns at most
// `per_page` rows, `totalPages` is `ceil(n<=50 / 50)`, which is 1. Always.
//
// The pagination block is `v-if="totalPages > 1"`. It therefore never renders,
// and with it `visiblePages` and `goToPage` are unreachable: an admin can read
// the fifty most recent events and has no way to ask for the fifty before them.
//
// What this spec does NOT prove: that the server has more than fifty rows to
// give, or that `?page=2` works. Tier 6 owns that. What is asserted here is
// that this component cannot ask.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({ getAuditLogs: vi.fn() }))
vi.mock('@/utils/api', () => ({ adminApi: mocks }))

import AuditLogTable from '@/components/AuditLogTable.vue'
import type { AuditLog } from '@/types'

function log(over: Partial<AuditLog> = {}): AuditLog {
  return {
    id: 'a1',
    event_type: 'user_login',
    user_id: 'user-1',
    actor_id: null,
    event_data: { ip: '10.0.0.1' },
    ip_address: '10.0.0.1',
    user_agent: 'Mozilla/5.0',
    created_at: '2026-01-14T09:30:00Z',
    ...over,
  }
}

const page = (n: number) => Array.from({ length: n }, (_, i) => log({ id: `a${i}` }))

beforeEach(() => {
  mocks.getAuditLogs.mockReset()
  mocks.getAuditLogs.mockResolvedValue({ success: true, data: [log()] })
})

async function table(rows: AuditLog[] = [log()]) {
  mocks.getAuditLogs.mockResolvedValue({ success: true, data: rows })
  const w = mount(AuditLogTable)
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof table>>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labeled ${JSON.stringify(label)}`)
  return b
}

describe('the request', () => {
  it('asks for the first page with no filter', async () => {
    await table()
    expect(mocks.getAuditLogs).toHaveBeenCalledWith(1, 50, undefined)
  })

  // `applyFilters` also resets `currentPage` to 1, and that line cannot be
  // observed from outside: `currentPage` can never be anything but 1, because
  // the only control that changes it lives in the pagination block that never
  // renders. An equivalent mutant produced by the defect this spec pins, rather
  // than a hole in the spec -- so it is recorded here instead of chased.
  it('re-asks from page one when the filter changes', async () => {
    const w = await table()
    await w.find('select').setValue('failed_login_attempt')
    await flushPromises()
    expect(mocks.getAuditLogs).toHaveBeenLastCalledWith(1, 50, 'failed_login_attempt')
  })

  it('sends no filter again when the filter is cleared', async () => {
    const w = await table()
    await w.find('select').setValue('user_login')
    await flushPromises()
    await w.find('select').setValue('')
    await flushPromises()
    expect(mocks.getAuditLogs).toHaveBeenLastCalledWith(1, 50, undefined)
  })
})

describe('a row as rendered', () => {
  it('splits the timestamp into a date and a time', async () => {
    const w = await table([log({ created_at: '2026-01-14T09:30:05Z' })])
    const cell = w.find('tbody tr td')

    expect(cell.text()).toContain(
      new Date('2026-01-14T09:30:05Z').toLocaleDateString('en-US', {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
      })
    )
    expect(cell.text()).toContain(
      new Date('2026-01-14T09:30:05Z').toLocaleTimeString('en-US', {
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
      })
    )
  })

  it('titles the event type and colors it', async () => {
    const w = await table([log({ event_type: 'failed_login_attempt' })])
    const badge = w.find('tbody .badge')
    expect(badge.text()).toBe('Failed Login Attempt')
    expect(badge.classes()).toContain('badge-error')
  })

  it('falls back to a neutral badge for a type it has no color for', async () => {
    const w = await table([log({ event_type: 'door_unlocked_card' })])
    const badge = w.find('tbody .badge')
    expect(badge.text()).toBe('Door Unlocked Card')
    expect(badge.classes()).toContain('badge-ghost')
  })

  it('marks an absent actor as System and an absent user with a dash', async () => {
    const w = await table([log({ user_id: null, actor_id: null, ip_address: null })])
    const cells = w.findAll('tbody tr td')
    expect(cells[2].text()).toBe('—')
    expect(cells[3].text()).toBe('System')
    expect(cells[4].text()).toBe('—')
  })

  it('shows the ids and the address when they are there', async () => {
    const w = await table([log({ user_id: 'u-1', actor_id: 'admin-1', ip_address: '10.0.0.9' })])
    const cells = w.findAll('tbody tr td')
    expect(cells[2].text()).toBe('u-1')
    expect(cells[3].text()).toBe('admin-1')
    expect(cells[4].text()).toBe('10.0.0.9')
  })
})

describe('the details modal', () => {
  it('opens on the row it was asked about and pretty-prints the event data', async () => {
    const w = await table([log({ event_data: { reason: 'bad password', attempts: 3 } })])
    await w.find('tbody button').trigger('click')
    await nextTick()

    expect(w.find('.modal-open').text()).toContain('Audit Log Details')
    expect(w.find('pre').text()).toBe('{\n  "reason": "bad password",\n  "attempts": 3\n}')
  })

  it('shows the user agent, which the table itself does not', async () => {
    const w = await table([log({ user_agent: 'curl/8.4.0' })])
    expect(w.find('tbody tr').text()).not.toContain('curl/8.4.0')

    await w.find('tbody button').trigger('click')
    await nextTick()
    expect(w.find('.modal-open').text()).toContain('curl/8.4.0')
  })

  it('closes again', async () => {
    const w = await table()
    await w.find('tbody button').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Close').trigger('click')
    await nextTick()
    expect(w.find('.modal-open').exists()).toBe(false)
  })
})

describe('the count and the pages', () => {
  // FINDING, pinned. `totalLogs` is the length of the page in hand, labeled
  // "Total Logs". The endpoint returns a bare `Vec<AuditLog>` with no count,
  // so the number the admin reads is the size of one page, presented as the
  // size of the trail.
  it('reports the page size as the total', async () => {
    const w = await table(page(50))
    expect(
      w.find('.stat-value').text(),
      'the total now comes from somewhere other than the page length -- if the ' +
        'endpoint started returning a count, delete this test'
    ).toBe('50')
  })

  // FINDING, pinned, and the one that costs an admin the audit trail.
  // `totalPages = ceil(totalLogs / 50)` where `totalLogs` is at most 50,
  // because the server never returns more than `per_page`. So `totalPages` is
  // 1 whatever the trail actually holds, the `v-if="totalPages > 1"` block
  // never renders, and there is no control anywhere that reaches page two.
  it('never renders pagination, even on a completely full page', async () => {
    const w = await table(page(50))

    expect(w.findAll('tbody tr')).toHaveLength(50)
    expect(
      w.find('.btn-group').exists(),
      'pagination now renders -- if the endpoint started returning a total, ' +
        'this test should assert the controls instead of their absence'
    ).toBe(false)
    expect(w.findAll('button').map((b) => b.text().trim())).not.toContain('»')
  })

  it('says so when there is nothing at all, and says why when a filter is on', async () => {
    const w = await table([])
    expect(w.text()).toContain('No audit logs are available.')

    mocks.getAuditLogs.mockResolvedValue({ success: true, data: [] })
    await w.find('select').setValue('user_login')
    await flushPromises()
    expect(w.text()).toContain('No logs match your filter criteria.')
  })
})

describe('the event-type filter', () => {
  // FINDING, pinned. `AuditEventType::as_str` in `server/src/models.rs` names
  // sixty-eight event types. This filter offers eleven. Every door event
  // (`door_unlocked_card`, `door_unlock_denied`, `door_unlocked_qr`), every
  // device event and every MFA event is absent -- which is to say, in an
  // access-control system, the events an admin would most want to isolate are
  // the ones the filter cannot reach.
  //
  // The exact set is checked against the server's enum in
  // `tests/structure/audit-event-types.spec.ts`; this asserts only the count,
  // so the two fail independently. Note that the *gap* has not narrowed: the
  // two profile-config options added since this was pinned came with two new
  // server variants, the safety-documentation option came with one, and the six
  // transactional-email options came with six, so the ratchet there still reads
  // 57. An option added without a server variant would be caught by the other
  // file's first test instead.
  it('offers eighteen of the event types the server can write', async () => {
    const w = await table()
    const offered = w
      .findAll('select option')
      .map((o) => o.attributes('value'))
      .filter((v) => v !== '')

    expect(
      offered,
      'the filter list changed -- update tests/structure/audit-event-types.spec.ts, ' +
        'which holds the comparison against the server enum. Went from eleven ' +
        'to twelve when training_documentation_acknowledged was added -- a record ' +
        'kept so it can be produced later is not usable if it cannot be found -- ' +
        'and to eighteen with the six transactional-email types.'
    ).toHaveLength(18)
  })

  it('titles every option it does offer', async () => {
    const w = await table()
    for (const option of w.findAll('select option')) {
      expect(option.text().trim()).not.toBe('')
    }
  })
})

describe('when the request fails', () => {
  it('shows the reason, offers a retry, and tells the parent', async () => {
    mocks.getAuditLogs.mockResolvedValue({ success: false, error: 'Admin role required' })
    const w = mount(AuditLogTable)
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('Admin role required')
    expect(w.emitted('error')?.[0]).toEqual(['Admin role required'])

    mocks.getAuditLogs.mockResolvedValue({ success: true, data: [log()] })
    await buttonNamed(w, 'Retry').trigger('click')
    await flushPromises()
    expect(w.find('tbody tr').exists()).toBe(true)
  })

  it('falls back to a generic reason when the refusal carries none', async () => {
    mocks.getAuditLogs.mockResolvedValue({ success: false })
    const w = mount(AuditLogTable)
    await flushPromises()
    expect(w.find('.alert-error').text()).toContain('Failed to load audit logs')
  })

  it('reads the server body on a thrown error, unlike most of its siblings', async () => {
    // Worth asserting as behavior to keep: this catch reads
    // `err.response?.data?.error`, which is the key the envelope actually
    // fills. ToolEventHistory reads `.message` and gets nothing.
    mocks.getAuditLogs.mockRejectedValue({ response: { data: { error: 'Token expired' } } })
    const w = mount(AuditLogTable)
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('Token expired')
    expect(w.emitted('error')?.[0]).toEqual(['Token expired'])
  })

  it('stops loading whether the request resolved or rejected', async () => {
    mocks.getAuditLogs.mockRejectedValue(new Error('Network Error'))
    const w = mount(AuditLogTable)
    await flushPromises()
    expect(w.find('.loading-spinner').exists()).toBe(false)
  })

  it('shows a spinner while the request is in flight', async () => {
    mocks.getAuditLogs.mockReturnValue(new Promise(() => {}))
    const w = mount(AuditLogTable)
    await nextTick()
    expect(w.find('.loading-spinner').exists()).toBe(true)
    expect(w.find('table').exists()).toBe(false)
  })
})
