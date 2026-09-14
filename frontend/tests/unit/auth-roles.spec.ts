import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useAuthStore } from '@/stores/auth'
import { UserRole, isMfaChallenge, type User } from '@/types'

/**
 * The RBAC tier hierarchy, written out here on purpose.
 *
 * It exists verbatim in the two client `roleHierarchy` maps already —
 * `stores/auth.ts` (`hasRole`) and `router/index.ts` (the nav guard) — and the
 * server resolves the same ladder from the seeded `roles.level` column. A test
 * that imported one of them would agree with it no matter what it said. This
 * copy is the independent statement; `tests/structure/role-hierarchy.spec.ts`
 * asserts the two TypeScript copies still match it.
 *
 * Levels are the dev taxonomy (guest 1 … admin 5); there is no level-0 role —
 * `guest` is the logged-in baseline that replaced the old `unknown`/`newbie`.
 */
const LEVELS: ReadonlyArray<[UserRole, number]> = [
  [UserRole.Guest, 1],
  [UserRole.Historical, 2],
  [UserRole.Active, 3],
  [UserRole.Staff, 4],
  [UserRole.Admin, 5],
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
          userLevel >= requiredLevel
        )
      }
    }
  })

  // Names a real weakness rather than papering over it.
  //
  // `roleHierarchy[userRoleString] || 0` maps anything unrecognized to 0. That
  // is fail-closed for the *user's* role, which is right. But it does the same
  // for the *required* role, so a guard asking for a role that does not exist
  // — a typo, a role removed from the server but not the client — resolves to
  // level 0 and admits everyone, including Unknown.
  //
  // Pinned as-is because changing it is a behavior change to the authorization
  // path and belongs with the server-side matrix work, not smuggled in here.
  it('admits everyone when the *required* role is unrecognized, which is fail-open', () => {
    const store = signedInAs(UserRole.Guest)
    expect(store.hasRole('supervisor' as UserRole)).toBe(true)
  })
})

describe('role getters', () => {
  const table: ReadonlyArray<[UserRole, boolean, boolean, boolean]> = [
    // role, isAdmin, isStaff, isMember
    [UserRole.Guest, false, false, false],
    [UserRole.Historical, false, false, false],
    [UserRole.Active, false, false, true],
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

  // The wire format is now lowercase ("admin"), but the getters still lowercase
  // defensively. Feeding a mixed-case value proves that normalization is intact:
  // this is the assertion that would catch someone "tidying" the toLowerCase
  // away, which would silently make a mixed-case role fail every gate.
  it('normalize case, so even a mixed-case role is recognized', () => {
    const store = useAuthStore()
    // Cast through `unknown`: 'Admin' is deliberately not a `UserRole` value
    // (the wire is lowercase now), and the point is that the getter still
    // normalizes it.
    store.user = { role: 'Admin' } as unknown as User
    expect(store.isAdmin).toBe(true)
  })
})

describe('isAuthenticated', () => {
  it('needs both a token and a user', () => {
    const store = useAuthStore()
    store.token = 'tok'
    store.user = null
    expect(store.isAuthenticated).toBe(false)

    store.user = { role: UserRole.Active } as User
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
