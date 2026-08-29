// Tier 2: PrerequisitesModal.
//
// Both of its write paths address the wrong thing, and neither says so.
//
// Adding posts to `/training/prerequisites` with a JSON object. The server has
// no such route: it declares `POST /training/steps/{step_id}/prerequisites`
// taking a bare `Json<Uuid>` (api/training.rs:130). Wrong path, and a body
// shape the right path would reject anyway.
//
// Removing sends a *TrainingStep* id where the server wants a
// `training_prerequisites` row id. `get_prerequisites` returns
// `Vec<TrainingStep>`, so the client never receives a link id at all; the
// delete then matches zero rows, and `remove_training_prerequisite`
// (database.rs:1073) discards the row count -- so the server answers 200 and
// the prerequisite is still there.
//
// That last one is a sibling of the `remove_tool_trainer` defect fixed earlier
// on this branch, which had exactly the same discarded `execute()` result.
//
// What this spec does NOT prove: what the server answers to either request.
// Tier 4 owns that. What is asserted here is the URL and the payload this
// component produces, and the fact that it announces success either way.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({
  getTrainingPrerequisites: vi.fn(),
  addTrainingPrerequisite: vi.fn(),
  removeTrainingPrerequisite: vi.fn(),
}))
vi.mock('@/utils/api', () => ({ trainingApi: mocks }))

import PrerequisitesModal from '@/components/PrerequisitesModal.vue'
import { AssessmentType, type TrainingStep } from '@/types/training'

function step(id: string, over: Partial<TrainingStep> = {}): TrainingStep {
  return {
    id,
    tool_id: '3f2a91b0-1111-2222-3333-444455556666',
    step_number: 1,
    step_name: `Step ${id}`,
    description: `Description for ${id}`,
    assessment_type: AssessmentType.Practical,
    is_active: true,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...over,
  }
}

const SUBJECT = step('s1', { step_number: 3, step_name: 'Cutting' })
const ALL = [SUBJECT, step('s2', { step_number: 1 }), step('s3', { step_number: 2 })]

let confirmResult = true

beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset()
  mocks.getTrainingPrerequisites.mockResolvedValue({ success: true, data: [] })
  mocks.addTrainingPrerequisite.mockResolvedValue({ success: true })
  mocks.removeTrainingPrerequisite.mockResolvedValue({ success: true })
  confirmResult = true
  vi.stubGlobal(
    'confirm',
    vi.fn(() => confirmResult)
  )
})

async function modal(prereqs: TrainingStep[] = [], subject: TrainingStep | null = SUBJECT) {
  mocks.getTrainingPrerequisites.mockResolvedValue({ success: true, data: prereqs })
  const w = mount(PrerequisitesModal, { props: { step: subject, allSteps: ALL } })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof modal>>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}

describe('loading the current prerequisites', () => {
  it("asks for the subject step's prerequisites", async () => {
    await modal()
    expect(mocks.getTrainingPrerequisites).toHaveBeenCalledWith('s1')
  })

  // FINDING, pinned. `watch(..., { immediate: true })` and `onMounted` both
  // call `loadPrerequisites`, so every open fires the request twice. Harmless
  // on a fast network and not harmless on a slow one, where the two responses
  // race and the later one wins regardless of which was asked for first.
  it('asks twice on every open', async () => {
    await modal()
    expect(
      mocks.getTrainingPrerequisites,
      'the duplicate load is gone -- if the onMounted call was removed, this ' + 'should be 1'
    ).toHaveBeenCalledTimes(2)
  })

  it('asks for nothing when there is no step', async () => {
    await modal([], null)
    expect(mocks.getTrainingPrerequisites).not.toHaveBeenCalled()
  })

  it('says so when there are none', async () => {
    expect((await modal()).find('.empty-state').exists()).toBe(true)
  })

  it('lists each prerequisite with its name and description', async () => {
    const w = await modal([step('s2', { step_name: 'Safety induction' })])
    expect(w.find('.prerequisite-item').text()).toContain('Safety induction')
    expect(w.find('.prerequisite-item').text()).toContain('Description for s2')
  })

  it('keeps the previous list, and says nothing, when a reload is refused', async () => {
    const w = await modal([step('s2')])
    expect(w.findAll('.prerequisite-item')).toHaveLength(1)

    mocks.getTrainingPrerequisites.mockResolvedValue({ success: false, error: 'Forbidden' })
    await w.setProps({ step: step('s1', { step_number: 4 }) })
    await flushPromises()

    expect(w.findAll('.prerequisite-item')).toHaveLength(1)
    expect(w.text()).not.toContain('Forbidden')
  })
})

describe('what may be chosen as a prerequisite', () => {
  it('offers every other step', async () => {
    const w = await modal()
    const offered = w
      .findAll('#prerequisite option')
      .map((o) => o.attributes('value'))
      .filter((v) => v !== '')
    expect(offered).toEqual(['s2', 's3'])
  })

  it('never offers the step itself', async () => {
    const w = await modal()
    const offered = w.findAll('#prerequisite option').map((o) => o.attributes('value'))
    expect(offered).not.toContain('s1')
  })

  it('drops a step that is already a prerequisite', async () => {
    const w = await modal([step('s2')])
    const offered = w
      .findAll('#prerequisite option')
      .map((o) => o.attributes('value'))
      .filter((v) => v !== '')
    expect(offered).toEqual(['s3'])
  })

  // FINDING, pinned. The filter removes the step itself and its existing
  // prerequisites, and nothing else. Step A may be made to require B while B
  // already requires A, and the modal offers it without comment. Nothing in
  // the client detects the cycle; whether the server does is Tier 6's.
  it('offers a step that already requires the one being edited', async () => {
    // `allSteps` carries no prerequisite relationships, so the component has
    // nothing to detect a cycle *with* -- which is the finding: the prop it
    // would need is not passed.
    const w = await modal()
    const offered = w
      .findAll('#prerequisite option')
      .map((o) => o.attributes('value'))
      .filter((v) => v !== '')
    expect(
      offered,
      'the picker now excludes something on cycle grounds -- if that was added, ' +
        'this test should assert which'
    ).toEqual(['s2', 's3'])
  })

  // FINDING, pinned. `getToolName` is a stub with a comment saying so, and it
  // ships: the picker labels each option "Tool 3f2a91b0..." followed by the
  // step. An operator choosing a prerequisite is shown a truncated UUID where
  // the tool's name should be.
  it('labels each option with a truncated UUID instead of a tool name', async () => {
    const w = await modal()
    const label = w.findAll('#prerequisite option')[1].text()
    expect(
      label,
      'the tool name is now resolved -- if a tools lookup was added, delete ' + 'this test'
    ).toContain('Tool 3f2a91b0...')
  })
})

describe('adding a prerequisite', () => {
  it('does nothing until one is chosen', async () => {
    const w = await modal()
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(mocks.addTrainingPrerequisite).not.toHaveBeenCalled()
  })

  // FINDING, pinned. The payload names the subject step and the prerequisite
  // as an object. The server's route is
  // `POST /training/steps/{step_id}/prerequisites` and its body is a bare
  // `Json<Uuid>` -- so this request is addressed to a path that does not exist,
  // carrying a shape the real path would reject. Adding a prerequisite cannot
  // work.
  it('sends an object to an endpoint that takes a bare uuid', async () => {
    const w = await modal()
    await w.find('#prerequisite').setValue('s2')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(
      mocks.addTrainingPrerequisite.mock.calls[0][0],
      'the payload changed -- if the request was aligned with ' +
        'POST /training/steps/{step_id}/prerequisites, delete this test'
    ).toEqual({ training_step_id: 's1', prerequisite_step_id: 's2' })
  })

  it('reloads, clears the picker and announces the change on success', async () => {
    const w = await modal()
    mocks.getTrainingPrerequisites.mockClear()
    await w.find('#prerequisite').setValue('s2')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(mocks.getTrainingPrerequisites).toHaveBeenCalledTimes(1)
    expect((w.find('#prerequisite').element as HTMLSelectElement).value).toBe('')
    expect(w.emitted('updated')).toHaveLength(1)
  })

  it("shows the server's reason for a refusal and does not announce a change", async () => {
    mocks.addTrainingPrerequisite.mockResolvedValue({ success: false, error: 'Would form a cycle' })
    const w = await modal()
    await w.find('#prerequisite').setValue('s2')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.text()).toContain('Would form a cycle')
    expect(w.emitted('updated')).toBeUndefined()
  })
})

describe('removing a prerequisite', () => {
  it('asks first, and does nothing if the answer is no', async () => {
    confirmResult = false
    const w = await modal([step('s2')])
    await buttonNamed(w, 'Remove').trigger('click')
    await flushPromises()
    expect(mocks.removeTrainingPrerequisite).not.toHaveBeenCalled()
  })

  // FINDING, pinned, and the one with the worst failure mode. The id sent is
  // the *TrainingStep's*, because `get_prerequisites` returns
  // `Vec<TrainingStep>` and the link row's id never reaches the client. The
  // server deletes `training_prerequisites WHERE id = <that>`, matches nothing,
  // discards the row count, and answers 200. So the component reloads, emits
  // `updated`, shows no error -- and the prerequisite is still on screen.
  //
  // Sibling of the `remove_tool_trainer` defect fixed earlier on this branch:
  // same discarded `execute()` result, different function.
  it('sends a step id where the server expects a link id, and calls it a success', async () => {
    const w = await modal([step('s2')])
    // The server would delete nothing; the mock stands in for the 200 it
    // answers regardless.
    await buttonNamed(w, 'Remove').trigger('click')
    await flushPromises()

    expect(
      mocks.removeTrainingPrerequisite.mock.calls[0][0],
      'the id sent changed -- if the endpoint started returning link ids, this ' +
        'test should assert the link id instead'
    ).toBe('s2')
    expect(w.emitted('updated')).toHaveLength(1)
    expect(w.text()).not.toContain('Failed')
  })

  it('reloads afterwards, so a removal that did nothing is silently undone on screen', async () => {
    const w = await modal([step('s2')])
    mocks.getTrainingPrerequisites.mockClear()
    // The reload returns the row again, which is what the server would do.
    mocks.getTrainingPrerequisites.mockResolvedValue({ success: true, data: [step('s2')] })
    await buttonNamed(w, 'Remove').trigger('click')
    await flushPromises()

    expect(mocks.getTrainingPrerequisites).toHaveBeenCalledTimes(1)
    expect(w.findAll('.prerequisite-item')).toHaveLength(1)
    expect(w.find('.error, .error-message').exists()).toBe(false)
  })

  it("shows the server's reason when a removal is refused", async () => {
    mocks.removeTrainingPrerequisite.mockResolvedValue({ success: false, error: 'Not permitted' })
    const w = await modal([step('s2')])
    await buttonNamed(w, 'Remove').trigger('click')
    await flushPromises()

    expect(w.text()).toContain('Not permitted')
    expect(w.emitted('updated')).toBeUndefined()
  })
})

describe('the flow visualisation', () => {
  it('orders the flow by step number, while the list above does not', async () => {
    // Recorded: the list renders `prerequisites` in server order and the flow
    // renders `sortedPrerequisites`. Two orderings of the same data on one
    // screen.
    const w = await modal([
      step('s3', { step_number: 9, step_name: 'Later' }),
      step('s2', { step_number: 1, step_name: 'Earlier' }),
    ])
    await nextTick()

    const listOrder = w.findAll('.prerequisite-item').map((n) => n.text())
    expect(listOrder[0]).toContain('Later')

    const flowOrder = w.findAll('.flow-item').map((n) => n.text())
    expect(flowOrder[0]).toContain('Earlier')
  })
})

describe('closing', () => {
  it('closes on the overlay and the header button', async () => {
    const w = await modal()
    await w.find('.modal-overlay').trigger('click')
    await w.find('.close-btn').trigger('click')
    expect(w.emitted('close')).toHaveLength(2)
  })

  it('does not close when the content is clicked', async () => {
    const w = await modal()
    await w.find('.modal-content').trigger('click')
    expect(w.emitted('close')).toBeUndefined()
  })
})
