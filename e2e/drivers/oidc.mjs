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

import { generateKeyPairSync, createHash, createHmac, createPublicKey, verify as cryptoVerify, sign as cryptoSign, randomBytes } from 'node:crypto'
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

  // ---- a card tap signs in to the space's own app (#241) ------------------------------
  {
  // A reader on a tool set to identify and to sign in to POP: the scan reply
  // carries a code POP redeems with its secret, or, with a second factor, a
  // ticket POP sends back with the member's TOTP. Fresh accounts: the ones
  // above were renamed and merged.
  const cardMember = await account('card_member')
  const CARD = `CARD-SIGN-IN-${RUN_TAG}`
  const issuedCard = await POST(`/api/cards/user/${cardMember.user.id}`, { ...T, body: { code: CARD } })
  assertEq('card-sign-in/card-issued', 200, issuedCard.status, issuedCard.text.slice(0, 200))
  const towerExt = `print-tower-${RUN_TAG}`
  const towerRes = await POST('/api/tools', {
    ...T,
    body: {
      name: `E2E print tower ${RUN_TAG}`, category: 'other', requires_training: false, external_id: towerExt,
      usage_flat_fee: null, usage_rate_per_min: null, usage_max_session_minutes: null,
    },
  })
  const towerId = towerRes.json?.data?.id
  ok('card-sign-in/tool-created', !!towerId, towerRes.text.slice(0, 200))
  const inv = await POST('/api/admin/devices/invite', { ...T, body: { expires_in_hours: 1 } })
  const reg = await POST('/api/devices/register', {
    body: {
      device_code: inv.json?.data?.device_code, name: `tower-reader-${RUN_TAG}`, capabilities: { roles: ['reader'] },
      mac_address: '02:00:00:00:24:01', software_version: '0.0.0-e2e', platform: 'linux',
    },
  })
  const reader = reg.json?.data?.auth_token ?? reg.json?.auth_token
  const readerId = reg.json?.data?.device_id ?? reg.json?.device_id
  const bound = await POST('/api/admin/device-bindings', {
    ...T, body: { resource_id: towerId, device_id: readerId, role: 'reader', name: 'tower reader' },
  })
  assertEq('card-sign-in/reader-bound', 201, bound.status, bound.text.slice(0, 200))
  const tap = async () => POST(`/api/resources/${towerExt}/scan`, { token: reader, body: { card: CARD } })
  const setTower = (body) => PATCH(`/api/admin/resources/${towerId}`, { ...T, body })

  // The app: the space's own, with a secret. Only such an app may be named.
  const towerApp = await mk({
    name: `POP tower ${RUN_TAG}`, redirect_uris: [REDIRECT], skip_consent: true,
    allowed_scopes: ['openid', 'profile', 'roles', 'training:read'],
  })
  const TOWER = { id: towerApp.client.client_id, secret: towerApp.client_secret, uuid: towerApp.client.id }
  ok('card-sign-in/discovery-offers-the-grant', (d.grant_types_supported ?? []).includes('urn:css:card-sign-in'),
    JSON.stringify(d.grant_types_supported))
  const notIdentifying = await setTower({ sign_in_app: TOWER.uuid })
  assertEq('card-sign-in/a-resource-that-does-not-identify-cannot-sign-in', 400, notIdentifying.status,
    notIdentifying.text.slice(0, 200))
  await setTower({ identify_member: true })
  for (const [why, app] of [['a-third-party-app', pop.client.id], ['a-public-app', spa.client.id], ['no-app-at-all', '00000000-0000-4000-8000-000000000000']]) {
    const r = await setTower({ sign_in_app: app })
    assertEq(`card-sign-in/${why}-cannot-be-signed-in-to`, 400, r.status, r.text.slice(0, 200))
  }
  const plainTap = await tap()
  ok('card-sign-in/off-by-default-a-tap-carries-no-sign-in', plainTap.json?.authorized === true && !('sign_in' in (plainTap.json ?? {})),
    plainTap.text.slice(0, 200))
  await POST(`/api/resources/${towerExt}/stop`, { token: reader, body: {} })
  const on = await setTower({ sign_in_app: TOWER.uuid })
  assertEq('card-sign-in/the-space-s-own-app-can-be-signed-in-to', 200, on.status, on.text.slice(0, 200))
  assertEq('card-sign-in/the-setting-is-reported', TOWER.uuid, on.json?.data?.sign_in_app)
  assertEq('card-sign-in/the-second-factor-is-off-by-default', 'off', on.json?.data?.second_factor)
  const keepsIdentify = await setTower({ identify_member: false })
  assertEq('card-sign-in/identify-cannot-be-turned-off-under-a-sign-in', 400, keepsIdentify.status, keepsIdentify.text.slice(0, 200))

  // A tap: a yes, the member named, and a code. It occupies nothing.
  const t1 = await tap()
  assertEq('card-sign-in/a-tap-is-a-yes', true, t1.json?.authorized, t1.text.slice(0, 200))
  assertEq('card-sign-in/a-tap-names-the-member', cardMember.user.id, t1.json?.member?.id)
  assertEq('card-sign-in/a-tap-carries-a-code', 'code', t1.json?.sign_in?.kind, t1.text.slice(0, 200))
  const toolNow = (await GET(`/api/tools/${towerId}`, T)).json?.data?.status
  assertEq('card-sign-in/a-sign-in-occupies-nothing', 'idle', toolNow)
  const t2 = await tap()
  assertEq('card-sign-in/the-next-tap-is-a-yes-too', true, t2.json?.authorized, t2.text.slice(0, 200))

  // Redeemed by the app, with its secret: tokens for the member.
  const redeem = (code, who = TOWER) => tokenRequest({ grant_type: 'urn:css:card-sign-in', code }, who)
  const got = await redeem(t1.json?.sign_in?.code)
  assertEq('card-sign-in/the-app-redeems-the-code', 200, got.status, got.text.slice(0, 200))
  const cardKeys = (await raw('GET', path(d.jwks_uri))).json?.keys ?? []
  ok('card-sign-in/the-id-token-verifies-against-the-published-keys', verifiesAgainst(got.json?.id_token, cardKeys))
  const cardIdc = decode(got.json?.id_token ?? 'e30.e30.x').claims
  assertEq('card-sign-in/the-id-token-names-the-member', cardMember.user.id, cardIdc.sub)
  assertEq('card-sign-in/the-id-token-is-for-the-app', TOWER.id, cardIdc.aud)
  assertEq('card-sign-in/the-id-token-says-a-card', '["hwk"]', JSON.stringify(cardIdc.amr))
  const tools = await raw('GET', '/api/oauth/me/tools', { headers: { Authorization: `Bearer ${got.json?.access_token}` } })
  assertEq('card-sign-in/the-access-token-reads-training', 200, tools.status, tools.text.slice(0, 200))
  ok('card-sign-in/no-app-scope-is-granted', !String(got.json?.scope ?? '').includes('notify:members'), String(got.json?.scope))

  // Once: a replay is refused, and ends what the first redemption issued.
  const replay = await redeem(t1.json?.sign_in?.code)
  assertEq('card-sign-in/a-code-is-good-once', 400, replay.status, replay.text.slice(0, 200))
  const afterReplay = await tokenRequest({ grant_type: 'refresh_token', refresh_token: got.json?.refresh_token }, TOWER)
  assertEq('card-sign-in/a-replay-ends-the-first-redemption-s-tokens', 400, afterReplay.status, afterReplay.text.slice(0, 200))
  // Not another app's, not a public client's, not expired.
  // Another enabled app with a secret (POP above was disabled by now).
  const byOther = await redeem(t2.json?.sign_in?.code, { id: trusted.client.client_id, secret: trusted.client_secret })
  assertEq('card-sign-in/another-app-cannot-redeem-it', 400, byOther.status, byOther.text.slice(0, 200))
  const byPublic = await tokenRequest({ grant_type: 'urn:css:card-sign-in', code: t2.json?.sign_in?.code, client_id: spa.client.client_id })
  ok('card-sign-in/a-public-client-cannot-redeem-it', byPublic.status === 400 || byPublic.status === 401, `${byPublic.status} ${byPublic.text.slice(0, 200)}`)
  const t3 = await tap()
  await new Promise((r) => setTimeout(r, 6000)) // the stack's code lifetime is five seconds
  const late = await redeem(t3.json?.sign_in?.code)
  assertEq('card-sign-in/an-expired-code-is-refused', 400, late.status, late.text.slice(0, 200))
  ok('card-sign-in/a-code-is-never-an-authorization-code',
    (await tokenRequest({ grant_type: 'authorization_code', code: t2.json?.sign_in?.code, redirect_uri: REDIRECT }, TOWER)).status === 400)

  // A second factor: a member with no TOTP is refused in words; with one,
  // a ticket, and the code only for their TOTP.
  await setTower({ second_factor: 'everyone' })
  const noTotp = await tap()
  assertEq('card-sign-in/a-second-factor-without-totp-is-refused', 'refused', noTotp.json?.sign_in?.kind, noTotp.text.slice(0, 200))
  ok('card-sign-in/the-refusal-says-where-to-set-one-up', /authenticator/.test(noTotp.json?.sign_in?.reason ?? ''),
    noTotp.json?.sign_in?.reason)
  const setup = await POST('/api/auth/mfa/totp/setup', { token: cardMember.token })
  const secret = setup.json?.data?.secret_base32
  const B32 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'
  const totp = (b32, at = Math.floor(Date.now() / 1000)) => {
    let bits = 0, value = 0
    const key = []
    for (const ch of b32.replace(/=+$/, '').toUpperCase()) {
      value = (value << 5) | B32.indexOf(ch); bits += 5
      if (bits >= 8) { key.push((value >>> (bits - 8)) & 0xff); bits -= 8 }
    }
    const buf = Buffer.alloc(8)
    const counter = Math.floor(at / 30)
    buf.writeUInt32BE(Math.floor(counter / 2 ** 32), 0); buf.writeUInt32BE(counter >>> 0, 4)
    const mac = createHmac('sha1', Buffer.from(key)).update(buf).digest()
    const o = mac[mac.length - 1] & 0x0f
    return String((((mac[o] & 0x7f) << 24) | (mac[o + 1] << 16) | (mac[o + 2] << 8) | mac[o + 3]) % 1_000_000).padStart(6, '0')
  }
  const confirmed = await POST('/api/auth/mfa/totp/confirm', { token: cardMember.token, body: { code: totp(secret) } })
  assertEq('card-sign-in/totp-enrolled', 200, confirmed.status, confirmed.text.slice(0, 200))
  // Past the step just used to confirm, so the next code is not a replay.
  await new Promise((r) => setTimeout(r, (30 - (Math.floor(Date.now() / 1000) % 30) + 1) * 1000))
  const withTicket = await tap()
  assertEq('card-sign-in/a-second-factor-asks-for-totp', 'totp', withTicket.json?.sign_in?.kind, withTicket.text.slice(0, 200))
  ok('card-sign-in/a-ticket-is-not-a-code', !('code' in (withTicket.json?.sign_in ?? {})))
  const ticket = withTicket.json?.sign_in?.ticket
  const second = (t, code, who = TOWER) => tokenRequest({ grant_type: 'urn:css:card-sign-in', ticket: t, totp: code }, who)
  const wrong = await second(ticket, totp(secret) === '000000' ? '111111' : '000000')
  assertEq('card-sign-in/a-wrong-totp-is-refused', 400, wrong.status, wrong.text.slice(0, 200))
  const right = await second(ticket, totp(secret))
  assertEq('card-sign-in/the-right-totp-signs-in', 200, right.status, right.text.slice(0, 200))
  assertEq('card-sign-in/the-id-token-says-card-and-otp', '["hwk","otp"]', JSON.stringify(decode(right.json?.id_token ?? 'e30.e30.x').claims.amr))
  const spent = await second(ticket, totp(secret))
  assertEq('card-sign-in/a-ticket-is-good-once', 400, spent.status, spent.text.slice(0, 200))
  // Three tries a ticket, then spent, whatever the fourth is. Each code is
  // from a fresh TOTP step: a code already used is refused as a replay
  // whatever the tries, and would let this case pass without the limit.
  const nextStep = () => new Promise((r) => setTimeout(r, (30 - (Math.floor(Date.now() / 1000) % 30) + 1) * 1000))
  const wrongCode = () => (totp(secret) === '000001' ? '000002' : '000001')
  // The control: two wrong tries, and the third, right, is taken.
  await nextStep()
  const t5 = await tap()
  for (let i = 0; i < 2; i++) await second(t5.json?.sign_in?.ticket, wrongCode())
  const third = await second(t5.json?.sign_in?.ticket, totp(secret))
  assertEq('card-sign-in/a-third-try-is-still-taken', 200, third.status, third.text.slice(0, 200))
  // Three wrong, and the fourth, right and fresh, is refused.
  await nextStep()
  const t4 = await tap()
  for (let i = 0; i < 3; i++) await second(t4.json?.sign_in?.ticket, wrongCode())
  const fourth = await second(t4.json?.sign_in?.ticket, totp(secret))
  assertEq('card-sign-in/a-ticket-allows-three-tries', 400, fourth.status, fourth.text.slice(0, 200))
  // Admins only: this member is not one, so a code, no ticket.
  await setTower({ second_factor: 'admins' })
  const memberTap = await tap()
  assertEq('card-sign-in/admins-only-spares-a-member', 'code', memberTap.json?.sign_in?.kind, memberTap.text.slice(0, 200))
  const badFactor = await setTower({ second_factor: 'sometimes' })
  assertEq('card-sign-in/an-unknown-second-factor-is-refused', 400, badFactor.status, badFactor.text.slice(0, 200))

  // A refusal carries no sign-in; and turning it off leaves the plain yes.
  const unknown = await POST(`/api/resources/${towerExt}/scan`, { token: reader, body: { card: 'NOBODY-CARD' } })
  ok('card-sign-in/a-refusal-carries-no-sign-in', unknown.json?.authorized === false && !('sign_in' in (unknown.json ?? {})),
    unknown.text.slice(0, 200))
  const off = await setTower({ sign_in_app: '' })
  assertEq('card-sign-in/sign-in-can-be-turned-off', null, off.json?.data?.sign_in_app, off.text.slice(0, 200))
  const afterOff = await tap()
  ok('card-sign-in/off-again-a-tap-carries-no-sign-in', !('sign_in' in (afterOff.json ?? {})), afterOff.text.slice(0, 200))

  // The record: each sign-in, and each wrong second factor, never a code.
  const signIns = await audit('card_sign_in')
  ok('card-sign-in/a-card-sign-in-is-audited',
    signIns.some((l) => l.user_id === cardMember.user.id && String(l.event_data?.resource_id ?? '') === towerId),
    JSON.stringify(signIns).slice(0, 300))
  ok('card-sign-in/a-wrong-second-factor-is-audited',
    (await audit('card_sign_in_second_factor_failed')).some((l) => l.user_id === cardMember.user.id))
  ok('card-sign-in/no-record-holds-a-code-or-ticket',
    !JSON.stringify(signIns).includes(t1.json?.sign_in?.code ?? '!') && !JSON.stringify(signIns).includes(ticket ?? '!'))
  }
})
