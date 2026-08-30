// Tier 2: DeviceManagement.
//
// Edge devices and the invite codes that enrol them. Everything here goes
// through `apiClient.raw` -- the bare axios instance -- rather than the typed
// client, so none of the envelope handling applies and every failure surfaces
// as `err.message`, which for axios is the status restated. Third component in
// the inventory to bypass the shared client, after PageViewer and
// ToolEditModal, and the only one to do it for every call.
//
// The consequence worth pinning is where those failures go: five of the six
// write paths report through `alert()`. A failed rename, a failed delete, a
// failed invite generation and a failed expiry all produce a browser modal
// saying "Failed to ... : Request failed with status code 403", which is the
// least useful form of the least useful message.
//
// What this spec does NOT prove: what the endpoints answer. Tier 4 owns that.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  post: vi.fn(),
  patch: vi.fn(),
  delete: vi.fn(),
}))
vi.mock('@/utils/api', () => ({ apiClient: { raw: mocks } }))

import DeviceManagement from '@/components/DeviceManagement.vue'

interface Device {
  id: string
  name: string
  kind: string
  mac_address: string | null
  platform: string | null
  software_version: string | null
  ipv4_address: string | null
  ipv6_address: string | null
  uptime: number
  last_seen_at: string | null
  is_online: boolean
}

function device(over: Partial<Device> = {}): Device {
  return {
    id: 'dev-1',
    name: 'Front door edge',
    kind: 'edge',
    mac_address: 'aa:bb:cc:dd:ee:ff',
    platform: 'linux',
    software_version: '0.4.1',
    ipv4_address: '10.0.0.9',
    ipv6_address: null,
    uptime: 90_000,
    last_seen_at: '2026-01-15T11:30:00Z',
    is_online: true,
    ...over,
  }
}

const invite = (over: Record<string, unknown> = {}) => ({
  device_code: '🔧🔩⚙️🔨🪛🧰🔗🪚',
  expires_at: '2026-01-16T12:00:00Z',
  used_at: null,
  used_by_device_name: null,
  ...over,
})

let alertSpy: { mock: { calls: unknown[][] }; mockClear: () => void }

beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset()
  mocks.get.mockImplementation((url: string) =>
    Promise.resolve({ data: { data: url.includes('invites') ? [] : [] } })
  )
  mocks.post.mockResolvedValue({ data: { data: { device_code: 'ABC', expires_at: 'x' } } })
  mocks.patch.mockResolvedValue({ data: {} })
  mocks.delete.mockResolvedValue({ data: {} })
  alertSpy = vi.spyOn(globalThis, 'alert').mockImplementation(() => {})
  vi.spyOn(console, 'error').mockImplementation(() => {})
})

async function page(devices: Device[] = [], invites: unknown[] = []) {
  mocks.get.mockImplementation((url: string) =>
    Promise.resolve({ data: { data: url.includes('invites') ? invites : devices } })
  )
  const w = mount(DeviceManagement)
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof page>>

// Several buttons carry an emoji icon in a nested span, so an exact match on
// the trimmed text misses them. Matched on the trailing words instead, which is
// still specific enough to tell Rename from Rename Device.
function buttonNamed(w: Wrapper, label: string) {
  const all = w.findAll('button')
  const b =
    all.find((btn) => btn.text().trim() === label) ??
    all.find((btn) => btn.text().trim().endsWith(label))
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}
const alerts = () =>
  alertSpy.mock.calls.map((c) => (typeof c[0] === 'string' ? c[0] : JSON.stringify(c[0])))

describe('loading', () => {
  it('reads devices and invites on open', async () => {
    await page()
    expect(mocks.get).toHaveBeenCalledWith('/admin/devices')
    expect(mocks.get).toHaveBeenCalledWith('/admin/devices/invites')
  })

  it('lists a device with its address, version and status', async () => {
    const w = await page([device()])
    const row = w.find('.devices-tab')
    expect(row.text()).toContain('Front door edge')
    expect(row.text()).toContain('10.0.0.9')
    expect(row.text()).toContain('0.4.1')
    expect(w.find('.status-dot').classes()).toContain('online')
  })

  it('marks an offline device differently, on the card as well as the dot', async () => {
    // Two separate bindings carry the same flag -- the card wrapper and the
    // status dot -- so both are asserted; a test on one alone leaves the other
    // unchecked.
    const offline = await page([device({ is_online: false })])
    expect(offline.find('.status-dot').classes()).not.toContain('online')
    expect(offline.find('.device-card, .device-item').classes()).not.toContain('online')

    const online = await page([device({ is_online: true })])
    expect(online.find('.status-dot').classes()).toContain('online')
    expect(online.find('.device-card, .device-item').classes()).toContain('online')
  })

  it('says so when there are no devices and no invites', async () => {
    const w = await page()
    expect(w.find('.devices-tab .empty-state').exists()).toBe(true)
  })

  it('reports a failed device load on the page, not in an alert', async () => {
    mocks.get.mockRejectedValue(new Error('Request failed with status code 403'))
    const w = mount(DeviceManagement)
    await flushPromises()

    expect(w.find('.devices-tab .error').text()).toContain('Failed to load devices')
    expect(alerts()).toHaveLength(0)
  })

  it('reports a failed invite load separately from the device load', async () => {
    mocks.get.mockImplementation((url: string) =>
      url.includes('invites')
        ? Promise.reject(new Error('boom'))
        : Promise.resolve({ data: { data: [device()] } })
    )
    const w = mount(DeviceManagement)
    await flushPromises()
    await buttonNamed(w, 'Invites').trigger('click')
    await nextTick()

    expect(w.find('.invites-tab .error').text()).toContain('Failed to load invites')
    expect(w.find('.devices-tab').exists()).toBe(false)
  })
})

describe('how time is shown', () => {
  // The clock is frozen at 2026-01-15T12:00:00Z.
  it('describes how long ago a device was last seen', async () => {
    const cases: [string | null, string][] = [
      [null, 'Never'],
      ['2026-01-15T11:59:30Z', 'Just now'],
      ['2026-01-15T11:30:00Z', '30 minutes ago'],
      ['2026-01-15T09:00:00Z', '3 hours ago'],
      ['2026-01-12T12:00:00Z', '3 days ago'],
    ]
    for (const [seen, expected] of cases) {
      const w = await page([device({ last_seen_at: seen })])
      expect(w.find('.devices-tab').text(), `for ${String(seen)}`).toContain(expected)
    }
  })

  it('summarises uptime at the largest useful unit', async () => {
    const cases: [number, string][] = [
      [90_000, '1d 1h'],
      [7_200, '2h 0m'],
      [600, '10m'],
    ]
    for (const [uptime, expected] of cases) {
      const w = await page([device({ uptime })])
      expect(w.find('.devices-tab').text(), `for ${uptime}s`).toContain(expected)
    }
  })

  // FINDING, pinned. `formatUptime` opens with `if (!seconds) return 'N/A'`, so
  // a device that has just restarted -- uptime zero -- is reported as having
  // no uptime information at all, which is the same thing this field shows for
  // a device that never reported any. On an access-control edge box, "just
  // rebooted" and "never told us" are different facts.
  it('reports a freshly rebooted device as having no uptime at all', async () => {
    const w = await page([device({ uptime: 0 })])
    expect(
      w.find('.devices-tab').text(),
      'zero uptime now renders as 0m -- if the falsy check became a nullish ' +
        'one, delete this test'
    ).toContain('N/A')
  })
})

describe('invites', () => {
  it('classifies an invite as active, used or expired', async () => {
    const w = await page(
      [],
      [
        invite({ device_code: 'A' }),
        invite({ device_code: 'B', used_at: '2026-01-14T00:00:00Z' }),
        invite({ device_code: 'C', expires_at: '2026-01-14T00:00:00Z' }),
      ]
    )
    await buttonNamed(w, 'Invites').trigger('click')
    await nextTick()

    const badges = w.findAll('.status-badge').map((b) => b.classes().join(' '))
    expect(badges[0]).toContain('active')
    expect(badges[1]).toContain('used')
    expect(badges[2]).toContain('expired')
  })

  it('offers to expire only an invite that is still usable', async () => {
    const w = await page(
      [],
      [invite({ device_code: 'A' }), invite({ device_code: 'B', used_at: '2026-01-14T00:00:00Z' })]
    )
    await buttonNamed(w, 'Invites').trigger('click')
    await nextTick()

    expect(w.findAll('.invites-table .btn-danger')).toHaveLength(1)
  })

  it('asks before expiring, and refreshes afterwards', async () => {
    vi.stubGlobal(
      'confirm',
      vi.fn(() => true)
    )
    const w = await page([], [invite({ device_code: 'ABC' })])
    await buttonNamed(w, 'Invites').trigger('click')
    await nextTick()
    mocks.get.mockClear()
    await w.find('.invites-table .btn-danger').trigger('click')
    await flushPromises()

    expect(mocks.delete).toHaveBeenCalledWith('/admin/devices/invites/ABC')
    expect(mocks.get).toHaveBeenCalledWith('/admin/devices/invites')
  })

  it('does nothing when the expiry is declined', async () => {
    vi.stubGlobal(
      'confirm',
      vi.fn(() => false)
    )
    const w = await page([], [invite()])
    await buttonNamed(w, 'Invites').trigger('click')
    await nextTick()
    await w.find('.invites-table .btn-danger').trigger('click')
    await flushPromises()

    expect(mocks.delete).not.toHaveBeenCalled()
  })

  it('generates a code and shows it', async () => {
    mocks.post.mockResolvedValue({
      data: { data: { device_code: '🔧🔩⚙️', expires_at: '2026-01-16T12:00:00Z' } },
    })
    const w = await page()
    await buttonNamed(w, 'Generate Device Invite').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Generate Invite').trigger('click')
    await flushPromises()

    expect(mocks.post).toHaveBeenCalledWith('/admin/devices/invite', {})
    expect(w.find('.generated-invite').text()).toContain('🔧🔩⚙️')
  })

  // Behaviour worth keeping, and the component's own comment explains why: the
  // clipboard write is awaited and its failure handled, so an admin on an
  // insecure origin is given the code in the fallback message rather than
  // being told it was copied when it was not.
  it('hands over the code in the message when the clipboard refuses', async () => {
    const writeText = vi.fn().mockRejectedValue(new Error('not allowed'))
    vi.stubGlobal('navigator', { clipboard: { writeText } })
    mocks.post.mockResolvedValue({
      data: { data: { device_code: 'CODE-9', expires_at: 'x' } },
    })
    const w = await page()
    await buttonNamed(w, 'Generate Device Invite').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Generate Invite').trigger('click')
    await flushPromises()
    alertSpy.mockClear()
    await buttonNamed(w, '📋 Copy Code').trigger('click')
    await flushPromises()

    expect(alerts().join('\n')).toContain('CODE-9')
    expect(alerts().join('\n')).not.toContain('copied to clipboard')
  })

  it('says it copied when it did', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    vi.stubGlobal('navigator', { clipboard: { writeText } })
    const w = await page()
    await buttonNamed(w, 'Generate Device Invite').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Generate Invite').trigger('click')
    await flushPromises()
    alertSpy.mockClear()
    await buttonNamed(w, '📋 Copy Code').trigger('click')
    await flushPromises()

    expect(writeText).toHaveBeenCalled()
    expect(alerts().join('\n')).toContain('copied to clipboard')
  })
})

describe('renaming and deleting', () => {
  it('opens the rename dialog seeded with the current name', async () => {
    const w = await page([device({ name: 'Front door edge' })])
    await w.find('.btn-secondary').trigger('click')
    await nextTick()

    expect((w.find('.form-input').element as HTMLInputElement).value).toBe('Front door edge')
  })

  it('sends the new name and refreshes', async () => {
    const w = await page([device({ id: 'dev-7' })])
    await w.find('.btn-secondary').trigger('click')
    await nextTick()
    await w.find('.form-input').setValue('Back door edge')
    mocks.get.mockClear()
    await buttonNamed(w, 'Rename').trigger('click')
    await flushPromises()

    expect(mocks.patch).toHaveBeenCalledWith('/admin/devices/dev-7/name', {
      name: 'Back door edge',
    })
    expect(mocks.get).toHaveBeenCalledWith('/admin/devices')
  })

  it('asks for confirmation in a dialog before deleting', async () => {
    const w = await page([device({ id: 'dev-7', name: 'Front door edge' })])
    await buttonNamed(w, 'Delete').trigger('click')
    await nextTick()

    expect(w.text()).toContain('Front door edge')
    expect(mocks.delete).not.toHaveBeenCalled()

    await buttonNamed(w, 'Delete Device').trigger('click')
    await flushPromises()
    expect(mocks.delete).toHaveBeenCalledWith('/admin/devices/dev-7')
  })
})

describe('where failures go', () => {
  // FINDING, pinned. Five of the six write paths report through `alert()`, and
  // the text is `err.message` -- which for axios is the status restated, not
  // the server's explanation. A rename refused because the name is taken says
  // "Failed to rename device: Request failed with status code 409" in a
  // browser modal, and the server's actual words are discarded.
  //
  // The two load paths do render their errors on the page, so this is
  // inconsistent as well as unhelpful.
  const writeFailures: [string, (w: Wrapper) => Promise<void>, string][] = [
    [
      'rename',
      async (w) => {
        await w.find('.btn-secondary').trigger('click')
        await nextTick()
        await buttonNamed(w, 'Rename').trigger('click')
        await flushPromises()
      },
      'Failed to rename device',
    ],
    [
      'delete',
      async (w) => {
        await buttonNamed(w, 'Delete').trigger('click')
        await nextTick()
        await buttonNamed(w, 'Delete Device').trigger('click')
        await flushPromises()
      },
      'Failed to delete device',
    ],
  ]

  for (const [name, act, expected] of writeFailures) {
    it(`reports a failed ${name} in a browser alert, with the axios message`, async () => {
      mocks.patch.mockRejectedValue(
        Object.assign(new Error('Request failed with status code 409'), {
          response: { data: { error: 'That name is already in use' } },
        })
      )
      mocks.delete.mockRejectedValue(
        Object.assign(new Error('Request failed with status code 409'), {
          response: { data: { error: 'That device is still assigned to a door' } },
        })
      )
      const w = await page([device()])
      alertSpy.mockClear()
      await act(w)

      const said = alerts().join('\n')
      expect(
        said,
        `the ${name} failure is now reported on the page -- if that was fixed, ` +
          'this test should assert the rendered message'
      ).toContain(expected)
      expect(said).toContain('Request failed with status code 409')
      expect(said).not.toContain('already in use')
      expect(said).not.toContain('still assigned')
    })
  }

  it('re-enables the rename button after a failure', async () => {
    mocks.patch.mockRejectedValue(new Error('down'))
    const w = await page([device()])
    await w.find('.btn-secondary').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Rename').trigger('click')
    await flushPromises()

    expect(buttonNamed(w, 'Rename').attributes('disabled')).toBeUndefined()
  })
})
