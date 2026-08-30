// Tier 2: TrainerManagement.
//
// The assign-and-manage surface for a tool's trainers, and the one that
// actually runs -- `AssignTrainerModal` implements the same assign form and is
// imported by nothing (see tests/structure/components-are-reachable.spec.ts).
// This copy emits and reloads correctly, which is why nobody noticed the other.
//
// Its own defect is a control that does not exist. `includeInactive` is a ref
// that is read once --
//
//     await trainerApi.getToolTrainers(props.tool.id, includeInactive.value)
//
// -- and set nowhere. Nothing in the template binds it. So the list is always
// fetched with `include_inactive=false`, while the UI has a Deactivate button,
// an Activate button and an "Inactive" status label. Deactivating a trainer
// removes them from the only list that can show them, and the Activate button
// is behind `v-else` on a row that can no longer render.
//
// What this spec does NOT prove: that the server honours `include_inactive`,
// or that a deactivated trainer is still stored. Tier 6 owns both. What is
// asserted here is that this component never asks for them.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({
  getToolTrainers: vi.fn(),
  assignToolTrainer: vi.fn(),
  updateToolTrainer: vi.fn(),
  removeToolTrainer: vi.fn(),
  getAllUsers: vi.fn(),
}))
vi.mock('@/utils/api', () => ({
  trainerApi: {
    getToolTrainers: mocks.getToolTrainers,
    assignToolTrainer: mocks.assignToolTrainer,
    updateToolTrainer: mocks.updateToolTrainer,
    removeToolTrainer: mocks.removeToolTrainer,
  },
  userApi: { getAllUsers: mocks.getAllUsers },
}))

const authState = vi.hoisted<{ user: { id: string; role: string } | null }>(() => ({ user: null }))
vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({
    get user() {
      return authState.user
    },
  }),
}))

import TrainerManagement from '@/components/TrainerManagement.vue'
import { UserRole, type Tool, type User } from '@/types'
import type { ToolTrainerWithUser } from '@/types/training'

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

function trainer(
  userId: string,
  over: Partial<ToolTrainerWithUser['trainer']> = {}
): ToolTrainerWithUser {
  return {
    trainer: {
      id: `tt-${userId}`,
      user_id: userId,
      tool_id: 'tool-1',
      authorized_by: 'admin-1',
      authorized_at: '2025-06-01T00:00:00Z',
      is_active: true,
      created_at: '2025-06-01T00:00:00Z',
      updated_at: '2025-06-01T00:00:00Z',
      ...over,
    },
    user_name: `u-${userId}`,
    user_email: `${userId}@example.test`,
    user_full_name: `Full ${userId}`,
  }
}

const stubs = {
  EditTrainerModal: {
    props: ['tool', 'trainerWithUser'],
    template: '<div class="edit-modal" />',
  },
}

let confirmResult = true

beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset()
  mocks.getToolTrainers.mockResolvedValue({ success: true, data: [] })
  mocks.getAllUsers.mockResolvedValue({ success: true, data: { items: [user('a')] } })
  mocks.assignToolTrainer.mockResolvedValue({ success: true })
  mocks.updateToolTrainer.mockResolvedValue({ success: true })
  mocks.removeToolTrainer.mockResolvedValue({ success: true })
  authState.user = { id: 'admin-1', role: UserRole.Admin }
  confirmResult = true
  vi.stubGlobal(
    'confirm',
    vi.fn(() => confirmResult)
  )
})

async function page(list: ToolTrainerWithUser[] = []) {
  mocks.getToolTrainers.mockResolvedValue({ success: true, data: list })
  const w = mount(TrainerManagement, { props: { tool: TOOL }, global: { stubs } })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof page>>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}
const assigned = () => mocks.assignToolTrainer.mock.calls[0][0] as Record<string, unknown>

describe('which trainers it asks for', () => {
  // FINDING, pinned. `includeInactive` is set nowhere -- no checkbox, no
  // toggle, no query parameter, nothing in the template binds it. It is read
  // once, always `false`.
  it('never asks for inactive trainers, because nothing can set the flag', async () => {
    await page()
    expect(
      mocks.getToolTrainers,
      'the flag is now settable -- if a control was added, this test should ' +
        'drive it and assert both values'
    ).toHaveBeenCalledWith('tool-1', false)
  })

  it('lists each trainer by name and email', async () => {
    const w = await page([trainer('a')])
    expect(w.find('.trainer-info').text()).toContain('Full a')
    expect(w.find('.trainer-email').text()).toBe('a@example.test')
  })

  it('says so when there are none', async () => {
    const w = await page()
    expect(w.find('.no-trainers').text()).toContain('No trainers assigned')
  })
})

describe('activating and deactivating', () => {
  it('asks before deactivating, and sends is_active false', async () => {
    const w = await page([trainer('a')])
    await buttonNamed(w, 'Deactivate').trigger('click')
    await flushPromises()

    expect(mocks.updateToolTrainer).toHaveBeenCalledWith('tool-1', 'a', { is_active: false })
    expect(w.emitted('trainer-updated')).toHaveLength(1)
  })

  it('does nothing if the deactivation is declined', async () => {
    confirmResult = false
    const w = await page([trainer('a')])
    await buttonNamed(w, 'Deactivate').trigger('click')
    await flushPromises()
    expect(mocks.updateToolTrainer).not.toHaveBeenCalled()
  })

  // FINDING, pinned, and the consequence of the flag above. The Activate
  // button is behind `v-else` on `trainer.is_active`, so it only renders for a
  // row whose trainer is inactive -- and an inactive trainer is never in the
  // list, because the fetch always asks for active ones only. The button
  // exists, is styled, is wired to a working handler, and cannot be reached.
  it('offers Activate only on a row that can never be fetched', async () => {
    const active = await page([trainer('a')])
    expect(active.findAll('button').map((b) => b.text().trim())).toContain('Deactivate')
    expect(active.findAll('button').map((b) => b.text().trim())).not.toContain('Activate')

    // Handed an inactive trainer directly -- which the server would only return
    // with include_inactive=true -- the button appears and works. That is the
    // proof it is unreachable rather than absent.
    const inactive = await page([trainer('a', { is_active: false })])
    expect(
      inactive.findAll('button').map((b) => b.text().trim()),
      'the Activate button is now reachable -- if an include-inactive control ' +
        'was added, delete this test and drive it instead'
    ).toContain('Activate')

    await buttonNamed(inactive, 'Activate').trigger('click')
    await flushPromises()
    expect(mocks.updateToolTrainer).toHaveBeenCalledWith('tool-1', 'a', { is_active: true })
  })

  it('shows the status a deactivated trainer would have', async () => {
    const w = await page([trainer('a', { is_active: false })])
    expect(w.text()).toContain('Inactive')
  })

  it('marks an expired trainer as expired rather than active', async () => {
    const w = await page([trainer('a', { expires_at: '2026-01-14T00:00:00Z' })])
    expect(w.text()).toContain('Expired')
  })

  it('marks a trainer whose expiry has not passed as active', async () => {
    const w = await page([trainer('a', { expires_at: '2026-06-01T00:00:00Z' })])
    expect(w.text()).toContain('Active')
    expect(w.text()).not.toContain('Expired')
  })
})

describe('removing a trainer', () => {
  it('warns that it cannot be undone, and does nothing if declined', async () => {
    confirmResult = false
    const w = await page([trainer('a')])
    await buttonNamed(w, 'Remove').trigger('click')
    await flushPromises()

    expect(vi.mocked(globalThis.confirm)).toHaveBeenCalledWith(
      'Are you sure you want to permanently remove this trainer? This action cannot be undone.'
    )
    expect(mocks.removeToolTrainer).not.toHaveBeenCalled()
  })

  it('removes, reloads and tells the parent', async () => {
    const w = await page([trainer('a')])
    mocks.getToolTrainers.mockClear()
    await buttonNamed(w, 'Remove').trigger('click')
    await flushPromises()

    expect(mocks.removeToolTrainer).toHaveBeenCalledWith('tool-1', 'a')
    expect(mocks.getToolTrainers).toHaveBeenCalledTimes(1)
    expect(w.emitted('trainer-updated')).toHaveLength(1)
  })

  it("reports the server's reason for a refusal", async () => {
    mocks.removeToolTrainer.mockResolvedValue({
      success: false,
      error: 'Trainer has open sessions',
    })
    const w = await page([trainer('a')])
    await buttonNamed(w, 'Remove').trigger('click')
    await flushPromises()

    expect(w.find('.error').text()).toBe('Trainer has open sessions')
  })
})

describe('assigning a trainer', () => {
  const openForm = async (w: Wrapper) => {
    await buttonNamed(w, 'Assign Trainer').trigger('click')
    await nextTick()
  }

  it('offers active users who are not already trainers', async () => {
    mocks.getAllUsers.mockResolvedValue({
      success: true,
      data: { items: [user('a'), user('b'), user('c', { is_active: false })] },
    })
    const w = await page([trainer('b')])
    await openForm(w)

    const offered = w
      .findAll('#user option')
      .map((o) => o.attributes('value'))
      .filter((v) => v !== '')
    expect(offered).toEqual(['a'])
  })

  it('says so when everyone eligible is already a trainer', async () => {
    mocks.getAllUsers.mockResolvedValue({ success: true, data: { items: [user('a')] } })
    const w = await page([trainer('a')])
    await openForm(w)
    expect(w.find('.no-users').exists()).toBe(true)
  })

  it('sends the tool, the user and the optional fields', async () => {
    const w = await page()
    await openForm(w)
    await w.find('#user').setValue('a')
    await w.find('#notes').setValue('Signed off by staff.')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(assigned()).toMatchObject({
      tool_id: 'tool-1',
      user_id: 'a',
      notes: 'Signed off by staff.',
    })
  })

  it('omits blank optional fields rather than sending empty strings', async () => {
    const w = await page()
    await openForm(w)
    await w.find('#user').setValue('a')
    await w.find('form').trigger('submit')
    await flushPromises()

    const json = JSON.parse(JSON.stringify(assigned())) as Record<string, unknown>
    expect(json).not.toHaveProperty('notes')
    expect(json).not.toHaveProperty('expires_at')
  })

  // FINDING, pinned. Third component sending a bare calendar date where
  // `api/trainers.rs:25` declares `Option<DateTime<Utc>>`, after
  // EditTrainerModal and AssignTrainerModal -- and the fourth whose date floor
  // is `toISOString()`, the UTC date rather than the user's.
  it('sends a bare calendar date, floored at the UTC date', async () => {
    const w = await page()
    await openForm(w)
    expect(w.find('#expires_at').attributes('min')).toBe('2026-01-15')

    // The frozen clock is 12:00Z, which is 06:00 on the same day in the suite's
    // timezone -- so the UTC and local dates agree there and the floor cannot
    // be told apart. Moved to an instant where they disagree.
    vi.setSystemTime(new Date('2026-01-16T02:00:00Z'))
    expect(new Date().getDate(), 'the suite timezone is not what this assumes').toBe(15)
    const late = await page()
    await openForm(late)
    expect(
      late.find('#expires_at').attributes('min'),
      'the floor is now the local date -- if that was fixed, delete this half'
    ).toBe('2026-01-16')
    vi.setSystemTime(new Date('2026-01-15T12:00:00.000Z'))

    await w.find('#user').setValue('a')
    await w.find('#expires_at').setValue('2026-04-01')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(
      assigned().expires_at,
      'the expiry is now a timestamp -- if that was fixed, delete this test'
    ).toBe('2026-04-01')
  })

  it('closes the form, reloads and tells the parent on success', async () => {
    const w = await page()
    await openForm(w)
    await w.find('#user').setValue('a')
    mocks.getToolTrainers.mockClear()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.assign-trainer-form').exists()).toBe(false)
    expect(mocks.getToolTrainers).toHaveBeenCalledTimes(1)
    expect(w.emitted('trainer-updated')).toHaveLength(1)
  })

  it('keeps the form open and reports a refusal', async () => {
    mocks.assignToolTrainer.mockResolvedValue({ success: false, error: 'Already a trainer' })
    const w = await page()
    await openForm(w)
    await w.find('#user').setValue('a')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.assign-trainer-form .error').text()).toBe('Already a trainer')
    expect(w.find('.assign-trainer-form').exists()).toBe(true)
    expect(w.emitted('trainer-updated')).toBeUndefined()
  })

  it('re-enables the submit button whether the request resolved or rejected', async () => {
    mocks.assignToolTrainer.mockRejectedValue(new Error('down'))
    const w = await page()
    await openForm(w)
    await w.find('#user').setValue('a')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('button[type="submit"]').attributes('disabled')).toBeUndefined()
  })
})

describe('what a member sees', () => {
  it('is shown the trainers and none of the controls', async () => {
    authState.user = { id: 'u1', role: UserRole.Member }
    const w = await page([trainer('a')])

    expect(w.find('.trainer-info').text()).toContain('Full a')
    expect(w.find('.trainer-actions').exists()).toBe(false)
    expect(w.findAll('button').map((b) => b.text().trim())).not.toContain('Assign Trainer')
  })

  it('does not even load the roster for someone who cannot assign', async () => {
    authState.user = { id: 'u1', role: UserRole.Member }
    await page()
    expect(mocks.getAllUsers).not.toHaveBeenCalled()
  })
})

describe('when the list cannot be loaded', () => {
  it('reports a refusal rather than showing an empty roster', async () => {
    // Worth asserting: unlike most of its siblings, `loadTrainers` has an else
    // branch and surfaces the reason.
    mocks.getToolTrainers.mockResolvedValue({ success: false, error: 'Forbidden' })
    const w = mount(TrainerManagement, { props: { tool: TOOL }, global: { stubs } })
    await flushPromises()

    expect(w.find('.error').text()).toBe('Forbidden')
  })

  it('stops loading whether the request resolved or rejected', async () => {
    mocks.getToolTrainers.mockRejectedValue(new Error('Network Error'))
    const w = mount(TrainerManagement, { props: { tool: TOOL }, global: { stubs } })
    await flushPromises()

    expect(w.find('.loading').exists()).toBe(false)
    expect(w.find('.error').text()).toBe('Network Error')
  })
})

describe('closing', () => {
  it('closes on the overlay and the header button', async () => {
    const w = await page()
    await w.find('.modal-overlay').trigger('click')
    await w.find('.close-btn').trigger('click')
    expect(w.emitted('close')).toHaveLength(2)
  })

  it('does not close when the content is clicked', async () => {
    const w = await page()
    await w.find('.modal-content').trigger('click')
    expect(w.emitted('close')).toBeUndefined()
  })
})
