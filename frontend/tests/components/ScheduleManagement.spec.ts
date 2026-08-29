// Tier 2: ScheduleManagement.
//
// A table plus a weekly-window editor. The editor is the interesting part,
// because it renders one list per day *sorted by start time* --
//
//     for (const list of Object.values(out)) list.sort((a, b) => a.start.localeCompare(b.start))
//
// -- while `removeInterval` and `updateInterval` map the clicked row back to
// the underlying array by counting matches in *array* order:
//
//     for (let i = 0; i < form.value.intervals.length; i++) {
//       if (form.value.intervals[i].day !== day) continue
//       if (seen === idxWithinDay) { ... }
//
// The two orders agree only while the array happens to be sorted. Editing a
// window so that it sorts earlier makes them disagree, and from then on the ×
// button removes a different window than the one it sits next to. That is the
// headline finding, pinned.
//
// What this spec does NOT prove: that the server accepts or rejects any
// particular interval set, or that overlapping windows mean anything to the
// door evaluator. Tier 1b's door vectors own that.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({
  list: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  remove: vi.fn(),
}))
vi.mock('@/utils/api', () => ({ schedulesApi: mocks }))

import ScheduleManagement from '@/components/ScheduleManagement.vue'
import { SCHEDULE_TEMPLATES } from '@/components/schedule_templates'
import type { DayOfWeek, Schedule, ScheduleInterval } from '@/types'

const iv = (day: DayOfWeek, start: string, end: string): ScheduleInterval => ({ day, start, end })

function schedule(over: Partial<Schedule> = {}): Schedule {
  return {
    id: 's1',
    name: 'Member Hours',
    description: null,
    intervals: [iv('mon', '09:00', '17:00')],
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    is_public: false,
    ...over,
  }
}

const stubs = { 'router-link': { props: ['to'], template: '<a><slot /></a>' } }

let confirmResult = true

beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset()
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

async function page(schedules: Schedule[] = []) {
  mocks.list.mockResolvedValue({ success: true, data: schedules })
  const w = mount(ScheduleManagement, { global: { stubs } })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof page>>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}

const body = () => mocks.create.mock.calls[0][0] as { intervals: ScheduleInterval[]; name: string }
const patched = () =>
  mocks.update.mock.calls[0][1] as { intervals: ScheduleInterval[]; name: string }

// The editor renders seven day rows; this picks the interval rows within one.
function dayRow(w: Wrapper, day: DayOfWeek) {
  const rows = w.findAll('.card-body > .flex.items-start')
  const found = rows.find((r) => r.text().toLowerCase().startsWith(day))
  if (!found) throw new Error(`no editor row for ${day}`)
  return found
}
const windowsFor = (w: Wrapper, day: DayOfWeek) =>
  dayRow(w, day).findAll('.flex.items-center.gap-2')

const timeValue = (row: ReturnType<typeof windowsFor>[number], which: 0 | 1) =>
  (row.findAll('input[type="time"]')[which].element as HTMLInputElement).value

// The time inputs are one-way bound (`:value` + `@change`), so the DOM value is
// re-asserted from the model on any re-render. `setValue` flushes a tick before
// the `change` fires, which is long enough for Vue to put the old value back --
// and the handler then reads that old value. Setting the property and firing
// `change` in one step is what a user typing actually produces.
async function setTime(row: ReturnType<typeof windowsFor>[number], which: 0 | 1, value: string) {
  const input = row.findAll('input[type="time"]')[which]
  ;(input.element as HTMLInputElement).value = value
  await input.trigger('change')
}

async function openNew(w: Wrapper) {
  await buttonNamed(w, '+ New schedule').trigger('click')
  await nextTick()
}

describe('the table', () => {
  it('says there is nothing yet', async () => {
    expect((await page()).text()).toContain('No schedules yet')
  })

  it('names each schedule and shows its description', async () => {
    const w = await page([schedule({ description: 'When members may enter' })])
    expect(w.find('tbody tr').text()).toContain('Member Hours')
    expect(w.find('tbody tr').text()).toContain('When members may enter')
  })

  it('collapses identical windows across consecutive days into one range', async () => {
    const w = await page([
      schedule({
        intervals: (['mon', 'tue', 'wed', 'thu', 'fri'] as DayOfWeek[]).map((d) =>
          iv(d, '09:00', '17:00')
        ),
      }),
    ])
    expect(w.find('tbody tr').text()).toContain('Mon–Fri 09:00–17:00')
  })

  it('breaks a non-consecutive run into separate ranges', async () => {
    const w = await page([
      schedule({
        intervals: (['mon', 'tue', 'wed', 'fri'] as DayOfWeek[]).map((d) =>
          iv(d, '09:00', '17:00')
        ),
      }),
    ])
    expect(w.find('tbody tr').text()).toContain('Mon–Wed, Fri 09:00–17:00')
  })

  it('lists differing windows as separate lines', async () => {
    const w = await page([
      schedule({ intervals: [iv('mon', '09:00', '12:00'), iv('mon', '13:00', '17:00')] }),
    ])
    const lines = w.findAll('tbody tr td')[1].findAll('div')
    expect(lines.map((l) => l.text())).toEqual(['Mon 09:00–12:00', 'Mon 13:00–17:00'])
  })

  it('says "never" for a schedule with no windows at all', async () => {
    const w = await page([schedule({ intervals: [] })])
    expect(w.find('tbody tr').text()).toContain('never')
  })

  it('marks a public schedule differently from an internal one', async () => {
    const w = await page([schedule({ is_public: true }), schedule({ id: 's2' })])
    expect(w.findAll('tbody tr')[0].find('.badge').text()).toBe('Public')
    expect(w.findAll('tbody tr')[1].find('.badge').text()).toBe('Internal')
  })

  it('hides the page chrome when embedded in another view', async () => {
    const w = mount(ScheduleManagement, { props: { embedded: true }, global: { stubs } })
    await flushPromises()
    expect(w.find('.breadcrumbs').exists()).toBe(false)
    expect(w.find('h1').exists()).toBe(false)
  })
})

describe('the window editor', () => {
  it('seeds a new schedule from the weekday preset', async () => {
    const w = await page()
    await openNew(w)

    // The default is looked up by id with no guard --
    // `SCHEDULE_TEMPLATES.find(...).build()` -- so a renamed preset is a
    // TypeError at the moment the user clicks New. Asserting the id exists
    // keeps that failure here rather than in front of an operator.
    expect(SCHEDULE_TEMPLATES.map((t) => t.id)).toContain('weekday-9-5')
    expect(windowsFor(w, 'mon')).toHaveLength(1)
    expect(timeValue(windowsFor(w, 'mon')[0], 0)).toBe('09:00')
    expect(windowsFor(w, 'sat')).toHaveLength(0)
  })

  it('offers every template the module exports', async () => {
    const w = await page()
    await openNew(w)
    const offered = w
      .findAll('select option')
      .map((o) => o.attributes('value'))
      .filter((v) => v !== '')
    expect(offered).toEqual(SCHEDULE_TEMPLATES.map((t) => t.id))
  })

  it('asks before a template replaces windows that are already there', async () => {
    confirmResult = false
    const w = await page()
    await openNew(w)
    await w.find('select').setValue('24-7')
    await nextTick()

    expect(windowsFor(w, 'sat')).toHaveLength(0)
  })

  it('applies a template when the replacement is accepted', async () => {
    const w = await page()
    await openNew(w)
    await w.find('select').setValue('24-7')
    await nextTick()

    expect(windowsFor(w, 'sat')).toHaveLength(1)
  })

  it('adds a window to the day it was asked for, and nowhere else', async () => {
    const w = await page()
    await openNew(w)
    await dayRow(w, 'sat').findAll('button').at(-1)?.trigger('click')
    await nextTick()

    expect(windowsFor(w, 'sat')).toHaveLength(1)
    expect(windowsFor(w, 'sun')).toHaveLength(0)
  })

  it("renders a day's windows in start-time order regardless of array order", async () => {
    const w = await page([
      schedule({ intervals: [iv('mon', '13:00', '17:00'), iv('mon', '09:00', '12:00')] }),
    ])
    await buttonNamed(w, 'Edit').trigger('click')
    await nextTick()

    expect(windowsFor(w, 'mon').map((r) => timeValue(r, 0))).toEqual(['09:00', '13:00'])
  })

  // FINDING, pinned. The row the user clicks × on and the row that gets
  // removed are found by two different orderings: the display sorts by start
  // time, the removal counts along the array. Hand the editor an unsorted
  // array -- which the server is free to return, and which `addInterval`
  // produces by appending -- and the two disagree.
  //
  // Here the display shows 09:00 first and 13:00 second. Clicking × on the
  // first row removes 13:00, the one the user could see they were not
  // pointing at.
  it('removes a different window than the one the × was next to', async () => {
    const w = await page([
      schedule({ intervals: [iv('mon', '13:00', '17:00'), iv('mon', '09:00', '12:00')] }),
    ])
    await buttonNamed(w, 'Edit').trigger('click')
    await nextTick()

    const displayed = windowsFor(w, 'mon').map((r) => timeValue(r, 0))
    expect(displayed).toEqual(['09:00', '13:00'])

    await windowsFor(w, 'mon')[0].find('button').trigger('click')
    await nextTick()

    expect(
      windowsFor(w, 'mon').map((r) => timeValue(r, 0)),
      'the × now removes the window it sits beside -- if the index mapping was ' +
        'fixed to use the sorted order, this should be ["13:00"]'
    ).toEqual(['09:00'])
  })

  // FINDING, pinned. Same mapping, same disagreement, on edit rather than
  // delete: typing into the first row's start time changes the second window.
  it('edits a different window than the one being typed into', async () => {
    const w = await page([
      schedule({ intervals: [iv('mon', '13:00', '17:00'), iv('mon', '09:00', '12:00')] }),
    ])
    await buttonNamed(w, 'Edit').trigger('click')
    await nextTick()

    // The user is looking at the 09:00-12:00 row -- it is displayed first --
    // and shortens it to end at 11:00. A perfectly ordinary edit.
    expect(timeValue(windowsFor(w, 'mon')[0], 0)).toBe('09:00')
    await setTime(windowsFor(w, 'mon')[0], 1, '11:00')
    await nextTick()

    // It landed on the 13:00 window instead, producing 13:00-11:00. The
    // validator then refuses to save and names a window the user never
    // touched, using a time they never typed against a start they never saw.
    await buttonNamed(w, 'Save').trigger('click')
    await flushPromises()

    expect(
      w.find('.alert-error').text(),
      'the edit now lands on the window that was typed into -- if the index ' +
        'mapping was fixed, this save should succeed'
    ).toContain('mon 13:00–11:00')
    expect(mocks.update).not.toHaveBeenCalled()
  })

  it('leaves other days alone when one day is edited', async () => {
    const w = await page([
      schedule({ intervals: [iv('mon', '09:00', '17:00'), iv('tue', '09:00', '17:00')] }),
    ])
    await buttonNamed(w, 'Edit').trigger('click')
    await nextTick()

    await windowsFor(w, 'tue')[0].find('button').trigger('click')
    await nextTick()

    expect(windowsFor(w, 'mon')).toHaveLength(1)
    expect(windowsFor(w, 'tue')).toHaveLength(0)
  })

  it('does not mutate the table row while editing', async () => {
    const rows = [schedule({ intervals: [iv('mon', '09:00', '17:00')] })]
    const w = await page(rows)
    await buttonNamed(w, 'Edit').trigger('click')
    await nextTick()
    await windowsFor(w, 'mon')[0].find('button').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Cancel').trigger('click')
    await nextTick()

    expect(w.find('tbody tr').text()).toContain('Mon 09:00–17:00')
  })
})

describe('saving', () => {
  it('requires a name', async () => {
    const w = await page()
    await openNew(w)
    expect(buttonNamed(w, 'Create').attributes('disabled')).toBeDefined()
    await w.find('input[type="text"]').setValue('  ')
    expect(buttonNamed(w, 'Create').attributes('disabled')).toBeDefined()
    await w.find('input[type="text"]').setValue('Member Hours')
    expect(buttonNamed(w, 'Create').attributes('disabled')).toBeUndefined()
  })

  it('trims the name and sends the windows', async () => {
    const w = await page()
    await openNew(w)
    await w.find('input[type="text"]').setValue('  Member Hours  ')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(body().name).toBe('Member Hours')
    expect(body().intervals.filter((i) => i.day === 'mon')).toEqual([
      { day: 'mon', start: '09:00', end: '17:00' },
    ])
  })

  it('refuses a window that ends before it starts, and names it', async () => {
    const w = await page()
    await openNew(w)
    await w.find('input[type="text"]').setValue('Member Hours')
    await setTime(windowsFor(w, 'mon')[0], 1, '08:00')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('mon 09:00–08:00')
    expect(mocks.create).not.toHaveBeenCalled()
  })

  it('refuses a zero-length window too', async () => {
    const w = await page()
    await openNew(w)
    await w.find('input[type="text"]').setValue('Member Hours')
    await setTime(windowsFor(w, 'mon')[0], 1, '09:00')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(mocks.create).not.toHaveBeenCalled()
  })

  // Recorded, not pinned as a defect: `iv.end <= iv.start` also rules out a
  // window that runs past midnight. A space open 22:00–02:00 has to be entered
  // as two windows on two days, and nothing in the editor says so. Whether
  // that is a limitation or a design is not this tier's call -- but it is not
  // an accident of the validation either, since the same rule is written into
  // the type's own doc comment.
  it('cannot express a window that runs past midnight', async () => {
    const w = await page()
    await openNew(w)
    await w.find('input[type="text"]').setValue('Late Hours')
    await setTime(windowsFor(w, 'mon')[0], 0, '22:00')
    await setTime(windowsFor(w, 'mon')[0], 1, '02:00')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(mocks.create).not.toHaveBeenCalled()
    expect(w.find('.alert-error').text()).toContain('22:00–02:00')
  })

  it('updates rather than creates when a row was opened for editing', async () => {
    const w = await page([schedule({ id: 's7' })])
    await buttonNamed(w, 'Edit').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Save').trigger('click')
    await flushPromises()

    expect(mocks.update.mock.calls[0][0]).toBe('s7')
    expect(patched().name).toBe('Member Hours')
    expect(patched().intervals).toEqual([{ day: 'mon', start: '09:00', end: '17:00' }])
    expect(mocks.create).not.toHaveBeenCalled()
  })

  it('reloads and reports success', async () => {
    const w = await page()
    await openNew(w)
    await w.find('input[type="text"]').setValue('Member Hours')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(w.find('.alert-success').text()).toContain('Schedule created')
    expect(mocks.list).toHaveBeenCalledTimes(2)
    expect(w.find('.modal-open').exists()).toBe(false)
  })

  it("reports the server's reason and keeps the form open", async () => {
    mocks.create.mockResolvedValue({ success: false, error: 'Name already used' })
    const w = await page()
    await openNew(w)
    await w.find('input[type="text"]').setValue('Member Hours')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('Name already used')
    expect(w.find('.modal-open').exists()).toBe(true)
  })
})

describe('deleting', () => {
  it('warns what deleting will do to the rules that reference it', async () => {
    confirmResult = false
    const w = await page([schedule({ name: 'Member Hours' })])
    await buttonNamed(w, 'Delete').trigger('click')
    await flushPromises()

    expect(vi.mocked(globalThis.confirm)).toHaveBeenCalledWith(
      'Delete "Member Hours"? Rules referencing it will revert to "always".'
    )
    expect(mocks.remove).not.toHaveBeenCalled()
  })

  it('removes and reloads', async () => {
    const w = await page([schedule({ id: 's9' })])
    await buttonNamed(w, 'Delete').trigger('click')
    await flushPromises()

    expect(mocks.remove).toHaveBeenCalledWith('s9')
    expect(mocks.list).toHaveBeenCalledTimes(2)
  })
})

describe('what a network error does', () => {
  // FINDING, pinned. Seventh component with this shape: `load()` has no
  // try/catch and clears `loading` only after the await.
  it('spins forever when the list rejects', async () => {
    const escaped: unknown[] = []
    mocks.list.mockRejectedValue(new Error('Network Error'))
    const w = mount(ScheduleManagement, {
      global: { stubs, config: { errorHandler: (e: unknown) => escaped.push(e) } },
    })
    await flushPromises()

    expect(w.find('.loading-spinner').exists()).toBe(true)
    expect(escaped).toHaveLength(1)
  })

  // FINDING, pinned. Second component with this shape: `save()` sets
  // `saving = true` with no `finally`.
  it('strands the save button when the save rejects', async () => {
    const escaped: unknown[] = []
    mocks.create.mockRejectedValue(new Error('Network Error'))
    const w = mount(ScheduleManagement, {
      global: { stubs, config: { errorHandler: (e: unknown) => escaped.push(e) } },
    })
    await flushPromises()
    await openNew(w)
    await w.find('input[type="text"]').setValue('Member Hours')
    await w.find('.modal-action .btn-primary').trigger('click')
    await flushPromises()

    expect(
      w.find('.modal-action .btn-primary').attributes('disabled'),
      'the save button now recovers -- if a try/finally was added, delete this test'
    ).toBeDefined()
    expect(w.find('.modal-open').exists()).toBe(true)
    expect(escaped).toHaveLength(1)
  })
})
