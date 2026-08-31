// Tier 1: the auth store's login flow.
//
// This is the branch that decides whether a password alone is enough, and
// nothing exercised it. `tests/unit/auth-roles.spec.ts` covers `isMfaChallenge`
// as a predicate, which is the *input* to the decision; the decision itself --
// `stores/auth.ts:71`, where a challenge response either does or does not
// authenticate the user -- had no test at any tier.
//
// The property that matters here is negative and cheap to lose: when the server
// answers a password with a challenge, the store must not set a token, must not
// mark the session authenticated, and must not write anything to localStorage.
// A refactor that hoisted the token assignment above the `isMfaChallenge` check
// would still return 'mfa', still route to the challenge form, and still hand
// the user a working session behind it -- and every other test in this
// repository would stay green.
//
// The store is mocked at `apiClient`, not at axios: `utils/api.ts` and
// `stores/auth.ts` import each other, and mocking one half of that cycle is
// what `tests/README.md` warns produces failures that look like a Vite bug.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  post: vi.fn(),
  put: vi.fn(),
  delete: vi.fn(),
}))

vi.mock('@/utils/api', () => ({ apiClient: mocks }))

import { useAuthStore } from '@/stores/auth'
import { UserRole, type User, type MfaChallenge } from '@/types'

const USER: User = {
  id: 'u1',
  username: 'ada',
  email: 'ada@example.invalid',
  full_name: 'Ada Lovelace',
  role: UserRole.Member,
  is_active: true,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
} as User

const CHALLENGE: MfaChallenge = {
  mfa_required: true,
  challenge_token: 'chal-abc',
  methods: ['totp', 'recovery'],
  webauthn_options: null,
}

/** What `/auth/login` answers when the password was enough. */
function tokenResponse(over: Record<string, unknown> = {}) {
  return {
    success: true,
    data: { token: 'jwt-xyz', user: USER, expires_in: 86400, ...over },
  }
}

beforeEach(() => {
  setActivePinia(createPinia())
  for (const m of Object.values(mocks)) m.mockReset()
})

describe('a password that is not enough on its own', () => {
  it('reports a challenge and leaves the session unauthenticated', async () => {
    // The headline. Every assertion here is about something that must *not*
    // have happened.
    mocks.post.mockResolvedValue({ success: true, data: CHALLENGE })
    const auth = useAuthStore()

    const result = await auth.login({ username_or_email: 'ada', password: 'pw' })

    expect(result).toBe('mfa')
    expect(auth.pendingMfa).toEqual(CHALLENGE)
    expect(auth.token, 'a challenge must not issue a token').toBeNull()
    expect(auth.user, 'a challenge must not populate the user').toBeNull()
    expect(auth.isAuthenticated).toBe(false)
    expect(
      localStorage.getItem('css_token'),
      'a challenge wrote a token to localStorage, so a page reload would ' +
        'restore a session the second factor never approved'
    ).toBeNull()
  })

  it('does not report the user as holding any role', async () => {
    // `hasRole` and the `isAdmin` / `isStaff` / `isMember` getters are what the
    // router guard and every admin control read. Mid-challenge they must all
    // answer no, whatever role the account actually carries.
    mocks.post.mockResolvedValue({
      success: true,
      data: { ...CHALLENGE, methods: ['totp'] },
    })
    const auth = useAuthStore()
    await auth.login({ username_or_email: 'root', password: 'pw' })

    expect(auth.isAdmin).toBe(false)
    expect(auth.isStaff).toBe(false)
    expect(auth.isMember).toBe(false)
    expect(auth.hasRole(UserRole.Newbie)).toBe(false)
  })

  it('carries the methods and the token the challenge form needs', async () => {
    mocks.post.mockResolvedValue({ success: true, data: CHALLENGE })
    const auth = useAuthStore()
    await auth.login({ username_or_email: 'ada', password: 'pw' })

    expect(auth.pendingMfa?.challenge_token).toBe('chal-abc')
    expect(auth.pendingMfa?.methods).toEqual(['totp', 'recovery'])
  })

  it('clears a challenge left over from an abandoned attempt', async () => {
    // Otherwise a user who backs out of one login and starts another sees the
    // challenge form for the *previous* attempt, whose token is now stale --
    // and the failure reads as "Unknown or expired challenge_token".
    const auth = useAuthStore()
    mocks.post.mockResolvedValue({ success: true, data: CHALLENGE })
    await auth.login({ username_or_email: 'ada', password: 'pw' })
    expect(auth.pendingMfa).not.toBeNull()

    mocks.post.mockResolvedValue({ success: false, error: 'Invalid credentials' })
    await auth.login({ username_or_email: 'ada', password: 'wrong' })

    expect(auth.pendingMfa).toBeNull()
  })
})

describe('a password that is enough', () => {
  it('signs the user in and persists the token', async () => {
    // The positive half. Without it, a store that refused every login would
    // satisfy every assertion in the block above.
    mocks.post.mockResolvedValue(tokenResponse())
    const auth = useAuthStore()

    expect(await auth.login({ username_or_email: 'ada', password: 'pw' })).toBe('ok')
    expect(auth.token).toBe('jwt-xyz')
    expect(auth.user).toEqual(USER)
    expect(auth.isAuthenticated).toBe(true)
    expect(auth.pendingMfa).toBeNull()
    expect(localStorage.getItem('css_token')).toBe('jwt-xyz')
  })

  it('passes the credentials through unchanged', async () => {
    mocks.post.mockResolvedValue(tokenResponse())
    const auth = useAuthStore()
    await auth.login({ username_or_email: ' Ada ', password: 'pw' })

    expect(mocks.post).toHaveBeenCalledWith('/auth/login', {
      username_or_email: ' Ada ',
      password: 'pw',
    })
  })

  it('remembers when the server said this user still has to enroll', async () => {
    // Drives the redirect to /profile/mfa. A dropped flag is a user who was
    // told to enroll and then silently was not asked.
    mocks.post.mockResolvedValue(tokenResponse({ must_enroll_mfa: true }))
    const auth = useAuthStore()
    await auth.login({ username_or_email: 'ada', password: 'pw' })

    expect(auth.mustEnrollMfa).toBe(true)
  })

  it('does not invent the enrollment demand when the server omitted it', async () => {
    mocks.post.mockResolvedValue(tokenResponse())
    const auth = useAuthStore()
    await auth.login({ username_or_email: 'ada', password: 'pw' })

    expect(auth.mustEnrollMfa).toBe(false)
  })

  it('clears the flag between logins', async () => {
    const auth = useAuthStore()
    mocks.post.mockResolvedValue(tokenResponse({ must_enroll_mfa: true }))
    await auth.login({ username_or_email: 'ada', password: 'pw' })
    expect(auth.mustEnrollMfa).toBe(true)

    mocks.post.mockResolvedValue(tokenResponse())
    await auth.login({ username_or_email: 'bob', password: 'pw' })
    expect(auth.mustEnrollMfa).toBe(false)
  })
})

describe('a login that fails', () => {
  it("reports the server's own words", async () => {
    mocks.post.mockResolvedValue({ success: false, error: 'Invalid credentials' })
    const auth = useAuthStore()

    expect(await auth.login({ username_or_email: 'ada', password: 'no' })).toBe('error')
    expect(auth.error).toBe('Invalid credentials')
    expect(auth.token).toBeNull()
    expect(localStorage.getItem('css_token')).toBeNull()
  })

  it('does not leave a rejection to escape the caller', async () => {
    mocks.post.mockRejectedValue(new Error('Network Error'))
    const auth = useAuthStore()

    expect(await auth.login({ username_or_email: 'ada', password: 'pw' })).toBe('error')
    expect(auth.error).toBe('Network error during login')
    expect(auth.isAuthenticated).toBe(false)
  })

  it('frees the loading flag whichever way it ended', async () => {
    const auth = useAuthStore()
    for (const arrange of [
      () => mocks.post.mockResolvedValue(tokenResponse()),
      () => mocks.post.mockResolvedValue({ success: false, error: 'no' }),
      () => mocks.post.mockResolvedValue({ success: true, data: CHALLENGE }),
      () => mocks.post.mockRejectedValue(new Error('Network Error')),
    ]) {
      arrange()
      await auth.login({ username_or_email: 'ada', password: 'pw' })
      expect(auth.isLoading, 'the login button would stay disabled').toBe(false)
    }
  })
})

describe('completing the second factor', () => {
  it('signs the user in with what /verify returned', async () => {
    const auth = useAuthStore()
    mocks.post.mockResolvedValue({ success: true, data: CHALLENGE })
    await auth.login({ username_or_email: 'ada', password: 'pw' })

    auth.completeMfa({ token: 'jwt-after-mfa', user: USER, expires_in: 86400 })

    expect(auth.token).toBe('jwt-after-mfa')
    expect(auth.user).toEqual(USER)
    expect(auth.isAuthenticated).toBe(true)
    expect(localStorage.getItem('css_token')).toBe('jwt-after-mfa')
    expect(auth.pendingMfa, 'the spent challenge must not linger').toBeNull()
  })

  it('carries the enrollment demand through the second factor too', () => {
    // A user whose role newly requires MFA but who logged in with a passkey
    // they already had: the server still says enroll, and the redirect after
    // /verify is the only place that is read.
    const auth = useAuthStore()
    auth.completeMfa({
      token: 't',
      user: USER,
      expires_in: 1,
      must_enroll_mfa: true,
    })
    expect(auth.mustEnrollMfa).toBe(true)
  })

  it('abandoning the challenge does not authenticate anybody', async () => {
    const auth = useAuthStore()
    mocks.post.mockResolvedValue({ success: true, data: CHALLENGE })
    await auth.login({ username_or_email: 'ada', password: 'pw' })

    auth.cancelMfa()

    expect(auth.pendingMfa).toBeNull()
    expect(auth.token).toBeNull()
    expect(auth.isAuthenticated).toBe(false)
    expect(localStorage.getItem('css_token')).toBeNull()
  })
})
