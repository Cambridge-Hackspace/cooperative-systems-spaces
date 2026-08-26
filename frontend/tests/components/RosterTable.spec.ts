// Tier 2: component conformance for RosterTable.
//
// The member roster, and the only place in the UI where a role is changed or an
// account is deactivated. Three things make it worth exact assertions:
//
// THE SELF-PROTECTION RULES. An admin must not be able to demote or deactivate
// themselves — not because it is destructive in itself, but because it is
// unrecoverable through the UI: the moment they stop being an admin, the
// control that would put them back is hidden from them. There is no
// confirmation dialog on either action, so the guard on rendering the button is
// the whole guard.
//
// THE REFRESH SEAM. `fetchUsers` is exposed so a parent can refresh after a
// change made elsewhere. It was *not* exposed until recently — `<script setup>`
// closes everything by default — so `rosterTable.value.fetchUsers()` in the
// parent was reading `undefined` off the component instance and the refresh
// button did nothing at all. It was invisible because the failure was silent
// and the list happened to be right most of the time.
//
// THE ROLE VOCABULARY. Five roles, each with a label and a badge class, mapped
// twice by hand. A `Record<UserRole, string>` makes the compiler check that both
// maps are total, but not that they agree with the server's five.
//
// WHAT THIS DOES NOT PROVE. That an admin actually may change a role. The
// permission is enforced server-side and asserted by the contract tier; here
// the claim is only about what the UI offers.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createTestingPinia } from '@pinia/testing'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

import RosterTable from '@/components/RosterTable.vue'
import { useAuthStore } from '@/stores/auth'
import { UserRole, type User } from '@/types'

// Typed rather than `vi.fn()`. An untyped mock returns `any`, which makes every
// forwarding wrapper an unsafe return and -- more to the point -- makes a
// response shape the component cannot actually receive indistinguishable from
// one it can.
type Envelope = Promise<{ success: boolean; error?: string; data?: unknown }>

// `vi.hoisted`, because `vi.mock` is hoisted above every `const` in the file and
// a factory that closes over an ordinary top-level binding throws
// "Cannot access 'getAllUsers' before initialization" -- from inside the
// *component's* import, which makes it read like a module-cycle problem in the
// application rather than a hoisting problem in the test.
const mocks = vi.hoisted(() => ({
  getAllUsers: vi.fn(),
  updateUserRole: vi.fn(),
  activateUser: vi.fn(),
  deactivateUser: vi.fn(),
  resetUserMfa: vi.fn(),
}))

vi.mock('@/utils/api', () => ({
  userApi: {
    getAllUsers: mocks.getAllUsers,
    updateUserRole: mocks.updateUserRole,
    activateUser: mocks.activateUser,
    deactivateUser: mocks.deactivateUser,
  },
  adminApi: { resetUserMfa: mocks.resetUserMfa },
  apiClient: {},
}))

// Typed views onto the hoisted mocks, so a response shape the component cannot
// receive is a compile error rather than a test that passes against a fixture
// the product would never send.
const getAllUsers = mocks.getAllUsers as unknown as ReturnType<typeof vi.fn<() => Envelope>>
const updateUserRole = mocks.updateUserRole as unknown as ReturnType<
  typeof vi.fn<(id: string, role: UserRole) => Envelope>
>
const activateUser = mocks.activateUser as unknown as ReturnType<
  typeof vi.fn<(id: string) => Envelope>
>
const deactivateUser = mocks.deactivateUser as unknown as ReturnType<
  typeof vi.fn<(id: string) => Envelope>
>
const resetUserMfa = mocks.resetUserMfa as unknown as ReturnType<
  typeof vi.fn<(id: string) => Envelope>
>

const ADMIN_ID = '00000000-0000-4000-8000-00000000000a'

// The roster links to each member's profile. Stubbed rather than given a real
// router: this tier is about what the component renders, and installing
// vue-router would make every test depend on the route table.
const stubs = { RouterLink: { template: '<a><slot /></a>' } }

function user(overrides: Partial<User> = {}): User {
  return {
    id: '00000000-0000-4000-8000-000000000001',
    username: 'member',
    email: 'member@example.invalid',
    full_name: 'A Member',
    is_active: true,
    role: UserRole.Member,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    mfa_enrolled_at: null,
    profile: {},
    meta: {},
    ...overrides,
  }
}

/** Mount with a signed-in viewer of the given role and a roster of `users`. */
async function mountRoster(role: UserRole, users: User[]) {
  const pinia = createTestingPinia({ createSpy: vi.fn, stubActions: true })
  const auth = useAuthStore(pinia)
  auth.user = user({ id: ADMIN_ID, username: 'viewer', role })
  // `isAdmin` is a getter; @pinia/testing makes getters writable.
  ;(auth as { isAdmin: boolean }).isAdmin = role === UserRole.Admin

  getAllUsers.mockResolvedValue({
    success: true,
    data: { items: users, total: users.length, total_pages: 1 },
  })

  const wrapper = mount(RosterTable, { global: { plugins: [pinia], stubs } })
  await flushPromises()
  return { wrapper, auth }
}

beforeEach(() => {
  for (const m of [getAllUsers, updateUserRole, activateUser, deactivateUser, resetUserMfa]) {
    m.mockReset()
  }
})

describe('the refresh seam', () => {
  it('exposes fetchUsers to its parent', async () => {
    // `<script setup>` closes a component's bindings by default, so without a
    // `defineExpose` the parent's `rosterTable.value.fetchUsers()` reads
    // undefined off the instance. That is not a type error at the call site --
    // the ref is typed loosely -- and it is not a runtime error either if the
    // caller guards; it is simply a refresh button that does nothing.
    const { wrapper } = await mountRoster(UserRole.Admin, [user()])
    const exposed = wrapper.vm as unknown as { fetchUsers?: () => Promise<void> }

    expect(
      typeof exposed.fetchUsers,
      'the parent calls rosterTable.value.fetchUsers(); without defineExpose it is undefined ' +
        'and the refresh silently does nothing'
    ).toBe('function')
  })

  it('actually refetches when the parent calls it', async () => {
    const { wrapper } = await mountRoster(UserRole.Admin, [user()])
    expect(getAllUsers).toHaveBeenCalledTimes(1)

    await (wrapper.vm as unknown as { fetchUsers: () => Promise<void> }).fetchUsers()
    expect(getAllUsers).toHaveBeenCalledTimes(2)
  })
})

describe('what an admin cannot do to themselves', () => {
  it('offers no role editor on the viewer’s own row', async () => {
    // Neither action has a confirmation dialog, so this is the entire guard.
    // An admin who demotes themselves loses the control that would undo it.
    const { wrapper } = await mountRoster(UserRole.Admin, [
      user({ id: ADMIN_ID, username: 'viewer', role: UserRole.Admin }),
      user({ id: 'other', username: 'other' }),
    ])

    const rows = wrapper.findAll('tbody tr')
    expect(rows).toHaveLength(2)

    expect(rows[0].find('button[title="Edit role"]').exists()).toBe(false)
    expect(rows[1].find('button[title="Edit role"]').exists()).toBe(true)
  })

  it('offers no deactivate control on the viewer’s own row', async () => {
    const { wrapper } = await mountRoster(UserRole.Admin, [
      user({ id: ADMIN_ID, username: 'viewer', role: UserRole.Admin }),
      user({ id: 'other', username: 'other' }),
    ])
    const rows = wrapper.findAll('tbody tr')

    expect(rows[0].find('button[title="Deactivate user"]').exists()).toBe(false)
    expect(rows[1].find('button[title="Deactivate user"]').exists()).toBe(true)
  })

  it('compares ids as strings, so a numeric id still matches', async () => {
    // `canToggleStatus` coerces both sides with String(). Comparing a numeric id
    // to a string one with === is false, and the guard silently stops guarding.
    const { wrapper } = await mountRoster(UserRole.Admin, [
      user({ id: ADMIN_ID, username: 'viewer' }),
    ])
    expect(wrapper.find('button[title^="Deactivate"]').exists()).toBe(false)
  })
})

describe('what a non-admin sees', () => {
  it.each([UserRole.Newbie, UserRole.Member, UserRole.Staff])(
    'offers %s no role, status or MFA control',
    async (role) => {
      const { wrapper } = await mountRoster(role, [user({ id: 'other', mfa_enrolled_at: 'x' })])

      expect(wrapper.find('button[title="Edit role"]').exists()).toBe(false)
      expect(wrapper.find('button[title="Deactivate user"]').exists()).toBe(false)
      expect(wrapper.find('button[title="Activate user"]').exists()).toBe(false)
      expect(wrapper.find('button[title*="MFA"]').exists()).toBe(false)
    }
  )

  it('still shows them the roster itself', async () => {
    // The controls are gated, not the page. A member who can see nothing at all
    // is a different bug from one who can see everything.
    const { wrapper } = await mountRoster(UserRole.Member, [user(), user({ id: 'b' })])
    expect(wrapper.findAll('tbody tr')).toHaveLength(2)
  })
})

describe('the role vocabulary', () => {
  const EXPECTED: Array<[UserRole, string, string]> = [
    [UserRole.Unknown, 'Unknown', 'badge-ghost'],
    [UserRole.Newbie, 'Newbie', 'badge-info'],
    [UserRole.Member, 'Member', 'badge-success'],
    [UserRole.Staff, 'Staff', 'badge-warning'],
    [UserRole.Admin, 'Admin', 'badge-error'],
  ]

  it('covers every role the enum declares', () => {
    // The table is hand-written so it is an independent statement. This is what
    // stops it falling behind the enum.
    expect(EXPECTED.map(([r]) => r).sort()).toEqual(Object.values(UserRole).sort())
  })

  it.each(EXPECTED)('renders %s as %s with %s', async (role, label, badge) => {
    const { wrapper } = await mountRoster(UserRole.Member, [user({ id: 'x', role })])
    const cell = wrapper.findAll('tbody td')[2]
    expect(cell.find('.badge').text()).toBe(label)
    expect(cell.find('.badge').classes()).toContain(badge)
  })

  it('offers only the four assignable roles in the editor, never Unknown', async () => {
    // `Unknown` is what the server produces for a role it does not recognise.
    // Offering it as a choice would let an admin assign a value that means
    // "something went wrong".
    const { wrapper } = await mountRoster(UserRole.Admin, [user({ id: 'other' })])
    await wrapper.find('button[title="Edit role"]').trigger('click')

    const options = wrapper.findAll('option').map((o) => o.text())
    expect(options).toEqual(['Newbie', 'Member', 'Staff', 'Admin'])
    expect(options).not.toContain('Unknown')
  })
})

describe('initials', () => {
  it.each([
    ['A Member', 'AM'],
    ['Prince', 'P'],
    ['Ada Byron King', 'AK'],
    ['  Padded  Name  ', 'PN'],
  ])('%s becomes %s', async (full_name, expected) => {
    const { wrapper } = await mountRoster(UserRole.Member, [user({ full_name })])
    expect(wrapper.find('.avatar span').text()).toBe(expected)
  })
})

describe('the states of the table', () => {
  it('shows a spinner while the first fetch is in flight', async () => {
    const pinia = createTestingPinia({ createSpy: vi.fn })
    getAllUsers.mockReturnValue(new Promise(() => {}))
    const wrapper = mount(RosterTable, { global: { plugins: [pinia], stubs } })
    // `isLoading` is set synchronously inside onMounted, but the render that
    // reflects it is not. Asserting before the tick reads the initial DOM.
    await nextTick()
    expect(wrapper.find('.loading-spinner').exists()).toBe(true)
    expect(wrapper.find('table').exists()).toBe(false)
  })

  it('shows the server error and offers a retry', async () => {
    const pinia = createTestingPinia({ createSpy: vi.fn })
    getAllUsers.mockResolvedValue({ success: false, error: 'Forbidden' })
    const wrapper = mount(RosterTable, { global: { plugins: [pinia], stubs } })
    await flushPromises()

    expect(wrapper.find('.alert-error').text()).toContain('Forbidden')
    expect(wrapper.find('table').exists()).toBe(false)
    // And it tells the parent, which is what puts a message somewhere the user
    // is actually looking.
    expect(wrapper.emitted('error')?.[0]?.[0]).toBe('Forbidden')
  })

  it('reports a transport failure rather than an empty roster', async () => {
    // The shape 92afb4c was about, in another component: a rejection with no
    // `response` takes the fallback, and an empty table would otherwise read as
    // "this space has no members".
    const pinia = createTestingPinia({ createSpy: vi.fn })
    getAllUsers.mockRejectedValue(new Error('Network Error'))
    const wrapper = mount(RosterTable, { global: { plugins: [pinia], stubs } })
    await flushPromises()

    expect(wrapper.find('.alert-error').text()).toContain('Network error loading users')
    expect(wrapper.emitted('error')?.[0]?.[0]).toBe('Network error loading users')
  })

  it('shows the total the server reported, not the number of rows on this page', async () => {
    const pinia = createTestingPinia({ createSpy: vi.fn })
    getAllUsers.mockResolvedValue({
      success: true,
      data: { items: [user()], total: 137, total_pages: 7 },
    })
    const wrapper = mount(RosterTable, { global: { plugins: [pinia], stubs } })
    await flushPromises()
    expect(wrapper.find('.stat-value').text()).toBe('137')
  })
})

describe('status changes', () => {
  it('calls deactivate for an active user and activate for an inactive one', async () => {
    updateUserRole.mockResolvedValue({ success: true, data: user() })
    deactivateUser.mockResolvedValue({ success: true, data: user({ is_active: false }) })
    activateUser.mockResolvedValue({ success: true, data: user({ is_active: true }) })

    const active = await mountRoster(UserRole.Admin, [user({ id: 'x', is_active: true })])
    await active.wrapper.find('button[title="Deactivate user"]').trigger('click')
    await flushPromises()
    expect(deactivateUser).toHaveBeenCalledWith('x')
    expect(activateUser).not.toHaveBeenCalled()

    const inactive = await mountRoster(UserRole.Admin, [user({ id: 'y', is_active: false })])
    await inactive.wrapper.find('button[title="Activate user"]').trigger('click')
    await flushPromises()
    expect(activateUser).toHaveBeenCalledWith('y')
  })

  it('replaces the row with the server’s version rather than guessing', async () => {
    // Optimistically flipping `is_active` locally would show Active on a row the
    // server refused to change, and the next refresh would flip it back with no
    // explanation.
    deactivateUser.mockResolvedValue({
      success: true,
      data: user({ id: 'x', is_active: false, role: UserRole.Newbie }),
    })
    const { wrapper } = await mountRoster(UserRole.Admin, [
      user({ id: 'x', is_active: true, role: UserRole.Member }),
    ])
    await wrapper.find('button[title="Deactivate user"]').trigger('click')
    await flushPromises()

    const cells = wrapper.findAll('tbody td')
    expect(cells[3].find('.badge').text()).toBe('Inactive')
    // The role came back changed too, and the row shows what the server said.
    expect(cells[2].find('.badge').text()).toBe('Newbie')
  })

  it('surfaces a refused status change without destroying the roster', async () => {
    // The defect this test was written against: `toggleUserStatus` wrote the
    // failure into `error`, which the template renders *instead of* the table.
    // One refused toggle therefore replaced every row with a banner, and the
    // only way back was a manual retry -- for a failure that concerned one row.
    //
    // The action channel is the emit; `error` is the load channel. Keeping them
    // separate is the fix, and the assertion that the table is still there is
    // the part that would have caught it.
    deactivateUser.mockResolvedValue({ success: false, error: 'Cannot deactivate the last admin' })
    const { wrapper } = await mountRoster(UserRole.Admin, [user({ id: 'x', is_active: true })])
    await wrapper.find('button[title="Deactivate user"]').trigger('click')
    await flushPromises()

    expect(wrapper.emitted('error')?.[0]?.[0]).toBe('Cannot deactivate the last admin')
    expect(wrapper.find('table').exists(), 'the roster was replaced by an error banner').toBe(true)
    expect(wrapper.findAll('tbody td')[3].find('.badge').text()).toBe('Active')
  })

  it('surfaces a refused role change without destroying the roster either', async () => {
    updateUserRole.mockResolvedValue({ success: false, error: 'Not permitted' })
    const { wrapper } = await mountRoster(UserRole.Admin, [user({ id: 'x' })])
    await wrapper.find('button[title="Edit role"]').trigger('click')
    await wrapper.find('select').setValue(UserRole.Staff)
    await flushPromises()

    expect(wrapper.emitted('error')?.[0]?.[0]).toBe('Not permitted')
    expect(wrapper.find('table').exists()).toBe(true)
  })
})
