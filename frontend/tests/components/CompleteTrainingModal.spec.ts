// Tier 2: CompleteTrainingModal.
//
// An instructor signs off a training step. The modal's whole validation story
// hangs off one field:
//
//     if (props.step.passing_score && form.value.passed) { ... }
//
// and `passing_score` does not exist. `training_steps` has no such column
// (schema.rs), `UpdateTrainingStepRequest` has no such field
// (api/training.rs:38), and the server therefore never sends one -- it is
// invented by `TrainingStep` in `types/training.ts`. See the
// EditTrainingStepModal spec for the full field-by-field comparison.
//
// So against a real server `step.passing_score` is always `undefined`, and
// with it: the score input never renders, the pass/fail feedback never
// renders, the minimum-score guard in `canSubmit` never fires, and the silent
// pass-to-fail flip in `completeTraining` never runs. Everything below is
// asserted twice -- once with the field absent, which is production, and once
// with it present, which is what the code was written for.
//
// What this spec does NOT prove: that the server stores or checks a score.
// It cannot; there is nowhere to put one.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const mocks = vi.hoisted(() => ({ completeTrainingSession: vi.fn() }))
vi.mock('@/utils/api', () => ({ trainingApi: mocks }))

import CompleteTrainingModal from '@/components/CompleteTrainingModal.vue'
import { UserRole, type User } from '@/types'
import { AssessmentType, type TrainingStep } from '@/types/training'

const TRAINEE: User = {
  id: 'trainee-1',
  username: 'ada',
  email: 'ada@example.test',
  full_name: 'Ada Lovelace',
  is_active: true,
  role: UserRole.Member,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  profile: {},
  meta: {},
}

/** What the server actually sends: no passing_score, no expiry_days. */
function serverStep(over: Partial<TrainingStep> = {}): TrainingStep {
  return {
    id: 'step-1',
    tool_id: 'tool-1',
    step_number: 1,
    step_name: 'Lathe safety',
    description: 'Guards, speeds, and what not to wear.',
    assessment_type: AssessmentType.Practical,
    is_active: true,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...over,
  }
}

beforeEach(() => {
  mocks.completeTrainingSession.mockReset()
  mocks.completeTrainingSession.mockResolvedValue({ success: true })
})

const modal = (step: TrainingStep = serverStep()) =>
  mount(CompleteTrainingModal, { props: { step, user: TRAINEE } })

type Wrapper = ReturnType<typeof modal>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}

const submit = (w: Wrapper) => w.find('button[type="submit"]')
const sent = () => mocks.completeTrainingSession.mock.calls[0][1] as Record<string, unknown>

describe('against the step the server actually sends', () => {
  // FINDING, pinned. With `passing_score` absent -- which is every real
  // response -- the score input does not render at all. The instructor has no
  // way to record an assessment score, on a modal built around one.
  it('offers no score field, because the field it keys off does not exist', () => {
    const w = modal()
    expect(
      w.find('#score').exists(),
      'the score input now renders -- if passing_score became a real server ' +
        'field, this spec should be rewritten around it'
    ).toBe(false)
    expect(w.find('.score-feedback').exists()).toBe(false)
  })

  it('needs only notes to submit', async () => {
    const w = modal()
    expect(submit(w).attributes('disabled')).toBeDefined()

    await w.find('#notes').setValue('Confident on the guard interlock.')
    expect(submit(w).attributes('disabled')).toBeUndefined()
  })

  it('rejects notes that are only whitespace', async () => {
    const w = modal()
    await w.find('#notes').setValue('   ')
    expect(submit(w).attributes('disabled')).toBeDefined()
  })

  it('sends a pass with the notes and no score', async () => {
    const w = modal()
    await w.find('#notes').setValue('Confident on the guard interlock.')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(mocks.completeTrainingSession.mock.calls[0][0]).toBe('trainee-1')
    expect(sent()).toEqual({
      training_step_id: 'step-1',
      passed: true,
      assessment_score: undefined,
      notes: 'Confident on the guard interlock.',
    })
  })

  it('sends a fail when the pass box is unticked', async () => {
    const w = modal()
    await w.find('.checkbox').setValue(false)
    await w.find('#notes').setValue('Needs another session on speeds.')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(sent().passed).toBe(false)
  })

  it('summarises what passing means, and shows nothing when failing', async () => {
    const w = modal()
    expect(w.find('.training-summary').exists()).toBe(true)

    await w.find('.checkbox').setValue(false)
    expect(w.find('.training-summary').exists()).toBe(false)
  })

  it('mentions an expiry only when the step has one, which it never does', () => {
    // `expiry_days` is the same story as `passing_score`: the server column is
    // `expires_after_days` and this name is never populated. So the "expires
    // after N days" line in the summary is unreachable too.
    expect(modal().find('.training-summary').text()).not.toContain('day')
  })
})

describe('against the step the code was written for', () => {
  const scored = () => serverStep({ passing_score: 80, expiry_days: 365 })

  it('shows the score field and the expiry line once the fields are present', () => {
    const w = modal(scored())
    expect(w.find('#score').exists()).toBe(true)
    expect(w.find('.training-summary').text()).toContain('365')
  })

  it('will not submit a pass below the minimum', async () => {
    const w = modal(scored())
    await w.find('#notes').setValue('Scraped through the written part.')
    await w.find('#score').setValue('79')
    expect(submit(w).attributes('disabled')).toBeDefined()

    await w.find('#score').setValue('80')
    expect(submit(w).attributes('disabled')).toBeUndefined()
  })

  it('carries the recorded score to the server', async () => {
    const w = modal(scored())
    await w.find('#notes').setValue('Comfortable at speed.')
    await w.find('#score').setValue('92')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(sent()).toEqual({
      training_step_id: 'step-1',
      passed: true,
      assessment_score: 92,
      notes: 'Comfortable at speed.',
    })
  })

  it('will submit any score at all when the box says they failed', async () => {
    const w = modal(scored())
    await w.find('.checkbox').setValue(false)
    await w.find('#notes').setValue('Did not meet the standard.')
    await w.find('#score').setValue('12')
    expect(submit(w).attributes('disabled')).toBeUndefined()
  })

  it('tells the instructor whether the score clears the bar', async () => {
    const w = modal(scored())
    await w.find('#score').setValue('90')
    expect(w.find('.score-pass').exists()).toBe(true)

    await w.find('#score').setValue('50')
    expect(w.find('.score-pass').exists()).toBe(false)
    expect(w.find('.score-feedback').exists()).toBe(true)
  })

  // FINDING, pinned. `completeTraining` re-checks the score and, if it falls
  // short, sets `form.passed = false` and submits anyway -- recording a
  // failure the instructor did not choose, with no confirmation and nothing on
  // screen to say the outcome was changed.
  //
  // `canSubmit` disables the button in exactly that case, so in a browser the
  // flip is unreachable; `trigger('submit')` bypasses the disabled attribute,
  // which is what makes it visible here. Two guards for one rule, and the
  // second one silently rewrites the answer instead of refusing it.
  it('silently records a failure when a short-scored pass is submitted anyway', async () => {
    const w = modal(scored())
    await w.find('#notes').setValue('Scraped through the written part.')
    await w.find('#score').setValue('79')

    expect(submit(w).attributes('disabled')).toBeDefined()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(
      sent().passed,
      'a short-scored pass is no longer flipped -- if the second guard was ' +
        'changed to refuse rather than rewrite, delete this test'
    ).toBe(false)
    expect(w.find('.error-message').exists()).toBe(false)
  })
})

describe('what happens after the request', () => {
  it('announces completion when the server agrees', async () => {
    const w = modal()
    await w.find('#notes').setValue('Signed off.')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.emitted('completed')).toHaveLength(1)
    expect(w.find('.error-message').exists()).toBe(false)
  })

  it("shows the server's reason for a refusal, and does not announce completion", async () => {
    mocks.completeTrainingSession.mockResolvedValue({
      success: false,
      error: 'No session in progress',
    })
    const w = modal()
    await w.find('#notes').setValue('Signed off.')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error-message').text()).toBe('No session in progress')
    expect(w.emitted('completed')).toBeUndefined()
  })

  it('falls back to a generic message when a refusal carries none', async () => {
    mocks.completeTrainingSession.mockResolvedValue({ success: false })
    const w = modal()
    await w.find('#notes').setValue('Signed off.')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error-message').text()).toBe('Failed to complete training session')
  })

  // The same axios-prose finding as StartTrainingModal and EditTrainerModal:
  // `err.message` is the status restated, and `err.response.data.error` is
  // discarded. Recorded once per component because the fix is per-component.
  it('shows axios prose rather than the server body on a thrown error', async () => {
    mocks.completeTrainingSession.mockRejectedValue(
      Object.assign(new Error('Request failed with status code 409'), {
        response: { data: { error: 'That step is already complete' } },
      })
    )
    const w = modal()
    await w.find('#notes').setValue('Signed off.')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error-message').text()).toBe('Request failed with status code 409')
    expect(w.text()).not.toContain('already complete')
  })

  it('re-enables the submit button whether the request resolved or rejected', async () => {
    mocks.completeTrainingSession.mockRejectedValue(new Error('down'))
    const w = modal()
    await w.find('#notes').setValue('Signed off.')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(submit(w).attributes('disabled')).toBeUndefined()
  })
})

describe('closing', () => {
  it('closes on the overlay, the header button and Cancel', async () => {
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
