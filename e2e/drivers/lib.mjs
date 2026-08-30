// Shared machinery for the stack battery's Node drivers.
//
// Every driver writes `name<TAB>status<TAB>message` to $CASES_OUT and exits
// non-zero if any case failed. run.sh folds that file into the stage's JUnit,
// so nothing here parses stdout and a driver is free to log whatever helps a
// human reading a failure.
//
// The file is written in a `finally`, for the reason stated in e2e/lib.sh: a
// driver that dies mid-way must still leave a file describing the failures it
// had, rather than no file at all -- which anything globbing out/*.xml would
// read as "this stage had no failures".

import { appendFileSync, writeFileSync } from 'node:fs'

export const BASE = process.env.CSS_BASE_URL ?? 'http://127.0.0.1:4399'
const CASES_OUT = process.env.CASES_OUT

const cases = []

export function record(name, status, message = '') {
  cases.push([name, status, String(message).replace(/[\t\n]/g, ' ')])
  const mark = status === 'ok' ? 'ok   ' : status === 'skip' ? 'skip ' : 'FAIL '
  console.log(`  ${mark} ${name}${message ? ` -- ${message}` : ''}`)
}

/**
 * `why` is for the assertions that pin a known defect in place rather than
 * asserting correct behavior. Those fail the day somebody fixes the defect,
 * and at that moment the reader needs to know that failing is the good outcome
 * and the assertion should be deleted -- which a bare "expected 500, got 400"
 * does not say.
 */
export function assertEq(name, expected, actual, why) {
  if (expected === actual) record(name, 'ok', why ? `${why}` : '')
  else record(name, 'fail', `expected [${expected}], got [${actual}]${why ? ` -- ${why}` : ''}`)
}

export function assertNe(name, forbidden, actual) {
  if (forbidden !== actual) record(name, 'ok')
  else record(name, 'fail', `must not be [${forbidden}]`)
}

export function ok(name, condition, message = '') {
  if (condition) record(name, 'ok')
  else record(name, 'fail', message)
}

export function flush() {
  if (!CASES_OUT) throw new Error('CASES_OUT is unset; run.sh always sets it')
  writeFileSync(CASES_OUT, cases.map((c) => c.join('\t')).join('\n') + '\n')
  return cases.filter((c) => c[1] === 'fail').length
}

/** Run a driver body, always writing results, and exit with the failure count. */
export async function main(body) {
  let crashed = null
  try {
    await body()
  } catch (e) {
    crashed = e
    record('driver/completed', 'fail', `threw: ${e?.stack ?? e}`)
  }
  const failures = flush()
  if (crashed) console.error(crashed)
  console.log(`driver finished: ${cases.length} case(s), ${failures} failure(s)`)
  process.exit(failures > 0 ? 1 : 0)
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------
// A thin wrapper, deliberately: it never throws on a non-2xx, because every
// interesting assertion in this tier is about a status code. `redirect: manual`
// so a 302 stays a 302 rather than becoming whatever it points at.

export async function req(method, path, { token, apiKey, body, headers } = {}) {
  const h = { ...(headers ?? {}) }
  if (token) h.Authorization = `Bearer ${token}`
  if (body !== undefined) h['Content-Type'] = 'application/json'
  // The toolguard endpoints accept either a device Bearer token or an
  // `api_key` query parameter, so both have to be expressible here -- a helper
  // that could only send one of them would leave half of that surface
  // untestable, which is how it came to be unauthenticated in the first place.
  const url = new URL(path, BASE)
  if (apiKey !== undefined) url.searchParams.set('api_key', apiKey)
  const res = await fetch(url, {
    method,
    headers: h,
    body: body === undefined ? undefined : JSON.stringify(body),
    redirect: 'manual',
  })
  const text = await res.text()
  let json = null
  try {
    json = JSON.parse(text)
  } catch {
    /* not JSON; `text` is what the caller gets */
  }
  return { status: res.status, text, json, headers: res.headers }
}

export const GET = (p, o) => req('GET', p, o)
export const POST = (p, o) => req('POST', p, o)
export const PUT = (p, o) => req('PUT', p, o)
export const DELETE = (p, o) => req('DELETE', p, o)

// ---------------------------------------------------------------------------
// Accounts, created through the shipping path only
// ---------------------------------------------------------------------------
// No driver inserts a user. Registration and login are the only way an account
// comes into being here, which is what makes an assertion about a member's
// permissions an assertion about the product rather than about the fixture.

/** A per-run tag so repeated runs against one cluster do not collide. */
export const RUN_TAG = process.env.CSS_RUN_TAG ?? String(process.pid)

export const PASSWORD = 'e2e-password-1234'

export function register(username, email, password = PASSWORD) {
  return POST('/api/auth/register', {
    body: { username, email, password, full_name: `E2E ${username}` },
  })
}

/** Note `username_or_email`, not `username`. The wire name is the contract. */
export function login(usernameOrEmail, password = PASSWORD) {
  return POST('/api/auth/login', {
    body: { username_or_email: usernameOrEmail, password },
  })
}

/**
 * The token out of a login response.
 *
 * Reads exactly one path -- `data.token` inside the `ApiResponse` envelope --
 * rather than falling back through several. A chain of `??` would keep working
 * if the envelope changed shape, and "the tests still pass after the API
 * changed its response format" is not a property worth having.
 */
export function tokenOf(res) {
  if (res.json?.success !== true) {
    throw new Error(`not a success envelope: ${res.text.slice(0, 400)}`)
  }
  const token = res.json?.data?.token
  if (typeof token !== 'string' || token.length === 0) {
    throw new Error(`no data.token in the login response: ${res.text.slice(0, 400)}`)
  }
  return token
}

/** Register-then-login, returning `{ username, email, token, user }`. */
export async function account(kind, { email } = {}) {
  const username = `e2e_${kind}_${RUN_TAG}`
  const addr = email ?? `${username}@e2e.invalid`
  const reg = await register(username, addr)
  if (reg.status !== 200 && reg.status !== 201) {
    throw new Error(`register ${username} -> ${reg.status}: ${reg.text.slice(0, 400)}`)
  }
  const li = await login(username)
  if (li.status !== 200) {
    throw new Error(`login ${username} -> ${li.status}: ${li.text.slice(0, 400)}`)
  }
  return { username, email: addr, token: tokenOf(li), user: li.json.data.user }
}

/**
 * The run's administrator.
 *
 * There is exactly one address that grants admin -- `[initial_setup]`'s -- so
 * only the first driver to run can register it. Every later driver has to sign
 * in as the account already there, and the first version of these drivers did
 * not: the concurrency stage died on `409 Email already exists` before its
 * first assertion, because the contract stage had run twenty seconds earlier.
 *
 * The password is the same for every account this suite creates, so signing in
 * is possible without having been the one to register. That is a property of
 * the fixture, not of the product.
 */
export async function adminAccount(kind) {
  const username = `e2e_${kind}_${RUN_TAG}`
  const reg = await register(username, ADMIN_EMAIL)

  if (reg.status === 200 || reg.status === 201) {
    const li = await login(username)
    if (li.status !== 200) {
      throw new Error(`login ${username} -> ${li.status}: ${li.text.slice(0, 400)}`)
    }
    return { username, email: ADMIN_EMAIL, token: tokenOf(li), user: li.json.data.user }
  }

  // Somebody already holds the address. Sign in as them.
  const li = await login(ADMIN_EMAIL)
  if (li.status !== 200) {
    throw new Error(
      `the admin address is taken (register said ${reg.status}) and signing in as it ` +
        `answered ${li.status}: ${li.text.slice(0, 400)}`,
    )
  }
  const user = li.json.data.user
  if (user?.role !== 'Admin') {
    throw new Error(`signed in as ${ADMIN_EMAIL} and got role ${user?.role}, not Admin`)
  }
  return { username: user.username, email: ADMIN_EMAIL, token: tokenOf(li), user }
}

/** Matches `[initial_setup] setup_admin_email` in e2e/stack-config.toml. */
export const ADMIN_EMAIL = 'admin@e2e.invalid'

/**
 * Every profile-config version, paged.
 *
 * `GET /api/profiles/config/versions` is `ORDER BY version DESC` with the
 * limit defaulting to 50, so one unqualified call returns the fifty *newest*
 * rows and silently drops the rest. Two drivers read it to decide whether the
 * numbering is a contiguous run from 1, and both treated that page as the
 * whole table.
 *
 * It held for as long as a run made fewer than fifty versions. The stages
 * share one database and all three write: the contract stage writes a couple,
 * the concurrency race writes ROUNDS x FANOUT (24 by default), and the journey
 * driver writes whatever its seed chooses. CI drew a seed that made 25 and
 * pushed the total to 51 -- so `versions-are-contiguous` reported "numbering
 * starts at 2, not 1", which was true of the page and false of the table.
 *
 * A truncated observation that reads as a product defect is the worst answer
 * this tier can give, so this is shared rather than fixed twice. 500 is the
 * endpoint's own clamp ceiling, so this makes the fewest requests that can be
 * correct.
 */
export async function allProfileConfigVersions(token) {
  const PAGE = 500
  const out = []
  for (let offset = 0; ; offset += PAGE) {
    const res = await GET(`/api/profiles/config/versions?limit=${PAGE}&offset=${offset}`, { token })
    const page = res.json?.data
    if (!Array.isArray(page)) break
    out.push(...page)
    if (page.length < PAGE) break
  }
  return out
}

export function appendLog(file, line) {
  try {
    appendFileSync(file, line + '\n')
  } catch {
    /* logging must never be the reason a stage fails */
  }
}
