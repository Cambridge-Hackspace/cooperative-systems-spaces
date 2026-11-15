// oidc -- CS:S as an OpenID Connect provider (#225), against a real stack.
//
// The whole flow an app such as POP drives: discovery, the JWKS, authorize
// with PKCE, CS:S's consent step, the code exchanged for tokens, the ID token
// verified HERE against the published keys with Node's own crypto (so the
// server's signing is checked by a second implementation, not by itself),
// userinfo, refresh with rotation, the client-credentials grant, and logout.
// Then every refusal the issue names: a wrong redirect URI, a missing PKCE
// verifier, a reused code, an expired code, and a token signed by a key CS:S
// never published. Then key rotation, and `sub` holding still through an email
// change and an account merge.
//
// What this does NOT prove: anything about a browser. The consent page's
// redirects are vitest's (OAuthConsentView.spec.ts); this drives the API the
// page calls, with a member's own session token.

import { generateKeyPairSync, createHash, createPublicKey, verify as cryptoVerify, sign as cryptoSign, randomBytes } from 'node:crypto'
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import {
  BASE,
  GET,
  POST,
  PUT,
  PATCH,
  PASSWORD,
  RUN_TAG,
  account,
  adminAccount,
  assertEq,
  main,
  ok,
} from './lib.mjs'

const REDIRECT = 'https://pop.e2e.invalid/auth/callback'
const AFTER_LOGOUT = 'https://pop.e2e.invalid/signed-out'
const b64u = (buf) => Buffer.from(buf).toString('base64url')

// --- PKCE, as an app makes it ------------------------------------------------
function pkce() {
  const verifier = b64u(randomBytes(32))
  const challenge = b64u(createHash('sha256').update(verifier).digest())
  return { verifier, challenge }
}

// --- a raw request: form bodies and Basic auth, which lib.mjs doesn't speak ---
async function raw(method, path, { headers = {}, body } = {}) {
  const res = await fetch(new URL(path, BASE), { method, headers, body, redirect: 'manual' })
  const text = await res.text()
  let json = null
  try {
    json = JSON.parse(text)
  } catch {
    /* not JSON */
  }
  return { status: res.status, text, json, headers: res.headers }
}

const form = (fields) => new URLSearchParams(Object.entries(fields).filter(([, v]) => v !== undefined)).toString()
const basic = (id, secret) =>
  'Basic ' + Buffer.from(`${encodeURIComponent(id)}:${encodeURIComponent(secret)}`).toString('base64')

async function tokenRequest(fields, { id, secret } = {}) {
  const headers = { 'Content-Type': 'application/x-www-form-urlencoded' }
  if (id && secret) headers.Authorization = basic(id, secret)
  return raw('POST', '/api/oauth/token', { headers, body: form(fields) })
}

// --- verifying a JWT the way a relying party does ----------------------------
function decode(jwt) {
  const [h, p] = String(jwt).split('.')
  return { header: JSON.parse(Buffer.from(h, 'base64url')), claims: JSON.parse(Buffer.from(p, 'base64url')) }
}
function verifiesAgainst(jwt, keys) {
  const [h, p, s] = String(jwt).split('.')
  const { header } = decode(jwt)
  const jwk = keys.find((k) => k.kid === header.kid)
  if (!jwk || header.alg !== 'RS256') return false
  return cryptoVerify('RSA-SHA256', Buffer.from(`${h}.${p}`), createPublicKey({ key: jwk, format: 'jwk' }), Buffer.from(s, 'base64url'))
}

// --- maildir (mirrors emails.mjs) --------------------------------------------
const MAILDIR = join(process.env.CSS_STACK_DIR ?? '/stack', 'mail')
function messages() {
  try {
    return readdirSync(MAILDIR)
      .filter((n) => n.endsWith('.eml'))
      .sort()
      .map((n) => readFileSync(join(MAILDIR, n), 'utf8'))
  } catch {
    return []
  }
}
async function confirmationTokenFor(addr, timeoutMs = 8000) {
  const deadline = Date.now() + timeoutMs
  for (;;) {
    const m = messages().filter((t) => t.includes(`X-Sink-Rcpt-To: <${addr}>`)).pop()
    if (m) {
      const plain = m.replace(/=\r?\n/g, '').replace(/=([0-9A-Fa-f]{2})/g, (_, x) => String.fromCharCode(parseInt(x, 16)))
      const t = plain.match(/[?&]token=([0-9a-f]+)/)
      if (t) return t[1]
    }
    if (Date.now() > deadline) return null
    await new Promise((r) => setTimeout(r, 100))
  }
}

await main(async () => {
  const admin = await adminAccount('oidc_admin')
  const member = await account('oidc_member')
  const T = { token: admin.token }

  // ---- discovery and keys -----------------------------------------------------
  const disc = await raw('GET', '/.well-known/openid-configuration')
  assertEq('oidc/discovery-is-200', 200, disc.status, disc.text.slice(0, 200))
  const d = disc.json ?? {}
  const issuer = d.issuer
  ok('oidc/discovery-names-an-issuer', typeof issuer === 'string' && /^https?:\/\//.test(issuer), String(issuer))
  for (const k of ['authorization_endpoint', 'token_endpoint', 'userinfo_endpoint', 'jwks_uri', 'end_session_endpoint']) {
    ok(`oidc/discovery-${k}-is-under-the-issuer`, String(d[k] ?? '').startsWith(`${issuer}/`), String(d[k]))
  }
  ok('oidc/discovery-offers-only-s256', JSON.stringify(d.code_challenge_methods_supported) === '["S256"]',
    JSON.stringify(d.code_challenge_methods_supported))
  ok('oidc/discovery-signs-with-rs256', (d.id_token_signing_alg_values_supported ?? []).includes('RS256'))
  const path = (url) => new URL(url).pathname
  const jwksRes = await raw('GET', path(d.jwks_uri))
  assertEq('oidc/jwks-is-200', 200, jwksRes.status, jwksRes.text.slice(0, 200))
  let keys = jwksRes.json?.keys ?? []
  ok('oidc/jwks-publishes-an-rsa-key', keys.length >= 1 && keys.every((k) => k.kty === 'RSA' && k.kid && k.n && k.e),
    JSON.stringify(keys).slice(0, 200))
  ok('oidc/jwks-publishes-no-private-part', keys.every((k) => !('d' in k) && !('p' in k)), 'a JWK carried a private member')

  // ---- apps --------------------------------------------------------------------
  const scopes = ['openid', 'profile', 'email', 'roles', 'training:read']
  const asMember = await POST('/api/admin/oauth/clients', {
    token: member.token,
    body: { name: 'nope', redirect_uris: [REDIRECT] },
  })
  assertEq('oidc/a-member-cannot-register-an-app', 403, asMember.status, asMember.text.slice(0, 200))
  const badUri = await POST('/api/admin/oauth/clients', {
    ...T,
    body: { name: 'bad', redirect_uris: ['http://not-loopback.e2e.invalid/cb'] },
  })
  assertEq('oidc/a-plain-http-redirect-is-refused', 400, badUri.status, badUri.text.slice(0, 200))
  const mk = async (body) => (await POST('/api/admin/oauth/clients', { ...T, body })).json?.data
  const pop = await mk({
    name: `POP ${RUN_TAG}`,
    redirect_uris: [REDIRECT],
    post_logout_redirect_uris: [AFTER_LOGOUT],
    allowed_scopes: scopes,
  })
  ok('oidc/an-app-is-registered-with-a-secret', !!pop?.client?.client_id && typeof pop?.client_secret === 'string',
    JSON.stringify(pop).slice(0, 200))
  ok('oidc/the-list-never-shows-a-secret',
    !JSON.stringify((await GET('/api/admin/oauth/clients', T)).json ?? {}).includes(pop?.client_secret ?? '!'),
    'the client list carried the secret')
  const trusted = await mk({ name: `Trusted ${RUN_TAG}`, redirect_uris: [REDIRECT], allowed_scopes: ['openid'], skip_consent: true })
  const spa = await mk({ name: `SPA ${RUN_TAG}`, confidential: false, redirect_uris: [REDIRECT], allowed_scopes: ['openid'] })
  ok('oidc/a-public-app-has-no-secret', spa?.client_secret === null, JSON.stringify(spa).slice(0, 200))
  const notifier = await mk({ name: `Notifier ${RUN_TAG}`, redirect_uris: [], allowed_scopes: ['openid', 'notify:members'] })
  const POP = { id: pop.client.client_id, secret: pop.client_secret }

  // ---- authorize: the refusals that must not redirect -----------------------------
  const authorizeUrl = (client, extra = {}) => {
    const q = new URLSearchParams({ response_type: 'code', client_id: client, redirect_uri: REDIRECT, scope: 'openid', ...extra })
    return `${path(d.authorization_endpoint)}?${q}`
  }
  const unknownClient = await raw('GET', authorizeUrl('css-nobody', pkceParams(pkce())))
  assertEq('oidc/an-unknown-app-is-refused-here', 400, unknownClient.status)
  ok('oidc/an-unknown-app-is-not-redirected', !unknownClient.headers.get('location'), unknownClient.headers.get('location') ?? '')
  const wrongRedirect = await raw('GET', authorizeUrl(POP.id, { ...pkceParams(pkce()), redirect_uri: 'https://evil.e2e.invalid/cb' }))
  assertEq('oidc/an-unregistered-redirect-is-refused-here', 400, wrongRedirect.status)
  ok('oidc/an-unregistered-redirect-is-never-followed', !wrongRedirect.headers.get('location'),
    wrongRedirect.headers.get('location') ?? '')
  const noPkce = await raw('GET', authorizeUrl(POP.id, { state: 'st' }))
  ok('oidc/no-pkce-goes-back-as-invalid-request',
    noPkce.status >= 300 && noPkce.status < 400 && /error=invalid_request/.test(noPkce.headers.get('location') ?? '') &&
      /state=st/.test(noPkce.headers.get('location') ?? ''),
    `${noPkce.status} ${noPkce.headers.get('location')}`)
  const plainPkce = await raw('GET', authorizeUrl(POP.id, { code_challenge: 'x'.repeat(43), code_challenge_method: 'plain' }))
  ok('oidc/plain-pkce-is-refused', /error=invalid_request/.test(plainPkce.headers.get('location') ?? ''),
    plainPkce.headers.get('location') ?? '')
  const tooMuch = await raw('GET', authorizeUrl(POP.id, { ...pkceParams(pkce()), scope: 'openid notify:members' }))
  ok('oidc/an-app-scope-cannot-be-asked-of-a-member', /error=invalid_scope/.test(tooMuch.headers.get('location') ?? ''),
    tooMuch.headers.get('location') ?? '')

  // ---- the code flow ------------------------------------------------------------------
  function pkceParams(p) {
    return { code_challenge: p.challenge, code_challenge_method: 'S256' }
  }
  /** authorize -> CS:S's page -> the member's answer -> the redirect back. */
  async function signIn(clientId, who, { scope = scopes.join(' '), approve = true, nonce } = {}) {
    const p = pkce()
    const state = b64u(randomBytes(9))
    const res = await raw('GET', authorizeUrl(clientId, { ...pkceParams(p), scope, state, nonce }))
    const loc = res.headers.get('location') ?? ''
    const id = loc.match(/^\/oauth\/consent\/([0-9a-f-]{36})$/)?.[1]
    if (!id) return { res, loc, p, state }
    const info = await GET(`/api/oauth/requests/${id}`, { token: who.token })
    const decision = await POST(`/api/oauth/requests/${id}/decision`, { token: who.token, body: { approve } })
    const back = new URL(decision.json?.data?.redirect ?? 'about:blank')
    return { res, loc, id, info, decision, back, code: back.searchParams.get('code'), state, p }
  }

  const first = await signIn(POP.id, member, { nonce: 'n-0123' })
  ok('oidc/authorize-hands-the-browser-to-css', !!first.id, `${first.res.status} ${first.loc}`)
  assertEq('oidc/the-consent-page-asks', false, first.info?.json?.data?.automatic, first.info?.text.slice(0, 200))
  assertEq('oidc/the-consent-page-names-the-app', `POP ${RUN_TAG}`, first.info?.json?.data?.app)
  ok('oidc/the-answer-returns-a-code-to-the-registered-redirect',
    `${first.back.origin}${first.back.pathname}` === REDIRECT && !!first.code, first.back.toString())
  assertEq('oidc/the-state-comes-back', first.state, first.back.searchParams.get('state'))
  const again = await POST(`/api/oauth/requests/${first.id}/decision`, { token: member.token, body: { approve: true } })
  assertEq('oidc/a-request-is-answered-once', 404, again.status, again.text.slice(0, 200))

  // A wrong secret is refused before the code is looked at.
  const wrongSecret = await tokenRequest(
    { grant_type: 'authorization_code', code: first.code, redirect_uri: REDIRECT, code_verifier: first.p.verifier },
    { id: POP.id, secret: 'wrong' }
  )
  assertEq('oidc/a-wrong-secret-is-401', 401, wrongSecret.status, wrongSecret.text.slice(0, 200))
  assertEq('oidc/a-wrong-secret-is-invalid-client', 'invalid_client', wrongSecret.json?.error)

  const tok = await tokenRequest(
    { grant_type: 'authorization_code', code: first.code, redirect_uri: REDIRECT, code_verifier: first.p.verifier },
    POP
  )
  assertEq('oidc/the-code-is-exchanged', 200, tok.status, tok.text.slice(0, 300))
  assertEq('oidc/tokens-are-not-cached', 'no-store', tok.headers.get('cache-control'))
  const t = tok.json ?? {}
  ok('oidc/the-id-token-verifies-against-the-published-keys', verifiesAgainst(t.id_token, keys), 'Node could not verify it')
  const idc = decode(t.id_token).claims
  assertEq('oidc/id-token-iss', issuer, idc.iss)
  assertEq('oidc/id-token-aud-is-the-app', POP.id, idc.aud)
  assertEq('oidc/id-token-sub-is-the-user-id', member.user.id, idc.sub)
  assertEq('oidc/id-token-carries-the-nonce', 'n-0123', idc.nonce)
  ok('oidc/id-token-expires-later', idc.exp > Date.now() / 1000 && idc.iat <= Date.now() / 1000 + 5, JSON.stringify(idc))
  assertEq('oidc/id-token-carries-the-name', member.user.full_name, idc.name)
  assertEq('oidc/id-token-carries-the-email', member.email, idc.email)
  ok('oidc/id-token-carries-roles', Array.isArray(idc.roles), JSON.stringify(idc.roles))
  ok('oidc/the-access-token-verifies-too', verifiesAgainst(t.access_token, keys), 'Node could not verify it')
  ok('oidc/a-refresh-token-is-issued', typeof t.refresh_token === 'string' && t.refresh_token.length >= 40)

  const ui = await GET('/api/oauth/userinfo', { token: t.access_token })
  assertEq('oidc/userinfo-is-200', 200, ui.status, ui.text.slice(0, 200))
  assertEq('oidc/userinfo-sub', member.user.id, ui.json?.sub)
  assertEq('oidc/userinfo-email', member.email, ui.json?.email)
  const notASession = await GET('/api/auth/me', { token: t.access_token })
  assertEq('oidc/an-access-token-is-not-a-css-session', 401, notASession.status, notASession.text.slice(0, 200))
  const idAsAccess = await GET('/api/oauth/userinfo', { token: t.id_token })
  assertEq('oidc/an-id-token-is-not-an-access-token', 401, idAsAccess.status)
  const mine = await GET('/api/oauth/me/tools', { token: t.access_token })
  assertEq('oidc/training-is-readable-with-its-scope', 200, mine.status, mine.text.slice(0, 200))
  ok('oidc/training-lists-tools-with-a-verdict', (mine.json?.data ?? []).every((x) => typeof x.authorized === 'boolean'),
    mine.text.slice(0, 200))

  // ---- the refusals the issue names ------------------------------------------------------
  const reused = await tokenRequest(
    { grant_type: 'authorization_code', code: first.code, redirect_uri: REDIRECT, code_verifier: first.p.verifier },
    POP
  )
  assertEq('oidc/a-reused-code-is-refused', 'invalid_grant', reused.json?.error, reused.text.slice(0, 200))
  const afterReuse = await tokenRequest({ grant_type: 'refresh_token', refresh_token: t.refresh_token }, POP)
  assertEq('oidc/a-reused-code-revokes-what-it-issued', 'invalid_grant', afterReuse.json?.error, afterReuse.text.slice(0, 200))

  const s2 = await signIn(POP.id, member)
  assertEq('oidc/a-remembered-consent-asks-nothing', true, s2.info?.json?.data?.automatic)
  const wrongRedirectAtToken = await tokenRequest(
    { grant_type: 'authorization_code', code: s2.code, redirect_uri: `${REDIRECT}/other`, code_verifier: s2.p.verifier },
    POP
  )
  assertEq('oidc/a-different-redirect-at-the-token-endpoint-is-refused', 'invalid_grant', wrongRedirectAtToken.json?.error)

  const s3 = await signIn(POP.id, member)
  const noVerifier = await tokenRequest({ grant_type: 'authorization_code', code: s3.code, redirect_uri: REDIRECT }, POP)
  assertEq('oidc/a-missing-pkce-verifier-is-refused', 'invalid_grant', noVerifier.json?.error, noVerifier.text.slice(0, 200))

  const s4 = await signIn(POP.id, member)
  const wrongVerifier = await tokenRequest(
    { grant_type: 'authorization_code', code: s4.code, redirect_uri: REDIRECT, code_verifier: pkce().verifier },
    POP
  )
  assertEq('oidc/a-wrong-pkce-verifier-is-refused', 'invalid_grant', wrongVerifier.json?.error)

  const s5 = await signIn(POP.id, member)
  await new Promise((r) => setTimeout(r, 6500)) // the stack's code_ttl_secs is 5
  const expired = await tokenRequest(
    { grant_type: 'authorization_code', code: s5.code, redirect_uri: REDIRECT, code_verifier: s5.p.verifier },
    POP
  )
  assertEq('oidc/an-expired-code-is-refused', 'invalid_grant', expired.json?.error, expired.text.slice(0, 200))

  // A token CS:S never signed, both with an unknown kid and wearing ours.
  const { privateKey } = generateKeyPairSync('rsa', { modulusLength: 2048 })
  const forge = (kid) => {
    const h = b64u(JSON.stringify({ alg: 'RS256', typ: 'JWT', kid }))
    const now = Math.floor(Date.now() / 1000)
    const p = b64u(JSON.stringify({ ...decode(t.access_token).claims, iat: now, exp: now + 600 }))
    return `${h}.${p}.${b64u(cryptoSign('RSA-SHA256', Buffer.from(`${h}.${p}`), privateKey))}`
  }
  assertEq('oidc/a-token-from-an-unpublished-key-is-refused', 401,
    (await GET('/api/oauth/userinfo', { token: forge('not-ours') })).status)
  assertEq('oidc/a-forged-token-naming-our-key-is-refused', 401,
    (await GET('/api/oauth/userinfo', { token: forge(keys[0].kid) })).status)

  // ---- refresh rotates -------------------------------------------------------------------
  const s6 = await signIn(POP.id, member)
  const t6 = (await tokenRequest(
    { grant_type: 'authorization_code', code: s6.code, redirect_uri: REDIRECT, code_verifier: s6.p.verifier },
    POP
  )).json ?? {}
  const r1 = await tokenRequest({ grant_type: 'refresh_token', refresh_token: t6.refresh_token }, POP)
  assertEq('oidc/a-refresh-works', 200, r1.status, r1.text.slice(0, 200))
  ok('oidc/a-refresh-rotates-the-refresh-token', r1.json?.refresh_token && r1.json.refresh_token !== t6.refresh_token)
  const replayOld = await tokenRequest({ grant_type: 'refresh_token', refresh_token: t6.refresh_token }, POP)
  assertEq('oidc/a-replayed-refresh-token-is-refused', 'invalid_grant', replayOld.json?.error)

  // A refresh may narrow the scope but never widen it. A refused refresh still
  // spends the token (it is used before the scope is judged), so this one
  // comes from its own sign-in.
  const s7 = await signIn(POP.id, member)
  const t7 = (await tokenRequest(
    { grant_type: 'authorization_code', code: s7.code, redirect_uri: REDIRECT, code_verifier: s7.p.verifier },
    POP
  )).json ?? {}
  const widened = await tokenRequest(
    { grant_type: 'refresh_token', refresh_token: t7.refresh_token, scope: 'openid notify:members' },
    POP
  )
  assertEq('oidc/a-refresh-cannot-widen-the-scope', 'invalid_scope', widened.json?.error, widened.text.slice(0, 200))

  // ---- consent, trust and refusal ------------------------------------------------------
  const tr = await signIn(trusted.client.client_id, member, { scope: 'openid' })
  assertEq('oidc/the-space-s-own-app-asks-nothing', true, tr.info?.json?.data?.automatic)
  const outsider = await account('oidc_outsider')
  const declined = await signIn(POP.id, outsider, { approve: false })
  assertEq('oidc/a-refusal-goes-back-as-access-denied', 'access_denied', declined.back.searchParams.get('error'),
    declined.back.toString())
  ok('oidc/a-refusal-carries-no-code', !declined.back.searchParams.get('code'))

  // A public app: PKCE alone, no secret.
  const pub = await signIn(spa.client.client_id, member, { scope: 'openid' })
  const pubTok = await tokenRequest({
    grant_type: 'authorization_code',
    code: pub.code,
    redirect_uri: REDIRECT,
    code_verifier: pub.p.verifier,
    client_id: spa.client.client_id,
  })
  assertEq('oidc/a-public-app-exchanges-with-pkce-alone', 200, pubTok.status, pubTok.text.slice(0, 200))

  // ---- the app acting as itself (client credentials, for #226) ----------------------
  const NOTE = { id: notifier.client.client_id, secret: notifier.client_secret }
  const cc = await tokenRequest({ grant_type: 'client_credentials', scope: 'notify:members' }, NOTE)
  assertEq('oidc/client-credentials-issues-an-app-token', 200, cc.status, cc.text.slice(0, 200))
  assertEq('oidc/an-app-token-names-the-app', `client:${NOTE.id}`, decode(cc.json?.access_token).claims.sub)
  ok('oidc/an-app-token-has-no-id-token', !cc.json?.id_token && !cc.json?.refresh_token)
  const ccMember = await tokenRequest({ grant_type: 'client_credentials', scope: 'openid' }, NOTE)
  assertEq('oidc/an-app-cannot-take-a-member-scope-for-itself', 'invalid_scope', ccMember.json?.error)
  const ccPublic = await tokenRequest({ grant_type: 'client_credentials', scope: 'notify:members', client_id: spa.client.client_id })
  assertEq('oidc/a-public-app-cannot-act-as-itself', 'unauthorized_client', ccPublic.json?.error, ccPublic.text.slice(0, 200))
  // A valid token without the `openid` scope: RFC 6750 section 3.1 makes
  // that 403 insufficient_scope, not 401. Refused either way.
  assertEq('oidc/an-app-token-is-not-a-member-token', 403,
    (await GET('/api/oauth/userinfo', { token: cc.json?.access_token })).status)

  // ---- key rotation ------------------------------------------------------------------
  // The replay above revoked every token this member held with POP, r1's
  // included. Checked first: a replay revokes whatever is live, so it must
  // come before the fresh tokens below rather than after them.
  const replayRevokedR1 = await tokenRequest({ grant_type: 'refresh_token', refresh_token: r1.json?.refresh_token }, POP)
  assertEq('oidc/a-replay-revokes-the-whole-chain', 'invalid_grant', replayRevokedR1.json?.error, replayRevokedR1.text.slice(0, 200))
  const s8 = await signIn(POP.id, member)
  const before = await tokenRequest(
    { grant_type: 'authorization_code', code: s8.code, redirect_uri: REDIRECT, code_verifier: s8.p.verifier },
    POP
  )
  assertEq('oidc/a-sign-in-after-a-replay-works', 200, before.status, before.text.slice(0, 200))
  const oldAccess = before.json?.access_token
  const oldKid = decode(oldAccess).header.kid
  const rot = await POST('/api/admin/oauth/keys/rotate', T)
  assertEq('oidc/a-key-can-be-rotated', 200, rot.status, rot.text.slice(0, 200))
  keys = (await raw('GET', path(d.jwks_uri))).json?.keys ?? []
  ok('oidc/the-retired-key-stays-published', keys.some((k) => k.kid === oldKid) && keys.some((k) => k.kid === rot.json?.data?.kid),
    keys.map((k) => k.kid).join(','))
  assertEq('oidc/a-token-signed-before-rotation-still-works', 200, (await GET('/api/oauth/userinfo', { token: oldAccess })).status)
  const after = await tokenRequest({ grant_type: 'refresh_token', refresh_token: before.json?.refresh_token }, POP)
  assertEq('oidc/new-tokens-use-the-new-key', rot.json?.data?.kid, decode(after.json?.access_token).header.kid)

  // ---- sub holds still through an email change ----------------------------------
  const second = `oidc_second_${RUN_TAG}@e2e.invalid`
  const base = `/api/users/${member.user.id}/emails`
  const added = await POST(base, { token: member.token, body: { email: second, current_password: PASSWORD } })
  const tokenForSecond = await confirmationTokenFor(second)
  if (tokenForSecond) await POST('/api/auth/email/verify', { body: { token: tokenForSecond } })
  const promoted = await PUT(`${base}/${added.json?.data?.id}/primary`, { token: member.token, body: { current_password: PASSWORD } })
  assertEq('oidc/the-email-was-changed', 200, promoted.status, promoted.text.slice(0, 200))
  const afterEmail = await GET('/api/oauth/userinfo', { token: after.json?.access_token })
  assertEq('oidc/sub-is-unchanged-after-an-email-change', member.user.id, afterEmail.json?.sub)
  assertEq('oidc/userinfo-shows-the-new-email', second, afterEmail.json?.email)

  // ---- and through an account merge -----------------------------------------------
  const absorbed = await account('oidc_absorbed')
  const ab = await signIn(POP.id, absorbed)
  const abTok = (await tokenRequest(
    { grant_type: 'authorization_code', code: ab.code, redirect_uri: REDIRECT, code_verifier: ab.p.verifier },
    POP
  )).json ?? {}
  const preview = await POST(`/api/admin/users/${member.user.id}/merge/preview`, { ...T, body: { absorbed_id: absorbed.user.id } })
  const codes = (preview.json?.data?.warnings ?? []).map((w) => w.code)
  const merged = await POST(`/api/admin/users/${member.user.id}/merge`, { ...T, body: { absorbed_id: absorbed.user.id, acknowledged: codes } })
  assertEq('oidc/the-accounts-were-merged', 200, merged.status, merged.text.slice(0, 200))
  const survivorAfter = await GET('/api/oauth/userinfo', { token: after.json?.access_token })
  assertEq('oidc/sub-is-unchanged-after-a-merge', member.user.id, survivorAfter.json?.sub)
  const absorbedRefresh = await tokenRequest({ grant_type: 'refresh_token', refresh_token: abTok.refresh_token }, POP)
  assertEq('oidc/the-absorbed-account-s-app-session-ends', 'invalid_grant', absorbedRefresh.json?.error,
    absorbedRefresh.text.slice(0, 200))

  // ---- logout ------------------------------------------------------------------------
  const lo = await raw('GET', `${path(d.end_session_endpoint)}?${new URLSearchParams({ id_token_hint: t.id_token, post_logout_redirect_uri: AFTER_LOGOUT, state: 'bye' })}`)
  ok('oidc/logout-hands-the-browser-to-css', /^\/oauth\/logout\?/.test(lo.headers.get('location') ?? ''),
    `${lo.status} ${lo.headers.get('location')}`)
  const end = await POST('/api/oauth/end-session', { body: { id_token_hint: t.id_token, post_logout_redirect_uri: AFTER_LOGOUT, state: 'bye' } })
  assertEq('oidc/end-session-follows-a-registered-redirect', `${AFTER_LOGOUT}?state=bye`, end.json?.data?.redirect, end.text.slice(0, 200))
  ok('oidc/end-session-clears-the-session-cookie', /css_session=;/.test(end.headers.get('set-cookie') ?? ''),
    end.headers.get('set-cookie') ?? '')
  const openRedirect = await POST('/api/oauth/end-session', { body: { id_token_hint: t.id_token, post_logout_redirect_uri: 'https://evil.e2e.invalid/' } })
  assertEq('oidc/end-session-is-not-an-open-redirect', '/', openRedirect.json?.data?.redirect)
  const noHint = await POST('/api/oauth/end-session', { body: { post_logout_redirect_uri: AFTER_LOGOUT } })
  assertEq('oidc/end-session-without-a-hint-goes-home', '/', noHint.json?.data?.redirect)

  // ---- a disabled app stops working --------------------------------------------------
  const dis = await PATCH(`/api/admin/oauth/clients/${pop.client.id}`, { ...T, body: { disabled: true } })
  assertEq('oidc/an-app-can-be-disabled', true, dis.json?.data?.disabled, dis.text.slice(0, 200))
  assertEq('oidc/a-disabled-app-s-tokens-stop-working', 401,
    (await GET('/api/oauth/userinfo', { token: after.json?.access_token })).status)
  const disRefresh = await tokenRequest({ grant_type: 'refresh_token', refresh_token: after.json?.refresh_token }, POP)
  assertEq('oidc/a-disabled-app-cannot-refresh', 'invalid_client', disRefresh.json?.error)

  // ---- the record ----------------------------------------------------------------------
  const audit = async (type) => (await GET(`/api/admin/audit-logs?event_type=${type}&per_page=100`, T)).json?.data ?? []
  ok('oidc/a-sign-in-through-an-app-is-audited',
    (await audit('oauth_signin')).some((l) => l.user_id === member.user.id && l.event_data?.client_id === POP.id))
  ok('oidc/registering-an-app-is-audited',
    (await audit('oauth_client_created')).some((l) => l.event_data?.client_id === POP.id))
  ok('oidc/rotating-the-key-is-audited',
    (await audit('oauth_signing_key_rotated')).some((l) => l.event_data?.kid === rot.json?.data?.kid))
  ok('oidc/no-audit-record-holds-a-secret',
    !JSON.stringify(await audit('oauth_client_created')).includes(POP.secret), 'a secret reached the audit log')
})
