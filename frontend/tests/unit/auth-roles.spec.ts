import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useAuthStore } from '@/stores/auth'
import { UserRole, isMfaChallenge, type User } from '@/types'

/**
 * The role hierarchy, written out here on purpose.
 *
 * It exists verbatim in three places already — `stores/auth.ts:180-186`,
 * `router/index.ts:222-228`, and as `UserRole::can_access_*` on the server
 * (`server/src/models.rs:83-99`). A test that imported one of them would agree
 * with it no matter what it said. This fourth copy is the independent
 * statement; `tests/structure/role-hierarchy.spec.ts` asserts the two
 * TypeScript copies still match it.
 */
const LEVELS: ReadonlyArray<[UserRole, number]> = [
  [UserRole.Unknown, 0],
  [UserRole.Newbie, 1],
  [UserRole.Member, 2],
  [UserRole.Staff, 3],
  [UserRole.Admin, 4],
]

function signedInAs(role: UserRole) {
  const store = useAuthStore()
  store.user = { id: 'u', username: 'u', email: 'u@e.com', full_name: 'U', role } as User
  store.token = 'tok'
  return store
}

beforeEach(() => setActivePinia(createPinia()))

describe('hasRole', () => {
  it('is false for every role when nobody is signed in', () => {
    const store = useAuthStore()
    store.user = null
    for (const [role] of LEVELS) expect(store.hasRole(role)).toBe(false)
  })

  it('grants exactly the roles at or below the user, for every pair', () => {
    // The whole matrix, not a sample: 25 cells, each one a statement about who
    // can reach what.
    for (const [userRole, userLevel] of LEVELS) {
      const store = signedInAs(userRole)
      for (const [requiredRole, requiredLevel] of LEVELS) {
        expect(store.hasRole(requiredRole), `${userRole} asked for ${requiredRole}`).toBe(
          userLevel >= requiredLevel,
        )
      }
    }
  })

  // Names a real weakness rather than papering over it.
  //
  // `roleHierarchy[userRoleString] || 0` maps anything unrecognised to 0. That
  // is fail-closed for the *user's* role, which is right. But it does the same
  // for the *required* role, so a guard asking for a role that does not exist
  // — a typo, a role removed from the server but not the client — resolves to
  // level 0 and admits everyone, including Unknown.
  //
  // Pinned as-is because changing it is a behaviour change to the authorization
  // path and belongs with the server-side matrix work, not smuggled in here.
  it('admits everyone when the *required* role is unrecognised, which is fail-open', () => {
    const store = signedInAs(UserRole.Unknown)
    expect(store.hasRole('supervisor' as UserRole)).toBe(true)
  })
})

describe('role getters', () => {
  const table: ReadonlyArray<[UserRole, boolean, boolean, boolean]> = [
    // role, isAdmin, isStaff, isMember
    [UserRole.Unknown, false, false, false],
    [UserRole.Newbie, false, false, false],
    [UserRole.Member, false, false, true],
    [UserRole.Staff, false, true, true],
    [UserRole.Admin, true, true, true],
  ]

  it('agree with the hierarchy for every role', () => {
    for (const [role, admin, staff, member] of table) {
      const store = signedInAs(role)
      expect([store.isAdmin, store.isStaff, store.isMember], role).toEqual([admin, staff, member])
    }
  })

  it('are all false with no user', () => {
    const store = useAuthStore()
    store.user = null
    expect([store.isAdmin, store.isStaff, store.isMember]).toEqual([false, false, false])
  })

  // The getters lowercase before comparing; the wire format is PascalCase.
  // This is the assertion that would catch someone "tidying" the toLowerCase
  // away, which would silently make every gate false.
  it('accept the PascalCase the server actually sends', () => {
    const store = useAuthStore()
    store.user = { role: 'Admin' } as User
    expect(store.isAdmin).toBe(true)
  })
})

describe('isAuthenticated', () => {
  it('needs both a token and a user', () => {
    const store = useAuthStore()
    store.token = 'tok'
    store.user = null
    expect(store.isAuthenticated).toBe(false)

    store.user = { role: UserRole.Member } as User
    expect(store.isAuthenticated).toBe(true)

    store.token = null
    expect(store.isAuthenticated).toBe(false)
  })
})

describe('isMfaChallenge', () => {
  it('is true only for an object carrying mfa_required === true', () => {
    expect(isMfaChallenge({ mfa_required: true })).toBe(true)
    expect(isMfaChallenge({ mfa_required: false })).toBe(false)
    // Deliberately strict: the string "true" is not a challenge. A login
    // response misread as a challenge would strand the user on an MFA prompt
    // they cannot satisfy.
    expect(isMfaChallenge({ mfa_required: 'true' })).toBe(false)
    expect(isMfaChallenge({})).toBe(false)
    expect(isMfaChallenge(null)).toBe(false)
    expect(isMfaChallenge(undefined)).toBe(false)
    expect(isMfaChallenge('mfa_required')).toBe(false)
    expect(isMfaChallenge(0)).toBe(false)
  })
})
