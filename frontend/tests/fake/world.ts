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
  /** Tokens the fake has issued, mapped to the user they belong to. */
  sessions = new Map<string, string>()
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
    ]
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
    this.sessions.clear()
    this.armed = []
    this.requests = []
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

  login(usernameOrEmail: string, password: string): { token: string; user: User } | null {
    if (password !== PASSWORD) return null
    const found = this.users.find(
      (u) => u.username === usernameOrEmail || u.email === usernameOrEmail
    )
    if (!found) return null
    const token = `fake-token-${found.id}-${this.sessions.size}`
    this.sessions.set(token, found.id)
    return { token, user: found }
  }
}

export const PASSWORD_FOR_SPECS = PASSWORD
