// Tier 5's fake API, as a Vite dev-server middleware.
//
// A middleware plugin rather than a standalone server, and the choice does a
// lot of work:
//
//   * `@/` resolves and TypeScript transpiles, so the fake can import the real
//     `validateProfile`, the real enums and the real types. It decides *what*
//     to answer and never *whether* an answer is valid — a fake that
//     reimplements the rules agrees with its own copy of them.
//   * the fake and the real bundle share **one origin**, so `baseURL: '/api'`
//     is exercised exactly as written. A separate server would need a proxy or
//     CORS, and would then be testing the proxy.
//   * there is one process to start and one port to wait for.
//
// The control surface lives under `/__fake`, outside `/api`, so nothing the
// application can reach touches it and no route in the fake's own table can
// collide with a real one.

import type { Connect, Plugin } from 'vite'
import type { IncomingMessage, ServerResponse } from 'node:http'

import { validateProfileAgainst } from '../../src/lib/profileValidation'
import { UserRole } from '../../src/types'
import { World, type Injection } from './world'

const world = new World()

// The real validator, imported directly.
//
// It lives in `src/lib/profileValidation.ts` rather than in the Pinia store for
// exactly this reason: **a Vite plugin is evaluated by Node when the config
// loads, before the `@/` alias exists**, so anything it imports has to resolve
// without the alias -- and `stores/profile.ts` imports `@/utils/api`.
//
// The first version of this file went through the store and failed in the
// session with `Cannot find package '@/utils'`, from inside a
// `vite.fake.config.ts.timestamp-*.mjs` -- an error naming a temporary file
// nobody wrote. Extracting the rules is both the fix and the shape they should
// have had: given a profile and a field list, is it valid? Nothing about that
// needs a store.
//
// The alternative -- a copy of the rules in this file -- would agree with itself
// no matter what the application did, which is the failure that makes fake-API
// tiers worthless.
function validateAgainstTheRealRules(profile: Record<string, unknown>) {
  return validateProfileAgainst(profile, world.profileFields)
}

function json(res: ServerResponse, status: number, body: unknown) {
  const text = JSON.stringify(body)
  res.statusCode = status
  res.setHeader('content-type', 'application/json')
  res.setHeader('content-length', Buffer.byteLength(text))
  res.end(text)
}

const ok = (res: ServerResponse, data: unknown, message?: string) =>
  json(res, 200, { success: true, data, ...(message ? { message } : {}) })

const err = (res: ServerResponse, status: number, error: string) =>
  json(res, status, { success: false, error })

function readBody(req: IncomingMessage): Promise<unknown> {
  return new Promise((resolve) => {
    const chunks: Buffer[] = []
    req.on('data', (c: Buffer) => chunks.push(c))
    req.on('end', () => {
      const raw = Buffer.concat(chunks).toString('utf8')
      if (!raw) return resolve({})
      try {
        resolve(JSON.parse(raw))
      } catch {
        resolve({ __unparseable: raw })
      }
    })
  })
}

/**
 * Every path the fake serves.
 *
 * Written out so the 404 can be decided before the credential gate, the way the
 * real router does it. Keeping it in step with the handlers below is the cost;
 * the alternative is a fake whose 401/404 precedence differs from the server's,
 * which is the one thing a fake must not get wrong -- the frontend logs the
 * user out on any 401.
 */
const KNOWN_PATHS: RegExp[] = [
  /^\/config\/public$/,
  /^\/calendar\/events$/,
  /^\/public\/(schedules|home-links)$/,
  /^\/auth\/(login|logout|me)$/,
  /^\/auth\/mfa\/verify$/,
  /^\/profiles\/config$/,
  /^\/profiles\/[^/]+$/,
  /^\/tools$/,
  /^\/users$/,
  /^\/doors\/[^/]+\/(info|checkin)$/,
]

/** A string, or the empty string. Never "[object Object]". */
function asText(v: unknown): string {
  return typeof v === 'string' ? v : ''
}

function bearer(req: IncomingMessage): string | undefined {
  const h = req.headers.authorization
  if (!h || !h.startsWith('Bearer ')) return undefined
  return h.slice('Bearer '.length)
}

/**
 * Apply an armed fault, if one matches. Returns true when the request was
 * answered (or deliberately abandoned) and the handler should not run.
 */
function injected(path: string, res: ServerResponse): boolean {
  const armed = world.takeArmed(path)
  if (!armed) return false

  switch (armed.kind) {
    case 'failNext':
      json(
        res,
        armed.status ?? 500,
        armed.body ?? {
          success: false,
          error: 'Injected failure',
        }
      )
      return true

    case 'abortNext':
      // Headers first, then a partial body, THEN destroy the socket.
      //
      // Destroying it before any bytes is the obvious implementation and it
      // does not work: **Chromium automatically retries an idempotent GET when
      // a persistent connection closes before any response is written.** The
      // fake's own log showed it retrying three times and succeeding on the
      // fourth, so the application never saw a transport failure and the test
      // asserted an error message that had no reason to exist.
      //
      // Raising the retry budget was the first fix and it is a guessing game --
      // the browser's retry count is not a contract. Announcing a
      // `content-length` far larger than what is sent and then dropping the
      // connection is a *truncated response*, which no browser retries: the
      // request is surfaced as a network error, which is exactly the shape with
      // no `response` on the axios error and therefore the only one that
      // reaches a `|| 'fallback'` branch.
      //
      // It is also the more realistic failure. A connection that dies mid-flight
      // has usually sent something.
      res.writeHead(200, {
        'content-type': 'application/json',
        'content-length': '4096',
      })
      // Destroyed in the write callback, not on the next line.
      //
      // `res.write()` buffers; `socket.destroy()` immediately after discards
      // the buffer before anything reaches the wire, which is a connection
      // closed with *no* bytes -- exactly the case Chromium retries, which is
      // what this was supposed to stop.
      //
      // The tell was in the local check and I read past it: curl exited 52,
      // "empty reply from server", where a genuine truncation is 18, "partial
      // transfer". The callback fires once the data is handed to the OS.
      res.write('{"truncated":', () => {
        res.socket?.destroy()
      })
      return true

    case 'hangNext':
      // Answer nothing, ever. The spec is responsible for its own timeout; the
      // point is what the UI does while waiting, and whether it ever stops.
      return true

    case 'malformNext':
      // A 200 whose body is not what the type says. `items` is absent, so
      // anything doing `data.items.map(...)` throws inside a promise.
      json(res, 200, armed.body ?? { success: true, data: { unexpected: true } })
      return true
  }
}

/**
 * Run a handler and guarantee the response is *ended*, whatever happens.
 *
 * The first version of this file ran each handler as `void (async () => {..})()`.
 * A throw inside that is an unhandled rejection, the response is never sent, and
 * the browser waits -- axios gives up after its 30-second timeout, which is
 * also Playwright's test timeout, so the whole run reports thirty identical
 * "locator.click: Test timeout" failures and not one word about the cause.
 *
 * A fake that can hang on its own bug is worse than no fake: it produces
 * failures that look like application defects and cannot be told apart from
 * them. So a throw becomes a 500 with the message in it, and a log line the
 * webServer output carries.
 *
 * Every request is logged either way. A tier whose failures are timeouts needs
 * to be able to say what was asked and what was answered.
 */
async function guard(
  which: string,
  req: IncomingMessage,
  res: ServerResponse,
  handler: () => Promise<void>
): Promise<void> {
  const started = Date.now()
  try {
    await handler()
  } catch (e) {
    const message = e instanceof Error ? (e.stack ?? e.message) : String(e)
    console.error(`[fake:${which}] ${req.method} ${req.url} threw:\n${message}`)
    if (!res.writableEnded) {
      err(res, 500, `the fake threw handling this request: ${message.split('\n')[0]}`)
    }
  } finally {
    if (!res.writableEnded) {
      // Reached by `hangNext`, which is deliberate -- and by a handler that
      // fell through every branch without answering, which is not. The log
      // distinguishes them.
      console.error(`[fake:${which}] ${req.method} ${req.url} left unanswered`)
    } else {
      console.error(
        `[fake:${which}] ${req.method} ${req.url} -> ${res.statusCode} (${Date.now() - started}ms)`
      )
    }
  }
}

export function fakeApi(): Plugin {
  return {
    name: 'css-fake-api',
    configureServer(server) {
      server.middlewares.use('/__fake', control)
      server.middlewares.use('/api', api)
    },
  }
}

// ---------------------------------------------------------------------------
// The control surface
// ---------------------------------------------------------------------------
const control: Connect.NextHandleFunction = (req, res) => {
  void guard('control', req, res, async () => {
    const url = new URL(req.url ?? '/', 'http://fake')
    const path = url.pathname

    if (path === '/reset') {
      world.reset()
      return ok(res, null, 'reset')
    }
    if (path === '/arm') {
      const body = (await readBody(req)) as Record<string, unknown>
      world.arm({
        kind: body.kind as Injection,
        // `asText` rather than `String()`: a control payload with an object
        // where a string belongs would stringify to "[object Object]" and arm a
        // fault that matches nothing, silently.
        path: asText(body.path),
        status: typeof body.status === 'number' ? body.status : undefined,
        body: body.body,
        times: typeof body.times === 'number' ? body.times : undefined,
      })
      return ok(res, world.armed.length)
    }
    if (path === '/requests') {
      return ok(res, world.requests)
    }
    if (path === '/state') {
      return ok(res, {
        users: world.users.length,
        tools: world.tools.length,
        doorRules: world.doorRules.length,
        profileFields: world.profileFields,
        armed: world.armed,
      })
    }
    return err(res, 404, `no such control endpoint: ${path}`)
  })
}

// ---------------------------------------------------------------------------
// The API
// ---------------------------------------------------------------------------
const api: Connect.NextHandleFunction = (req, res) => {
  void guard('api', req, res, async () => {
    const url = new URL(req.url ?? '/', 'http://fake')
    const path = url.pathname
    const method = (req.method ?? 'GET').toUpperCase()
    world.requests.push({ method, path })

    if (injected(path, res)) return

    const body = ['POST', 'PUT', 'PATCH'].includes(method)
      ? ((await readBody(req)) as Record<string, unknown>)
      : {}
    const me = world.userByToken(bearer(req))

    // --- public ------------------------------------------------------------
    if (path === '/config/public') {
      // The shape `PublicConfig` declares, not one invented here.
      //
      // The first version of this handler returned `site_name` at the top
      // level and no `pages` block at all -- a shape no server sends. It
      // produced a genuine finding by accident (four shallow optional chains
      // that froze the whole application), and it would have gone on producing
      // false ones forever: a fake whose responses do not match the contract is
      // a fake that tests the client against a server that does not exist.
      return ok(res, {
        site: { site_name: 'Fake Space' },
        pages: {
          wiki_enabled: false,
          wiki_link: 'None',
          site_enabled: false,
          site_link: 'None',
        },
        tools: { tool_categories: [{ value: 'saw', label: 'Saw' }] },
        registration_challenge: {
          enabled: false,
          hint: '',
          throttle_enabled: false,
          terms_of_service_checkbox: false,
          terms_of_service_md: '',
          recaptcha_enabled: false,
          recaptcha_site_key: '',
        },
        doors: { enabled: true },
        calendar: { enabled: false },
        toolguard: { enabled: true },
      })
    }

    if (path === '/auth/login' && method === 'POST') {
      const session = world.login(asText(body.username_or_email), asText(body.password))
      if (!session) return err(res, 401, 'Wrong credentials')
      if (session.kind === 'mfa') {
        // Shaped exactly like `build_login_challenge`'s return: `mfa_required`
        // is what `isMfaChallenge` keys off, and it must be the boolean rather
        // than the string, or the client takes the token branch with no token.
        return ok(res, {
          mfa_required: true,
          challenge_token: session.challengeToken,
          methods: session.methods,
          webauthn_options: null,
        })
      }
      return ok(res, { token: session.token, user: session.user, expires_in: 86400 })
    }

    // Public on the real server -- `Guard::Public` in the contract tier's route
    // table -- because the caller has no token yet. It is the challenge token,
    // not a credential, that says who they are.
    if (path === '/auth/mfa/verify' && method === 'POST') {
      const outcome = world.verifyMfa(
        asText(body.challenge_token),
        asText(body.method),
        asText(body.code)
      )
      if ('error' in outcome) return err(res, outcome.status, outcome.error)
      return ok(res, { token: outcome.token, user: outcome.user, expires_in: 86400 })
    }

    if (path === '/auth/logout' && method === 'POST') {
      return ok(res, null, 'Logged out')
    }

    // Public on the real server -- `Guard::Public` in the contract tier's route
    // table -- and asked for by the home page whether or not anybody is signed
    // in. Serving them below the credential gate would have made them 401 here
    // and 200 in production, which is the fake teaching a spec the wrong thing.
    if (path === '/calendar/events') return ok(res, [])
    if (path === '/public/schedules') return ok(res, [])
    if (path === '/public/home-links') return ok(res, [])

    // --- the 404, decided before the credential gate -------------------------
    // The real router mounts `api_routes()` with its own fallback, and axum
    // never runs an extractor for a path that matched no route -- so an unknown
    // endpoint is 404 whether or not the caller is signed in. A fake that
    // answered 401 for an unknown path would teach a spec the wrong precedence,
    // and the frontend logs the user out on any 401.
    if (!KNOWN_PATHS.some((p) => p.test(path))) {
      return err(res, 404, `No such endpoint: /api${path}`)
    }

    // --- everything below needs a credential --------------------------------
    // 401 for no credential, 403 for the wrong role. The distinction is the one
    // the server had wrong, and the frontend's interceptor logs the user out on
    // any 401 -- so a fake that answered 401 for both would make the app behave
    // correctly here and wrongly against the real server.
    if (!me) return err(res, 401, 'Missing credentials')

    if (path === '/auth/me') return ok(res, me)

    if (path === '/profiles/config' && method === 'GET') {
      return ok(res, {
        profile_fields: world.profileFields,
        profiles_enabled: world.profilesEnabled,
      })
    }

    if (path === '/profiles/config' && method === 'PUT') {
      if (me.role !== UserRole.Admin) return err(res, 403, 'Insufficient permissions')
      world.profileFields = (body.profile_fields ?? []) as never
      world.profilesEnabled = Boolean(body.profiles_enabled)
      return ok(res, {
        profile_fields: world.profileFields,
        profiles_enabled: world.profilesEnabled,
      })
    }

    const profileMatch = path.match(/^\/profiles\/([^/]+)$/)
    if (profileMatch) {
      const target = world.users.find((u) => u.id === profileMatch[1])
      if (!target) return err(res, 404, 'User not found')
      if (method === 'GET') {
        return ok(res, { user_id: target.id, profile: target.profile })
      }
      if (method === 'PUT') {
        // The real validator, imported rather than reimplemented. This is the
        // whole reason the fake is a Vite plugin: a standalone server could not
        // resolve `@/stores/profile`, and a hand-written copy of these rules
        // would agree with itself.
        const incoming = (body.profile ?? {}) as Record<string, unknown>
        const { valid, errors } = validateAgainstTheRealRules(incoming)
        if (!valid) {
          return json(res, 400, {
            success: false,
            error: errors.join('; '),
          })
        }
        target.profile = incoming
        return ok(res, { user_id: target.id, profile: target.profile })
      }
    }

    if (path === '/tools' && method === 'GET') return ok(res, world.tools)

    if (path === '/users' && method === 'GET') {
      if (me.role !== UserRole.Admin) return err(res, 403, 'Insufficient permissions')
      return ok(res, {
        items: world.users,
        total: world.users.length,
        page: 1,
        per_page: 50,
        total_pages: 1,
      })
    }

    const doorInfo = path.match(/^\/doors\/([^/]+)\/info$/)
    if (doorInfo) {
      const door = world.doors.find((d) => d.id === doorInfo[1])
      if (!door) return err(res, 404, 'Door not found')
      return ok(res, door)
    }

    const doorCheckin = path.match(/^\/doors\/([^/]+)\/checkin$/)
    if (doorCheckin && method === 'POST') {
      const door = world.doors.find((d) => d.id === doorCheckin[1])
      if (!door) return err(res, 404, 'Door not found')
      if (!door.enabled) return ok(res, { unlocked: false, reason: 'Door is disabled' })
      if (!door.you_are_authorized) {
        return ok(res, { unlocked: false, reason: 'Not authorized' })
      }
      return ok(res, { unlocked: true, reason: null })
    }

    // The API's own 404, in the same envelope -- matching what the server does
    // now that `api_routes()` owns a fallback. A fake that answered HTML here
    // would hide the very defect that fallback was added for.
    return err(res, 404, `No such endpoint: /api${path}`)
  })
}
