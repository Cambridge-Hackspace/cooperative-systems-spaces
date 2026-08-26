// Tier 8: concurrency.
//
// Two rules shape every scenario here, and both are the difference between a
// tier that finds races and one that reports green while they happen.
//
// ASSERT THE RESOURCE, NEVER THE RESPONSE TALLY. "Seven of eight requests got a
// 409" is a statement about this run's scheduling. "The database holds two
// devices for one single-use invite" is a statement about the product. The
// first is flaky; the second is a defect, and it is the same defect whether one
// request lost or seven did.
//
// EVERY SCENARIO HAS A NON-RACING SIBLING. A race test that fails to reproduce
// looks exactly like a race test whose setup was wrong. The sequential sibling
// runs the same operations one at a time and asserts the same invariant, so a
// green concurrent case beside a red sequential one says "the setup is broken",
// and a green sequential case beside a red concurrent one says "this is a
// race". Without the pair you cannot tell those apart.
//
// A race that loses four times in five is still a race, so each scenario runs
// several rounds and reports the worst. And the server's pool is configured
// above the fan-out in e2e/stack.sh on purpose: a pool smaller than the number
// of concurrent requests serialises them, which makes a race disappear and this
// tier report a pass it did not earn.
//
// WHAT THIS DOES NOT PROVE. Absence. A round that finds nothing means this
// scheduling did not lose, not that the window is closed. Only the code can
// say that -- an interleaving-free implementation is a `WHERE used_at IS NULL`
// on the update, or a transaction, and the finding is worth acting on whether
// or not a later round reproduces it.

import { GET, POST, PUT, adminAccount, record, ok, assertEq, main, RUN_TAG } from './lib.mjs'

const FANOUT = Number(process.env.CSS_RACE_FANOUT ?? 8)
const ROUNDS = Number(process.env.CSS_RACE_ROUNDS ?? 3)
const TAG = `${RUN_TAG}_${process.hrtime.bigint().toString(36).slice(-6)}`
const ENCODING = process.env.CSS_DB_ENCODING ?? 'UTF8'

main(async () => {
  console.log(`fan-out ${FANOUT}, rounds ${ROUNDS}, tag ${TAG}`)
  const admin = await adminAccount(`race_admin_${TAG}`)

  await inviteRedemption(admin)
  await profileConfigVersions(admin)
})

// ---------------------------------------------------------------------------
// Scenario 1 -- device-invite redemption
// ---------------------------------------------------------------------------
// `devices::register_device` reads the invite, checks `used_at.is_some()`,
// inserts a device, inserts its auth token, and only then marks the invite
// used -- four statements, no transaction, and no `WHERE used_at IS NULL` on
// the update. Two redemptions that interleave between the check and the mark
// both pass the check.
//
// The consequence is not an extra row. A device row carries an auth token, and
// that token is a standing credential on the toolguard and door surface. A
// single-use invite that mints two of them hands one to somebody who was never
// meant to have it, and every audit trail afterwards shows one legitimate
// registration.
async function inviteRedemption(admin) {
  // A device invite code is eight emoji -- `SpaceDeviceAuthRequest::new_device_code`
  // picks from a ~250-entry emoji alphabet, so that a code can be read aloud
  // across a workshop. It also means `space_device_auth_requests` cannot be
  // written at all on a database whose encoding has no emoji, and nothing in
  // the application or its documentation says the encoding is a requirement.
  //
  // So on a non-UTF-8 cluster this scenario asserts the finding instead of the
  // race. That is a narrowing and it covers exactly one scenario: device-invite
  // redemption on a non-UTF-8 cluster. The profile-config race below runs
  // either way, and the nightly UTF-8 run exercises this one for real.
  if (ENCODING !== 'UTF8') {
    const probe = await POST('/api/admin/devices/invite', {
      token: admin.token,
      body: { expires_in_hours: 1 },
    })
    assertEq(
      'findings/device-invite-codes-require-a-utf8-database',
      500,
      probe.status,
      `PINNED FINDING, not a passing behaviour: on a ${ENCODING} cluster a device ` +
        'invite cannot be created at all, because the code is eight emoji. ' +
        'Device registration is therefore impossible, and nothing says the ' +
        'application requires a UTF-8 database. If this assertion fails, either ' +
        'the code alphabet changed or the cluster did -- check which. ' +
        'See TESTING.md, "Known defects".',
    )
    record('race/invite/not-run-on-this-cluster', 'skip',
      `the invite race needs an invite, and this cluster (${ENCODING}) cannot ` +
      'store one. The profile-config race below still runs.')
    return
  }

  let worst = null

  for (let round = 0; round < ROUNDS; round += 1) {
    const invite = await POST('/api/admin/devices/invite', {
      token: admin.token,
      body: { expires_in_hours: 1 },
    })
    if (invite.status !== 200 && invite.status !== 201) {
      record('race/invite/setup', 'fail', `creating an invite answered ${invite.status}`)
      return
    }
    const code = invite.json?.data?.device_code
    if (!code) {
      record('race/invite/setup', 'fail', `no data.device_code in ${invite.text.slice(0, 200)}`)
      return
    }

    const prefix = `race-dev-${TAG}-r${round}`
    // Built first, fired second. Awaiting inside the loop would serialise them
    // and the window would never open.
    const thunks = []
    for (let i = 0; i < FANOUT; i += 1) {
      thunks.push(() =>
        POST('/api/devices/register', {
          body: {
            device_code: code,
            name: `${prefix}-${i}`,
            kind: 'edge',
            mac_address: `02:00:00:00:${round.toString(16).padStart(2, '0')}:${i.toString(16).padStart(2, '0')}`,
            software_version: '0.0.0-e2e',
            platform: 'linux',
          },
        }),
      )
    }
    const results = await Promise.all(thunks.map((t) => t()))

    // The oracle: how many devices exist, not how many requests said they
    // succeeded.
    const devices = await GET('/api/admin/devices', { token: admin.token })
    const created = (devices.json?.data ?? []).filter((d) =>
      String(d.name ?? '').startsWith(prefix),
    )

    const fiveHundreds = results.filter((r) => r.status >= 500).length
    if (created.length > 1 && (worst === null || created.length > worst.created)) {
      worst = { round, created: created.length, fiveHundreds, prefix }
    }
    if (fiveHundreds > 0 && worst === null) {
      worst = { round, created: created.length, fiveHundreds, prefix }
    }
    console.log(
      `  invite round ${round}: ${created.length} device(s) from one invite, ` +
        `${results.filter((r) => r.status < 300).length} accepted, ${fiveHundreds} 5xx`,
    )
  }

  if (worst && worst.created > 1) {
    record('race/invite/one-invite-mints-one-device', 'fail',
      `${worst.created} devices were created from a single single-use invite in round ` +
        `${worst.round} (fan-out ${FANOUT}). register_device checks used_at and marks it ` +
        'in separate statements with no transaction and no WHERE used_at IS NULL, so two ' +
        'redemptions that interleave between the two both pass. Each extra device carries ' +
        'its own standing auth token.')
  } else {
    record('race/invite/one-invite-mints-one-device', 'ok',
      `${ROUNDS} rounds at fan-out ${FANOUT} did not reproduce; this is not evidence ` +
        'the window is closed, only that this scheduling did not lose')
  }
  if (worst && worst.fiveHundreds > 0) {
    record('race/invite/losers-are-refused-not-faulted', 'fail',
      `${worst.fiveHundreds} redemption(s) answered 5xx. A losing racer should be told no, ` +
        'not told the server broke.')
  } else {
    record('race/invite/losers-are-refused-not-faulted', 'ok')
  }

  // --- the non-racing sibling ---------------------------------------------
  const invite = await POST('/api/admin/devices/invite', {
    token: admin.token,
    body: { expires_in_hours: 1 },
  })
  const code = invite.json?.data?.device_code
  const prefix = `race-seq-${TAG}`
  const first = await registerOne(code, `${prefix}-0`, '02:00:00:00:ff:00')
  const second = await registerOne(code, `${prefix}-1`, '02:00:00:00:ff:01')

  assertEq('race/invite/sequential-first-redemption-succeeds', true, first.status < 300)
  assertEq('race/invite/sequential-second-redemption-is-refused', 400, second.status)

  const devices = await GET('/api/admin/devices', { token: admin.token })
  const created = (devices.json?.data ?? []).filter((d) => String(d.name ?? '').startsWith(prefix))
  assertEq('race/invite/sequential-creates-exactly-one-device', 1, created.length)
}

function registerOne(code, name, mac) {
  return POST('/api/devices/register', {
    body: {
      device_code: code,
      name,
      kind: 'edge',
      mac_address: mac,
      software_version: '0.0.0-e2e',
      platform: 'linux',
    },
  })
}

// ---------------------------------------------------------------------------
// Scenario 2 -- the profile-config version race
// ---------------------------------------------------------------------------
// `insert_profile_config_version` does `SELECT max(version)` and then
// `INSERT version = max + 1`, inside a READ COMMITTED transaction, against a
// UNIQUE constraint on version. Two concurrent admin edits both read N and one
// gets a unique violation.
//
// The lost update is the smaller half of the finding. The larger half is what
// the loser is told: the failure returns `DatabaseError::Diesel`, whose
// conversion special-cases only NotFound, so it becomes a 500 -- while the
// direct `From<diesel::result::Error>` path in the same file *does* map
// UniqueViolation to Conflict. Two conversion paths, two answers for one
// failure, and the one that fires here is the one that tells an admin the site
// is broken when their edit merely needs retrying.
async function profileConfigVersions(admin) {
  const cfg = await GET('/api/profiles/config', { token: admin.token })
  if (cfg.status !== 200) {
    record('race/config/setup', 'fail', `reading the profile config answered ${cfg.status}`)
    return
  }
  const body = cfg.json?.data ?? {}

  let worstFive = 0
  let worstGap = null

  for (let round = 0; round < ROUNDS; round += 1) {
    const before = await versions(admin)

    const thunks = []
    for (let i = 0; i < FANOUT; i += 1) {
      thunks.push(() => PUT('/api/profiles/config', { token: admin.token, body }))
    }
    const results = await Promise.all(thunks.map((t) => t()))

    const five = results.filter((r) => r.status >= 500).length
    const conflict = results.filter((r) => r.status === 409).length
    const okd = results.filter((r) => r.status < 300).length
    worstFive = Math.max(worstFive, five)

    const after = await versions(admin)
    const gap = numberingProblem(after)
    if (gap && !worstGap) worstGap = gap

    console.log(
      `  config round ${round}: ${before.length} -> ${after.length} versions, ` +
        `${okd} accepted, ${conflict} conflict, ${five} 5xx${gap ? `, ${gap}` : ''}`,
    )
  }

  if (worstFive > 0) {
    record('race/config/loser-gets-a-conflict-not-a-fault', 'fail',
      `${worstFive} concurrent profile-config write(s) answered 5xx. A lost update is a ` +
        'conflict the caller can retry; errors.rs has two conversion paths for a unique ' +
        'violation and this one takes the path that does not recognise it. Fixing the ' +
        'conversion is separate from closing the race, and both are worth doing.')
  } else {
    record('race/config/loser-gets-a-conflict-not-a-fault', 'ok')
  }

  const final = await versions(admin)
  const problem = numberingProblem(final)
  ok('race/config/version-numbering-is-contiguous', problem === null, String(problem))

  // --- the non-racing sibling ---------------------------------------------
  const before = await versions(admin)
  for (let i = 0; i < 3; i += 1) {
    const res = await PUT('/api/profiles/config', { token: admin.token, body })
    assertEq(`race/config/sequential-write-${i}-succeeds`, 200, res.status)
  }
  const after = await versions(admin)
  assertEq('race/config/sequential-writes-each-add-one-version',
    before.length + 3, after.length)
  ok('race/config/sequential-numbering-is-contiguous',
    numberingProblem(after) === null, String(numberingProblem(after)))
}

async function versions(admin) {
  const res = await GET('/api/profiles/config/versions', { token: admin.token })
  const list = Array.isArray(res.json?.data) ? res.json.data : []
  return list.map((v) => Number(v.version)).filter((n) => Number.isFinite(n))
}

/**
 * `null` when the version numbers are a contiguous run from 1 with no
 * duplicates, and a description of what is wrong otherwise.
 *
 * Duplicates and gaps are different defects. A duplicate means the unique
 * constraint is not doing what the code assumes; a gap means a version was
 * allocated and then lost, which is the visible trace of an update that
 * silently did not happen.
 */
function numberingProblem(list) {
  if (list.length === 0) return 'no versions at all'
  const sorted = [...list].sort((a, b) => a - b)
  const unique = new Set(sorted)
  if (unique.size !== sorted.length) return `duplicate version numbers in ${sorted.join(',')}`
  if (sorted[0] !== 1) return `numbering starts at ${sorted[0]}, not 1`
  for (let i = 1; i < sorted.length; i += 1) {
    if (sorted[i] !== sorted[i - 1] + 1) {
      return `gap between ${sorted[i - 1]} and ${sorted[i]} in ${sorted.join(',')}`
    }
  }
  return null
}
