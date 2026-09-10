// Tier 2: PowerManagement (#42).
//
// The facility power editor: circuits, outlets, receptacles, and plugging tools
// into receptacles. PlacePicker is stubbed (its own spec covers it). What is
// under test here is that the lists render what the API returns, the create
// form posts, delete removes, and the assignment dropdown calls through.
//
// What this spec does NOT prove: the server's cycle guard, receptacle
// uniqueness, or the FK cascade. Those are the e2e `circuits` stage's, which
// asserts them against a real database.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const power = vi.hoisted(() => ({
  config: vi.fn(),
  listCircuits: vi.fn(),
  getCircuit: vi.fn(),
  createCircuit: vi.fn(),
  updateCircuit: vi.fn(),
  removeCircuit: vi.fn(),
  listOutlets: vi.fn(),
  getOutlet: vi.fn(),
  createOutlet: vi.fn(),
  updateOutlet: vi.fn(),
  removeOutlet: vi.fn(),
  listReceptacles: vi.fn(),
  createReceptacle: vi.fn(),
  updateReceptacle: vi.fn(),
  removeReceptacle: vi.fn(),
  assignToolReceptacle: vi.fn(),
}))
const places = vi.hoisted(() => ({ list: vi.fn() }))
const tools = vi.hoisted(() => ({ getTools: vi.fn() }))

vi.mock('@/utils/api', () => ({
  powerApi: power,
  placesApi: places,
  toolsApi: tools,
}))

import PowerManagement from '@/components/PowerManagement.vue'

const PlacePickerStub = {
  props: ['modelValue', 'places', 'hideNull'],
  emits: ['update:modelValue'],
  template: '<select class="place-picker" @change="$emit(\'update:modelValue\', \'p1\')" />',
}

const ok = <T>(data: T) => Promise.resolve({ success: true, data })

function circuit(id: string, label: string, parent: string | null = null) {
  return {
    id,
    breaker_label: label,
    voltage_rating: 120,
    amperage_limit: '20',
    parent_circuit_id: parent,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  }
}

function mountPower() {
  return mount(PowerManagement, {
    props: { embedded: true },
    global: { stubs: { PlacePicker: PlacePickerStub } },
  })
}

beforeEach(() => {
  vi.clearAllMocks()
  power.listCircuits.mockReturnValue(ok([circuit('c1', 'A-12')]))
  power.listOutlets.mockReturnValue(ok([]))
  power.listReceptacles.mockReturnValue(ok([]))
  places.list.mockReturnValue(ok([]))
  tools.getTools.mockReturnValue(ok([{ id: 't1', name: 'Laser', receptacle_id: null }]))
  power.createCircuit.mockReturnValue(ok(circuit('c2', 'B-4')))
  power.removeCircuit.mockReturnValue(ok(undefined))
  power.assignToolReceptacle.mockReturnValue(ok(undefined))
})

describe('PowerManagement', () => {
  it('lists circuits the API returns', async () => {
    const w = mountPower()
    await flushPromises()
    expect(power.listCircuits).toHaveBeenCalled()
    expect(w.text()).toContain('A-12')
  })

  it('creates a circuit from the form', async () => {
    const w = mountPower()
    await flushPromises()
    await w.find('input').setValue('B-4') // breaker_label is the first input
    const inputs = w.findAll('input')
    await inputs[1].setValue('240') // voltage
    await inputs[2].setValue('30') // amperage
    await w.find('form').trigger('submit.prevent')
    await flushPromises()
    expect(power.createCircuit).toHaveBeenCalledWith(
      expect.objectContaining({ breaker_label: 'B-4', amperage_limit: '30' })
    )
  })

  it('deletes a circuit', async () => {
    const w = mountPower()
    await flushPromises()
    const del = w.findAll('button').find((b) => b.text() === 'Delete')
    expect(del, 'a Delete button for the circuit row').toBeTruthy()
    await del.trigger('click')
    await flushPromises()
    expect(power.removeCircuit).toHaveBeenCalledWith('c1')
  })

  it('assigns a tool to a receptacle from the Assignments tab', async () => {
    power.listReceptacles.mockReturnValue(
      ok([{ id: 'r1', outlet_id: 'o1', label: 'Left', created_at: '', updated_at: '' }])
    )
    const w = mountPower()
    await flushPromises()
    const assignTab = w.findAll('[role="tab"]').find((t) => t.text() === 'Assignments')
    await assignTab.trigger('click')
    await flushPromises()
    const select = w.find('tbody select')
    expect(select.exists()).toBe(true)
    await select.setValue('r1')
    await flushPromises()
    expect(power.assignToolReceptacle).toHaveBeenCalledWith('t1', 'r1')
  })
})
