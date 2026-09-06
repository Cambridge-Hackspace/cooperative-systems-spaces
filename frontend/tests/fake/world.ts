// Tier 5's fake server: the world it serves, and the faults it can inject.
//
// A *fake*, not a mock. It holds state, it answers consistently, and a
// sequence of requests behaves the way a sequence of requests does. A mock
// returns canned responses and would make every spec a restatement of what the
// spec author expected the client to send.
//
// WHAT IT DELIBERATELY DOES NOT DECIDE. It never re-implements a rule the
// application owns. `validateProfile` is imported from `@/stores/profile` and
// the enums from `@/types`, so the fake decides *what to answer* and never
// *whether an answer is valid*. A fake that reimplements validation agrees with
// its own copy of the rules, which is the failure mode that makes fake-API
// tiers worthless.
//
// THE FOUR INJECTION SHAPES, and why there are four rather than one:
//
//   failNext     an HTTP error response. The common case, and the only one most
//                suites inject.
//   abortNext    the connection drops. **This is the important one.** axios
//                attaches a `response` to an HTTP error, so
//                `e?.response?.data?.error || 'Failed to load door'` takes its
//                first branch for every `failNext` and its fallback for no
//                `failNext` at all. Only a transport-level abort reaches the
//                fallback — which is exactly the branch 92afb4c added and
//                exactly the branch a 500-only suite never executes.
//   hangNext     the request never answers. Catches spinners with no timeout
//                and buttons that stay disabled forever.
//   malformNext  a 200 with a body that is not what the type says. Catches
//                `response.data.items.map` on a response whose `items` is
//                absent, which is a TypeError in a promise nobody awaited.
//
// Each is armed for the next matching request and consumed by it, so a spec
// says "the next call to this endpoint fails" rather than "the server is
// broken", and the assertions after it are about recovery.

// Relative, not aliased: this module is reachable from the Vite plugin, which
// Node evaluates before the `@/` alias exists.
import { ProfileFieldType, UserRole, type ProfileField, type User } from '../../src/types'

export type Injection = 'failNext' | 'abortNext' | 'hangNext' | 'malformNext'

export interface Armed {
  kind: Injection
  /** Matched as a prefix against the request path, without the /api root. */
  path: string
  /** For failNext. */
  status?: number
  /** For failNext: the body. Defaults to the standard error envelope. */
  body?: unknown
  /**
   * How many matching requests this fault applies to. Defaults to 1.
   *
   * Kept because a spec occasionally wants a fault to survive a retry the
   * *application* makes -- but it is no longer how `abortNext` defeats the
   * browser's retry. That was the first attempt, and raising a retry budget is
   * a guessing game: the browser's retry count is not a contract, and the log
   * showed Chromium going three deep and succeeding on the fourth request.
   *
   * `abortNext` now sends headers and a partial body before dropping the
   * connection, which is a truncated response and is not retried at all. See
   * the note in plugin.ts.
   */
  times?: number
}

const PASSWORD = 'fake-password'

/**
 * The code this fake accepts as a valid authenticator code.
 *
 * A constant, and deliberately not a real HMAC. The fake's rule is that it
 * decides *what to answer* and never *whether an answer is valid* -- but TOTP
 * is the one place where honoring that literally would mean reimplementing
 * RFC 6238 in the fake, which is exactly the "agrees with its own copy of the
 * rules" failure the header warns about.
 *
 * So the split is drawn differently here: **the cryptography is not this
 * tier's question.** Whether a code verifies is settled deterministically in
 * `server/src/mfa.rs` (skew, digits, wrong secret) and end to end against a
 * real HMAC in the stack battery's `mfa` stage. What this tier owns is the
 * protocol around it -- that a challenge does not authenticate, that a wrong
 * code does not, that a spent challenge cannot be reused -- and none of that
 * depends on how the six digits were derived.
 */
const VALID_TOTP = '123456'

function user(over: Partial<User> = {}): User {
  return {
    id: '00000000-0000-4000-8000-000000000001',
    username: 'member',
    email: 'member@fake.invalid',
    full_name: 'A Member',
    is_active: true,
    role: UserRole.Member,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    mfa_enrolled_at: null,
    profile: {},
    meta: {},
    ...over,
  }
}

/** The whole world, rebuilt for each test run so specs cannot leak into each other. */
export class World {
  users: User[] = []
  profileFields: ProfileField[] = []
  profilesEnabled = true
  tools: Array<Record<string, unknown>> = []
  doors: Array<Record<string, unknown>> = []
  doorRules: Array<Record<string, unknown>> = []
  /** cmi5 modules a learner can launch; `completed` flips when the content in the
   *  player iframe reports a pass to the fake LRS. */
  cmi5Modules: Array<Record<string, unknown>> = []
  /** Tokens the fake has issued, mapped to the user they belong to. */
  sessions = new Map<string, string>()
  /**
   * Outstanding MFA challenges, mapped to the user awaiting verification.
   *
   * A map rather than a single value because the store on the real server is
   * one too, and because a spec that abandons one challenge and starts another
   * must not find the first still spendable.
   */
  challenges = new Map<string, string>()
  /** Unspent recovery codes for the enrolled user. Consumed on use. */
  recoveryCodes: string[] = []
  armed: Armed[] = []
  /** Every request the fake has seen, so a spec can assert on retries. */
  requests: Array<{ method: string; path: string }> = []

  constructor() {
    this.reset()
  }

  reset() {
    this.users = [
      user({ id: 'admin-1', username: 'ada', email: 'ada@fake.invalid', role: UserRole.Admin }),
      user({ id: 'member-1', username: 'grace', email: 'grace@fake.invalid' }),
      user({
        id: 'newbie-1',
        username: 'alan',
        email: 'alan@fake.invalid',
        role: UserRole.Newbie,
      }),
      // The only user in the world with a second factor. Without one, the
      // browser tier never takes the challenge branch of `/auth/login` --
      // which is how the entire MFA login flow went untested at this tier
      // while `mfa_enrolled_at: null` sat in the fixture looking deliberate.
      user({
        id: 'enrolled-1',
        username: 'hedy',
        email: 'hedy@fake.invalid',
        mfa_enrolled_at: '2026-01-02T00:00:00Z',
      }),
    ]
    this.challenges.clear()
    this.recoveryCodes = ['ABCD-EFGH-JKLM', 'NPQR-STUV-WXYZ']
    this.profileFields = [
      {
        key: 'card_id',
        label: 'Access Card',
        field_type: ProfileFieldType.Text,
        required: false,
        help_text: 'RFID card identifier',
      },
      {
        key: 'bio',
        label: 'Bio',
        field_type: ProfileFieldType.Text,
        required: false,
      },
    ]
    this.profilesEnabled = true
    this.tools = [
      {
        id: 'tool-1',
        name: 'Bandsaw',
        category: 'saw',
        status: 'idle',
        description: null,
        location: 'Bay 3',
        manufacturer: null,
        model: null,
        serial_number: null,
        purchase_date: null,
        purchase_price: null,
        created_at: '2026-01-01T00:00:00Z',
        updated_at: '2026-01-01T00:00:00Z',
      },
    ]
    this.doors = [
      {
        id: 'door-1',
        name: 'Front Door',
        location: 'Entrance',
        enabled: true,
        you_are_authorized: true,
        reason: null,
      },
    ]
    this.doorRules = []
    this.cmi5Modules = [
      {
        au_id: 'au-1',
        au_title: 'Fake Safety Module',
        course_id: 'course-1',
        course_title: 'Fake Course',
        tool_id: 'tool-1',
        training_step_id: 'step-1',
        completed: false,
      },
    ]
    this.sessions.clear()
    this.armed = []
    this.requests = []
  }

  /** Mark the module for an AU completed, as the LRS would after a pass. */
  markCmi5Completed(auId: string) {
    for (const m of this.cmi5Modules) {
      if (m.au_id === auId) m.completed = true
    }
  }

  /** Arm a fault for the next request(s) whose path starts with `path`. */
  arm(a: Armed) {
    this.armed.push({
      ...a,
      times: a.times ?? 1,
    })
  }

  /**
   * The armed fault matching this path, if any.
   *
   * Decrements its remaining count and removes it when exhausted, so a
   * multi-use fault survives a browser-level retry without staying armed for
   * the rest of the run.
   */
  takeArmed(path: string): Armed | undefined {
    const i = this.armed.findIndex((a) => path.startsWith(a.path))
    if (i < 0) return undefined
    const armed = this.armed[i]
    if (!armed) return undefined
    const remaining = (armed.times ?? 1) - 1
    if (remaining <= 0) this.armed.splice(i, 1)
    else this.armed[i] = { ...armed, times: remaining }
    return armed
  }

  userByToken(token: string | undefined): User | undefined {
    if (!token) return undefined
    const id = this.sessions.get(token)
    return this.users.find((u) => u.id === id)
  }

  login(
    usernameOrEmail: string,
    password: string
  ):
    | { kind: 'ok'; token: string; user: User }
    | { kind: 'mfa'; challengeToken: string; methods: string[] }
    | null {
    if (password !== PASSWORD) return null
    const found = this.users.find(
      (u) => u.username === usernameOrEmail || u.email === usernameOrEmail
    )
    if (!found) return null

    // The branch the real server takes at api/auth.rs:242. A user with a
    // second factor gets a challenge, never a token -- so nothing in
    // `sessions` is created here, and there is nothing for the client to
    // mistake for a session.
    if (found.mfa_enrolled_at) {
      const challengeToken = `fake-challenge-${found.id}-${this.challenges.size}`
      this.challenges.set(challengeToken, found.id)
      const methods = ['totp']
      if (this.recoveryCodes.length > 0) methods.push('recovery')
      return { kind: 'mfa', challengeToken, methods }
    }

    return { kind: 'ok', ...this.issueSession(found) }
  }

  private issueSession(u: User): { token: string; user: User } {
    const token = `fake-token-${u.id}-${this.sessions.size}`
    this.sessions.set(token, u.id)
    return { token, user: u }
  }

  /**
   * Complete a challenge, or refuse it.
   *
   * The challenge is consumed **before** the code is judged, which is what the
   * real server does: `verify_login` calls `take_login` at the top, so a wrong
   * code costs the user the whole challenge and not just the attempt. Getting
   * this wrong in the fake would hide the finding pinned in
   * `tests/e2e/mfa-login.spec.ts`.
   */
  verifyMfa(
    challengeToken: string,
    method: string,
    code: string
  ): { token: string; user: User } | { error: string; status: number } {
    const userId = this.challenges.get(challengeToken)
    if (!userId) {
      return { error: 'Unknown or expired challenge_token', status: 401 }
    }
    this.challenges.delete(challengeToken)

    const found = this.users.find((u) => u.id === userId)
    if (!found) return { error: 'User no longer exists', status: 401 }

    if (method === 'totp') {
      if (code !== VALID_TOTP) return { error: 'Invalid TOTP code', status: 401 }
      return this.issueSession(found)
    }
    if (method === 'recovery') {
      const i = this.recoveryCodes.indexOf(code)
      if (i < 0) return { error: 'Invalid recovery code', status: 401 }
      // Single use, like the real `mark_recovery_code_used`.
      this.recoveryCodes.splice(i, 1)
      return this.issueSession(found)
    }
    return { error: `Unknown method: ${method}`, status: 400 }
  }
}

export const PASSWORD_FOR_SPECS = PASSWORD
export const VALID_TOTP_FOR_SPECS = VALID_TOTP
