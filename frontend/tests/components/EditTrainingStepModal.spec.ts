// Tier 2: EditTrainingStepModal.
//
// This form edits seven fields. The server's update request declares seven
// fields too, and only three of them are the same ones:
//
//   sent by this form   server's UpdateTrainingStepRequest   training_steps column
//   -----------------   ---------------------------------   ---------------------
//   step_name           step_name                            step_name
//   description         description                          description
//   assessment_type     assessment_type                      assessment_type
//   step_number         --                                   step_number
//   passing_score       --                                   --
//   expires_after_days         expires_after_days                   expires_after_days
//   is_active           --                                   --
//   --                  training_materials_url               training_materials_url
//   --                  requires_assessment                  requires_assessment
//   --                  duration_minutes                     duration_minutes
//
// (api/training.rs:38, models/training.rs:179, schema.rs `training_steps`.)
//
// Serde ignores unknown fields, so the four on the left with no match are
// dropped in silence and the request answers 200. `passing_score` and
// `is_active` have no column at all -- the checkbox labelled "Active (visible
// to users)" controls nothing that exists. `step_number` is real but is only
// updatable through a separate endpoint, `update_training_step_position`.
//
// What this spec does NOT prove: that the server answers 200 to any of it, or
// that the row is unchanged afterwards. Tier 2's remit is the bytes this
// component puts on the wire, and the divergence is asserted there. Tiers 4
// and 6 own the response and the row.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({ updateTrainingStep: vi.fn(), deleteTrainingStep: vi.fn() }))
vi.mock('@/utils/api', () => ({ trainingApi: mocks }))

import EditTrainingStepModal from '@/components/EditTrainingStepModal.vue'
import type { Tool } from '@/types'
import { AssessmentType, type TrainingStep } from '@/types/training'

const TOOL = { id: 'tool-1', name: 'Lathe' } as unknown as Tool

// The three field names the server's update request actually reads. Written out
// rather than derived, so this is a claim about the contract and not a
// restatement of whatever the component happens to send.
const SERVER_READS = ['step_name', 'description', 'assessment_type']
const SERVER_IGNORES = ['step_number', 'passing_score', 'expires_after_days', 'is_active']

function step(over: Partial<TrainingStep> = {}): TrainingStep {
  return {
    id: 'step-1',
    tool_id: 'tool-1',
    step_number: 2,
    step_name: 'Lathe safety',
    description: 'Guards, speeds, and what not to wear.',
    assessment_type: AssessmentType.Practical,
    passing_score: 80,
    expires_after_days: 365,
    is_active: true,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...over,
  }
}

let confirmResult = true

beforeEach(() => {
  mocks.updateTrainingStep.mockReset()
  mocks.deleteTrainingStep.mockReset()
  mocks.updateTrainingStep.mockImplementation((_id: string, body: unknown) =>
    Promise.resolve({ success: true, data: { ...step(), ...(body as object) } })
  )
  mocks.deleteTrainingStep.mockResolvedValue({ success: true })
  confirmResult = true
  vi.stubGlobal(
    'confirm',
    vi.fn(() => confirmResult)
  )
})

const modal = (s: TrainingStep | null = step(), existingSteps: TrainingStep[] = []) =>
  mount(EditTrainingStepModal, { props: { step: s, tool: TOOL, existingSteps } })

type Wrapper = ReturnType<typeof modal>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}

const sent = () => mocks.updateTrainingStep.mock.calls[0][1] as Record<string, unknown>

describe('what the form starts with', () => {
  it('loads every field from the step it was given', () => {
    const w = modal()
    expect((w.find('#step_number').element as HTMLInputElement).value).toBe('2')
    expect((w.find('#step_name').element as HTMLInputElement).value).toBe('Lathe safety')
    expect((w.find('#description').element as HTMLTextAreaElement).value).toBe(
      'Guards, speeds, and what not to wear.'
    )
    expect((w.find('#assessment_type').element as HTMLSelectElement).value).toBe('practical')
    expect((w.find('#passing_score').element as HTMLInputElement).value).toBe('80')
    expect((w.find('#expires_after_days').element as HTMLInputElement).value).toBe('365')
    expect((w.find('.checkbox').element as HTMLInputElement).checked).toBe(true)
  })

  it('reloads when a different step is handed in', async () => {
    const w = modal()
    await w.setProps({ step: step({ id: 'step-2', step_name: 'Chuck keys' }) })
    await nextTick()
    expect((w.find('#step_name').element as HTMLInputElement).value).toBe('Chuck keys')
  })

  it('offers every assessment type the enum defines', () => {
    const offered = modal()
      .findAll('#assessment_type option')
      .map((o) => o.attributes('value'))
    expect([...offered].sort()).toEqual([...Object.values(AssessmentType)].sort())
  })

  it('hides the passing score for an observation-only assessment', async () => {
    // Asserted through the enum member rather than the string literal the
    // template compares against, so a change to the wire value fails here.
    const w = modal()
    expect(w.find('#passing_score').exists()).toBe(true)
    await w.find('#assessment_type').setValue(AssessmentType.ObservationOnly)
    expect(w.find('#passing_score').exists()).toBe(false)
  })
})

describe('what the form sends', () => {
  it('addresses the step it was opened on', async () => {
    const w = modal()
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(mocks.updateTrainingStep.mock.calls[0][0]).toBe('step-1')
  })

  it('sends the three fields the server actually reads', async () => {
    const w = modal()
    await w.find('#step_name').setValue('Lathe safety II')
    await w.find('#description').setValue('Now with more guards.')
    await w.find('#assessment_type').setValue(AssessmentType.Both)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(sent().step_name).toBe('Lathe safety II')
    expect(sent().description).toBe('Now with more guards.')
    expect(sent().assessment_type).toBe(AssessmentType.Both)
    for (const key of SERVER_READS) {
      expect(Object.keys(sent()), `${key} must reach the server`).toContain(key)
    }
  })

  // FINDING, pinned. Four of the seven fields this form edits are not declared
  // by `UpdateTrainingStepRequest`, so serde discards them and the request
  // still answers 200. Two of the four -- `passing_score` and `is_active` --
  // have no column in `training_steps` either, so the "Active (visible to
  // users)" checkbox and the passing-score box edit nothing that exists
  // anywhere. `step_number` is real, but only `update_training_step_position`
  // can change it.
  it('sends four fields the server has no field for', async () => {
    const w = modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    for (const key of SERVER_IGNORES) {
      expect(
        Object.keys(sent()),
        `${key} is no longer sent -- if the request was aligned with the ` +
          "server's, delete it from SERVER_IGNORES"
      ).toContain(key)
    }
  })

  // FINDING, pinned, and the other half of the same divergence. The server can
  // update four fields this form never offers, and one of them --
  // `expires_after_days` -- is the field the form's "Expires After (days)" box
  // is plainly meant to be editing under a different name.
  it('now sends the expiry under the name the server reads', async () => {
    // Was `expiry_days`, which `UpdateTrainingStepRequest` does not declare, so
    // the "Expires After (days)" box edited nothing. The field is
    // `expires_after_days` on both sides now.
    const w = modal()
    await w.find('#expires_after_days').setValue('90')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(sent().expires_after_days).toBe(90)
    expect(Object.keys(sent())).not.toContain('expiry_days')
  })

  it('still sends none of the other three fields the server can update', async () => {
    const w = modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    for (const key of ['training_materials_url', 'requires_assessment', 'duration_minutes']) {
      expect(
        Object.keys(sent()),
        `${key} is now sent -- if the form grew a control for it, this test ` +
          'should assert the value rather than its absence'
      ).not.toContain(key)
    }
  })

  // FINDING, pinned. `v-model.number` on an emptied number input yields the
  // empty *string*, not undefined: Vue's `looseToNumber` returns its argument
  // unchanged when `parseFloat` gives NaN, and `parseFloat('')` is NaN. The
  // matching server fields are `Option<i32>`, which cannot deserialise `""`.
  it('sends an empty string, not nothing, when a number field is cleared', async () => {
    const w = modal()
    await w.find('#expires_after_days').setValue('')
    await w.find('#passing_score').setValue('')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(
      sent().expires_after_days,
      'a cleared number field now sends something else -- if it was fixed to ' +
        'send undefined or null, delete this test'
    ).toBe('')
    expect(sent().passing_score).toBe('')
  })

  // FINDING, pinned. The passing score is hidden by `v-if` when the assessment
  // becomes observation-only, but the value stays in the form object and is
  // sent anyway. The field the user can no longer see still travels.
  it('keeps sending a passing score after the assessment stops having one', async () => {
    const w = modal()
    await w.find('#assessment_type').setValue(AssessmentType.ObservationOnly)
    await nextTick()
    expect(w.find('#passing_score').exists()).toBe(false)

    await w.find('form').trigger('submit')
    await flushPromises()

    expect(
      sent().passing_score,
      'the score is now cleared when the field is hidden -- if that was fixed, ' +
        'delete this test'
    ).toBe(80)
  })

  // FINDING, pinned. `existingSteps` is declared as a prop and read nowhere.
  // It is the only thing that could support a duplicate step-number check, and
  // there is none: two steps can be given the same number without a word.
  it('accepts a step number another step already has', async () => {
    const w = modal(step({ step_number: 2 }), [
      step({ id: 'other', step_number: 5, step_name: 'Chuck keys' }),
    ])
    await w.find('#step_number').setValue('5')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(
      w.find('.error-message').exists(),
      'a duplicate step number is now rejected -- if a check was added using ' +
        'the existingSteps prop, this test should assert the message'
    ).toBe(false)
    expect(mocks.updateTrainingStep).toHaveBeenCalledTimes(1)
  })

  it('does nothing at all when there is no step to edit', async () => {
    const w = modal(null)
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(mocks.updateTrainingStep).not.toHaveBeenCalled()
  })
})

describe('after an update', () => {
  it('hands the updated step up and closes', async () => {
    const w = modal()
    await w.find('#step_name').setValue('Lathe safety II')
    await w.find('form').trigger('submit')
    await flushPromises()

    const emitted = w.emitted('step-updated')
    expect(emitted).toHaveLength(1)
    expect((emitted?.[0][0] as TrainingStep).step_name).toBe('Lathe safety II')
    expect(w.emitted('close')).toHaveLength(1)
  })

  it("reports the server's reason and stays open on a refusal", async () => {
    mocks.updateTrainingStep.mockResolvedValue({ success: false, error: 'Step is locked' })
    const w = modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error-message').text()).toBe('Step is locked')
    expect(w.emitted('close')).toBeUndefined()
    expect(w.emitted('step-updated')).toBeUndefined()
  })

  it('treats a success carrying no step as a failure', async () => {
    // `if (response.success && response.data)`, so a 200 with an empty body
    // falls through to the error branch rather than emitting undefined.
    mocks.updateTrainingStep.mockResolvedValue({ success: true })
    const w = modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error-message').text()).toBe('Failed to update training step')
    expect(w.emitted('step-updated')).toBeUndefined()
  })

  it('re-enables both buttons whether the request succeeded or failed', async () => {
    mocks.updateTrainingStep.mockRejectedValue(new Error('down'))
    const w = modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(buttonNamed(w, 'Update Training Step').attributes('disabled')).toBeUndefined()
    expect(buttonNamed(w, 'Delete Step').attributes('disabled')).toBeUndefined()
  })
})

describe('deleting', () => {
  it('asks first, and does nothing if the answer is no', async () => {
    confirmResult = false
    const w = modal()
    await buttonNamed(w, 'Delete Step').trigger('click')
    await flushPromises()

    expect(mocks.deleteTrainingStep).not.toHaveBeenCalled()
    expect(w.emitted('close')).toBeUndefined()
  })

  it('announces the deleted id and closes', async () => {
    const w = modal()
    await buttonNamed(w, 'Delete Step').trigger('click')
    await flushPromises()

    expect(mocks.deleteTrainingStep).toHaveBeenCalledWith('step-1')
    expect(w.emitted('step-deleted')?.[0]).toEqual(['step-1'])
    expect(w.emitted('close')).toHaveLength(1)
  })

  it('reports a refused delete and stays open', async () => {
    mocks.deleteTrainingStep.mockResolvedValue({ success: false, error: 'Step is in use' })
    const w = modal()
    await buttonNamed(w, 'Delete Step').trigger('click')
    await flushPromises()

    expect(w.find('.error-message').text()).toBe('Step is in use')
    expect(w.emitted('close')).toBeUndefined()
  })

  // FINDING, pinned. Both handlers share one `loading` flag, and both buttons
  // read their label off it. Pressing Delete makes the submit button announce
  // "Updating...", which is the one thing that is definitely not happening.
  it('makes the submit button claim it is updating while a delete runs', async () => {
    mocks.deleteTrainingStep.mockReturnValue(new Promise(() => {}))
    const w = modal()
    await buttonNamed(w, 'Delete Step').trigger('click')
    await nextTick()

    expect(
      w.find('button[type="submit"]').text(),
      'the two buttons now track separate flags -- if that was fixed, this ' +
        'test should assert that the submit button says nothing about updating'
    ).toBe('Updating...')
    expect(w.find('.btn-danger').text()).toBe('Deleting...')
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
