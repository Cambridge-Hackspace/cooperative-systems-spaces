// Tier 2: AssignTrainerModal.
//
// NOTHING IMPORTS THIS COMPONENT. `tests/structure/components-are-reachable.spec.ts`
// records it as unreferenced: the assign form that operators actually use is
// inlined in `TrainerManagement.vue`, and that one emits and reloads properly.
// So the defect below is real and nobody is hitting it -- which is also the
// reason it survived: a success path that does nothing is invisible when no
// parent is listening.
//
// `submitForm` is `loadUsers` with the request swapped out and nothing else
// changed. Every branch still talks about loading users:
//
//     if (response.success) {
//       console.log('Loaded users:', users.value.length)   // <- success path
//     } else {
//       error.value = response.error || 'Failed to load users'
//     }
//
// FIXED. Every branch used to talk about loading users: the success path
// logged a user count and emitted nothing, so the parent was never told, and
// every failure of the *submit* reported "Failed to load users". It now emits
// `assigned` and describes the operation it actually failed at.
//
// What this spec does NOT prove: that a real browser lets an empty submit
// through. The select carries `required`, and native constraint validation
// blocks a genuine form submission; `trigger('submit')` dispatches the event
// directly and bypasses it. What is asserted here is that the *component* has
// no guard of its own, which is a different and weaker claim, made explicitly.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({ getAllUsers: vi.fn(), assignToolTrainer: vi.fn() }))
vi.mock('@/utils/api', () => ({
  userApi: { getAllUsers: mocks.getAllUsers },
  trainerApi: { assignToolTrainer: mocks.assignToolTrainer },
}))

import AssignTrainerModal from '@/components/AssignTrainerModal.vue'
import { UserRole, type Tool, type User } from '@/types'

const TOOL = { id: 'tool-1', name: 'Lathe' } as unknown as Tool

function user(id: string, over: Partial<User> = {}): User {
  return {
    id,
    username: `u-${id}`,
    email: `${id}@example.test`,
    full_name: `Full ${id}`,
    is_active: true,
    role: UserRole.Member,
    created_at: '2025-01-01T00:00:00Z',
    updated_at: '2025-01-01T00:00:00Z',
    profile: {},
    meta: {},
    ...over,
  }
}

beforeEach(() => {
  mocks.getAllUsers.mockReset()
  mocks.assignToolTrainer.mockReset()
  mocks.getAllUsers.mockResolvedValue({ success: true, data: { items: [user('a')] } })
  mocks.assignToolTrainer.mockResolvedValue({ success: true })
  vi.spyOn(console, 'log').mockImplementation(() => {})
  vi.spyOn(console, 'error').mockImplementation(() => {})
})

async function modal(existingTrainers: string[] = []) {
  const w = mount(AssignTrainerModal, { props: { tool: TOOL, existingTrainers } })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof modal>>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}

const sent = () => mocks.assignToolTrainer.mock.calls[0][0] as Record<string, unknown>

describe('who can be assigned', () => {
  it('names the tool in the heading', async () => {
    expect((await modal()).find('.modal-header h3').text()).toBe('Assign Trainer to Lathe')
  })

  it('offers every active user who is not already a trainer', async () => {
    mocks.getAllUsers.mockResolvedValue({
      success: true,
      data: { items: [user('a'), user('b'), user('c')] },
    })
    const w = await modal(['b'])

    const offered = w
      .findAll('option')
      .map((o) => o.attributes('value'))
      .filter((v) => v !== '')
    expect(offered).toEqual(['a', 'c'])
  })

  it('leaves out a deactivated user', async () => {
    mocks.getAllUsers.mockResolvedValue({
      success: true,
      data: { items: [user('a'), user('b', { is_active: false })] },
    })
    const w = await modal()
    expect(
      w
        .findAll('option')
        .map((o) => o.attributes('value'))
        .filter((v) => v !== '')
    ).toEqual(['a'])
  })

  it('labels a candidate by name and email, falling back to the username', async () => {
    mocks.getAllUsers.mockResolvedValue({
      success: true,
      data: { items: [user('a', { full_name: 'Ada Lovelace' }), user('b', { full_name: '' })] },
    })
    const w = await modal()
    const labels = w.findAll('option').map((o) => o.text())
    expect(labels).toContain('Ada Lovelace (a@example.test)')
    expect(labels).toContain('u-b (b@example.test)')
  })

  it('reports a refused roster load', async () => {
    mocks.getAllUsers.mockResolvedValue({ success: false, error: 'Forbidden' })
    const w = await modal()
    expect(w.find('.error').text()).toBe('Forbidden')
  })

  // FINDING, pinned. The `v-if="loading"` / `v-else-if="no users"` chain sits
  // *beside* the form rather than around it, so the form is on screen the whole
  // time. "Loading users..." appears above a select that is still empty, and
  // "No available users to assign as trainers." appears above a select the user
  // is invited to submit anyway.
  it('shows the form underneath its own loading state', async () => {
    mocks.getAllUsers.mockReturnValue(new Promise(() => {}))
    const w = mount(AssignTrainerModal, { props: { tool: TOOL, existingTrainers: [] } })
    await nextTick()

    expect(w.find('.loading').exists()).toBe(true)
    expect(
      w.find('form').exists(),
      'the form is now hidden while loading -- if the branch chain was ' +
        'restructured to wrap it, delete this test'
    ).toBe(true)
  })

  it('shows the form underneath its own empty state, and lets it be submitted', async () => {
    const w = await modal(['a'])

    expect(w.find('.no-users').text()).toContain('No available users')
    expect(
      w.find('form').exists(),
      'the form is now hidden when there is nobody to pick -- if the branch ' +
        'chain was restructured to wrap it, delete this test'
    ).toBe(true)
    expect(buttonNamed(w, 'Assign Trainer').attributes('disabled')).toBeUndefined()
  })
})

describe('what the form sends', () => {
  it('addresses the tool from the prop and the user from the select', async () => {
    const w = await modal()
    await w.find('#user').setValue('a')
    await w.find('#notes').setValue('Signed off by staff.')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(sent().tool_id).toBe('tool-1')
    expect(sent().user_id).toBe('a')
    expect(sent().notes).toBe('Signed off by staff.')
  })

  it('omits notes and expiry when they are blank', async () => {
    const w = await modal()
    await w.find('#user').setValue('a')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(JSON.parse(JSON.stringify(sent()))).not.toHaveProperty('notes')
    expect(JSON.parse(JSON.stringify(sent()))).not.toHaveProperty('expires_at')
  })

  // FINDING, pinned. Same as EditTrainerModal: the `<input type="date">` value
  // is forwarded verbatim, and `api/trainers.rs:25` declares
  // `expires_at: Option<DateTime<Utc>>`, whose serde implementation wants
  // RFC 3339. "2026-04-01" is not one.
  it('sends a bare calendar date where the server declares a timestamp', async () => {
    const w = await modal()
    await w.find('#user').setValue('a')
    await w.find('#expires_at').setValue('2026-04-01')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(
      sent().expires_at,
      'the expiry is now sent as a timestamp -- if that was fixed, delete this ' +
        'test; the server field is Option<DateTime<Utc>>'
    ).toBe('2026-04-01')
  })

  // FINDING, pinned. Same as EditTrainerModal: `today` is
  // `toISOString().split('T')[0]`, the UTC date rather than the user's.
  it("floors the date picker at the UTC date, which is not the user's date", async () => {
    expect((await modal()).find('#expires_at').attributes('min')).toBe('2026-01-15')

    vi.setSystemTime(new Date('2026-01-16T02:00:00Z'))
    expect(new Date().getDate(), 'the suite timezone is not what this test assumes').toBe(15)
    expect(
      (await modal()).find('#expires_at').attributes('min'),
      'the floor is now computed from the local date -- if that was fixed, ' + 'delete this test'
    ).toBe('2026-01-16')
  })

  // Recorded, with its limits stated. The select carries `required`, so a real
  // browser refuses to submit an empty one and this path is not reachable
  // there. `trigger('submit')` bypasses constraint validation, which is what
  // makes the absence of a component-side guard visible at all -- so this
  // asserts the guard is missing, not that users can hit it.
  it('has no guard of its own against submitting with nobody selected', async () => {
    const w = await modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('#user').attributes('required')).toBeDefined()
    expect(mocks.assignToolTrainer).toHaveBeenCalledTimes(1)
    expect(sent().user_id).toBe('')
  })
})

describe('what happens after a successful assignment', () => {
  // FINDING, pinned, and the one that makes the feature not work. The success
  // branch of `submitForm` is `console.log('Loaded users:', users.value.length)`
  // -- copied wholesale from `loadUsers` and never adapted. Nothing is emitted,
  // so the parent never refreshes its trainer list; the modal does not close;
  // no confirmation appears. From the operator's side, assigning a trainer
  // looks exactly like nothing happening.
  it('announces the assignment so the parent can refresh', async () => {
    const w = await modal()
    await w.find('#user').setValue('a')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(mocks.assignToolTrainer).toHaveBeenCalledTimes(1)
    expect(w.emitted('assigned')).toHaveLength(1)
    expect(w.find('.error').exists()).toBe(false)
  })

  it('re-enables the submit button either way', async () => {
    const w = await modal()
    await w.find('#user').setValue('a')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('button[type="submit"]').attributes('disabled')).toBeUndefined()
    expect(w.find('button[type="submit"]').text()).toBe('Assign Trainer')
  })

  it('disables the submit button and says so while the request is in flight', async () => {
    mocks.assignToolTrainer.mockReturnValue(new Promise(() => {}))
    const w = await modal()
    await w.find('#user').setValue('a')
    await w.find('form').trigger('submit')
    await flushPromises()

    const submit = w.find('button[type="submit"]')
    expect(submit.attributes('disabled')).toBeDefined()
    expect(submit.text()).toBe('Assigning...')
  })
})

describe('what happens after a refused assignment', () => {
  // FINDING, pinned. The failure branch is the other half of the same
  // copy-paste. A refusal with no message reads "Failed to load users" -- for
  // an operation that loaded nothing.
  it('describes the operation it actually failed at', async () => {
    mocks.assignToolTrainer.mockResolvedValue({ success: false })
    const w = await modal()
    await w.find('#user').setValue('a')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error').text()).toBe('Failed to assign trainer')
    expect(w.emitted('assigned')).toBeUndefined()
  })

  it("does show the server's own reason when there is one", async () => {
    mocks.assignToolTrainer.mockResolvedValue({ success: false, error: 'Already a trainer' })
    const w = await modal()
    await w.find('#user').setValue('a')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error').text()).toBe('Already a trainer')
  })

  it('clears a previous error before trying again', async () => {
    mocks.assignToolTrainer.mockResolvedValue({ success: false, error: 'Already a trainer' })
    const w = await modal()
    await w.find('#user').setValue('a')
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(w.find('.error').exists()).toBe(true)

    mocks.assignToolTrainer.mockReturnValue(new Promise(() => {}))
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(w.find('.error').exists()).toBe(false)
  })

  // The catch in `submitForm` cannot be reached in production:
  // `api.ts:864` makes `assignToolTrainer` swallow its own rejection and return
  // `{ success: false, error }`. Asserted so the dead branch is documented as
  // dead rather than mistaken for coverage -- and note it, too, says "Failed to
  // load users".
  it("reads the server's body if the call ever does reject", async () => {
    mocks.assignToolTrainer.mockRejectedValue({
      response: { data: { error: 'Already a trainer for this tool' } },
    })
    const w = await modal()
    await w.find('#user').setValue('a')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error').text()).toBe('Already a trainer for this tool')
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
