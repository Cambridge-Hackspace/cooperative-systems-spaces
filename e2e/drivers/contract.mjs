// Tier 6, `contract`: the authorization surface against a real database.
//
// This is the tier that can answer what the offline matrix cannot. Three
// distinct claims live here and nowhere else:
//
//  1. **Positive authorization.** The offline matrix proves that no guarded
//     route answers anything but 401 *without* a valid credential. It cannot
//     prove that a valid credential is accepted, because every route that would
//     accept one reaches the database. A suite that only ever asserts refusals
//     would stay green on a server that refused everybody.
//
//  2. **The 24 deferred device pairs.** `DeviceAuth` validates an opaque token
//     by looking it up, so offline it hits the dead pool and answers 500. Those
//     six routes times four content-based credentials are explicitly deferred
//     here by `the_offline_device_surface_is_exactly_this_narrow`, and if this
//     file did not assert them the deferral would be a hole rather than a split.
//
//  3. **The hostile encoding.** LATIN1 rejects astral-plane text at the server
//     with SQLSTATE 22P05. Whether the application turns that into a 4xx or a
//     500 is invisible to every tier that runs against a UTF-8 cluster.

import {
  GET, PUT, POST, DELETE,
  account, login, register,
  assertEq, assertNe, ok, record, main, RUN_TAG, ADMIN_EMAIL,
} from './lib.mjs'

/** The six routes behind a device credential, per server/tests/common/mod.rs. */
const DEVICE_ROUTES = [
  ['GET', '/api/devices/ws'],
  ['POST', '/api/toolguard/boot-reset'],
  ['GET', '/api/toolguard/sync'],
  // `seconds` too. `ToolLogRequest` requires it, and Query is a
  // FromRequestParts extractor -- so without it the request is rejected with
  // 400 before the handler's authentication check runs, and the row would be
  // asserting the extractor rather than the credential.
  ['GET', '/api/toolguard/tool-log?card=AA11&tool_id=t1&seconds=1'],
  ['GET', '/api/toolguard/tool-off?card=AA11&tool_id=t1'],
  ['GET', '/api/toolguard/tool-on?card=AA11&tool_id=t1'],
]

/**
 * The four credentials the offline matrix could not judge on device routes:
 * everything that survives the shape checks and therefore requires a lookup.
 * Kept verbatim in step with `CREDS` in server/tests/contract_matrix.rs -- the
 * duplication is deliberate, because a list derived from that file would agree
 * with it however it changed.
 */
const CONTENT_CREDS = [
  ['Bearer with nothing after it', 'Bearer '],
  ['a token that is not a JWT', 'Bearer not-a-jwt'],
  [
    'a JWT signed with the wrong key',
    'Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxIn0.wrong',
  ],
  ['a JWT claiming alg=none', 'Bearer eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJzdWIiOiIxIn0.'],
]

main(async () => {
  // -----------------------------------------------------------------------
  // Accounts, through the shipping path
  // -----------------------------------------------------------------------
  const admin = await account('admin', { email: ADMIN_EMAIL })
  assertEq('contract/initial-setup-grants-admin', 'Admin', admin.user?.role)

  const newbie = await account('newbie')
  assertEq('contract/new-accounts-are-newbies', 'Newbie', newbie.user?.role)

  // A second registration on the setup address must not mint a second admin.
  // `should_grant_admin_role` matches on the address alone, so if the guard
  // were only "does the address match" every later signup with it would be an
  // admin -- and nothing else in the suite looks at that path.
  const impostor = await register(`e2e_impostor_${RUN_TAG}`, ADMIN_EMAIL)
  if (impostor.status === 200 || impostor.status === 201) {
    const li = await login(`e2e_impostor_${RUN_TAG}`)
    const role = li.json?.data?.user?.role
    assertNe('contract/setup-address-is-not-a-standing-admin-grant', 'Admin', role)
  } else {
    // Refusing the duplicate address outright is the stronger answer.
    record('contract/setup-address-is-not-a-standing-admin-grant', 'ok',
      `duplicate address refused with ${impostor.status}`)
  }

  // -----------------------------------------------------------------------
  // The four fixes the acceptance test reverts
  // -----------------------------------------------------------------------

  // 5c2fa3c: the profile-configuration *read* is every authenticated user's.
  // Before the fix it was admin-only, which broke the profile page for
  // everybody who was not an admin.
  assertEq('contract/5c2fa3c/newbie-reads-profile-config', 200,
    (await GET('/api/profiles/config', { token: newbie.token })).status)
  assertEq('contract/5c2fa3c/admin-reads-profile-config', 200,
    (await GET('/api/profiles/config', { token: admin.token })).status)

  // 11c4f42: the *write* is not.
  const cfg = await GET('/api/profiles/config', { token: admin.token })
  ok('contract/profile-config-read-has-a-body', cfg.json?.success === true,
    `body was ${cfg.text.slice(0, 200)}`)

  assertEq('contract/11c4f42/newbie-cannot-write-profile-config', 403,
    (await PUT('/api/profiles/config', { token: newbie.token, body: cfg.json?.data ?? {} })).status)

  const adminWrite = await PUT('/api/profiles/config', {
    token: admin.token, body: cfg.json?.data ?? {},
  })
  assertEq('contract/11c4f42/admin-can-write-profile-config', 200, adminWrite.status)

  // A finding this stage found by being run against a read-only config mount,
  // recorded because the shape survives the mount being writable.
  //
  // `update_profile_config` inserts the new version row, and *then* writes
  // `profiles_enabled` back to the configuration file. The two are not in a
  // transaction and there is no compensation: if the file write fails -- a
  // read-only ConfigMap, a full disk, a permissions change -- the version row
  // is already committed and the caller is told 500. The admin sees a failure,
  // the history shows their change, and the two disagree permanently.
  //
  // Not asserted as a failure here, because with a writable config the call
  // succeeds and there is nothing to assert. Named so it is in the report.
  record('findings/profile-config-write-is-not-atomic', 'ok',
    'insert_profile_config_version commits before the config file is written, ' +
    'with no transaction across the two; a failed file write leaves a committed ' +
    'version row and returns 500. See TESTING.md, "Known defects".')

  // And the positive path actually did something: a write that returns 200 and
  // records nothing would satisfy the assertion above while the feature was
  // entirely broken.
  const versions = await GET('/api/profiles/config/versions', { token: admin.token })
  assertEq('contract/profile-config-versions-listable', 200, versions.status)
  const count = Array.isArray(versions.json?.data) ? versions.json.data.length : -1
  ok('contract/admin-write-created-a-version', count >= 2,
    `expected at least the bootstrap plus this write, saw ${count}`)

  // -----------------------------------------------------------------------
  // The 24 pairs the offline matrix deferred
  // -----------------------------------------------------------------------
  // Every one of them must be 401. Not "a client error" -- 401 exactly, and
  // never 500, because 500 is what the offline fixture returns for all of them
  // and is precisely the answer this tier exists to distinguish from a
  // considered refusal.
  let pairs = 0
  for (const [method, path] of DEVICE_ROUTES) {
    for (const [what, header] of CONTENT_CREDS) {
      pairs += 1
      const res = await fetchRaw(method, path, header)
      const name = `contract/device/${method} ${path.split('?')[0]} with ${what}`
      if (res.status === 500) {
        record(name, 'fail',
          'answered 500: the device lookup faulted rather than refusing the credential')
      } else {
        assertEq(name, 401, res.status)
      }
    }
  }
  assertEq('contract/device/all-deferred-pairs-asserted', 24, pairs)

  // The toolguard `api_key` path is the other half of the credential the
  // security fix accepts, and it has its own refusal to prove. A wrong key must
  // be refused rather than ignored -- "ignored" is what the endpoints did
  // before the fix, and it read identically from the outside until you noticed
  // the door had opened.
  for (const [path, extra] of [
    ['/api/toolguard/tool-on', ''],
    ['/api/toolguard/tool-off', ''],
    ['/api/toolguard/tool-log', '&seconds=1'],
  ]) {
    assertEq(`contract/toolguard/${path} refuses a wrong api_key`, 401,
      (await GET(`${path}?card=AA11&tool_id=t1${extra}`, { apiKey: 'not-a-real-key' })).status)
  }

  // -----------------------------------------------------------------------
  // The hostile encoding
  // -----------------------------------------------------------------------
  // LATIN1 cannot represent an emoji. Postgres refuses the byte sequence with
  // SQLSTATE 22P05 at the server, and the question this tier answers is what
  // the application does with that: a 4xx naming a bad request, or a 500 that
  // tells the user the site is broken. `DatabaseError::Diesel` becomes a 500,
  // so this is expected to find something.
  const astral = await register(`e2e_astral_${RUN_TAG}`, `astral_${RUN_TAG}@e2e.invalid`)
  record('contract/astral/baseline-registration', astral.status < 300 ? 'ok' : 'fail',
    `${astral.status}`)

  const emoji = await register(`e2e_emoji_${RUN_TAG}_\u{1F6A7}`, `emoji_${RUN_TAG}@e2e.invalid`)
  assertEq(
    'findings/astral-text-is-a-500-not-a-4xx',
    500,
    emoji.status,
    'PINNED FINDING, not a passing behaviour: text a LATIN1 database cannot ' +
      'store is refused with SQLSTATE 22P05 and the application returns 500, ' +
      'telling the user the site is broken about an input only they can change. ' +
      'If this assertion fails, the defect was fixed -- delete it. ' +
      'See TESTING.md, "Known defects".',
    // Pinned, not left red. A suite that stays red teaches people to ignore
    // red; an assertion that pins a defect in place fails the day somebody
    // fixes it, which is exactly when it should be read and deleted.
    //
    // The finding: text a LATIN1 database cannot store is refused by Postgres
    // with SQLSTATE 22P05, and the application turns that into a 500 -- so the
    // user is told the site is broken about an input only they can change.
    // Classifying it needs the SQLSTATE, which diesel's
    // DatabaseErrorInformation does not expose, so the only way to recognise it
    // today is matching English prose that changes with the server's
    // lc_messages. See TESTING.md, "Known defects".
  )

  // What the same request does on a UTF-8 cluster, for contrast: it succeeds.
  // Recorded so a reader knows the finding is about the encoding and not about
  // the character.
  record('findings/astral-text-context', 'ok',
    'this case only fires on a non-UTF-8 cluster; on UTF-8 the same registration succeeds')

  // -----------------------------------------------------------------------
  // Case folding, under lc_ctype=C
  // -----------------------------------------------------------------------
  // With C ctype there is no case folding outside ASCII, so whatever
  // case-insensitive matching the application does it must be doing itself.
  // This asserts the behaviour rather than the mechanism.
  const upper = await login(newbie.username.toUpperCase())
  assertEq(
    'findings/login-is-case-sensitive',
    401,
    upper.status,
    'PINNED FINDING, not a passing behaviour: login is case-sensitive on both ' +
      'username and email because both lookups filter with a plain eq. ' +
      'If this assertion fails, the defect was fixed -- delete it. ' +
      'See TESTING.md, "Known defects".',
    // Not a collation artifact. `find_user_by_username` and
    // `find_user_by_email` both filter with a plain `eq`, with no lower() on
    // either side, so this is the behaviour on every cluster.
    //
    // It matters most for the email path, which is the one people actually
    // retype: somebody who registered as Alice@example.com and types
    // alice@example.com is told "Wrong credentials", which is indistinguishable
    // from a wrong password and sends them to reset a password that was right.
    //
    // The other half is worse and is not asserted here because it needs a
    // second account: the unique index is on the raw column, so
    // Alice@example.com and alice@example.com are two accounts.
    //
    // Not fixed here. Making the lookup case-insensitive means a migration, a
    // functional index, and a decision about existing rows that already
    // collide -- a product change, not a status-code correction.
  )
  const byEmail = await login(newbie.email)
  assertEq('contract/login-accepts-the-email-address', 200, byEmail.status)

  // -----------------------------------------------------------------------
  // A guarded route with a valid credential is not refused
  // -----------------------------------------------------------------------
  // The meta-assertion. Without it, a server that rejected every credential
  // would satisfy every refusal assertion above and this whole tier would
  // report green.
  assertEq('contract/a-valid-credential-is-accepted', 200,
    (await GET('/api/auth/me', { token: newbie.token })).status)
  assertEq('contract/an-admin-only-route-accepts-an-admin', 200,
    (await GET('/api/users', { token: admin.token })).status)
  assertEq('contract/an-admin-only-route-refuses-a-newbie', 403,
    (await GET('/api/users', { token: newbie.token })).status)

  // A removal that removed nothing has to say so.
  //
  // `DatabaseManager::remove_tool_trainer` deactivated by row filter and threw
  // the row count away with `.map(|_| ())`, so a DELETE naming a tool or a user
  // that does not exist answered 200. The handler then wrote a
  // `trainer_removed` audit record for a removal that never happened, and on a
  // LATIN1 stack run whose fuzz seed reached this route with a synthetic user
  // id, that insert violated audit_logs_user_id_fkey. The audit write is
  // swallowed -- `if let Err(e) = ... { tracing::error!(...) }` and then
  // `Ok(())` -- so the only trace was one ERROR line, which is how the logs
  // oracle caught it and nothing else would have.
  //
  // The status carries two claims. It tells the caller the truth, and it is
  // also what stops the audit write from happening at all: 404 returns before
  // the logger is reached. Asserting the status therefore covers the audit
  // defect without depending on log scraping to notice it.
  const ghostTool = '00000000-0000-4000-8000-0000000000aa'
  const ghostUser = '00000000-0000-4000-8000-0000000000bb'
  assertEq('contract/removing-a-trainer-that-is-not-there-is-a-404', 404,
    (await DELETE(`/api/trainers/tools/${ghostTool}/trainers/${ghostUser}`,
      { token: admin.token })).status)

  // Logging out and reusing the token. JWTs are stateless here, so this
  // records the actual behaviour rather than asserting a property the design
  // does not have.
  const out = await POST('/api/auth/logout', { token: newbie.token })
  assertEq('contract/logout-succeeds', 200, out.status)
  const after = await GET('/api/auth/me', { token: newbie.token })
  record('contract/token-after-logout', after.status === 200 ? 'ok' : 'ok',
    `${after.status} -- recorded, not asserted: the token is a stateless JWT ` +
    'and logout is a client-side discard. If that ever becomes a revocation ' +
    'this line is where the change should be noticed.')

  // Deleting the accounts this run created, through the shipping path, so a
  // cluster without rollback does not accumulate them across runs.
  for (const u of [newbie, admin]) {
    const res = await DELETE(`/api/users/${u.user?.id}`, { token: admin.token })
    record(`contract/cleanup/${u.username}`, res.status < 400 ? 'ok' : 'skip', `${res.status}`)
  }
})

/** A request with a literal Authorization header, valid or not. */
async function fetchRaw(method, path, header) {
  const base = process.env.CSS_BASE_URL ?? 'http://127.0.0.1:4399'
  const res = await fetch(`${base}${path}`, {
    method,
    headers: { Authorization: header },
    redirect: 'manual',
  })
  await res.text()
  return { status: res.status }
}
