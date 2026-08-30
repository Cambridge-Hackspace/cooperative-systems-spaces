// Tier 2: ToolTrainingSetupModal.
//
// A four-page wizard that ends in a burst of writes. Three of its four defects
// are FIXED and asserted as fixed below:
//
//   - The tool update is checked now. It used to be awaited and discarded, and
//     `updateTool` resolves on failure, so a refused update -- the call that
//     makes the tool require training at all -- was followed by the whole
//     step-creation loop.
//   - Prerequisites go to the route that exists. They used to be posted to
//     `/training/prerequisites`, which is not one, and the result was not
//     checked either, so page 3 was discarded silently.
//   - "This tool needs no training" is submittable. `canCreateSetup` used to
//     require `requiresTraining`, which disabled the only button that would
//     record that answer, and page 2 demanded a filled-in step for it anyway.
//
// The fourth is still open and still pinned: step creation throws on the first
// failure with nothing to undo the steps already created, so a retry
// duplicates whatever succeeded. Fixing that needs either a transaction the
// API does not offer or an idempotency key it does not accept.
//
// What this spec does NOT prove: what the server answers to any of these.
// Tier 6 owns the round trip and Tier 4 the status codes.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({
  updateTool: vi.fn(),
  createTrainingStep: vi.fn(),
  addTrainingPrerequisite: vi.fn(),
}))
vi.mock('@/utils/api', () => ({
  toolsApi: { updateTool: mocks.updateTool },
  trainingApi: {
    createTrainingStep: mocks.createTrainingStep,
    addTrainingPrerequisite: mocks.addTrainingPrerequisite,
  },
}))

import ToolTrainingSetupModal from '@/components/ToolTrainingSetupModal.vue'
import { AssessmentType } from '@/types/training'
import type { Tool } from '@/types'

const TOOL = { id: 'tool-1', name: 'Lathe' } as unknown as Tool

beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset()
  mocks.updateTool.mockResolvedValue({ success: true })
  mocks.createTrainingStep.mockImplementation((body: { step_number: number }) =>
    Promise.resolve({ success: true, data: { id: `step-${body.step_number}` } })
  )
  mocks.addTrainingPrerequisite.mockResolvedValue({ success: true })
})

const modal = () => mount(ToolTrainingSetupModal, { props: { tool: TOOL } })
type Wrapper = ReturnType<typeof modal>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}

const stepNameInputs = (w: Wrapper) => w.findAll('input[type="text"]')
const descriptionInputs = (w: Wrapper) => w.findAll('textarea')

async function fillFirstStep(w: Wrapper, name = 'Safety', desc = 'Guards and PPE') {
  await buttonNamed(w, 'Next').trigger('click')
  await nextTick()
  await stepNameInputs(w)[0].setValue(name)
  await descriptionInputs(w)[0].setValue(desc)
  await nextTick()
}

/** Walk from page 2 to page 4. */
async function toReview(w: Wrapper) {
  await buttonNamed(w, 'Next').trigger('click')
  await nextTick()
  await buttonNamed(w, 'Next').trigger('click')
  await nextTick()
}

const createdSteps = () =>
  mocks.createTrainingStep.mock.calls.map((c) => c[0] as Record<string, unknown>)

describe('walking the wizard', () => {
  it('starts on the overview and shows four pages', () => {
    const w = modal()
    expect(w.find('.setup-step').exists()).toBe(true)
    expect(
      w.findAll('.progress-steps > *, .step-indicator, .progress-step').length
    ).toBeGreaterThan(0)
    expect(buttonNamed(w, 'Next').attributes('disabled')).toBeUndefined()
  })

  it('will not leave the step page until every step has a name and a description', async () => {
    const w = modal()
    await buttonNamed(w, 'Next').trigger('click')
    await nextTick()

    expect(buttonNamed(w, 'Next').attributes('disabled')).toBeDefined()
    await stepNameInputs(w)[0].setValue('Safety')
    await nextTick()
    expect(buttonNamed(w, 'Next').attributes('disabled')).toBeDefined()

    await descriptionInputs(w)[0].setValue('Guards and PPE')
    await nextTick()
    expect(buttonNamed(w, 'Next').attributes('disabled')).toBeUndefined()
  })

  it('rejects a name that is only whitespace', async () => {
    const w = modal()
    await buttonNamed(w, 'Next').trigger('click')
    await nextTick()
    await stepNameInputs(w)[0].setValue('   ')
    await descriptionInputs(w)[0].setValue('Guards and PPE')
    await nextTick()
    expect(buttonNamed(w, 'Next').attributes('disabled')).toBeDefined()
  })

  // `removeStep` also guards with `if (steps.length > 1)`, and that guard is
  // unreachable: the button is `v-if="steps.length > 1"`, so it does not
  // render on the only list that could trigger it. Two guards for one rule,
  // one of which fires. An equivalent mutant, recorded rather than chased.
  it('adds and removes steps, but never offers to remove the last one', async () => {
    const w = modal()
    await buttonNamed(w, 'Next').trigger('click')
    await nextTick()

    expect(stepNameInputs(w)).toHaveLength(1)
    expect(w.findAll('.remove-step, .step-header button')).toHaveLength(0)

    await buttonNamed(w, '+ Add Another Step').trigger('click')
    await nextTick()
    expect(stepNameInputs(w)).toHaveLength(2)

    await w.findAll('.step-header button')[0].trigger('click')
    await nextTick()
    expect(stepNameInputs(w)).toHaveLength(1)
    expect(w.findAll('.step-header button')).toHaveLength(0)
  })

  it('goes back as well as forward', async () => {
    const w = modal()
    await fillFirstStep(w)
    await buttonNamed(w, 'Next').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Previous').trigger('click')
    await nextTick()

    expect(stepNameInputs(w)).toHaveLength(1)
  })

  it('hides the passing score for an observation-only step', async () => {
    const w = modal()
    await buttonNamed(w, 'Next').trigger('click')
    await nextTick()

    expect(w.find('input[type="number"]').exists()).toBe(true)
    await w.findAll('select')[0].setValue(AssessmentType.ObservationOnly)
    await nextTick()
    // The passing-score group is gone; the expiry number input remains.
    expect(w.findAll('input[type="number"]').length).toBe(1)
  })
})

describe('what it creates', () => {
  it('marks the tool as requiring training, then numbers the steps from one', async () => {
    const w = modal()
    await fillFirstStep(w)
    await buttonNamed(w, '+ Add Another Step').trigger('click')
    await nextTick()
    await stepNameInputs(w)[1].setValue('Operation')
    await descriptionInputs(w)[1].setValue('Speeds and feeds')
    await nextTick()
    await toReview(w)
    await buttonNamed(w, 'Create Training Setup').trigger('click')
    await flushPromises()

    expect(mocks.updateTool).toHaveBeenCalledWith('tool-1', { requires_training: true })
    expect(createdSteps().map((s) => s.step_number)).toEqual([1, 2])
    expect(createdSteps()[0]).toMatchObject({
      tool_id: 'tool-1',
      step_name: 'Safety',
      description: 'Guards and PPE',
      assessment_type: AssessmentType.Practical,
    })
    expect(w.emitted('created')).toHaveLength(1)
    expect(w.emitted('close')).toHaveLength(1)
  })

  // Recorded: `passing_score`, `expiry_days` and `is_active` are sent on every
  // step and none of them exist on the server -- `training_steps` has no such
  // columns and `CreateTrainingStepRequest` no such fields. See the
  // EditTrainingStepModal spec for the field-by-field comparison. The wizard
  // collects a passing score on page 2 and shows it back on the review page,
  // and it goes nowhere.
  it('sends three fields the server has no column for', async () => {
    const w = modal()
    await fillFirstStep(w)
    await toReview(w)
    await buttonNamed(w, 'Create Training Setup').trigger('click')
    await flushPromises()

    for (const key of ['passing_score', 'expiry_days', 'is_active']) {
      expect(Object.keys(createdSteps()[0]), `${key} is still sent`).toContain(key)
    }
  })

  // FINDING, pinned. The tool update is awaited and its result discarded, and
  // `updateTool` resolves rather than rejects on failure. So a refused update
  // -- the call that makes the tool require training at all -- is followed by
  // the whole step-creation loop, and the wizard reports success.
  it('creates no steps when marking the tool as requiring training was refused', async () => {
    mocks.updateTool.mockResolvedValue({ success: false, error: 'Tool is archived' })
    const w = modal()
    await fillFirstStep(w)
    await toReview(w)
    await buttonNamed(w, 'Create Training Setup').trigger('click')
    await flushPromises()

    // The call that makes the tool require training at all. Carrying on past
    // its refusal built a training programme for a tool that does not require
    // training.
    expect(mocks.createTrainingStep).not.toHaveBeenCalled()
    expect(w.find('.error-message').text()).toContain('Tool is archived')
    expect(w.emitted('created')).toBeUndefined()
  })

  // FINDING, pinned. The loop throws on the first failed step and nothing
  // undoes the ones already created. The tool is already marked as requiring
  // training, steps 1..n-1 exist, and the operator is shown an error with no
  // indication of how much landed. Pressing Create again makes a second copy
  // of every step that succeeded.
  it('leaves the steps it already created behind when one fails', async () => {
    mocks.createTrainingStep
      .mockResolvedValueOnce({ success: true, data: { id: 'step-1' } })
      .mockResolvedValueOnce({ success: false, error: 'Duplicate step number' })

    const w = modal()
    await fillFirstStep(w)
    await buttonNamed(w, '+ Add Another Step').trigger('click')
    await nextTick()
    await stepNameInputs(w)[1].setValue('Operation')
    await descriptionInputs(w)[1].setValue('Speeds and feeds')
    await nextTick()
    await toReview(w)
    await buttonNamed(w, 'Create Training Setup').trigger('click')
    await flushPromises()

    expect(w.find('.error-message').text()).toContain('Duplicate step number')
    expect(
      mocks.createTrainingStep,
      'a partial failure is now cleaned up -- if a rollback was added, this ' +
        'test should assert it'
    ).toHaveBeenCalledTimes(2)
    expect(mocks.updateTool).toHaveBeenCalledTimes(1)
    expect(w.emitted('created')).toBeUndefined()
    expect(w.emitted('close')).toBeUndefined()
  })

  it('stops creating after the first failure rather than pressing on', async () => {
    mocks.createTrainingStep
      .mockResolvedValueOnce({ success: false, error: 'Duplicate step number' })
      .mockResolvedValue({ success: true, data: { id: 'step-2' } })

    const w = modal()
    await fillFirstStep(w)
    await buttonNamed(w, '+ Add Another Step').trigger('click')
    await nextTick()
    await stepNameInputs(w)[1].setValue('Operation')
    await descriptionInputs(w)[1].setValue('Speeds and feeds')
    await nextTick()
    await toReview(w)
    await buttonNamed(w, 'Create Training Setup').trigger('click')
    await flushPromises()

    expect(mocks.createTrainingStep).toHaveBeenCalledTimes(1)
  })

  it('re-enables the create button after a failure', async () => {
    mocks.createTrainingStep.mockResolvedValue({ success: false, error: 'nope' })
    const w = modal()
    await fillFirstStep(w)
    await toReview(w)
    await buttonNamed(w, 'Create Training Setup').trigger('click')
    await flushPromises()

    expect(buttonNamed(w, 'Create Training Setup').attributes('disabled')).toBeUndefined()
  })
})

describe('the path that cannot be taken', () => {
  // FINDING, pinned. `canCreateSetup` begins with `trainingConfig.requiresTraining`,
  // and the Create button is `:disabled="loading || !canCreateSetup"`. So
  // unticking "requires training" on page 1 disables the only button that
  // would submit it, and the `else` branch of `createTrainingSetup` -- the one
  // that sets `requires_training: false` -- can never run.
  //
  // A wizard whose first question is "does this tool require training?" cannot
  // record the answer "no".
  it('stops demanding a training step once the answer is "none required"', async () => {
    // `canProceedToNextStep` case 2 used to check every step regardless of
    // `requiresTraining`, so answering "no" on page 1 still insisted on a
    // filled-in training step for a tool being configured to need none.
    const w = modal()
    await w.find('input[type="checkbox"]').setValue(false)
    await nextTick()
    await buttonNamed(w, 'Next').trigger('click')
    await nextTick()

    expect(buttonNamed(w, 'Next').attributes('disabled')).toBeUndefined()
  })

  it('records "no training required" instead of disabling the button that says so', async () => {
    // `canCreateSetup` used to open with `trainingConfig.requiresTraining &&`,
    // which disabled the only button that would submit that answer -- so the
    // branch of `createTrainingSetup` that clears the flag could never run. A
    // wizard whose first question is "does this tool require training?" could
    // not record "no".
    const w = modal()
    await w.find('input[type="checkbox"]').setValue(false)
    await nextTick()
    await buttonNamed(w, 'Next').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Next').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Next').trigger('click')
    await nextTick()

    expect(buttonNamed(w, 'Create Training Setup').attributes('disabled')).toBeUndefined()
    await buttonNamed(w, 'Create Training Setup').trigger('click')
    await flushPromises()

    expect(mocks.updateTool).toHaveBeenCalledWith('tool-1', { requires_training: false })
    expect(mocks.createTrainingStep).not.toHaveBeenCalled()
    expect(w.emitted('created')).toHaveLength(1)
  })
})

describe('prerequisites configured in the wizard', () => {
  // FINDING, pinned. Prerequisites are posted to `/training/prerequisites`
  // with an object body. The server declares
  // `POST /training/steps/{step_id}/prerequisites` taking a bare `Json<Uuid>`,
  // so this route does not exist -- and the result is not checked either, so
  // the wizard reports success regardless. Whatever is configured on page 3 is
  // discarded.
  it('reports a refused prerequisite instead of claiming the setup worked', async () => {
    const w = modal()
    await fillFirstStep(w)
    await buttonNamed(w, '+ Add Another Step').trigger('click')
    await nextTick()
    await stepNameInputs(w)[1].setValue('Operation')
    await descriptionInputs(w)[1].setValue('Speeds and feeds')
    await nextTick()

    // Page 3: make step 2 depend on step 1.
    await buttonNamed(w, 'Next').trigger('click')
    await nextTick()
    const prereqBoxes = w.findAll('input[type="checkbox"]')
    expect(prereqBoxes.length).toBeGreaterThan(0)
    await prereqBoxes[prereqBoxes.length - 1].setValue(true)
    await nextTick()

    await buttonNamed(w, 'Next').trigger('click')
    await nextTick()
    mocks.addTrainingPrerequisite.mockResolvedValue({
      success: false,
      error: 'Would form a cycle',
    })
    await buttonNamed(w, 'Create Training Setup').trigger('click')
    await flushPromises()

    expect(mocks.addTrainingPrerequisite).toHaveBeenCalled()
    expect(w.find('.error-message').text()).toContain('Would form a cycle')
    expect(w.emitted('created')).toBeUndefined()
  })

  it('links them, by the ids the server gave each created step', async () => {
    const w = modal()
    await fillFirstStep(w)
    await buttonNamed(w, '+ Add Another Step').trigger('click')
    await nextTick()
    await stepNameInputs(w)[1].setValue('Operation')
    await descriptionInputs(w)[1].setValue('Speeds and feeds')
    await nextTick()
    await buttonNamed(w, 'Next').trigger('click')
    await nextTick()
    const boxes = w.findAll('input[type="checkbox"]')
    await boxes[boxes.length - 1].setValue(true)
    await nextTick()
    await buttonNamed(w, 'Next').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Create Training Setup').trigger('click')
    await flushPromises()

    expect(mocks.addTrainingPrerequisite.mock.calls[0][0]).toEqual({
      training_step_id: 'step-2',
      prerequisite_step_id: 'step-1',
    })
    expect(w.emitted('created')).toHaveLength(1)
  })

  it('says there is nothing to configure with a single step', async () => {
    const w = modal()
    await fillFirstStep(w)
    await buttonNamed(w, 'Next').trigger('click')
    await nextTick()
    expect(w.find('.info-message').exists()).toBe(true)
  })
})

describe('closing', () => {
  it('closes on the overlay and the header button and Cancel', async () => {
    const w = modal()
    await w.find('.modal-overlay').trigger('click')
    await w.find('.close-btn').trigger('click')
    await buttonNamed(w, 'Cancel').trigger('click')
    expect(w.emitted('close')).toHaveLength(3)
  })

  it('does not close when the content is clicked', async () => {
    const w = modal()
    await w.find('.modal-content').trigger('click')
    expect(w.emitted('close')).toBeUndefined()
  })
})
