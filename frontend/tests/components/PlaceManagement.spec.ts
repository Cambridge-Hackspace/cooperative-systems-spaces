// Tier 2: PlaceManagement.
//
// The hierarchy editor. `PlaceNode` and `PlacePicker` are stubbed -- both have
// their own specs, and what is under test here is which places go into which
// list, which types the form offers, and which parents it will let you move to.
//
// The type list is derived from `formParent`:
//
//     const allowedTypes = computed(() => { ... config.types.slice(i + 1) })
//
// and `formParent` is set when the modal opens. The "Move to (parent)" picker
// writes `form.parent_id`. Nothing connects the two, so choosing a different
// parent leaves the Type dropdown showing the levels that were legal under the
// *old* parent. That is the finding below.
//
// What this spec does NOT prove: that the server enforces the level rules, or
// rejects a cycle. The client's cycle guard is asserted here; the server's is
// Tier 6's.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({
  config: vi.fn(),
  list: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  remove: vi.fn(),
}))
vi.mock('@/utils/api', () => ({ placesApi: mocks }))

import PlaceManagement from '@/components/PlaceManagement.vue'
import type { Place, PlaceConfig } from '@/types'

const TYPES = ['Building', 'Floor', 'Room']

function place(id: string, name: string, over: Partial<Place> = {}): Place {
  return {
    id,
    name,
    parent_id: null,
    place_type: 'Building',
    description: null,
    external_id: null,
    is_special: false,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...over,
  }
}

// Records what it was handed so the two lists can be told apart, and re-emits
// on demand so the parent's handlers can be driven.
const PlaceNodeStub = {
  props: ['place', 'childrenMap', 'config'],
  emits: ['add-child', 'edit', 'delete'],
  template: '<div class="node" :data-id="place.id">{{ place.name }}</div>',
}
const PlacePickerStub = {
  props: ['modelValue', 'places', 'nullLabel'],
  emits: ['update:modelValue'],
  template:
    '<div class="picker" :data-options="places.map((p) => p.id).join(\',\')">{{ modelValue }}</div>',
}

const stubs = {
  'router-link': { props: ['to'], template: '<a><slot /></a>' },
  PlaceNode: PlaceNodeStub,
  PlacePicker: PlacePickerStub,
}

let confirmResult = true

beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset()
  mocks.config.mockResolvedValue({
    success: true,
    data: { enabled: true, types: TYPES } as PlaceConfig,
  })
  mocks.list.mockResolvedValue({ success: true, data: [] })
  mocks.create.mockResolvedValue({ success: true })
  mocks.update.mockResolvedValue({ success: true })
  mocks.remove.mockResolvedValue({ success: true })
  confirmResult = true
  vi.stubGlobal(
    'confirm',
    vi.fn(() => confirmResult)
  )
})

async function page(places: Place[] = [], cfg?: Partial<PlaceConfig>) {
  mocks.list.mockResolvedValue({ success: true, data: places })
  if (cfg) {
    mocks.config.mockResolvedValue({
      success: true,
      data: { enabled: true, types: TYPES, ...cfg },
    })
  }
  const w = mount(PlaceManagement, { global: { stubs } })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof page>>

// One cast, here rather than at every call site. `findComponent` on a plain
// options object yields an untyped `vm`; naming the single method being called
// keeps the type-aware lint honest about what is being assumed.
interface Emitter {
  $emit: (event: string, ...args: unknown[]) => void
}
function emitFrom(
  w: Wrapper,
  stub: typeof PlaceNodeStub | typeof PlacePickerStub,
  index: number,
  event: string,
  ...args: unknown[]
) {
  const c = w.findAllComponents(stub)[index]
  if (!c) throw new Error(`no ${event} target at index ${index}`)
  ;(c.vm as unknown as Emitter).$emit(event, ...args)
}

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}

const nodeIds = (w: Wrapper, within: string) =>
  w.findAll(`${within} .node`).map((n) => n.attributes('data-id'))

const typeOptions = (w: Wrapper) =>
  w.findAll('.modal-box select option').map((o) => o.attributes('value'))

describe('the two lists', () => {
  it('says there is nothing yet', async () => {
    expect((await page()).text()).toContain('No places yet')
  })

  it('renders only non-special roots in the hierarchy card', async () => {
    const w = await page([
      place('b1', 'Main Building'),
      place('f1', 'First Floor', { parent_id: 'b1', place_type: 'Floor' }),
      place('sp', 'Outside', { is_special: true, place_type: 'Outside' }),
    ])

    // Children are PlaceNode's job, so only the roots appear at this level.
    expect(nodeIds(w, '.bg-base-100')).toEqual(['b1'])
    expect(nodeIds(w, '.bg-base-200')).toEqual(['sp'])
  })

  it('sorts each list by name', async () => {
    const w = await page([
      place('b2', 'Zebra Shed'),
      place('b1', 'Annex'),
      place('s2', 'Parking', { is_special: true }),
      place('s1', 'Outside', { is_special: true }),
    ])
    expect(nodeIds(w, '.bg-base-100')).toEqual(['b1', 'b2'])
    expect(nodeIds(w, '.bg-base-200')).toEqual(['s1', 's2'])
  })

  it('omits the special card entirely when there are none', async () => {
    const w = await page([place('b1', 'Main Building')])
    expect(w.find('.bg-base-200').exists()).toBe(false)
  })

  it('labels the hierarchy card only when there is a special card to distinguish it from', async () => {
    const withSpecial = await page([place('b1', 'B'), place('s1', 'Outside', { is_special: true })])
    expect(withSpecial.text()).toContain('Locations')

    const without = await page([place('b1', 'B')])
    expect(without.text()).not.toContain('Locations')
  })

  it('names the configured level vocabulary in the blurb', async () => {
    expect((await page()).text()).toContain('Building › Floor › Room')
  })
})

describe('what the server config gates', () => {
  it('warns and disables creation when the module is switched off', async () => {
    const w = await page([], { enabled: false })
    expect(w.find('.alert-warning').text()).toContain('places module is disabled')
    expect(buttonNamed(w, '+ New root place').attributes('disabled')).toBeDefined()
    expect(buttonNamed(w, '+ Special place').attributes('disabled')).toBeDefined()
  })

  it('disables root creation, but not special creation, when no levels are configured', async () => {
    const w = await page([], { types: [] })
    expect(buttonNamed(w, '+ New root place').attributes('disabled')).toBeDefined()
    expect(buttonNamed(w, '+ Special place').attributes('disabled')).toBeUndefined()
  })

  // FIXED. Both create buttons are gated on `config?.enabled`, and `loadAll`
  // wrote `config` only on success -- so a refused config left it null: no
  // warning, the vocabulary rendered as "…", both buttons dead, and nothing to
  // say why. It looked exactly like the module being switched off, which is a
  // different thing entirely.
  it('says so when the config is refused, instead of looking switched off', async () => {
    mocks.config.mockResolvedValue({ success: false, error: 'Forbidden' })
    mocks.list.mockResolvedValue({ success: true, data: [place('b1', 'Main Building')] })
    const w = mount(PlaceManagement, { global: { stubs } })
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('Forbidden')
  })
})

describe('the type vocabulary the form offers', () => {
  it('offers every level for a root place', async () => {
    const w = await page([place('b1', 'Main Building')])
    await buttonNamed(w, '+ New root place').trigger('click')
    await nextTick()
    expect(typeOptions(w)).toEqual(TYPES)
  })

  it('offers only levels deeper than the parent when adding a child', async () => {
    const w = await page([place('b1', 'Main Building', { place_type: 'Building' })])
    emitFrom(w, PlaceNodeStub, 0, 'add-child', place('b1', 'Main Building'))
    await nextTick()

    expect(typeOptions(w)).toEqual(['Floor', 'Room'])
    expect(w.find('.modal-box').text()).toContain('inside Main Building')
  })

  it('falls back to every level when the parent has a type not in the vocabulary', async () => {
    const w = await page([place('x1', 'Odd', { place_type: 'Annexe' })])
    emitFrom(w, PlaceNodeStub, 0, 'add-child', place('x1', 'Odd', { place_type: 'Annexe' }))
    await nextTick()
    expect(typeOptions(w)).toEqual(TYPES)
  })

  it('offers no dropdown at all for a special place, just free text', async () => {
    const w = await page()
    await buttonNamed(w, '+ Special place').trigger('click')
    await nextTick()

    expect(w.find('.modal-box select').exists()).toBe(false)
    expect(w.find('.modal-box').text()).toContain('Free-form label')
  })

  it('clears the parent when a place is toggled to special', async () => {
    const w = await page([place('b1', 'Main Building')])
    emitFrom(w, PlaceNodeStub, 0, 'add-child', place('b1', 'Main Building'))
    await nextTick()
    expect(w.find('.modal-box').text()).toContain('inside Main Building')

    await w.find('.modal-box input[type="checkbox"]').setValue(true)
    await nextTick()
    expect(w.find('.modal-box').text()).not.toContain('inside Main Building')

    // And the parent really is dropped from the payload, not just from the
    // caption. A special place with a parent is exactly what the toggle exists
    // to prevent.
    await w.find('.modal-box input[type="text"]').setValue('Outside')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()
    expect(mocks.create.mock.calls[0][0]).toMatchObject({ parent_id: null, is_special: true })
  })

  // FINDING, pinned. `allowedTypes` reads `formParent`, which is set once when
  // the modal opens. The "Move to (parent)" picker writes `form.parent_id`.
  // Nothing keeps them in step, so after moving a place the Type dropdown is
  // still showing the levels that were legal under its *previous* parent --
  // and the level rule the whole vocabulary exists to enforce is not applied
  // to the one operation that can break it.
  it('does not re-derive the legal levels when the parent is changed', async () => {
    const w = await page([
      place('b1', 'Main Building', { place_type: 'Building' }),
      place('f1', 'First Floor', { parent_id: 'b1', place_type: 'Floor' }),
      place('r1', 'Room 5', { parent_id: 'f1', place_type: 'Room' }),
    ])
    // Edit the floor: its parent is the Building, so Floor and Room are legal.
    emitFrom(
      w,
      PlaceNodeStub,
      0,
      'edit',
      place('f1', 'First Floor', { parent_id: 'b1', place_type: 'Floor' })
    )
    await nextTick()
    expect(typeOptions(w)).toEqual(['Floor', 'Room'])

    // Move it to the root. Every level is now legal, including Building.
    emitFrom(w, PlacePickerStub, 0, 'update:modelValue', null)
    await nextTick()

    expect(
      typeOptions(w),
      'the type list now follows the parent picker -- if that was fixed, this ' +
        'should be the full vocabulary'
    ).toEqual(['Floor', 'Room'])
  })
})

describe('where a place may be moved', () => {
  it('offers every other place, and never the place itself', async () => {
    const w = await page([
      place('b1', 'Main Building'),
      place('f1', 'First Floor', { parent_id: 'b1', place_type: 'Floor' }),
      place('b2', 'Annex'),
    ])
    emitFrom(w, PlaceNodeStub, 0, 'edit', place('b2', 'Annex'))
    await nextTick()

    const offered = w.findComponent(PlacePickerStub).props('places') as Place[]
    expect(offered.map((p) => p.id).sort()).toEqual(['b1', 'f1'])
  })

  it('excludes the whole subtree, not just the direct children', async () => {
    const w = await page([
      place('b1', 'Main Building'),
      place('f1', 'First Floor', { parent_id: 'b1', place_type: 'Floor' }),
      place('r1', 'Room 5', { parent_id: 'f1', place_type: 'Room' }),
      place('b2', 'Annex'),
    ])
    emitFrom(w, PlaceNodeStub, 0, 'edit', place('b1', 'Main Building'))
    await nextTick()

    // Moving a building under its own grandchild would make a cycle, and the
    // list is the only thing preventing it.
    const offered = w.findComponent(PlacePickerStub).props('places') as Place[]
    expect(offered.map((p) => p.id)).toEqual(['b2'])
  })

  it('offers no parent picker for a special place', async () => {
    const w = await page([place('s1', 'Outside', { is_special: true })])
    emitFrom(w, PlaceNodeStub, 0, 'edit', place('s1', 'Outside', { is_special: true }))
    await nextTick()
    expect(w.findComponent(PlacePickerStub).exists()).toBe(false)
  })
})

describe('saving', () => {
  it('needs a name and a type', async () => {
    const w = await page()
    await buttonNamed(w, '+ New root place').trigger('click')
    await nextTick()
    expect(buttonNamed(w, 'Create').attributes('disabled')).toBeDefined()

    await w.find('.modal-box input[type="text"]').setValue('  ')
    expect(buttonNamed(w, 'Create').attributes('disabled')).toBeDefined()

    await w.find('.modal-box input[type="text"]').setValue('Main Building')
    expect(buttonNamed(w, 'Create').attributes('disabled')).toBeUndefined()
  })

  it('creates a root with the trimmed name and the first legal type', async () => {
    const w = await page()
    await buttonNamed(w, '+ New root place').trigger('click')
    await nextTick()
    await w.find('.modal-box input[type="text"]').setValue('  Main Building  ')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(mocks.create.mock.calls[0][0]).toEqual({
      name: 'Main Building',
      place_type: 'Building',
      parent_id: null,
      description: null,
      external_id: null,
      is_special: false,
    })
  })

  it('creates a child under the place it was invoked from', async () => {
    const w = await page([place('b1', 'Main Building')])
    emitFrom(w, PlaceNodeStub, 0, 'add-child', place('b1', 'Main Building'))
    await nextTick()
    await w.find('.modal-box input[type="text"]').setValue('First Floor')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    const body = mocks.create.mock.calls[0][0] as Record<string, unknown>
    expect(body.parent_id).toBe('b1')
    expect(body.place_type).toBe('Floor')
  })

  it('updates rather than creates when a row was opened for editing', async () => {
    const w = await page([place('b1', 'Main Building')])
    emitFrom(w, PlaceNodeStub, 0, 'edit', place('b1', 'Main Building'))
    await nextTick()
    await buttonNamed(w, 'Save').trigger('click')
    await flushPromises()

    expect(mocks.update.mock.calls[0][0]).toBe('b1')
    expect(mocks.create).not.toHaveBeenCalled()
  })

  it('reloads both the config and the list after a save', async () => {
    const w = await page()
    await buttonNamed(w, '+ New root place').trigger('click')
    await nextTick()
    await w.find('.modal-box input[type="text"]').setValue('Main Building')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(w.find('.alert-success').text()).toContain('Place created')
    expect(mocks.list).toHaveBeenCalledTimes(2)
    expect(mocks.config).toHaveBeenCalledTimes(2)
    expect(w.find('.modal-open').exists()).toBe(false)
  })

  it("reports the server's reason and keeps the form open", async () => {
    mocks.create.mockResolvedValue({ success: false, error: 'A Room cannot hold a Building' })
    const w = await page()
    await buttonNamed(w, '+ New root place').trigger('click')
    await nextTick()
    await w.find('.modal-box input[type="text"]').setValue('Main Building')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('A Room cannot hold a Building')
    expect(w.find('.modal-open').exists()).toBe(true)
  })
})

describe('deleting', () => {
  it('says what will happen to the attached hardware', async () => {
    confirmResult = false
    const w = await page([place('b1', 'Main Building')])
    emitFrom(w, PlaceNodeStub, 0, 'delete', place('b1', 'Main Building'))
    await flushPromises()

    expect(vi.mocked(globalThis.confirm)).toHaveBeenCalledWith(
      'Delete "Main Building"? Attached doors/tools/devices will be detached but kept.'
    )
    expect(mocks.remove).not.toHaveBeenCalled()
  })

  it('suggests the likely cause when a delete is refused with no reason', async () => {
    mocks.remove.mockResolvedValue({ success: false })
    const w = await page([place('b1', 'Main Building')])
    emitFrom(w, PlaceNodeStub, 0, 'delete', place('b1', 'Main Building'))
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('move/delete children first')
  })

  it('removes and reloads', async () => {
    const w = await page([place('b1', 'Main Building')])
    emitFrom(w, PlaceNodeStub, 0, 'delete', place('b1', 'Main Building'))
    await flushPromises()

    expect(mocks.remove).toHaveBeenCalledWith('b1')
    expect(mocks.list).toHaveBeenCalledTimes(2)
  })
})

describe('what a network error does', () => {
  // FIXED. The loader had no try/catch and cleared `loading` only after the
  // await, so a rejection spun forever and escaped to an
  // `app.config.errorHandler` that `src/main.ts` never sets.
  it('reports a rejected load and stops spinning', async () => {
    mocks.list.mockRejectedValue(new Error('Network Error'))
    const w = mount(PlaceManagement, { global: { stubs } })
    await flushPromises()

    expect(w.find('.loading-spinner').exists()).toBe(false)
    expect(w.find('.alert-error').text()).toContain('Network Error')
  })

  // FIXED. The save set its busy flag with no `finally`, so a rejection left
  // the button disabled with no way to retry.
  it('frees the button and reports the failure when the save rejects', async () => {
    mocks.create.mockRejectedValue(new Error('Network Error'))
    const w = mount(PlaceManagement, { global: { stubs } })
    await flushPromises()
    await buttonNamed(w, '+ New root place').trigger('click')
    await nextTick()
    await w.find('.modal-box input[type="text"]').setValue('Main Building')
    await w.find('.modal-action .btn-primary').trigger('click')
    await flushPromises()

    expect(w.find('.modal-action .btn-primary').attributes('disabled')).toBeUndefined()
    expect(w.find('.alert-error').text()).toContain('Network Error')
  })
})
