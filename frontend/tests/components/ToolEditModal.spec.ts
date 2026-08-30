// Tier 2: ToolEditModal.
//
// The sibling of ToolCreateModal, and it repeats that modal's central defect:
//
//     await toolsApi.updateTool(props.tool.id, toolData)
//     emit('updated')
//
// `api.ts` makes `updateTool` catch its own rejection and return
// `{ success: false, error }`, so the await always resolves, the flag is never
// read, and a refused update is announced as a success.
//
// It differs interestingly in one place. Its category list comes from
// `GET /api/config/tools`, which returns all twelve categories from the
// server's own config -- while ToolCreateModal hardcodes six. So a tool can be
// edited into a category it could not have been created in, and the endpoint
// that would fix the create modal is one the create modal does not call.
//
// What this spec does NOT prove: that the server accepts every category it
// advertises. Tier 6 owns the round trip.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({
  updateTool: vi.fn(),
  listSchedules: vi.fn(),
  axiosGet: vi.fn(),
}))
vi.mock('@/utils/api', () => ({
  toolsApi: { updateTool: mocks.updateTool },
  schedulesApi: { list: mocks.listSchedules },
}))
vi.mock('axios', () => ({ default: { get: mocks.axiosGet } }))

import ToolEditModal from '@/components/ToolEditModal.vue'
import { ToolCategory, ToolStatus, type Tool } from '@/types/tools'
import type { Schedule } from '@/types'

// The full set the server's `ToolConfig::default()` ships, in its order.
const SERVER_CATEGORIES = [
  { value: 'saw', label: 'Saw' },
  { value: 'powertool', label: 'Power Tools' },
  { value: 'hand_tools', label: 'Hand Tools' },
  { value: 'measuring', label: 'Measuring' },
  { value: 'safety', label: 'Safety' },
  { value: 'electronics', label: 'Electronics' },
  { value: 'woodworking', label: 'Woodworking' },
  { value: 'metalworking', label: 'Metalworking' },
  { value: '3d_printing', label: '3D Printing' },
  { value: 'laser_cutting', label: 'Laser Cutting' },
  { value: 'welding', label: 'Welding' },
  { value: 'other', label: 'Other' },
]

const SCHEDULE: Schedule = {
  id: 'sch-1',
  name: 'Member Hours',
  description: null,
  intervals: [],
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  is_public: false,
}

function tool(over: Partial<Tool> = {}): Tool {
  return {
    id: 'tool-1',
    name: 'Bandsaw',
    category: ToolCategory.Saw,
    status: ToolStatus.Idle,
    requires_training: true,
    created_by: 'admin-1',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    manufacturer: 'Startrite',
    model: '351',
    serial_number: 'SN-9',
    barcode: 'BC-9',
    location: 'Wood shop',
    description: 'Big one, by the door.',
    ...over,
  }
}

const SchedulePickerStub = {
  props: ['modelValue', 'schedules'],
  emits: ['update:modelValue'],
  template: '<div class="sched" :data-count="schedules.length">{{ modelValue }}</div>',
}

beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset()
  mocks.updateTool.mockResolvedValue({ success: true, data: { id: 'tool-1' } })
  mocks.listSchedules.mockResolvedValue({ success: true, data: [SCHEDULE] })
  mocks.axiosGet.mockResolvedValue({ data: { data: { tool_categories: SERVER_CATEGORIES } } })
  vi.spyOn(console, 'error').mockImplementation(() => {})
})

async function modal(t: Tool = tool()) {
  const w = mount(ToolEditModal, {
    props: { tool: t },
    global: { stubs: { SchedulePicker: SchedulePickerStub } },
  })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof modal>>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}

const sent = () => mocks.updateTool.mock.calls[0][1] as Record<string, unknown>
const categoryValues = (w: Wrapper) =>
  w
    .findAll('#category option')
    .map((o) => o.attributes('value'))
    .filter((v) => v !== '')

describe('where the category list comes from', () => {
  it('reads it from the server config endpoint', async () => {
    await modal()
    expect(mocks.axiosGet).toHaveBeenCalledWith('/api/config/tools')
  })

  it('offers every category the server advertises', async () => {
    const w = await modal()
    expect(categoryValues(w)).toEqual(SERVER_CATEGORIES.map((c) => c.value))
    expect(categoryValues(w)).toHaveLength(Object.values(ToolCategory).length)
  })

  // Recorded, and it makes the ToolCreateModal finding sharper rather than
  // being a defect of this component: the endpoint that serves all twelve
  // categories is one the create modal never calls, so a tool can be edited
  // into a category it could not have been created in.
  it('offers categories the create modal has no option for', async () => {
    const w = await modal()
    for (const c of [
      'electronics',
      'woodworking',
      'metalworking',
      '3d_printing',
      'laser_cutting',
      'welding',
    ]) {
      expect(categoryValues(w)).toContain(c)
    }
  })

  it('uses the config endpoint rather than the shared client', async () => {
    // Recorded: `axios.get` directly, so no Authorization header, no base URL
    // and none of `utils/api`'s interceptors. Second component in the tier-2
    // inventory to bypass the shared client, after PageViewer.
    await modal()
    expect(mocks.axiosGet).toHaveBeenCalledTimes(1)
    expect(mocks.axiosGet.mock.calls[0]).toHaveLength(1)
  })

  // FINDING, pinned. The catch falls back to a hardcoded list of five, and the
  // five leave out `saw` -- so a bandsaw being edited during a config outage
  // shows an empty category box. The value survives in the model until the box
  // is touched, so the tool is not silently recategorised; it just cannot be
  // read, and any edit to the category has to start from blank.
  it('falls back to five of the twelve, omitting the category of the tool on screen', async () => {
    mocks.axiosGet.mockRejectedValue(new Error('Network Error'))
    const w = await modal(tool({ category: ToolCategory.Saw }))

    expect(
      categoryValues(w),
      'the fallback list changed -- if it was completed, this test should ' +
        'assert it matches the server default instead'
    ).toEqual(['powertool', 'hand_tools', 'measuring', 'safety', 'other'])
    expect(categoryValues(w)).not.toContain(ToolCategory.Saw)
    expect((w.find('#category').element as HTMLSelectElement).value).toBe('')
  })

  it('still sends the original category when the fallback cannot show it', async () => {
    mocks.axiosGet.mockRejectedValue(new Error('Network Error'))
    const w = await modal(tool({ category: ToolCategory.Saw }))
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(sent().category).toBe(ToolCategory.Saw)
  })
})

describe('what the form starts with', () => {
  it('loads every field off the tool', async () => {
    const w = await modal()
    expect((w.find('#name').element as HTMLInputElement).value).toBe('Bandsaw')
    expect((w.find('#category').element as HTMLSelectElement).value).toBe('saw')
    expect((w.find('#manufacturer').element as HTMLInputElement).value).toBe('Startrite')
    expect((w.find('#serial_number').element as HTMLInputElement).value).toBe('SN-9')
    expect((w.find('#location').element as HTMLInputElement).value).toBe('Wood shop')
  })

  it('blanks the fields the tool does not carry rather than showing undefined', async () => {
    const w = await modal(
      tool({ manufacturer: undefined, model: undefined, notes: undefined, barcode: undefined })
    )
    expect((w.find('#manufacturer').element as HTMLInputElement).value).toBe('')
    expect((w.find('#notes').element as HTMLTextAreaElement).value).toBe('')
  })

  // FINDING, pinned. `onMounted` awaits the category request and the schedule
  // request before it calls `loadToolData`, and there is no loading state. A
  // slow or hanging config endpoint therefore leaves the whole form blank --
  // not "loading", blank -- with a Save button that is enabled and would
  // submit nulls over the tool's real values.
  it('leaves every field blank, and Save enabled, while the config request hangs', async () => {
    mocks.axiosGet.mockReturnValue(new Promise(() => {}))
    const w = mount(ToolEditModal, {
      props: { tool: tool() },
      global: { stubs: { SchedulePicker: SchedulePickerStub } },
    })
    await nextTick()

    expect((w.find('#name').element as HTMLInputElement).value).toBe('')
    expect(
      w.find('button[type="submit"]').attributes('disabled'),
      'the form now guards submission while it is still loading -- if a ' +
        'loading state was added, this test should assert it'
    ).toBeUndefined()
  })

  it('passes the loaded schedules to the picker', async () => {
    const w = await modal()
    expect(w.find('.sched').attributes('data-count')).toBe('1')
  })

  it('carries on with an empty picker when the schedule list fails', async () => {
    mocks.listSchedules.mockRejectedValue(new Error('Network Error'))
    const w = await modal()
    expect(w.find('.sched').attributes('data-count')).toBe('0')
  })
})

describe('what the form sends', () => {
  it('addresses the tool it was opened on and carries the edits', async () => {
    const w = await modal()
    await w.find('#name').setValue('Bandsaw (large)')
    await w.find('#category').setValue(ToolCategory.Woodworking)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(mocks.updateTool.mock.calls[0][0]).toBe('tool-1')
    expect(sent()).toMatchObject({
      name: 'Bandsaw (large)',
      category: ToolCategory.Woodworking,
    })
  })

  it('turns cleared fields into null rather than an empty string', async () => {
    const w = await modal()
    await w.find('#manufacturer').setValue('')
    await w.find('#notes').setValue('')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(sent().manufacturer).toBeNull()
    expect(sent().notes).toBeNull()
  })
})

describe('what happens after the request', () => {
  it('announces the update when the server agrees', async () => {
    const w = await modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.emitted('updated')).toHaveLength(1)
    expect(w.find('.error').exists()).toBe(false)
  })

  // FINDING, pinned, and the same one ToolCreateModal has. The success flag is
  // never examined and `updateTool` resolves rather than rejects on failure, so
  // a refusal follows exactly the same path as a success: `updated` is
  // emitted, the parent refreshes, and the edit that did not happen looks like
  // one that did.
  it("reports a refusal in the server's own words, and announces nothing", async () => {
    mocks.updateTool.mockResolvedValue({ success: false, error: 'Barcode already in use' })
    const w = await modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error').text()).toBe('Barcode already in use')
    expect(w.emitted('updated')).toBeUndefined()
  })

  it('falls back to a generic message when a refusal carries none', async () => {
    mocks.updateTool.mockResolvedValue({ success: false })
    const w = await modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error').text()).toBe('Failed to update tool')
    expect(w.emitted('updated')).toBeUndefined()
  })

  it("reads the server's body if the call ever does reject", async () => {
    // Defence rather than a production path -- `updateTool` catches its own
    // rejection -- but it reads `error` now, the key the envelope fills, where
    // it used to read `message` and get the generic fallback.
    mocks.updateTool.mockRejectedValue({
      response: { data: { error: 'Barcode already in use' } },
    })
    const w = await modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error').text()).toBe('Barcode already in use')
    expect(w.emitted('updated')).toBeUndefined()
  })

  it('re-enables the submit button whether the request resolved or rejected', async () => {
    mocks.updateTool.mockRejectedValue(new Error('down'))
    const w = await modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('button[type="submit"]').attributes('disabled')).toBeUndefined()
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
