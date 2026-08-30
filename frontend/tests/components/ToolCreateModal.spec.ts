// Tier 2: ToolCreateModal.
//
// FIXED, and the tests below assert the fix rather than the defect.
//
// This modal used to do
//
//     await toolsApi.createTool(toolData)
//     emit('created')
//
// and `createTool` catches its own rejection and resolves with
// `{ success: false, error }` -- so the await always succeeded, the flag was
// never read, and every refusal was announced as a success: `created` emitted,
// the parent refreshing a list that had not changed, nothing on screen. It now
// reads the flag and reports the server's words.
//
// What this spec does NOT prove: which categories the *server* accepts. The
// category list is compared against the TypeScript enum, which is a claim
// about the two halves of the client agreeing. Tier 1b's enum parity owns the
// third copy.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({ createTool: vi.fn(), listSchedules: vi.fn() }))
vi.mock('@/utils/api', () => ({
  toolsApi: { createTool: mocks.createTool },
  schedulesApi: { list: mocks.listSchedules },
}))

import ToolCreateModal from '@/components/ToolCreateModal.vue'
import { ToolCategory, ToolStatus } from '@/types/tools'
import type { Schedule } from '@/types'

const SCHEDULE: Schedule = {
  id: 'sch-1',
  name: 'Member Hours',
  description: null,
  intervals: [],
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  is_public: false,
}

// A transparent stub: SchedulePicker has its own spec, and what matters here is
// what this modal hands it and what it does with what comes back.
const SchedulePickerStub = {
  props: ['modelValue', 'schedules'],
  emits: ['update:modelValue'],
  template: '<div class="sched" :data-count="schedules.length">{{ modelValue }}</div>',
}

beforeEach(() => {
  mocks.createTool.mockReset()
  mocks.listSchedules.mockReset()
  mocks.createTool.mockResolvedValue({ success: true, data: { id: 't1' } })
  mocks.listSchedules.mockResolvedValue({ success: true, data: [SCHEDULE] })
})

async function modal() {
  const w = mount(ToolCreateModal, { global: { stubs: { SchedulePicker: SchedulePickerStub } } })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof modal>>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}

const sent = () => mocks.createTool.mock.calls[0][0] as Record<string, unknown>

async function fillMinimum(w: Wrapper) {
  await w.find('#name').setValue('Bandsaw')
  await w.find('#category').setValue(ToolCategory.Saw)
}

describe('the category list', () => {
  // FINDING, pinned. `ToolCategory` names twelve categories. This select offers
  // six. The six it leaves out are electronics, woodworking, metalworking,
  // 3d_printing, laser_cutting and welding -- which, in a makerspace, is the
  // laser cutter, the 3D printers and the welder. A tool that is one of those
  // has to be filed as "Other".
  it('offers half the categories the enum defines', async () => {
    const w = await modal()
    const offered = w
      .findAll('#category option')
      .map((o) => o.attributes('value'))
      .filter((v) => v !== '')

    const missing = Object.values(ToolCategory).filter((c) => !offered.includes(c))
    expect(
      missing.sort(),
      'the category list changed -- if it was completed, this test should ' +
        'assert that every ToolCategory is offered'
    ).toEqual(
      [
        ToolCategory.Electronics,
        ToolCategory.Woodworking,
        ToolCategory.Metalworking,
        ToolCategory.ThreeDPrinting,
        ToolCategory.LaserCutting,
        ToolCategory.Welding,
      ].sort()
    )
  })

  it('offers nothing the enum does not define', async () => {
    // The other direction, and this one is a hard rule rather than a ratchet:
    // an option the enum does not know is a value the server will reject.
    const w = await modal()
    const known = new Set<string>(Object.values(ToolCategory))
    const offered = w
      .findAll('#category option')
      .map((o) => o.attributes('value') ?? '')
      .filter((v) => v !== '')

    expect(offered.filter((c) => !known.has(c))).toEqual([])
  })

  it('offers the four statuses a new tool may start in', async () => {
    const w = await modal()
    const offered = w.findAll('#status option').map((o) => o.attributes('value'))
    expect(offered).toEqual([
      ToolStatus.Idle,
      ToolStatus.Maintenance,
      ToolStatus.Broken,
      ToolStatus.Repair,
    ])
    // `in_use` and `retired` are deliberately absent: a tool cannot be created
    // already in use, and creating one already retired is not a thing anyone
    // wants. Asserted so that stays a decision rather than an omission.
    expect(offered).not.toContain(ToolStatus.InUse)
    expect(offered).not.toContain(ToolStatus.Retired)
  })
})

describe('what the form sends', () => {
  it('carries every field the user filled in', async () => {
    const w = await modal()
    await fillMinimum(w)
    await w.find('#manufacturer').setValue('Startrite')
    await w.find('#model').setValue('351')
    await w.find('#serial_number').setValue('SN-9')
    await w.find('#barcode').setValue('BC-9')
    await w.find('#location').setValue('Wood shop')
    await w.find('#purchase_price').setValue('1250')
    await w.find('.checkbox').setValue(true)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(sent()).toMatchObject({
      name: 'Bandsaw',
      category: ToolCategory.Saw,
      manufacturer: 'Startrite',
      model: '351',
      serial_number: 'SN-9',
      barcode: 'BC-9',
      location: 'Wood shop',
      purchase_price: 1250,
      requires_training: true,
      status: ToolStatus.Idle,
    })
  })

  it('turns every blank optional field into null rather than an empty string', async () => {
    const w = await modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    for (const key of [
      'description',
      'manufacturer',
      'model',
      'serial_number',
      'barcode',
      'location',
      'purchase_date',
      'notes',
    ]) {
      expect(sent()[key], `${key} should be null, not ""`).toBeNull()
    }
  })

  it('passes the loaded schedules to the picker and sends the chosen one', async () => {
    const w = await modal()
    expect(w.find('.sched').attributes('data-count')).toBe('1')

    const picker = w.findComponent(SchedulePickerStub)
    ;(picker.vm as unknown as { $emit: (e: string, v: unknown) => void }).$emit(
      'update:modelValue',
      'sch-1'
    )
    await nextTick()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(sent().schedule_id).toBe('sch-1')
  })

  it('sends no schedule when none was chosen', async () => {
    const w = await modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(sent().schedule_id).toBeNull()
  })

  it('carries on with an empty picker when the schedule list fails', async () => {
    // `loadSchedules` has a catch that resets to `[]`, so a failed schedule
    // load does not stop a tool being created. Asserted as behaviour worth
    // keeping: this is the one request in the modal whose failure is handled.
    mocks.listSchedules.mockRejectedValue(new Error('Network Error'))
    const w = await modal()

    expect(w.find('.sched').attributes('data-count')).toBe('0')
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(mocks.createTool).toHaveBeenCalledTimes(1)
  })
})

describe('what happens after the request', () => {
  it('announces the new tool when the server agrees', async () => {
    const w = await modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.emitted('created')).toHaveLength(1)
    expect(w.find('.error').exists()).toBe(false)
  })

  // FINDING, pinned, and the reason this modal is worth reading twice. The
  // success flag is never examined, and `createTool` resolves rather than
  // rejects on failure, so a refusal follows exactly the same path as a
  // success: `created` is emitted, the parent refreshes, and nothing is shown.
  it("reports a refusal in the server's own words, and announces nothing", async () => {
    mocks.createTool.mockResolvedValue({ success: false, error: 'Barcode already in use' })
    const w = await modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error').text()).toBe('Barcode already in use')
    expect(w.emitted('created')).toBeUndefined()
  })

  it('falls back to a generic message when a refusal carries none', async () => {
    mocks.createTool.mockResolvedValue({ success: false })
    const w = await modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error').text()).toBe('Failed to create tool')
    expect(w.emitted('created')).toBeUndefined()
  })

  // The catch is defence rather than a path production takes, because
  // rejects. Asserted as a pair so the dead branch is documented as dead --
  // and note it reads `err.response?.data?.message`, where the envelope fills
  // `error`, so even reached it would discard the server's words.
  it("reads the server's body if the call ever does reject", async () => {
    // `createTool` catches its own rejection, so this branch is defence rather
    // than a path production takes. It reads `error` now -- the key the
    // envelope actually fills -- where it used to read `message` and get
    // nothing but the generic fallback.
    mocks.createTool.mockRejectedValue({
      response: { data: { error: 'Barcode already in use' } },
    })
    const w = await modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error').text()).toBe('Barcode already in use')
    expect(w.emitted('created')).toBeUndefined()
  })

  it('re-enables the submit button whether the request resolved or rejected', async () => {
    mocks.createTool.mockRejectedValue(new Error('down'))
    const w = await modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('button[type="submit"]').attributes('disabled')).toBeUndefined()
  })

  it('disables the submit button while the request is in flight', async () => {
    mocks.createTool.mockReturnValue(new Promise(() => {}))
    const w = await modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('button[type="submit"]').attributes('disabled')).toBeDefined()
  })
})

describe('closing', () => {
  it('closes on the overlay, the header button and Cancel', async () => {
    const w = await modal()
    await w.find('.modal-overlay').trigger('click')
    await w.find('.close-btn').trigger('click')
    await buttonNamed(w, 'Cancel').trigger('click')
    expect(w.emitted('close')).toHaveLength(3)
  })

  it('does not close when the content is clicked', async () => {
    const w = await modal()
    await w.find('.modal-content').trigger('click')
    expect(w.emitted('close')).toBeUndefined()
  })
})
