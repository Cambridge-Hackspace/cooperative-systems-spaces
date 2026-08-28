// Tier 2: CreateTrainingStepModal.
//
// A form modal, and its one piece of real logic is a conditional field: the
// passing score is hidden for an observation-only assessment, because there is
// nothing to score. The condition is written as a string comparison —
//
//     v-if="form.assessment_type !== 'observation_only'"
//
// — against a value that comes from the `AssessmentType` enum. The two agree
// today. If the enum's wire value ever changes, that condition silently stops
// matching and the field reappears for observation-only training, asking an
// instructor to enter a score for something that has none. The spec asserts
// through the enum member rather than the literal, so a rename fails here.
//
// The rest is modal mechanics: an overlay that closes on a click, content that
// does not, and a submit path that reports refusals.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

// `vi.hoisted` and a direct reference, not a forwarding arrow. `vi.mock` is
// hoisted above every top-level `const`, so a factory that closes over an
// ordinary binding throws "Cannot access 'createTrainingStep' before
// initialization" from inside the *component's* import -- and a forwarding
// `(...a) => createTrainingStep(...a)` wrapper, the other way to defer it,
// returns `any`.
const mocks = vi.hoisted(() => ({ createTrainingStep: vi.fn() }))
const createTrainingStep = mocks.createTrainingStep

vi.mock('@/utils/api', () => ({
  trainingApi: { createTrainingStep: mocks.createTrainingStep },
}))

import CreateTrainingStepModal from '@/components/CreateTrainingStepModal.vue'
import { AssessmentType, type Tool } from '@/types'

const TOOLS = [
  { id: 't1', name: 'Lathe' },
  { id: 't2', name: 'Laser cutter' },
] as unknown as Tool[]

// A block body, not `() => createTrainingStep.mockReset()`.
//
// `mockReset()` returns the mock, and vitest treats a function returned from
// `beforeEach` as a teardown callback -- so the concise form makes vitest CALL
// the mock during teardown. With a throwing implementation that is an unhandled
// rejection attributed to the test; with a pending one, vitest awaits the
// returned promise and the hook times out at exactly 10s. Two failures, one
// stray return value, and neither message mentions the mock.
beforeEach(() => {
  createTrainingStep.mockReset()
})

const modal = () => mount(CreateTrainingStepModal, { props: { tools: TOOLS } })

// Throws rather than returning undefined. Under the non-strict base tsconfig a
// `!` at the call site is a no-op the linter correctly rejects, and without one
// a missing button surfaces as "Cannot read properties of undefined (reading
// 'trigger')" instead of naming the label it looked for.
function buttonNamed(w: ReturnType<typeof modal>, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}

async function fillMinimum(w: ReturnType<typeof modal>) {
  await w.find('#tool_id').setValue('t1')
  await w.find('#step_name').setValue('Safety orientation')
}

describe('the tool list', () => {
  it('offers every tool plus an explicit prompt', () => {
    const options = modal().find('#tool_id').findAll('option')
    expect(options).toHaveLength(TOOLS.length + 1)
    expect(options[0].text()).toBe('Select a tool')
    expect(options.slice(1).map((o) => o.text())).toEqual(['Lathe', 'Laser cutter'])
  })
})

describe('the passing score field', () => {
  it('is offered for an assessment that can be scored', async () => {
    const w = modal()
    await w.find('#assessment_type').setValue(AssessmentType.Practical)
    expect(w.find('#passing_score').exists()).toBe(true)
  })

  it('is hidden for observation-only training', async () => {
    // Asserted through the enum member, not the string. The template compares
    // against a literal; if the enum's value is renamed the two stop agreeing
    // and an instructor is asked to score something unscoreable.
    const w = modal()
    await w.find('#assessment_type').setValue(AssessmentType.ObservationOnly)
    expect(
      w.find('#passing_score').exists(),
      'observation-only training has nothing to score, so the field must not ' +
        'be asked for. The template compares against a string literal — if this ' +
        'fails, AssessmentType.ObservationOnly no longer equals it.'
    ).toBe(false)
  })

  it.each([AssessmentType.Practical, AssessmentType.Written, AssessmentType.Both])(
    'is offered for %s',
    async (type) => {
      const w = modal()
      await w.find('#assessment_type').setValue(type)
      expect(w.find('#passing_score').exists()).toBe(true)
    }
  )
})

describe('submitting', () => {
  it('sends what the form holds and emits the created step', async () => {
    const created = { id: 'step-1', step_name: 'Safety orientation' }
    createTrainingStep.mockResolvedValue({ success: true, data: created })

    const w = modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(createTrainingStep).toHaveBeenCalledTimes(1)
    const sent = createTrainingStep.mock.calls[0][0] as Record<string, unknown>
    expect(sent.tool_id).toBe('t1')
    expect(sent.step_name).toBe('Safety orientation')
    // `v-model.number`. Without the modifier this is the string "1", and the
    // server takes an integer — a mismatch the form cannot show.
    expect(sent.step_number).toBe(1)
    expect(sent.is_active).toBe(true)

    expect(w.emitted('created')?.[0]).toEqual([created])
  })

  it('reports a refusal in the server’s own words', async () => {
    createTrainingStep.mockResolvedValue({ success: false, error: 'Step number already used' })
    const w = modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.text()).toContain('Step number already used')
    expect(w.emitted('created')).toBeUndefined()
  })

  it('reports a thrown failure too', async () => {
    // `mockImplementation`, not `mockRejectedValue`: the latter builds the
    // rejected promise when the mock is configured rather than when it is
    // called, so vitest sees an unhandled rejection before the component has
    // had a chance to catch anything. The body returns a rejected promise
    // rather than being `async` and throwing -- same observable result, and it
    // is a real rejection rather than a synchronous throw, which is what an
    // http client actually does.
    createTrainingStep.mockImplementation(() => Promise.reject(new Error('Network down')))
    const w = modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(w.text()).toContain('Network down')
  })

  it('disables submit while the request is in flight', async () => {
    // A promise that never settles, and therefore no `flushPromises` here --
    // flushing waits on a macrotask that the pending request keeps alive, and
    // the test times out instead of asserting. `nextTick` is enough: the
    // component sets `loading` synchronously before awaiting.
    // Settled at the end rather than left pending: a promise that never
    // resolves keeps the request alive past the test, and vitest waits it out
    // instead of reporting the assertion.
    let release!: (v: unknown) => void
    createTrainingStep.mockImplementation(
      () =>
        new Promise((resolve) => {
          release = resolve
        })
    )

    const w = modal()
    await fillMinimum(w)
    await w.find('form').trigger('submit')
    await nextTick()

    expect(
      w.find('button[type="submit"]').attributes('disabled'),
      'without this a double click creates two training steps'
    ).toBeDefined()

    release({ success: true, data: { id: 'step-1' } })
    await flushPromises()
    expect(w.find('button[type="submit"]').attributes('disabled')).toBeUndefined()
  })
})

describe('dismissing', () => {
  it('closes when the overlay is clicked', async () => {
    const w = modal()
    await w.find('.modal-overlay').trigger('click')
    expect(w.emitted('close')).toHaveLength(1)
  })

  it('does not close when the form itself is clicked', async () => {
    // `@click.stop` on the content. Without it every click inside the form
    // bubbles to the overlay and closes the modal mid-typing.
    const w = modal()
    await w.find('.modal-content').trigger('click')
    expect(w.emitted('close')).toBeUndefined()
  })

  it('closes from both the header button and Cancel', async () => {
    const w = modal()
    await w.find('.close-btn').trigger('click')
    await buttonNamed(w, 'Cancel').trigger('click')
    expect(w.emitted('close')).toHaveLength(2)
  })
})
