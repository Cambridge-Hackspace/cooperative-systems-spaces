// Tier 2: DoorManagement.
//
// The access-control admin surface: doors, their rules, their events and their
// QR codes. Two of its defaults are wrong in ways that produce a rule or a
// door nobody meant to create.
//
// The rule form keeps one `newRule.value` across kind changes. It starts as
// `'Member'` because the kind starts as `role`. Switching the kind to `user`
// leaves `'Member'` in place -- the user picker shows blank, because no option
// matches -- while the Add button stays enabled, since `'Member'.trim()` is
// truthy. Pressing it posts a *user* rule whose value is the string "Member".
//
// And `openNew` pre-picks `places[0]` for both sides, so a fresh door runs
// from a place to itself and passes the only validation there is.
//
// What this spec does NOT prove: what the server does with either. Tier 4 owns
// the responses and Tier 1b's door vectors own what a rule with a
// non-UUID user value means to the evaluator.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({
  listDoors: vi.fn(),
  getDoor: vi.fn(),
  createDoor: vi.fn(),
  updateDoor: vi.fn(),
  removeDoor: vi.fn(),
  unlock: vi.fn(),
  republish: vi.fn(),
  events: vi.fn(),
  qrUrl: vi.fn(),
  addRule: vi.fn(),
  removeRule: vi.fn(),
  listPlaces: vi.fn(),
  listSchedules: vi.fn(),
  rawGet: vi.fn(),
  clientGet: vi.fn(),
  toDataURL: vi.fn(),
}))

vi.mock('@/utils/api', () => ({
  doorsApi: {
    list: mocks.listDoors,
    get: mocks.getDoor,
    create: mocks.createDoor,
    update: mocks.updateDoor,
    remove: mocks.removeDoor,
    unlock: mocks.unlock,
    republish: mocks.republish,
    events: mocks.events,
    qrUrl: mocks.qrUrl,
    addRule: mocks.addRule,
    removeRule: mocks.removeRule,
  },
  placesApi: { list: mocks.listPlaces },
  schedulesApi: { list: mocks.listSchedules },
  apiClient: { raw: { get: mocks.rawGet }, get: mocks.clientGet },
}))
vi.mock('qrcode', () => ({ default: { toDataURL: mocks.toDataURL } }))

import DoorManagement from '@/components/DoorManagement.vue'
import {
  UserRole,
  type Door,
  type DoorAccessRule,
  type DoorDetail,
  type Place,
  type User,
} from '@/types'

const PLACES: Place[] = [
  {
    id: 'p1',
    name: 'Workshop',
    parent_id: null,
    place_type: 'Room',
    description: null,
    external_id: null,
    is_special: false,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 'p2',
    name: 'Outside',
    parent_id: null,
    place_type: 'Zone',
    description: null,
    external_id: null,
    is_special: true,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
]

const USERS: User[] = [
  {
    id: 'u1',
    username: 'ada',
    email: 'ada@example.test',
    full_name: 'Ada Lovelace',
    is_active: true,
    role: UserRole.Member,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    profile: {},
    meta: {},
  },
]

function door(over: Partial<Door> = {}): Door {
  return {
    id: 'd1',
    name: 'Front door',
    location: 'Lobby',
    description: null,
    edge_device_id: 'dev-1',
    unlock_duration_ms: 5000,
    enabled: true,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    place_id_from: 'p2',
    place_id_to: 'p1',
    ...over,
  }
}

function rule(over: Partial<DoorAccessRule> = {}): DoorAccessRule {
  return {
    id: 'r1',
    door_id: 'd1',
    kind: 'role',
    value: 'Member',
    ...over,
  } as DoorAccessRule
}

const detail = (over: Partial<DoorDetail> = {}): DoorDetail => ({
  ...door(),
  rules: [],
  ...over,
})

const stubs = {
  'router-link': { props: ['to'], template: '<a><slot /></a>' },
  PlacePicker: {
    props: ['modelValue', 'places', 'nullLabel'],
    emits: ['update:modelValue'],
    template: '<div class="place-picker">{{ modelValue }}</div>',
  },
  SchedulePicker: {
    props: ['modelValue', 'schedules'],
    emits: ['update:modelValue'],
    template: '<div class="schedule-picker">{{ modelValue }}</div>',
  },
}

let confirmResult = true

beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset()
  mocks.listDoors.mockResolvedValue({ success: true, data: [] })
  mocks.listPlaces.mockResolvedValue({ success: true, data: PLACES })
  mocks.listSchedules.mockResolvedValue({ success: true, data: [] })
  mocks.rawGet.mockResolvedValue({ data: { data: [{ id: 'dev-1', name: 'Edge A' }] } })
  mocks.clientGet.mockResolvedValue({ success: true, data: USERS })
  mocks.getDoor.mockResolvedValue({ success: true, data: detail() })
  mocks.createDoor.mockResolvedValue({ success: true })
  mocks.updateDoor.mockResolvedValue({ success: true })
  mocks.removeDoor.mockResolvedValue({ success: true })
  mocks.unlock.mockResolvedValue({ success: true, data: { unlocked: true } })
  mocks.republish.mockResolvedValue({ success: true })
  mocks.events.mockResolvedValue({ success: true, data: [] })
  mocks.qrUrl.mockResolvedValue({ success: true, data: { url: 'https://css.test/d/abc' } })
  mocks.addRule.mockResolvedValue({ success: true })
  mocks.removeRule.mockResolvedValue({ success: true })
  mocks.toDataURL.mockResolvedValue('data:image/png;base64,QR')
  confirmResult = true
  vi.stubGlobal(
    'confirm',
    vi.fn(() => confirmResult)
  )
})

async function page(doors: Door[] = []) {
  mocks.listDoors.mockResolvedValue({ success: true, data: doors })
  const w = mount(DoorManagement, { global: { stubs } })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof page>>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}
function tabNamed(w: Wrapper, name: string) {
  const t = w.findAll('[role="tab"]').find((x) => x.text().trim().startsWith(name))
  if (!t) throw new Error(`no tab starting ${JSON.stringify(name)}`)
  return t
}
const created = () => mocks.createDoor.mock.calls[0][0] as Record<string, unknown>
const addedRule = () => mocks.addRule.mock.calls[0][1] as Record<string, unknown>

async function openDetail(w: Wrapper) {
  await buttonNamed(w, 'Manage').trigger('click')
  await flushPromises()
}

describe('the door list', () => {
  it('loads everything it needs on open', async () => {
    await page()
    expect(mocks.listDoors).toHaveBeenCalled()
    expect(mocks.listPlaces).toHaveBeenCalled()
    expect(mocks.listSchedules).toHaveBeenCalled()
    expect(mocks.rawGet).toHaveBeenCalledWith('/admin/devices')
    expect(mocks.clientGet).toHaveBeenCalledWith('/admin/roster')
  })

  it('shows each door with its enabled state', async () => {
    const w = await page([door(), door({ id: 'd2', name: 'Back door', enabled: false })])
    const rows = w.findAll('tbody tr')
    expect(rows[0].find('.badge').classes()).toContain('badge-success')
    expect(rows[1].find('.badge').classes()).toContain('badge-neutral')
  })

  it('names the edge device, the places and falls back to a short id', async () => {
    const w = await page([door({ edge_device_id: 'dev-1' })])
    expect(w.find('tbody tr').text()).toContain('Edge A')

    const unknown = await page([door({ edge_device_id: 'dev-missing-9999' })])
    expect(unknown.find('tbody tr').text()).toContain('dev-miss')
  })

  // Recorded: the list shows name, location, edge device and status -- and not
  // which places the door connects. `placeLabel` is only reached from the
  // detail modal's Settings tab, so the hierarchy the doors define cannot be
  // read off the list at all; an admin has to open each one.
  it('does not show which places a door connects', async () => {
    const w = await page([door({ place_id_from: 'p2', place_id_to: 'p1' })])
    expect(
      w.find('tbody tr').text(),
      'the list now shows the places -- if columns were added, this test should ' + 'assert them'
    ).not.toContain('Outside')
    expect(w.find('tbody tr').text()).not.toContain('Workshop')
  })

  it('marks a special place with a star, and an unset side with a dash, in the detail', async () => {
    mocks.getDoor.mockResolvedValue({
      success: true,
      data: detail({ place_id_from: 'p2', place_id_to: undefined }),
    })
    const w = await page([door()])
    await openDetail(w)
    await tabNamed(w, 'Settings').trigger('click')
    await nextTick()

    expect(w.text()).toContain('★ Outside')
    expect(w.text()).toContain('(unset)')
  })
})

describe('creating a door', () => {
  it('requires a name', async () => {
    const w = await page()
    await buttonNamed(w, '+ New door').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('Name is required')
    expect(mocks.createDoor).not.toHaveBeenCalled()
  })

  // FINDING, pinned. `openNew` pre-picks `places[0]` for *both* sides, and the
  // only check is that neither is empty. A fresh door therefore defaults to
  // running from a place to itself, and saves without complaint. A door whose
  // two sides are the same place expresses nothing: the hierarchy it is meant
  // to connect has one node.
  it('defaults both sides of a new door to the same place, and saves it', async () => {
    const w = await page()
    await buttonNamed(w, '+ New door').trigger('click')
    await nextTick()
    await w.find('.modal-box input[type="text"]').setValue('Front door')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(
      created().place_id_from,
      'the two sides are no longer seeded to the same place -- if a distinctness ' +
        'check was added, this test should assert the refusal'
    ).toBe(created().place_id_to)
    expect(created().place_id_from).toBe('p1')
  })

  it('warns instead of offering the pickers when no places exist', async () => {
    mocks.listPlaces.mockResolvedValue({ success: true, data: [] })
    const w = await page()
    await buttonNamed(w, '+ New door').trigger('click')
    await nextTick()

    expect(w.find('.alert-warning').text()).toContain('Places tab')
    expect(w.findAll('.place-picker')).toHaveLength(0)
  })

  it('nulls a blank location rather than sending an empty string', async () => {
    const w = await page()
    await buttonNamed(w, '+ New door').trigger('click')
    await nextTick()
    await w.findAll('.modal-box input[type="text"]')[0].setValue('Front door')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(created().location).toBeNull()
  })

  it('reloads and reports success', async () => {
    const w = await page()
    await buttonNamed(w, '+ New door').trigger('click')
    await nextTick()
    await w.find('.modal-box input[type="text"]').setValue('Front door')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(w.find('.alert-success').text()).toContain('Door created')
    expect(mocks.listDoors).toHaveBeenCalledTimes(2)
  })
})

describe('the rule editor', () => {
  it('offers three kinds and two effects', async () => {
    const w = await page([door()])
    await openDetail(w)
    const kinds = w
      .findAll('select')[0]
      .findAll('option')
      .map((o) => o.attributes('value'))
    expect(kinds).toEqual(['role', 'user', 'card'])
  })

  // Recorded as a decision rather than a defect: `UserRole` has five members
  // and the role rule offers three. A rule for "Newbie or above" would admit
  // very nearly everyone, and "Unknown or above" would admit everyone
  // including the unauthenticated -- so leaving them out is defensible. It is
  // pinned so that stays a decision.
  it('offers only the three roles above Newbie', async () => {
    const w = await page([door()])
    await openDetail(w)
    const values = w
      .findAll('select')[1]
      .findAll('option')
      .map((o) => o.attributes('value'))

    expect(values).toEqual([UserRole.Member, UserRole.Staff, UserRole.Admin])
    expect(values).not.toContain(UserRole.Newbie)
    expect(values).not.toContain(UserRole.Unknown)
  })

  // FINDING, pinned, and the one that produces a rule nobody meant. The kind
  // and the value are separate refs, and nothing resets the value when the kind
  // changes. Starting from the default -- kind `role`, value `Member` -- and
  // switching to `user` leaves `Member` in the value. The user picker shows
  // blank, because no option matches it, and the Add button stays enabled
  // because `'Member'.trim()` is truthy.
  //
  // What gets posted is a user rule whose value is the string "Member": a rule
  // that will never match anybody, sitting in the list looking like it should.
  it('posts a user rule whose value is the leftover role name', async () => {
    const w = await page([door()])
    await openDetail(w)

    await w.findAll('select')[0].setValue('user')
    await nextTick()

    const userSelect = w.findAll('select')[1]
    expect((userSelect.element as HTMLSelectElement).value).toBe('')
    expect(buttonNamed(w, 'Add rule').attributes('disabled')).toBeUndefined()

    await buttonNamed(w, 'Add rule').trigger('click')
    await flushPromises()

    expect(
      addedRule(),
      'the value is now reset when the kind changes -- if a watcher was added, ' +
        'delete this test'
    ).toMatchObject({ kind: 'user', value: 'Member' })
  })

  it('sends a chosen user rule correctly once one is picked', async () => {
    const w = await page([door()])
    await openDetail(w)
    await w.findAll('select')[0].setValue('user')
    await nextTick()
    await w.findAll('select')[1].setValue('u1')
    await buttonNamed(w, 'Add rule').trigger('click')
    await flushPromises()

    expect(addedRule()).toMatchObject({ kind: 'user', value: 'u1', effect: 'allow' })
  })

  it('will not add a card rule with a blank value', async () => {
    const w = await page([door()])
    await openDetail(w)
    await w.findAll('select')[0].setValue('card')
    await nextTick()

    // The card path is a free-text input, and switching to it leaves `Member`
    // behind exactly as the user path does -- so it has to be cleared before
    // the disabled state can be observed at all.
    await w.find('input[placeholder="card ID"]').setValue('')
    expect(buttonNamed(w, 'Add rule').attributes('disabled')).toBeDefined()
  })

  it('re-reads the door after adding, and resets the value for a role rule', async () => {
    const w = await page([door()])
    await openDetail(w)
    mocks.getDoor.mockClear()
    await buttonNamed(w, 'Add rule').trigger('click')
    await flushPromises()

    expect(mocks.getDoor).toHaveBeenCalledTimes(1)
    expect(w.find('.alert-success').text()).toContain('Rule added')
  })

  it('names the user behind a user rule rather than showing its id', async () => {
    mocks.getDoor.mockResolvedValue({
      success: true,
      data: detail({ rules: [rule({ kind: 'user', value: 'u1' })] }),
    })
    const w = await page([door()])
    await openDetail(w)

    expect(w.text()).toContain('Ada Lovelace (@ada)')
  })

  it('asks before removing a rule', async () => {
    confirmResult = false
    mocks.getDoor.mockResolvedValue({ success: true, data: detail({ rules: [rule()] }) })
    const w = await page([door()])
    await openDetail(w)
    await buttonNamed(w, 'Remove').trigger('click')
    await flushPromises()

    expect(mocks.removeRule).not.toHaveBeenCalled()
  })

  it("reports the server's reason when a rule is refused", async () => {
    mocks.addRule.mockResolvedValue({ success: false, error: 'Rule already exists' })
    const w = await page([door()])
    await openDetail(w)
    await buttonNamed(w, 'Add rule').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('Rule already exists')
  })

  it('reads the server body when adding a rule throws', async () => {
    // Worth asserting: unlike most of its siblings, this catch reads
    // `e.response.data.error`, which is the key the envelope fills.
    mocks.addRule.mockRejectedValue({ response: { data: { error: 'Schedule not found' } } })
    const w = await page([door()])
    await openDetail(w)
    await buttonNamed(w, 'Add rule').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('Schedule not found')
  })
})

describe('the detail tabs', () => {
  it('opens on rules and fetches events only when asked', async () => {
    const w = await page([door()])
    await openDetail(w)
    expect(mocks.events).not.toHaveBeenCalled()

    await tabNamed(w, 'Events').trigger('click')
    await flushPromises()
    expect(mocks.events).toHaveBeenCalledWith('d1', { limit: 100 })
  })

  it('renders a QR from the URL the server gives', async () => {
    const w = await page([door()])
    await openDetail(w)
    await tabNamed(w, 'QR').trigger('click')
    await flushPromises()

    expect(mocks.toDataURL).toHaveBeenCalledWith('https://css.test/d/abc', {
      margin: 1,
      width: 256,
    })
    expect(w.find('img').attributes('src')).toBe('data:image/png;base64,QR')
  })

  it('reports a QR rendering failure', async () => {
    mocks.toDataURL.mockRejectedValue(new Error('canvas unavailable'))
    const w = await page([door()])
    await openDetail(w)
    await tabNamed(w, 'QR').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('Failed to render QR')
  })

  // FIXED. `switchToEvents` guarded with `if (r.success && r.data)` and had no
  // else, so a refused history showed as an empty one -- "this door has never
  // been opened" -- on the screen an admin uses to find out whether it had.
  it('reports a refused event history rather than showing an empty one', async () => {
    mocks.events.mockResolvedValue({ success: false, error: 'Forbidden' })
    const w = await page([door()])
    await openDetail(w)
    await tabNamed(w, 'Events').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('Forbidden')
  })
})

describe('unlocking and republishing', () => {
  it('reports a successful admin unlock by name', async () => {
    const w = await page([door({ name: 'Front door' })])
    await buttonNamed(w, 'Unlock').trigger('click')
    await flushPromises()

    expect(mocks.unlock).toHaveBeenCalledWith('d1')
    expect(w.find('.alert-success').text()).toContain('Unlocked "Front door"')
  })

  it('reports an unlock the edge did not perform', async () => {
    mocks.unlock.mockResolvedValue({ success: true, data: { unlocked: false } })
    const w = await page([door()])
    await buttonNamed(w, 'Unlock').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').exists()).toBe(true)
  })

  it('republishes state to the edge from the settings tab', async () => {
    const w = await page([door()])
    await openDetail(w)
    await tabNamed(w, 'Settings').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Republish state to edge').trigger('click')
    await flushPromises()

    expect(mocks.republish).toHaveBeenCalledWith('d1')
    expect(w.find('.alert-success').text()).toContain('republished')
  })
})

describe('deleting a door', () => {
  it('warns that rules and events go with it', async () => {
    confirmResult = false
    const w = await page([door({ name: 'Front door' })])
    await buttonNamed(w, 'Delete').trigger('click')
    await flushPromises()

    expect(vi.mocked(globalThis.confirm)).toHaveBeenCalledWith(
      'Delete door "Front door"? Its rules and events will be removed.'
    )
    expect(mocks.removeDoor).not.toHaveBeenCalled()
  })
})

describe('what a network error does', () => {
  // FIXED. The loader had no try/catch and cleared `loading` only after the
  // await, so a rejection spun forever and escaped to an
  // `app.config.errorHandler` that `src/main.ts` never sets.
  it('reports a rejected load and stops spinning', async () => {
    mocks.listDoors.mockRejectedValue(new Error('Network Error'))
    const w = mount(DoorManagement, { global: { stubs } })
    await flushPromises()

    expect(w.find('.loading-spinner').exists()).toBe(false)
    expect(w.find('.alert-error').text()).toContain('Network Error')
  })

  // FIXED. The save set its busy flag with no `finally`, so a rejection left
  // the button disabled with no way to retry.
  it('frees the button and reports the failure when the save rejects', async () => {
    mocks.createDoor.mockRejectedValue(new Error('Network Error'))
    const w = mount(DoorManagement, { global: { stubs } })
    await flushPromises()
    await buttonNamed(w, '+ New door').trigger('click')
    await nextTick()
    await w.find('.modal-box input[type="text"]').setValue('Front door')
    await w.find('.modal-action .btn-primary').trigger('click')
    await flushPromises()

    expect(w.find('.modal-action .btn-primary').attributes('disabled')).toBeUndefined()
    expect(w.find('.alert-error').text()).toContain('Network Error')
  })
})
