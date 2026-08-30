// Tier 9's invariants: what must be true of the world after any sequence of
// actions, independent of which actions were taken.
//
// Separated from the journey driver on purpose, so that `selftest.mjs` can feed
// each of them what a broken server would send and confirm it fires. An
// invariant that has never fired is indistinguishable from a passing suite, and
// the whole tier rests on these being right — a journey that runs a thousand
// actions past a broken invariant reports a thousand successes.
//
// Each invariant takes a *shadow model*: what the driver believes the world
// should look like, built from the actions it took and the responses it got.
// It returns null when satisfied, or a string describing the violation.
//
// THE SHAPE OF EACH ONE MATTERS. They assert properties of the accumulated
// world, not of individual responses:
//
//   * a response assertion says "this request did the right thing";
//   * an invariant says "nothing in the last two hundred requests broke this".
//
// The second is what catches the defects that only appear after history
// accumulates, which is the entire point of the tier.

/** @typedef {{ id: string, username: string, email: string, role: string, active: boolean }} ShadowUser */

/**
 * The roster the driver believes exists must equal the roster the server
 * returns — as a set of ids, and with no duplicates on either side.
 *
 * Modeled on a real shape: a duplicate roster entry after a role change, where
 * the update inserted rather than updated. The set comparison catches the
 * absence; the duplicate check catches the presence, and neither catches the
 * other.
 */
export function rosterMatches(model, observed) {
  const expected = new Set(model.users.filter((u) => u.exists).map((u) => u.id))
  const actualIds = observed.map((u) => u.id)
  const actual = new Set(actualIds)

  if (actualIds.length !== actual.size) {
    const dupes = actualIds.filter((id, i) => actualIds.indexOf(id) !== i)
    return `the roster contains duplicate ids: ${[...new Set(dupes)].join(', ')}`
  }

  const missing = [...expected].filter((id) => !actual.has(id))
  const extra = [...actual].filter((id) => !expected.has(id))

  if (missing.length) return `created but absent from the roster: ${missing.join(', ')}`
  if (extra.length) return `in the roster and never created (or already deleted): ${extra.join(', ')}`
  return null
}

/**
 * Every role the driver set is the role the server reports.
 *
 * The 5c2fa3c shape generalised: a permission read that answers correctly for
 * one class of caller and wrongly for another is invisible to a journey that
 * only ever runs as an admin.
 */
export function rolesMatch(model, observed) {
  const byId = new Map(observed.map((u) => [u.id, u]))
  const wrong = []
  for (const u of model.users.filter((x) => x.exists)) {
    const actual = byId.get(u.id)
    if (!actual) continue // rosterMatches owns absence
    if (actual.role !== u.role) {
      wrong.push(`${u.username}: model says ${u.role}, server says ${actual.role}`)
    }
  }
  return wrong.length ? wrong.join('; ') : null
}

/**
 * A rule the server accepted must appear in the list it returns.
 *
 * This is 92afb4c's shape at the world level rather than the response level: the
 * request answered 200, the UI showed nothing, and the rule was not there. A
 * per-response assertion sees the 200 and stops.
 */
export function acceptedRulesArePresent(model, observed) {
  const present = new Set(observed.map((r) => r.id))
  const missing = model.doorRules
    .filter((r) => r.accepted && !r.deleted)
    .filter((r) => !present.has(r.id))
    .map((r) => `${r.kind}=${r.value} on door ${r.doorId}`)
  return missing.length
    ? `the server accepted these rules and does not list them: ${missing.join('; ')}`
    : null
}

/**
 * Profile-config versions are a contiguous run from 1, with no duplicates.
 *
 * The lost-update race made visible. `[1, 2, 2, 3]` means the unique constraint
 * is not doing what the code assumes; `[1, 2, 4]` means a version was allocated
 * and lost, which is the visible trace of an update that silently did not
 * happen. They are different defects and the message says which.
 */
export function versionsAreContiguous(_model, observed) {
  const numbers = observed.map((v) => Number(v.version)).filter(Number.isFinite)
  if (numbers.length === 0) return 'no profile config versions at all; the boot bootstrap did not run'

  const sorted = [...numbers].sort((a, b) => a - b)
  if (new Set(sorted).size !== sorted.length) {
    return `duplicate version numbers: ${sorted.join(',')}`
  }
  if (sorted[0] !== 1) return `numbering starts at ${sorted[0]}, not 1`
  for (let i = 1; i < sorted.length; i += 1) {
    if (sorted[i] !== sorted[i - 1] + 1) {
      return `gap between ${sorted[i - 1]} and ${sorted[i]} in ${sorted.join(',')}`
    }
  }
  return null
}

/**
 * A single-use invite produced at most one device.
 *
 * Asserted on the resource, never on how many requests said they succeeded. The
 * extra device is not the harm — it carries a standing auth token on the
 * toolguard and door surface, and the audit trail shows one registration.
 */
export function invitesAreSingleUse(model, observed) {
  const byInvite = new Map()
  for (const d of observed) {
    if (!d.inviteCode) continue
    byInvite.set(d.inviteCode, (byInvite.get(d.inviteCode) ?? 0) + 1)
  }
  const over = [...byInvite.entries()].filter(([, n]) => n > 1)
  if (over.length) {
    return over.map(([code, n]) => `invite ${code} produced ${n} devices`).join('; ')
  }
  // And the model's own count, so a server that returns no devices at all does
  // not satisfy this by returning nothing.
  const expected = model.devices.filter((d) => d.registered).length
  if (observed.length < expected) {
    return `${expected} device(s) were registered and the server lists ${observed.length}`
  }
  return null
}

/**
 * Nothing the driver deactivated is reported active.
 *
 * Deliberately one-directional. A user the driver never touched may have been
 * changed by another actor in an accumulated world, so "active in the model"
 * proves nothing — but "the driver deactivated this and it is active" is a
 * statement about an action the driver definitely took.
 */
export function deactivationsHeld(model, observed) {
  const byId = new Map(observed.map((u) => [u.id, u]))
  const resurrected = model.users
    .filter((u) => u.exists && u.deactivated)
    .filter((u) => byId.get(u.id)?.is_active === true)
    .map((u) => u.username)
  return resurrected.length
    ? `deactivated and reported active: ${resurrected.join(', ')}`
    : null
}

/**
 * Every invariant, in the order a report should list them.
 *
 * Exported as a list rather than referenced by name at each call site, so
 * `selftest.mjs` can assert it has a case for every one of them — which is what
 * stops an invariant being added without a test that it fires.
 */
export const INVARIANTS = [
  { name: 'roster-matches', fn: rosterMatches },
  { name: 'roles-match', fn: rolesMatch },
  { name: 'accepted-rules-are-present', fn: acceptedRulesArePresent },
  { name: 'versions-are-contiguous', fn: versionsAreContiguous },
  { name: 'invites-are-single-use', fn: invitesAreSingleUse },
  { name: 'deactivations-held', fn: deactivationsHeld },
]
