// Tier 2: ToolTrainingCard.
//
// NOTHING IMPORTS THIS COMPONENT. `tests/structure/components-are-reachable.spec.ts`
// records it as unreferenced: `ToolTrainingModal` renders the same training
// flow, and this copy is superseded. So every defect pinned below is real and
// none of it is reaching users today -- including the red debug banner, which
// nobody has seen. They arrive the day it is wired up, which is why they are
// pinned rather than left for that day to discover.
//
// A member's view of their training on one tool, and the button that would let
// them begin is behind a staff check:
//
//     <div v-if="canManageTraining" class="action-buttons">
//       <button v-if="stepWithProgress.is_available && !user_progress" ...>Start</button>
//
// `canManageTraining` is `role === 'staff' || role === 'admin'`. So a member
// looking at this card sees the steps, sees their progress, and has no control
// to start any of it. Only staff and admins can press Start -- for themselves,
// or for someone else through the `user` prop.
//
// That compounds a finding from StartTrainingModal, whose instructor list is
// loaded from an admin-scoped roster endpoint: the member-initiated training
// flow is gated at both ends.
//
// What this spec does NOT prove: that the server would refuse a member's
// start request. Tier 4 owns that, and the two answers could differ -- which
// is the interesting case, because it would mean the capability exists and
// only the UI withholds it.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({ getToolTrainingOverview: vi.fn() }))
vi.mock('@/utils/api', () => ({ trainingApi: mocks }))

const authState = vi.hoisted<{ user: { id: string; role: string } | null }>(() => ({ user: null }))
vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({
    get user() {
      return authState.user
    },
  }),
}))

import ToolTrainingCard from '@/components/ToolTrainingCard.vue'
import { UserRole, type Tool, type User } from '@/types'
import {
  AssessmentType,
  TrainingStatus,
  type ToolTrainingOverview,
  type TrainingStep,
  type TrainingStepWithProgress,
  type UserTrainingProgress,
} from '@/types/training'

const TOOL = { id: 'tool-1', name: 'Lathe' } as unknown as Tool

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

function progress(status: TrainingStatus, over: Partial<UserTrainingProgress> = {}) {
  return {
    id: 'p1',
    user_id: 'u1',
    training_step_id: 'step-1',
    status,
    ...over,
  } as unknown as UserTrainingProgress
}

function withProgress(
  n: number,
  over: Partial<TrainingStepWithProgress> = {}
): TrainingStepWithProgress {
  return { step: step(n), prerequisites: [], is_available: true, ...over }
}

function overview(over: Partial<ToolTrainingOverview> = {}): ToolTrainingOverview {
  return {
    tool_id: 'tool-1',
    tool_name: 'Lathe',
    steps: [withProgress(1)],
    overall_progress: 0,
    can_access_tool: false,
    ...over,
  }
}

const stubs = {
  StartTrainingModal: { props: ['step', 'user'], template: '<div class="start-modal" />' },
  CompleteTrainingModal: { props: ['step', 'user'], template: '<div class="complete-modal" />' },
  ToolTrainingSetupModal: { props: ['tool'], template: '<div class="setup-modal" />' },
}

// Declared against exactly the two members this file uses, rather than
// `ReturnType<typeof vi.spyOn>` -- which resolves loosely enough that reading
// `.mock.calls` off it trips the type-aware lint.
interface ConsoleSpy {
  mockClear: () => void
  mock: { calls: unknown[][] }
}

/** The first argument of each logged call, as a string. */
const firstArgs = (spy: ConsoleSpy): string[] =>
  spy.mock.calls.map((c) => (typeof c[0] === 'string' ? c[0] : JSON.stringify(c[0])))
let logSpy: ConsoleSpy

beforeEach(() => {
  mocks.getToolTrainingOverview.mockReset()
  mocks.getToolTrainingOverview.mockResolvedValue({ success: true, data: overview() })
  authState.user = { id: 'u1', role: UserRole.Member }
  logSpy = vi.spyOn(console, 'log').mockImplementation(() => {})
  vi.spyOn(console, 'error').mockImplementation(() => {})
})

async function card(ov: ToolTrainingOverview = overview(), user?: User) {
  mocks.getToolTrainingOverview.mockResolvedValue({ success: true, data: ov })
  const w = mount(ToolTrainingCard, { props: { tool: TOOL, user }, global: { stubs } })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof card>>

const asRole = (role: UserRole) => {
  authState.user = { id: 'u1', role }
}
const buttonLabels = (w: Wrapper) => w.findAll('button').map((b) => b.text().trim())

describe('which overview it asks for', () => {
  it('asks about the signed-in user when no user prop is given', async () => {
    await card()
    expect(mocks.getToolTrainingOverview).toHaveBeenCalledWith('tool-1', 'me')
  })

  it('asks about the named user when one is', async () => {
    await card(overview(), { id: 'other-1' } as User)
    expect(mocks.getToolTrainingOverview).toHaveBeenCalledWith('tool-1', 'other-1')
  })

  it('tells the parent whether the tool may be used', async () => {
    const w = await card(overview({ can_access_tool: true }))
    expect(w.emitted('training-status-changed')?.[0]).toEqual(['tool-1', true])
  })
})

describe('the headline status', () => {
  it('says training is required when nothing has been done', async () => {
    const w = await card(overview({ overall_progress: 0, can_access_tool: false }))
    expect(w.find('.training-status').text()).toContain('Training Required')
    expect(w.find('.training-status span').classes()).toContain('status-pending')
  })

  it('says how far along a partly finished one is', async () => {
    const w = await card(overview({ overall_progress: 66.6, can_access_tool: false }))
    expect(w.find('.training-status').text()).toContain('Training in Progress (67%)')
    expect(w.find('.training-status span').classes()).toContain('status-progress')
  })

  it('says training is complete when the tool may be used', async () => {
    const w = await card(overview({ overall_progress: 100, can_access_tool: true }))
    expect(w.find('.training-status').text()).toContain('Training Complete')
    expect(w.find('.training-status span').classes()).toContain('status-success')
  })
})

describe('what a member can do', () => {
  // FINDING, pinned. The action buttons -- Start, Complete and Retry -- are
  // wrapped in `v-if="canManageTraining"`, which is staff or admin. A member
  // looking at their own training on a tool sees the steps and their progress
  // and has no way to begin.
  it('shows a member no way to start their own training', async () => {
    asRole(UserRole.Member)
    const w = await card(overview({ steps: [withProgress(1, { is_available: true })] }))

    expect(w.find('.steps-list').exists()).toBe(true)
    expect(
      w.find('.action-buttons').exists(),
      'members now get action buttons -- if the gate was changed, this test ' +
        'should assert which buttons they see'
    ).toBe(false)
    expect(buttonLabels(w)).not.toContain('Start Training')
  })

  it('shows the same to a newbie and to somebody with no role at all', async () => {
    for (const role of [UserRole.Newbie, UserRole.Unknown]) {
      asRole(role)
      const w = await card()
      expect(w.find('.action-buttons').exists()).toBe(false)
    }
  })

  it('gives staff the Start button', async () => {
    asRole(UserRole.Staff)
    const w = await card(overview({ steps: [withProgress(1, { is_available: true })] }))
    expect(w.find('.action-buttons').exists()).toBe(true)
  })

  it('withholds Start for a step that is not yet available', async () => {
    asRole(UserRole.Staff)
    const w = await card(overview({ steps: [withProgress(1, { is_available: false })] }))
    expect(w.find('.action-buttons').text()).not.toContain('Start')
  })

  it('offers Retry after a failure, and nothing after a completion', async () => {
    asRole(UserRole.Staff)
    const failed = await card(
      overview({ steps: [withProgress(1, { user_progress: progress(TrainingStatus.Failed) })] })
    )
    expect(failed.find('.action-buttons').text()).toContain('Retry')

    const done = await card(
      overview({ steps: [withProgress(1, { user_progress: progress(TrainingStatus.Completed) })] })
    )
    expect(done.find('.action-buttons').text()).not.toContain('Retry')
    expect(done.find('.action-buttons').text()).not.toContain('Start')
  })
})

describe('who may sign a session off', () => {
  // FINDING, pinned. `isInstructor` is a stub, and says so:
  //
  //     // This would need to check if the current user is certified as an
  //     // instructor. For now, we'll use staff/admin as proxy
  //     return canManageTraining.value
  //
  // The system has a real notion of a certified instructor -- there are
  // `instructor_certified` and `instructor_revoked` audit events and a
  // `revoke_instructor_certification` database call -- and this component
  // ignores all of it. Every staff member can sign off any session; no
  // certified instructor who is only a member can sign off any.
  it('lets any staff member complete a session, certified or not', async () => {
    asRole(UserRole.Staff)
    const w = await card(
      overview({
        steps: [withProgress(1, { user_progress: progress(TrainingStatus.InProgress) })],
      })
    )
    expect(
      w.find('.action-buttons').text(),
      'instructor certification is now checked -- if that was wired up, this ' +
        'test should assert against a certified and an uncertified staff member'
    ).toContain('Complete')
  })

  it('lets no member complete a session, certified or not', async () => {
    asRole(UserRole.Member)
    const w = await card(
      overview({
        steps: [withProgress(1, { user_progress: progress(TrainingStatus.InProgress) })],
      })
    )
    expect(w.find('.action-buttons').exists()).toBe(false)
  })
})

describe('the steps list', () => {
  it('marks a completed step and leaves the others plain', async () => {
    const w = await card(
      overview({
        steps: [
          withProgress(1, { user_progress: progress(TrainingStatus.Completed) }),
          withProgress(2),
        ],
      })
    )
    const numbers = w.findAll('.step-number')
    expect(numbers[0].classes()).toContain('step-number-completed')
    expect(numbers[1].classes()).not.toContain('step-number-completed')
    expect(numbers[0].text()).toContain('✓')
    expect(numbers[1].text()).toBe('2')
  })

  it('names the assessment type in words', async () => {
    const w = await card(
      overview({
        steps: [withProgress(1, { step: step(1, { assessment_type: AssessmentType.Both }) })],
      })
    )
    expect(w.find('.step-meta').text()).toContain('Practical + Written')
  })

  it('offers a setup prompt, to staff only, when there are no steps', async () => {
    asRole(UserRole.Member)
    const asMember = await card(overview({ steps: [] }))
    expect(asMember.find('.no-training').exists()).toBe(true)
    expect(asMember.find('.no-training button').exists()).toBe(false)

    asRole(UserRole.Staff)
    const asStaff = await card(overview({ steps: [] }))
    expect(asStaff.find('.no-training button').exists()).toBe(true)
  })
})

describe('debug output', () => {
  // FINDING, pinned, and the one a user actually sees. The template opens with
  // a red banner:
  //
  //     <!-- Debug: Component mounted check -->
  //     <div style="background: red; color: white; padding: 5px; margin: 5px">
  //       ToolTrainingCard is rendering! Tool: {{ tool.name }}
  //
  // It is outside `.training-card`, so it would render above every tool's
  // training section for every user -- if anything mounted this component. See
  // the note at the top of this file: nothing does.
  it('renders a red debug banner above the card', async () => {
    const w = await card()
    expect(
      w.text(),
      'the debug banner is gone -- delete this test, and the one below it if ' +
        'the console logging went with it'
    ).toContain('ToolTrainingCard is rendering!')
    expect(w.html()).toContain('background: red')
  })

  // FINDING, pinned. Seven `console.log` calls ship in this component, several
  // with emoji prefixes, and one of them is inside `getStepNumberClass` -- a
  // function called from the template, for every step, on every render. A
  // member opening a tool with six steps writes twelve lines to the console
  // before touching anything, and every re-render writes twelve more.
  it('writes to the console during render, once per step per render', async () => {
    logSpy.mockClear()
    const w = await card(overview({ steps: [withProgress(1), withProgress(2), withProgress(3)] }))

    const fromRender = firstArgs(logSpy).filter((a) => a === 'Step')
    expect(
      fromRender.length,
      'the render-time logging is gone -- if the console.log in ' +
        'getStepNumberClass was removed, delete this test'
    ).toBe(3)

    logSpy.mockClear()
    await w.setProps({ tool: { ...TOOL, name: 'Lathe (large)' } })
    await nextTick()
    expect(firstArgs(logSpy).filter((a) => a === 'Step').length).toBeGreaterThan(0)
  })

  it('logs the request and the response on every load', async () => {
    logSpy.mockClear()
    await card()
    const logged = firstArgs(logSpy).join('\n')
    expect(logged).toContain('Loading training overview for tool')
    expect(logged).toContain('API Response')
  })
})

describe('reloading after a child modal finishes', () => {
  it('re-reads the overview and tells the parent when training is started', async () => {
    asRole(UserRole.Staff)
    const w = await card(overview({ steps: [withProgress(1, { is_available: true })] }))
    await w.find('.action-buttons button').trigger('click')
    await nextTick()
    expect(w.find('.start-modal').exists()).toBe(true)

    mocks.getToolTrainingOverview.mockClear()
    const startModal = w.findComponent(stubs.StartTrainingModal)
    ;(startModal.vm as unknown as { $emit: (e: string) => void }).$emit('started')
    await flushPromises()

    expect(mocks.getToolTrainingOverview).toHaveBeenCalledTimes(1)
    expect(w.emitted('training-updated')).toHaveLength(1)
    expect(w.find('.start-modal').exists()).toBe(false)
  })
})

describe('when the overview cannot be loaded', () => {
  // FINDING, pinned, and the worst on this card. `error` is set on every
  // failure path and rendered nowhere -- the template has no error branch at
  // all. So a refused or failed overview falls through to the `v-else` and
  // shows "No training steps configured for this tool."
  //
  // That is not a blank screen. It tells a member that a tool they are not
  // allowed to read the training for has no training requirement. Same shape
  // as ToolEventHistory answering "no events" to a 403, and worse in
  // consequence, because this one is about whether a machine needs training
  // before you touch it.
  it('says the tool needs no training when the request was refused', async () => {
    mocks.getToolTrainingOverview.mockResolvedValue({ success: false, error: 'Forbidden' })
    const w = mount(ToolTrainingCard, { props: { tool: TOOL }, global: { stubs } })
    await flushPromises()

    expect(
      w.find('.no-training').text(),
      'the card now reports a failed load -- if an error branch was added, ' +
        'this test should assert the message instead'
    ).toContain('No training steps configured for this tool.')
    expect(w.text()).not.toContain('Forbidden')
  })

  it('tells the parent nothing when the load fails', async () => {
    mocks.getToolTrainingOverview.mockResolvedValue({ success: false, error: 'Forbidden' })
    const w = mount(ToolTrainingCard, { props: { tool: TOOL }, global: { stubs } })
    await flushPromises()

    // The parent uses `training-status-changed` to decide whether to offer the
    // tool. Staying silent on failure is right: claiming `false` would be a
    // guess, and claiming `true` would be worse.
    expect(w.emitted('training-status-changed')).toBeUndefined()
  })

  it('says the same thing when the request rejects outright', async () => {
    mocks.getToolTrainingOverview.mockRejectedValue(new Error('Network Error'))
    const w = mount(ToolTrainingCard, { props: { tool: TOOL }, global: { stubs } })
    await flushPromises()

    expect(w.find('.no-training').exists()).toBe(true)
    expect(w.text()).not.toContain('Network Error')
  })

  // FINDING, pinned. `formatTrainingStatus()` returns 'Loading...' whenever
  // `trainingOverview` is null, and a failed load leaves it null forever. So
  // after a failure the card reads "Loading..." in the header and "No training
  // steps configured for this tool." in the body -- at the same time,
  // permanently, and neither is true.
  it('leaves the status reading "Loading..." forever after a failure', async () => {
    mocks.getToolTrainingOverview.mockResolvedValue({ success: false, error: 'Forbidden' })
    const w = mount(ToolTrainingCard, { props: { tool: TOOL }, global: { stubs } })
    await flushPromises()

    expect(
      w.find('.training-status').text(),
      'the status no longer says Loading after a failure -- if a failed state ' +
        'was added, this test should assert it'
    ).toContain('Loading...')
    expect(w.find('.no-training').exists()).toBe(true)
    expect(w.find('.training-status span').classes()).toContain('status-unknown')
  })
})
