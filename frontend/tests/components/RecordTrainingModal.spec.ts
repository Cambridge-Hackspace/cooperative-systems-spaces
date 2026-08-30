// Tier 2: RecordTrainingModal.
//
// An instructor records a completed session after the fact. Two dates matter
// and both are computed the same wrong way:
//
//     training_date: new Date().toISOString().split('T')[0]   // the default
//     const today = computed(() => new Date().toISOString().split('T')[0])  // the max
//
// `toISOString()` is UTC. West of UTC the two disagree with the user's date for
// the last hours of every day, so a session trained on the evening of the 15th
// is dated the 16th -- and the `max` moves with it, so nothing refuses it.
// Third component on this branch with this defect, after EditTrainerModal and
// AssignTrainerModal.
//
// Unlike those two, the *format* is right here: the server's field is
// `training_date: NaiveDate` (api/trainers.rs), which accepts "2026-01-15".
// It is the day that is wrong, not the shape.
//
// What this spec does NOT prove: what the server does with a future date, or
// whether it cross-checks the trainee against the tool. Tier 6 owns both.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({ getAllUsers: vi.fn(), createTrainingRecord: vi.fn() }))
vi.mock('@/utils/api', () => ({
  userApi: { getAllUsers: mocks.getAllUsers },
  trainerApi: { createTrainingRecord: mocks.createTrainingRecord },
}))

import RecordTrainingModal from '@/components/RecordTrainingModal.vue'
import { UserRole, type Tool, type User } from '@/types'
import { TrainingCompletionStatus } from '@/types/training'

const TOOL = { id: 'tool-1', name: 'Lathe' } as unknown as Tool

function user(id: string, over: Partial<User> = {}): User {
  return {
    id,
    username: `u-${id}`,
    email: `${id}@example.test`,
    full_name: `Full ${id}`,
    is_active: true,
    role: UserRole.Member,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    profile: {},
    meta: {},
    ...over,
  }
}

beforeEach(() => {
  mocks.getAllUsers.mockReset()
  mocks.createTrainingRecord.mockReset()
  mocks.getAllUsers.mockResolvedValue({ success: true, data: { items: [user('a')] } })
  mocks.createTrainingRecord.mockResolvedValue({ success: true })
})

async function modal(tool: Tool = TOOL) {
  const w = mount(RecordTrainingModal, { props: { tool } })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof modal>>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}

const sent = () => mocks.createTrainingRecord.mock.calls[0][0] as Record<string, unknown>

async function fillMinimum(w: Wrapper) {
  await w.find('#trainee').setValue('a')
}

describe('who can be recorded against', () => {
  it('names the tool and does not let it be changed', async () => {
    const w = await modal()
    const input = w.find('#tool')
    expect((input.element as HTMLInputElement).value).toBe('Lathe')
    expect(input.attributes('disabled')).toBeDefined()
  })

  it('leaves the tool blank when none was given', async () => {
    // Mounted directly rather than through `modal(undefined)`: a JavaScript
    // default parameter applies when the argument *is* `undefined`, so that
    // call would have quietly used the default tool and asserted nothing.
    const w = mount(RecordTrainingModal, { props: {} })
    await flushPromises()
    expect((w.find('#tool').element as HTMLInputElement).value).toBe('')
  })

  it('offers only active members as trainees', async () => {
    mocks.getAllUsers.mockResolvedValue({
      success: true,
      data: { items: [user('a'), user('b', { is_active: false }), user('c')] },
    })
    const w = await modal()
    const offered = w
      .findAll('#trainee option')
      .map((o) => o.attributes('value'))
      .filter((v) => v !== '')
    expect(offered).toEqual(['a', 'c'])
  })

  it('reports a refused roster load', async () => {
    mocks.getAllUsers.mockResolvedValue({ success: false, error: 'Forbidden' })
    const w = await modal()
    expect(w.find('.error').text()).toBe('Forbidden')
  })

  it('offers every completion status the enum defines', async () => {
    const w = await modal()
    const offered = w
      .findAll('#completion_status option')
      .map((o) => o.attributes('value'))
      .filter((v) => v !== '')
    expect([...offered].sort()).toEqual([...Object.values(TrainingCompletionStatus)].sort())
  })
})

describe('the training date', () => {
  // The clock is frozen at 2026-01-15T12:00:00Z and the suite timezone is
  // America/Chicago; see tests/unit/suite-environment.spec.ts.
  it('defaults to today and refuses anything later', async () => {
    const w = await modal()
    expect((w.find('#training_date').element as HTMLInputElement).value).toBe('2026-01-15')
    expect(w.find('#training_date').attributes('max')).toBe('2026-01-15')
  })

  // FIXED. Both the default and the ceiling were the UTC date, so an instructor
  // recording a session at eight in the evening was handed tomorrow -- and
  // because the ceiling moved with it, nothing objected. The session then sat
  // in the record dated a day after it happened.
  it("dates an evening session today, in the instructor's timezone", async () => {
    vi.setSystemTime(new Date('2026-01-16T02:00:00Z'))
    expect(new Date().getDate(), 'the suite timezone is not what this test assumes').toBe(15)

    const w = await modal()
    expect((w.find('#training_date').element as HTMLInputElement).value).toBe('2026-01-15')
    expect(w.find('#training_date').attributes('max')).toBe('2026-01-15')
    vi.setSystemTime(new Date('2026-01-15T12:00:00.000Z'))
  })

  it('sends the date in the shape the server parses', async () => {
    const w = await modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    // `NaiveDate` on the server, so a bare calendar date is correct here --
    // unlike EditTrainerModal, whose field is a `DateTime<Utc>` and which sends
    // this same shape to something that cannot parse it.
    expect(sent().training_date).toMatch(/^\d{4}-\d{2}-\d{2}$/)
  })
})

describe('the skills list', () => {
  const skillInput = (w: Wrapper) => w.findAll('input[type="text"]').at(-1)

  it('adds a skill and clears the box', async () => {
    const w = await modal()
    await skillInput(w)?.setValue('Chuck changing')
    await buttonNamed(w, 'Add').trigger('click')
    await nextTick()

    expect(w.find('.skills-list, .skill-tag').exists()).toBe(true)
    expect(w.text()).toContain('Chuck changing')
    expect((skillInput(w)?.element as HTMLInputElement).value).toBe('')
  })

  it('trims what it adds', async () => {
    const w = await modal()
    await skillInput(w)?.setValue('  Chuck changing  ')
    await buttonNamed(w, 'Add').trigger('click')
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(sent().skills_covered).toEqual(['Chuck changing'])
  })

  it('refuses a duplicate and refuses an empty one', async () => {
    const w = await modal()
    await skillInput(w)?.setValue('Chuck changing')
    await buttonNamed(w, 'Add').trigger('click')
    await nextTick()
    await skillInput(w)?.setValue('Chuck changing')
    await buttonNamed(w, 'Add').trigger('click')
    await nextTick()
    await skillInput(w)?.setValue('   ')
    await buttonNamed(w, 'Add').trigger('click')
    await nextTick()

    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(sent().skills_covered).toEqual(['Chuck changing'])
  })

  it('removes the skill at the position clicked', async () => {
    const w = await modal()
    for (const s of ['One', 'Two', 'Three']) {
      await skillInput(w)?.setValue(s)
      await buttonNamed(w, 'Add').trigger('click')
      await nextTick()
    }
    await w.findAll('.skill-remove')[1].trigger('click')
    await nextTick()

    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(sent().skills_covered).toEqual(['One', 'Three'])
  })
})

describe('what the form sends', () => {
  it('carries the tool, the trainee and the status', async () => {
    const w = await modal()
    await fillMinimum(w)
    await w.find('#completion_status').setValue(TrainingCompletionStatus.Partial)
    await w.find('#minutes_trained').setValue('45')
    await w.find('#notes').setValue('Slow on the tailstock.')
    await w.find('#next_steps').setValue('Another hour on threading.')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(sent()).toMatchObject({
      tool_id: 'tool-1',
      trainee_user_id: 'a',
      completion_status: TrainingCompletionStatus.Partial,
      minutes_trained: 45,
      notes: 'Slow on the tailstock.',
      next_steps: 'Another hour on threading.',
    })
  })

  it('omits the optional text fields rather than sending empty strings', async () => {
    const w = await modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    const json = JSON.parse(JSON.stringify(sent())) as Record<string, unknown>
    expect(json).not.toHaveProperty('notes')
    expect(json).not.toHaveProperty('next_steps')
    expect(json).not.toHaveProperty('skills_covered')
  })

  // FINDING, pinned. `v-model.number` on an emptied number input yields the
  // empty *string*: Vue's `looseToNumber` returns its argument unchanged when
  // `parseFloat` gives NaN, and `parseFloat('')` is NaN. The server's field is
  // `Option<i32>`, which cannot deserialise `""`. Same defect as
  // EditTrainingStepModal, and the same fix.
  it('sends an empty string, not nothing, when the minutes box is cleared', async () => {
    const w = await modal()
    await fillMinimum(w)
    await w.find('#minutes_trained').setValue('45')
    await w.find('#minutes_trained').setValue('')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(
      sent().minutes_trained,
      'a cleared minutes box now sends something else -- if it was fixed to send ' +
        'undefined, delete this test; the server field is Option<i32>'
    ).toBe('')
  })

  it('has no guard of its own against submitting with no trainee', async () => {
    // The select carries `required`, so a browser refuses the submit and this
    // path is not reachable there. `trigger('submit')` bypasses constraint
    // validation, which is what makes the absence of a component-side guard
    // visible. Asserts the guard is missing, not that a user can hit it.
    const w = await modal()
    expect(w.find('#trainee').attributes('required')).toBeDefined()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(mocks.createTrainingRecord).toHaveBeenCalledTimes(1)
    expect(sent().trainee_user_id).toBe('')
  })
})

describe('what happens after the request', () => {
  it('announces the record when the server agrees', async () => {
    const w = await modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.emitted('recorded')).toHaveLength(1)
    expect(w.find('.error').exists()).toBe(false)
  })

  it("shows the server's reason and does not announce the record", async () => {
    mocks.createTrainingRecord.mockResolvedValue({ success: false, error: 'Not a trainer here' })
    const w = await modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error').text()).toBe('Not a trainer here')
    expect(w.emitted('recorded')).toBeUndefined()
  })

  it('falls back to a message about recording, not about loading users', async () => {
    // Worth asserting: this is the modal AssignTrainerModal was copied from or
    // alongside, and unlike that one its failure branches describe the
    // operation they belong to.
    mocks.createTrainingRecord.mockResolvedValue({ success: false })
    const w = await modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error').text()).toBe('Failed to record training session')
  })

  it('re-enables the submit button whether the request resolved or rejected', async () => {
    mocks.createTrainingRecord.mockRejectedValue(new Error('down'))
    const w = await modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('button[type="submit"]').attributes('disabled')).toBeUndefined()
  })

  it('disables the submit button while the request is in flight', async () => {
    mocks.createTrainingRecord.mockReturnValue(new Promise(() => {}))
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

  it('does not close when the modal body is clicked', async () => {
    const w = await modal()
    await w.find('.modal').trigger('click')
    expect(w.emitted('close')).toBeUndefined()
  })
})
