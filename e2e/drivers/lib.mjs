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

export function assertEq(name, expected, actual) {
  if (expected === actual) record(name, 'ok')
  else record(name, 'fail', `expected [${expected}], got [${actual}]`)
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

export function appendLog(file, line) {
  try {
    appendFileSync(file, line + '\n')
  } catch {
    /* logging must never be the reason a stage fails */
  }
}
