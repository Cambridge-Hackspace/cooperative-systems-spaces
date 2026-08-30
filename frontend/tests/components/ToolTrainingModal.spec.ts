// Tier 2: ToolTrainingModal.
//
// The training UI that actually ships -- ToolTrainingCard renders the same flow
// and is imported by nothing.
//
// FIXED, and this was the largest finding of the tier-2 sweep. None of the
// three action buttons could render, for anyone, on any step:
//
//     v-if="stepWithProgress.can_start && !stepWithProgress.progress && ..."
//     v-if="stepWithProgress.progress?.status === 'in_progress' && isInstructor"
//     v-if="stepWithProgress.progress?.status === 'failed'"
//
// `progress` and `can_start` were fields in the TypeScript interface labelled
// "Alias for user_progress" and "Alias for is_available" -- and nothing
// populated an alias. The server serialises its own names
// (models/training.rs:280), so both arrived `undefined` on every response.
// `getStepStatusClass` read them too, so every step was classed `step-locked`
// whatever its real state.
//
// The component now reads `user_progress` and `is_available`, and the aliases
// are deleted from the type so nothing can reach for them again. The tests
// below build steps the way the server actually sends them, which is what they
// did before -- the difference is that the buttons now appear.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({
  getToolTrainingOverview: vi.fn(),
  checkTrainerAuthorization: vi.fn(),
  createTrainingRecord: vi.fn(),
  getTrainingHistory: vi.fn(),
  getUsersForTraining: vi.fn(),
}))
vi.mock('@/utils/api', () => ({
  trainingApi: { getToolTrainingOverview: mocks.getToolTrainingOverview },
  trainerApi: {
    checkTrainerAuthorization: mocks.checkTrainerAuthorization,
    createTrainingRecord: mocks.createTrainingRecord,
  },
  userApi: {
    getTrainingHistory: mocks.getTrainingHistory,
    getUsersForTraining: mocks.getUsersForTraining,
  },
}))

const authState = vi.hoisted<{ user: { id: string; role: string } | null }>(() => ({ user: null }))
vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({
    get user() {
      return authState.user
    },
  }),
}))

import ToolTrainingModal from '@/components/ToolTrainingModal.vue'
import { UserRole, type Tool } from '@/types'
import {
  AssessmentType,
  TrainingStatus,
  type ToolTrainingOverview,
  type TrainingStep,
  type TrainingStepWithProgress,
  type UserTrainingProgress,
} from '@/types/training'

const TOOL = { id: 'tool-1', name: 'Lathe', requires_training: true } as unknown as Tool

function step(n: number, over: Partial<TrainingStep> = {}): TrainingStep {
  return {
    id: `step-${n}`,
    tool_id: 'tool-1',
    step_number: n,
    step_name: `Step ${n}`,
    description: `Description ${n}`,
    assessment_type: AssessmentType.Practical,
    is_active: true,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...over,
  }
}

const progress = (status: TrainingStatus) =>
  ({
    id: 'p1',
    user_id: 'u1',
    training_step_id: 'step-1',
    status,
  }) as unknown as UserTrainingProgress

/** Exactly what the server sends: no `progress`, no `can_start`. */
function serverStep(n: number, over: Partial<TrainingStepWithProgress> = {}) {
  return {
    step: step(n),
    prerequisites: [],
    is_available: true,
    ...over,
  } as TrainingStepWithProgress
}

function overview(over: Partial<ToolTrainingOverview> = {}): ToolTrainingOverview {
  return {
    tool_id: 'tool-1',
    tool_name: 'Lathe',
    steps: [serverStep(1)],
    overall_progress: 0,
    can_access_tool: false,
    ...over,
  }
}

const stubs = {
  StartTrainingModal: { props: ['step', 'user'], template: '<div class="start-modal" />' },
  CompleteTrainingModal: { props: ['step', 'user'], template: '<div class="complete-modal" />' },
  ToolTrainingSetupModal: { props: ['tool'], template: '<div class="setup-modal" />' },
  CreateTrainingStepModal: { props: ['tools'], template: '<div class="create-step-modal" />' },
  EditTrainingStepModal: {
    props: ['step', 'tool', 'existingSteps'],
    template: '<div class="edit-step-modal" />',
  },
  TrainerManagement: { props: ['tool'], template: '<div class="trainer-management" />' },
  RecordTrainingModal: { props: ['tool'], template: '<div class="record-modal" />' },
}

beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset()
  mocks.getToolTrainingOverview.mockResolvedValue({ success: true, data: overview() })
  mocks.checkTrainerAuthorization.mockResolvedValue({ success: true, data: false })
  mocks.getTrainingHistory.mockResolvedValue({ success: true, data: [] })
  mocks.getUsersForTraining.mockResolvedValue({ success: true, data: { items: [] } })
  mocks.createTrainingRecord.mockResolvedValue({ success: true })
  authState.user = { id: 'u1', role: UserRole.Member }
  vi.spyOn(console, 'log').mockImplementation(() => {})
  vi.spyOn(console, 'error').mockImplementation(() => {})
  vi.spyOn(console, 'debug').mockImplementation(() => {})
})

async function modal(ov: ToolTrainingOverview = overview()) {
  mocks.getToolTrainingOverview.mockResolvedValue({ success: true, data: ov })
  const w = mount(ToolTrainingModal, { props: { tool: TOOL }, global: { stubs } })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof modal>>

const asRole = (role: UserRole) => {
  authState.user = { id: 'u1', role }
}
const labels = (w: Wrapper) => w.findAll('button').map((b) => b.text().trim())

describe('the action buttons, against what the server actually sends', () => {
  // Every one of these builds its step the way the server does -- `is_available`
  // and `user_progress`, no aliases -- which is exactly what they did while the
  // buttons were unreachable. The difference is that they now appear.
  it('offers Start on an available step with no progress yet', async () => {
    for (const role of [UserRole.Member, UserRole.Staff, UserRole.Admin]) {
      asRole(role)
      const w = await modal(overview({ steps: [serverStep(1, { is_available: true })] }))
      expect(labels(w), `${role} should be offered Start`).toContain('Start Training')
    }
  })

  it('withholds Start on a step whose prerequisites are not met', async () => {
    asRole(UserRole.Staff)
    const w = await modal(overview({ steps: [serverStep(1, { is_available: false })] }))
    expect(labels(w)).not.toContain('Start Training')
  })

  it('withholds Start once a step has been started', async () => {
    asRole(UserRole.Staff)
    const w = await modal(
      overview({
        steps: [
          serverStep(1, {
            is_available: true,
            user_progress: progress(TrainingStatus.InProgress),
          }),
        ],
      })
    )
    expect(labels(w)).not.toContain('Start Training')
  })

  it('offers Mark Complete for a session in progress, to an instructor', async () => {
    asRole(UserRole.Staff)
    const w = await modal(
      overview({
        steps: [serverStep(1, { user_progress: progress(TrainingStatus.InProgress) })],
      })
    )
    expect(labels(w)).toContain('Mark Complete')
  })

  it('offers Retry after a failure', async () => {
    asRole(UserRole.Staff)
    const w = await modal(
      overview({ steps: [serverStep(1, { user_progress: progress(TrainingStatus.Failed) })] })
    )
    expect(labels(w)).toContain('Retry Training')
  })

  it('offers nothing on a completed step', async () => {
    asRole(UserRole.Staff)
    const w = await modal(
      overview({ steps: [serverStep(1, { user_progress: progress(TrainingStatus.Completed) })] })
    )
    expect(labels(w)).not.toContain('Start Training')
    expect(labels(w)).not.toContain('Mark Complete')
    expect(labels(w)).not.toContain('Retry Training')
  })

  it('opens the start modal on the step the button belongs to', async () => {
    asRole(UserRole.Staff)
    const w = await modal(overview({ steps: [serverStep(1, { is_available: true })] }))
    await w
      .findAll('button')
      .filter((b) => b.text().trim() === 'Start Training')[0]
      .trigger('click')
    await nextTick()

    expect(w.find('.start-modal').exists()).toBe(true)
  })
})

describe('how a step is classed', () => {
  // Used to class every step `step-locked` whatever its real state, because
  // `getStepStatusClass` read the two aliases. A completed step and an
  // unavailable one looked identical.
  it('classes a step by its real state', async () => {
    const w = await modal(
      overview({
        steps: [
          serverStep(1, { user_progress: progress(TrainingStatus.Completed), is_available: true }),
          serverStep(2, { is_available: false }),
        ],
      })
    )
    const classes = w.findAll('.step-item, .training-step').map((n) => n.classes().join(' '))
    expect(classes[0]).toContain('step-completed')
    expect(classes[1]).toContain('step-locked')
  })

  it('classes an available step it has not started as available', async () => {
    const w = await modal(overview({ steps: [serverStep(1, { is_available: true })] }))
    expect(w.find('.step-item, .training-step').classes().join(' ')).toContain('step-available')
  })

  it('does not put a newline in the class it returns', async () => {
    // `getStepStatusClass` used to end in a template literal with two trailing
    // newlines. The DOM normalises whitespace so nothing broke, but the value
    // was still wrong, and a `class` carrying line breaks is the kind of thing
    // that stops being harmless the moment somebody compares it as a string.
    const w = await modal(
      overview({ steps: [serverStep(1, { user_progress: progress(TrainingStatus.Completed) })] })
    )
    for (const c of w.find('.step-item, .training-step').classes()) {
      expect(c).not.toMatch(/\s/)
    }
  })

  // The step *number* class always read `user_progress` and was always right;
  // the two functions used to disagree about the same step, which was the
  // evidence that one of them had the wrong field rather than both being
  // broken together. They agree now.
  it('classes the step number by progress too', async () => {
    const w = await modal(
      overview({
        steps: [
          serverStep(1, { user_progress: progress(TrainingStatus.Completed) }),
          serverStep(2, { user_progress: progress(TrainingStatus.InProgress) }),
          serverStep(3),
        ],
      })
    )
    const numbers = w.findAll('.step-number').map((n) => n.classes().join(' '))
    expect(numbers[0]).toContain('number-completed')
    expect(numbers[1]).toContain('number-in-progress')
    // Third step has no progress and defaults to available, so it is offered
    // rather than locked. Locked needs `is_available: false`.
    expect(numbers[2]).toContain('number-available')
  })

  it('classes an unavailable step number as locked', async () => {
    const w = await modal(overview({ steps: [serverStep(1, { is_available: false })] }))
    expect(w.find('.step-number').classes().join(' ')).toContain('number-locked')
  })
})

describe('the "no training required" branch', () => {
  // FINDING, pinned. The branch is
  //
  //     v-else-if="!tool.requires_training || !trainingOverview || steps.length === 0"
  //
  // so the *tool's own flag* short-circuits it, ahead of anything the overview
  // says. A tool that has training steps configured but whose
  // `requires_training` is false tells the member "This tool does not require
  // any special training. You can use it freely." -- with the steps loaded and
  // sitting unread in `trainingOverview`.
  //
  // The two can disagree because they are set by different paths:
  // ToolTrainingSetupModal sets the flag, and each step is created separately,
  // and it does not check the flag update succeeded before creating them.
  it('says a tool needs no training whenever its flag is off, steps or not', async () => {
    const w = mount(ToolTrainingModal, {
      props: { tool: { ...TOOL, requires_training: false } },
      global: { stubs },
    })
    await flushPromises()

    expect(
      w.text(),
      'the branch now consults the loaded steps -- if the condition was ' +
        'reordered, delete this test'
    ).toContain('does not require any special training')
    expect(w.find('.steps-section').exists()).toBe(false)
  })

  it('says the same when the tool requires training but has no steps', async () => {
    const w = await modal(overview({ steps: [] }))
    expect(w.text()).toContain('does not require any special training')
  })
})

describe('what it loads', () => {
  it('reads the overview and the trainer check on open', async () => {
    await modal()
    expect(mocks.getToolTrainingOverview).toHaveBeenCalledWith('tool-1', 'me')
    expect(mocks.checkTrainerAuthorization).toHaveBeenCalledWith('tool-1', 'u1')
  })

  it('tells the parent whether the tool may be used', async () => {
    const w = await modal(overview({ can_access_tool: true }))
    expect(w.emitted('training-status-changed')?.[0]).toEqual(['tool-1', true])
  })

  it('reloads both when the tool changes', async () => {
    const w = await modal()
    mocks.getToolTrainingOverview.mockClear()
    mocks.checkTrainerAuthorization.mockClear()

    await w.setProps({ tool: { ...TOOL, id: 'tool-2' } })
    await flushPromises()

    expect(mocks.getToolTrainingOverview).toHaveBeenCalledTimes(1)
    expect(mocks.checkTrainerAuthorization).toHaveBeenCalledTimes(1)
  })

  it('checks no authorization for a signed-out visitor', async () => {
    authState.user = null
    await modal()
    expect(mocks.checkTrainerAuthorization).not.toHaveBeenCalled()
  })

  it('reports a refused overview', async () => {
    mocks.getToolTrainingOverview.mockResolvedValue({ success: false, error: 'Forbidden' })
    const w = mount(ToolTrainingModal, { props: { tool: TOOL }, global: { stubs } })
    await flushPromises()

    // Worth asserting: unlike ToolTrainingCard, which renders `error` nowhere,
    // this one does show it.
    expect(w.text()).toContain('Forbidden')
  })
})

describe('who sees the management controls', () => {
  it('shows step management to staff and admins only', async () => {
    asRole(UserRole.Member)
    expect(labels(await modal())).not.toContain('Add Training Step')

    asRole(UserRole.Staff)
    expect(labels(await modal())).toContain('Add Training Step')
  })

  it('shows a trainer-only section to an authorised trainer who is not staff', async () => {
    asRole(UserRole.Member)
    mocks.checkTrainerAuthorization.mockResolvedValue({ success: true, data: true })
    const w = await modal()
    expect(w.find('.trainer-section').exists()).toBe(true)
  })

  it('does not show it to a member who is not a trainer', async () => {
    asRole(UserRole.Member)
    const w = await modal()
    expect(w.find('.trainer-section').exists()).toBe(false)
  })

  it('treats a refused authorization check as "not a trainer"', async () => {
    asRole(UserRole.Member)
    mocks.checkTrainerAuthorization.mockResolvedValue({ success: false, error: 'boom' })
    const w = await modal()
    expect(w.find('.trainer-section').exists()).toBe(false)
  })
})

describe('recording a session inline', () => {
  it('loads the training roster the first time the form is opened, and not again', async () => {
    asRole(UserRole.Staff)
    mocks.getUsersForTraining.mockResolvedValue({
      success: true,
      data: { items: [{ id: 'u2', is_active: true, full_name: 'Ada' }] },
    })
    const w = await modal()

    const toggleFor = (wr: Wrapper) => {
      const t = wr.findAll('button').find((b) => b.text().includes('Record'))
      if (!t) throw new Error('no record-training control')
      return t
    }

    await toggleFor(w).trigger('click')
    await flushPromises()
    expect(mocks.getUsersForTraining).toHaveBeenCalledWith('tool-1')
    expect(mocks.getUsersForTraining).toHaveBeenCalledTimes(1)

    // Closed and reopened: the guard is `usersForRecord.length === 0`, so a
    // second open must not re-fetch. Opening once cannot tell the guard from
    // its absence.
    await toggleFor(w).trigger('click')
    await flushPromises()
    await toggleFor(w).trigger('click')
    await flushPromises()
    expect(
      mocks.getUsersForTraining,
      'the roster is fetched again on reopen -- if the guard was removed ' +
        'deliberately, delete this expectation'
    ).toHaveBeenCalledTimes(1)
  })

  // FIXED, alongside RecordTrainingModal, which has the same inline form.
  it("defaults the training date to the instructor's date", async () => {
    asRole(UserRole.Staff)
    vi.setSystemTime(new Date('2026-01-16T02:00:00Z'))
    expect(new Date().getDate(), 'the suite timezone is not what this assumes').toBe(15)

    const w = await modal()
    const toggle = w.findAll('button').find((b) => b.text().includes('Record'))
    if (!toggle) throw new Error('no record-training control')
    await toggle.trigger('click')
    await flushPromises()

    expect((w.find('input[type="date"]').element as HTMLInputElement).value).toBe('2026-01-15')
    vi.setSystemTime(new Date('2026-01-15T12:00:00.000Z'))
  })
})

describe('the child modals it owns', () => {
  it('reloads and announces an update when training is started', async () => {
    asRole(UserRole.Staff)
    const w = await modal(overview({ steps: [serverStep(1, { is_available: true })] }))
    await w
      .findAll('button')
      .filter((b) => b.text().trim() === 'Start Training')[0]
      .trigger('click')
    await nextTick()

    mocks.getToolTrainingOverview.mockClear()
    const startModal = w.findComponent(stubs.StartTrainingModal)
    ;(startModal.vm as unknown as { $emit: (e: string) => void }).$emit('started')
    await flushPromises()

    expect(mocks.getToolTrainingOverview).toHaveBeenCalledTimes(1)
    expect(w.emitted('training-updated')).toHaveLength(1)
    expect(w.find('.start-modal').exists()).toBe(false)
  })

  it('re-checks trainer authorization after the trainer list changes', async () => {
    asRole(UserRole.Staff)
    const w = await modal()
    const open = w.findAll('button').find((b) => b.text().includes('Trainer'))
    if (!open) throw new Error('no trainer-management control')
    await open.trigger('click')
    await nextTick()

    mocks.checkTrainerAuthorization.mockClear()
    const tm = w.findComponent(stubs.TrainerManagement)
    ;(tm.vm as unknown as { $emit: (e: string) => void }).$emit('trainer-updated')
    await flushPromises()

    expect(mocks.checkTrainerAuthorization).toHaveBeenCalledTimes(1)
  })
})

describe('closing', () => {
  it('closes from the header button', async () => {
    const w = await modal()
    await w.find('.close-btn').trigger('click')
    expect(w.emitted('close')).toHaveLength(1)
  })
})
