// Session-cookie stage (#120/#135): the browser session JWT rides an httpOnly
// cookie instead of localStorage.
//
// What this proves, as two oracles:
//   1. login installs a hardened cookie (css_session, HttpOnly, SameSite=Strict);
//   2. that cookie ALONE authenticates /auth/me -- no Authorization header -- AND
//      /auth/me with neither cookie nor header is 401, so the cookie is doing the
//      work rather than the route being open;
//   3. logout expires the cookie (Max-Age=0), which is the only way to clear an
//      httpOnly cookie the SPA cannot touch.
//
// What this does NOT prove: the Bearer path still works -- that is covered by
// every other stage, all of which authenticate with `Authorization: Bearer`.

import { BASE, account, login, ok, assertEq, main } from './lib.mjs'

/** The css_session Set-Cookie line from a response, or null. */
function sessionSetCookie(res) {
  const all =
    typeof res.headers.getSetCookie === 'function'
      ? res.headers.getSetCookie()
      : [res.headers.get('set-cookie')].filter(Boolean)
  return all.find((c) => c && c.startsWith('css_session=')) ?? null
}

/** The cookie's value out of its Set-Cookie line. */
function sessionValue(setCookie) {
  const m = /^css_session=([^;]*)/.exec(setCookie)
  return m ? m[1] : null
}

main(async () => {
  const acct = await account('cookie')

  // (1) login installs a hardened cookie.
  const li = await login(acct.username)
  assertEq('cookie/login-200', 200, li.status)
  const setCookie = sessionSetCookie(li)
  ok('cookie/login-sets-session-cookie', !!setCookie, 'no css_session Set-Cookie on login')
  ok(
    'cookie/httponly',
    /;\s*HttpOnly/i.test(setCookie ?? ''),
    `session cookie is not HttpOnly: ${setCookie}`
  )
  ok(
    'cookie/samesite-strict',
    /;\s*SameSite=Strict/i.test(setCookie ?? ''),
    `session cookie is not SameSite=Strict: ${setCookie}`
  )

  // (2a) the cookie ALONE authenticates -- deliberately no Authorization header.
  const value = sessionValue(setCookie ?? '')
  const meViaCookie = await fetch(new URL('/api/auth/me', BASE), {
    headers: { Cookie: `css_session=${value}` },
  })
  const meBody = await meViaCookie.json().catch(() => null)
  assertEq('cookie/me-200-via-cookie', 200, meViaCookie.status)
  ok(
    'cookie/me-is-the-signed-in-user',
    meBody?.data?.username === acct.username,
    `/auth/me via cookie returned ${JSON.stringify(meBody?.data?.username)}`
  )

  // (2b) the second oracle: with neither cookie nor header, /auth/me is 401. If
  // this passed as 200 the test above would prove nothing about the cookie.
  const meAnon = await fetch(new URL('/api/auth/me', BASE))
  assertEq('cookie/me-401-without-credentials', 401, meAnon.status)

  // (3) logout expires the cookie.
  const lo = await fetch(new URL('/api/auth/logout', BASE), { method: 'POST' })
  assertEq('cookie/logout-200', 200, lo.status)
  const loCookie = sessionSetCookie(lo) ?? ''
  ok(
    'cookie/logout-expires-the-cookie',
    /;\s*Max-Age=0/i.test(loCookie),
    `logout did not expire the session cookie: ${loCookie}`
  )
})
