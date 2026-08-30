// Tier 2: StartTrainingModal.
//
// A form with one select, one textarea, and a submit. The interesting part is
// the select, because "no instructor" is a real choice here rather than an
// absence, and the component represents it two different ways depending on
// whether the user touched the control:
//
//     const form = ref<StartTrainingRequest>({ instructor_id: undefined, ... })
//     <option value="">Self-study (No instructor)</option>
//
// Untouched, `instructor_id` is `undefined` and JSON.stringify drops it. Chosen
// deliberately, it is `''`. The server declares `instructor_id: Option<Uuid>`,
// and `""` is not a Uuid -- so clicking the option that describes the default
// produces a request the server rejects, while leaving it alone produces one it
// accepts. Both findings below are pinned to the current behavior.
//
// What this spec does NOT prove: what the server actually answers for
// `instructor_id: ""`. That is Tier 4's question. Here the claim is only about
// the bytes this component puts on the wire.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

// `vi.hoisted` and direct references, not forwarding arrows -- `vi.mock` is
// hoisted above every top-level `const`, and a forwarding wrapper returns `any`.
const mocks = vi.hoisted(() => ({ startTrainingSession: vi.fn(), getAllUsers: vi.fn() }))
const startTrainingSession = mocks.startTrainingSession
const getAllUsers = mocks.getAllUsers

vi.mock('@/utils/api', () => ({
  trainingApi: { startTrainingSession: mocks.startTrainingSession },
  userApi: { getAllUsers: mocks.getAllUsers },
}))

import StartTrainingModal from '@/components/StartTrainingModal.vue'
import { UserRole, type User } from '@/types'
import { AssessmentType, type TrainingStep } from '@/types/training'

function user(id: string, role: UserRole, over: Partial<User> = {}): User {
  return {
    id,
    username: `u-${id}`,
    email: `${id}@example.test`,
    full_name: `Full ${id}`,
    is_active: true,
    role,
    created_at: '2026-01-15T12:00:00Z',
    updated_at: '2026-01-15T12:00:00Z',
    profile: {},
    meta: {},
    ...over,
  }
}

const TRAINEE = user('trainee', UserRole.Member)

function step(over: Partial<TrainingStep> = {}): TrainingStep {
  return {
    id: 'step-1',
    tool_id: 'tool-1',
    step_number: 1,
    step_name: 'Lathe safety',
    description: 'Guards, speeds, and what not to wear.',
    assessment_type: AssessmentType.Practical,
    passing_score: 80,
    is_active: true,
    created_at: '2026-01-15T12:00:00Z',
    updated_at: '2026-01-15T12:00:00Z',
    ...over,
  }
}

beforeEach(() => {
  startTrainingSession.mockReset()
  getAllUsers.mockReset()
  // Every test mounts, and mounting fetches the roster. A default keeps each
  // test's setup about the thing it is testing.
  getAllUsers.mockResolvedValue({ success: true, data: { items: [] } })
})

async function modal(over: Partial<TrainingStep> = {}) {
  const w = mount(StartTrainingModal, { props: { step: step(over), user: TRAINEE } })
  await flushPromises()
  return w
}

function buttonNamed(w: Awaited<ReturnType<typeof modal>>, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labeled ${JSON.stringify(label)}`)
  return b
}

describe('what the step summary shows', () => {
  it('names the step and repeats its description', async () => {
    const w = await modal()
    expect(w.find('.step-info h4').text()).toBe('Lathe safety')
    expect(w.find('.step-info p').text()).toBe('Guards, speeds, and what not to wear.')
  })

  it('has a label for every assessment type the enum defines', async () => {
    // Exhaustive over the enum rather than over a list restated here.
    // `formatAssessmentType` is a lookup with a `|| type` fallback, so a
    // renamed wire value degrades to showing the raw value -- 'observation_only'
    // in the UI -- instead of failing. Adding a variant to `AssessmentType`
    // without adding a label fails this test.
    for (const value of Object.values(AssessmentType)) {
      const w = await modal({ assessment_type: value })
      const shown = w.find('.assessment-type').text().replace('Assessment Type:', '').trim()
      expect(shown, `${value} has no label and fell back to its wire value`).not.toBe(value)
      expect(shown).not.toBe('')
    }
  })

  it('shows the passing score when there is one', async () => {
    expect((await modal({ passing_score: 70 })).find('.passing-score').text()).toContain('70%')
  })

  it('shows no passing score when the step has none', async () => {
    expect((await modal({ passing_score: undefined })).find('.passing-score').exists()).toBe(false)
  })

  // FINDING, pinned. `v-if="step.passing_score"` is a truthiness test, so a
  // passing score of zero is indistinguishable from no passing score at all.
  // A step scored out of zero is arguably meaningless, but the component does
  // not decide that -- it just disappears the field, and an instructor reading
  // the modal cannot tell the difference between "not scored" and "scored, and
  // anything passes".
  it('hides a passing score of zero, which is not the same as having none', async () => {
    const w = await modal({ passing_score: 0 })
    expect(
      w.find('.passing-score').exists(),
      'a zero passing score now renders -- if that was fixed deliberately, ' +
        'delete this test; the field is guarded by `v-if="step.passing_score"`'
    ).toBe(false)
  })
})

describe('who is offered as an instructor', () => {
  it('offers staff and admins and nobody else', async () => {
    // Exhaustive over UserRole: one user per role, and the assertion names
    // exactly which roles may appear. A new role defaults to "not an
    // instructor" here, and this test is where that decision has to be made
    // again rather than inherited by accident.
    const everyone = Object.values(UserRole).map((role) => user(role, role))
    getAllUsers.mockResolvedValue({ success: true, data: { items: everyone } })

    const w = await modal()
    const offered = w
      .findAll('option')
      .map((o) => o.attributes('value'))
      .filter((v) => v !== '')

    expect(offered.sort()).toEqual([UserRole.Admin, UserRole.Staff].sort())
  })

  it('labels an instructor by full name, falling back to the username', async () => {
    getAllUsers.mockResolvedValue({
      success: true,
      data: {
        items: [
          user('a', UserRole.Staff, { full_name: 'Ada Lovelace' }),
          user('b', UserRole.Admin, { full_name: '', username: 'grace' }),
        ],
      },
    })
    const w = await modal()
    const labels = w.findAll('option').map((o) => o.text())
    expect(labels).toContain('Ada Lovelace')
    expect(labels).toContain('grace')
  })

  it('always offers self-study', async () => {
    const w = await modal()
    const selfStudy = w.findAll('option').find((o) => o.attributes('value') === '')
    expect(selfStudy?.text()).toBe('Self-study (No instructor)')
  })

  // FINDING, pinned. `loadInstructors` catches everything and writes it to
  // `console.error`. The roster request is the one call this modal makes on
  // open, it is admin-scoped, and a member opening this modal gets a 403 -- so
  // the failure is not hypothetical. The user sees a select containing only
  // self-study and no indication that anything went wrong.
  //
  // Same shape as AppBoot, SiteIndexContent and ThemePicker: the failure path
  // is the one nobody looks at.
  it('says nothing at all when the roster cannot be loaded', async () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    getAllUsers.mockRejectedValue(new Error('Request failed with status code 403'))

    const w = await modal()

    expect(w.find('.error-message').exists()).toBe(false)
    expect(w.text()).not.toContain('403')
    expect(
      w.findAll('option').length,
      'the picker now shows something other than the bare self-study option ' +
        'after a failed roster load -- if a failure message was added, this ' +
        'test should assert it instead'
    ).toBe(1)
    expect(spy).toHaveBeenCalled()
    spy.mockRestore()
  })

  it('offers nobody when the response says success but carries no items', async () => {
    getAllUsers.mockResolvedValue({ success: true, data: {} })
    expect((await modal()).findAll('option')).toHaveLength(1)
  })
})

describe('what the form sends', () => {
  it('omits the instructor entirely when the select is never touched', async () => {
    startTrainingSession.mockResolvedValue({ success: true })
    const w = await modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(startTrainingSession).toHaveBeenCalledWith('trainee', {
      training_step_id: 'step-1',
      instructor_id: undefined,
      notes: '',
    })
  })

  // FINDING, pinned. Choosing the option that describes the default sends
  // `instructor_id: ""`, and the server declares `Option<Uuid>` -- an empty
  // string is not a Uuid, so it is a deserialization failure rather than
  // self-study. The two ways to express "no instructor" are not equivalent,
  // and the one a user has to click is the broken one.
  it('sends an empty string when self-study is chosen deliberately', async () => {
    startTrainingSession.mockResolvedValue({ success: true })
    const w = await modal()

    await w.find('#instructor').setValue('')
    await w.find('form').trigger('submit')
    await flushPromises()

    const sent = startTrainingSession.mock.calls[0][1] as { instructor_id?: string }
    expect(
      sent.instructor_id,
      'self-study now sends something other than "" -- if it was fixed to send ' +
        'undefined or null, delete this test; the server takes Option<Uuid> and ' +
        '"" is not one'
    ).toBe('')
  })

  it('sends the chosen instructor and the notes', async () => {
    getAllUsers.mockResolvedValue({
      success: true,
      data: { items: [user('inst', UserRole.Staff)] },
    })
    startTrainingSession.mockResolvedValue({ success: true })
    const w = await modal()

    await w.find('#instructor').setValue('inst')
    await w.find('#notes').setValue('Second attempt.')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(startTrainingSession).toHaveBeenCalledWith('trainee', {
      training_step_id: 'step-1',
      instructor_id: 'inst',
      notes: 'Second attempt.',
    })
  })
})

describe('what happens after the request', () => {
  it('emits started and shows no error when the server agrees', async () => {
    startTrainingSession.mockResolvedValue({ success: true })
    const w = await modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.emitted('started')).toHaveLength(1)
    expect(w.find('.error-message').exists()).toBe(false)
  })

  it("shows the server's own words when it refuses, and does not emit started", async () => {
    startTrainingSession.mockResolvedValue({ success: false, error: 'Already in progress' })
    const w = await modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error-message').text()).toBe('Already in progress')
    expect(w.emitted('started')).toBeUndefined()
  })

  it('falls back to a generic message when a refusal carries no error text', async () => {
    startTrainingSession.mockResolvedValue({ success: false })
    const w = await modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error-message').text()).toBe('Failed to start training session')
  })

  // FINDING, pinned. The catch reads `err.message`, which for an axios
  // rejection is "Request failed with status code 409" -- the status restated
  // in prose. The server's own explanation is at `err.response.data.error` and
  // is discarded. DoorCheckinView reads that path (92afb4c); this component
  // does not, so the same server refusal is legible in one screen and opaque
  // in the other.
  it('discards the server body on a thrown error and shows axios prose instead', async () => {
    const axiosish = Object.assign(new Error('Request failed with status code 409'), {
      response: { data: { error: 'That tool is already booked for this slot' } },
    })
    startTrainingSession.mockRejectedValue(axiosish)

    const w = await modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(
      w.find('.error-message').text(),
      "the component now reads the server's error body -- if that was fixed, " +
        'this test should assert the body instead of the axios message'
    ).toBe('Request failed with status code 409')
    expect(w.text()).not.toContain('already booked')
  })

  it('re-enables the submit button whether the request succeeded or failed', async () => {
    startTrainingSession.mockRejectedValue(new Error('down'))
    const w = await modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('button[type="submit"]').attributes('disabled')).toBeUndefined()
    expect(w.find('button[type="submit"]').text()).toBe('Start Training')
  })

  it('disables the submit button and says so while the request is in flight', async () => {
    startTrainingSession.mockReturnValue(new Promise(() => {}))
    const w = await modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    const submit = w.find('button[type="submit"]')
    expect(submit.attributes('disabled')).toBeDefined()
    expect(submit.text()).toBe('Starting...')
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

  it('does not close when the content itself is clicked', async () => {
    const w = await modal()
    await w.find('.modal-content').trigger('click')
    expect(w.emitted('close')).toBeUndefined()
  })
})
