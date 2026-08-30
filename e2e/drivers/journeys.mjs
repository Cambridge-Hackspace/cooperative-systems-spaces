// Tier 9: simulated users.
//
// The oracle for this tier was written first and has been sitting in
// e2e/journeys/invariants.mjs with a 20-case self-test and nothing to judge.
// This is the half that gives it a world: a seeded sequence of actions taken
// through the shipping HTTP API, a shadow model of what those actions should
// have produced, and a periodic check that every invariant still holds.
//
// The distinction that makes the tier worth having, restated because it is easy
// to lose while writing one of these:
//
//   * a response assertion says "this request did the right thing";
//   * an invariant says "nothing in the last two hundred requests broke this".
//
// So this driver deliberately does NOT assert on most responses. A 400 from a
// nemesis action is fine and expected; what matters is whether the accumulated
// world still satisfies the invariants afterwards. Asserting per-response here
// would duplicate the contract tier and bury the thing this tier is for.
//
// Nothing is created behind the API's back. Every user is registered through
// /api/auth/register, every role change goes through PUT /api/users/{id}, and
// the place vocabulary is read from the server rather than guessed at -- a
// driver that seeds its own rows tests a world the application never built.

import {
  GET, POST, PUT, DELETE,
  record, ok, main, RUN_TAG, PASSWORD,
  adminAccount, register,
} from './lib.mjs'
import { INVARIANTS } from '../journeys/invariants.mjs'

const ITERATIONS = Number(process.env.CSS_JOURNEY_ITERATIONS ?? 200)
const CHECK_EVERY = Number(process.env.CSS_JOURNEY_CHECK_EVERY ?? 20)
const ENCODING = process.env.CSS_DB_ENCODING ?? 'UTF8'
const CONTROL = process.env.REAPER_CONTROL ?? ''

// Seeded, and printed first so a finding can be replayed.
//
// The seed reproduces the *sequence of choices*, not the run: ids are minted by
// the server and differ every time, so a replay follows a similar path rather
// than an identical one. Every violation therefore carries its own action log
// verbatim, which needs no replay at all.
const SEED = Number(process.env.CSS_JOURNEY_SEED ?? Math.floor(Date.now() % 2147483647))

function mulberry32(a) {
  return function rng() {
    a |= 0
    a = (a + 0x6d2b79f5) | 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}
const rand = mulberry32(SEED)
const pick = (xs) => xs[Math.floor(rand() * xs.length)]
const chance = (p) => rand() < p

// RUN_TAG is drawn OUTSIDE the seeded stream, deliberately. Replaying a seed has
// to reproduce the sequence of choices while still minting unique usernames --
// a replay that collides with the previous run's rows is testing a different
// world and reports different results for reasons unrelated to the seed.
let counter = 0
const unique = (p) => `${p}_${RUN_TAG}_${(counter += 1)}`

// ---------------------------------------------------------------------------
// The shadow model
// ---------------------------------------------------------------------------
// Exactly the shape invariants.mjs documents. Kept deliberately dumb: it
// records what the driver asked for and what the server said, and nothing
// derived. A model that computes what it thinks the server should have done
// agrees with itself.
const model = { users: [], doorRules: [], devices: [] }

const log = []
function note(action, detail) {
  log.push(`${String(log.length + 1).padStart(4, ' ')}  ${action} ${detail}`)
}

// ---------------------------------------------------------------------------
// The transcript -- Tier 11's raw material
// ---------------------------------------------------------------------------
// Tier 9 asks whether the world stayed consistent. Tier 11 asks a question no
// invariant can: would any of this make sense to a person?
//
// So every action also records what a human would have been shown -- the status
// and, when the server refused, the message it gave. An assertion can tell you
// a request answered 404; only a reader can tell you that the 404 said
// "Requested resource not found" when the user typed a tool name, or that a
// message meant for an administrator was shown to a member who cannot act on it.
//
// Written as JSONL so the reader needs no parser and no dependency, and so a
// partial file from a crashed run is still readable line by line.
const transcript = []
function witness(actor, action, target, res) {
  const body = res?.json
  transcript.push({
    step: transcript.length + 1,
    actor,
    action,
    target,
    status: res?.status ?? null,
    // The message a person would actually see. `error` is the envelope's field;
    // a success carries none, which is itself worth recording.
    message: typeof body?.error === 'string' ? body.error : null,
  })
}


// ---------------------------------------------------------------------------
// Repeated assertions are tallied, not re-recorded.
//
// The nemesis actions run dozens of times in a 200-action journey, and calling
// ok() each time produced 60 of the stage's 70 cases as copies of three
// assertions. A report that is mostly repeats is one nobody reads, and the
// three real results were buried in it.
//
// So each is counted and recorded once at the end, with the number of times it
// held. The count is the interesting part anyway: "refused 47 times" says more
// than one line saying "refused".
const tally = new Map()
function tallied(name, condition, message) {
  const t = tally.get(name) ?? { pass: 0, fail: 0, worst: '' }
  if (condition) t.pass += 1
  else {
    t.fail += 1
    t.worst = message
  }
  tally.set(name, t)
}

function flushTally() {
  for (const [name, t] of tally) {
    if (t.fail > 0) {
      record(name, 'fail', `${t.fail} of ${t.pass + t.fail} attempt(s) failed: ${t.worst}`)
    } else {
      record(name, 'ok', `held ${t.pass} time(s)`)
    }
  }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------
const ROLES = ['Newbie', 'Member', 'Staff', 'Admin']

let admin = null
let places = []
let doors = []

async function createUser() {
  const username = unique('journey')
  const res = await register(username, `${username}@e2e.invalid`)
  const id = res.json?.data?.user?.id ?? res.json?.data?.id
  if (res.status < 300 && id) {
    model.users.push({
      id, username, email: `${username}@e2e.invalid`,
      role: res.json?.data?.user?.role ?? 'Newbie', exists: true, deactivated: false,
    })
    note('createUser', `${username} -> ${id}`)
  } else {
    note('createUser', `${username} REFUSED ${res.status}`)
  }
}

function livingUsers() {
  return model.users.filter((u) => u.exists)
}

async function changeRole() {
  const u = pick(livingUsers())
  if (!u) return
  const role = pick(ROLES)
  const res = await PUT(`/api/users/${u.id}`, { token: admin.token, body: { role } })
  if (res.status < 300) {
    u.role = role
    note('changeRole', `${u.username} -> ${role}`)
    witness('an administrator', `changed ${u.username}'s role to ${role}`, u.username, res)
  } else {
    note('changeRole', `${u.username} -> ${role} REFUSED ${res.status}`)
    witness('an administrator', `tried to change ${u.username}'s role to ${role}`, u.username, res)
  }
}

async function setActive(active) {
  const u = pick(livingUsers())
  if (!u) return
  const res = await PUT(`/api/users/${u.id}`, { token: admin.token, body: { is_active: active } })
  if (res.status < 300) {
    u.deactivated = !active
    note(active ? 'reactivate' : 'deactivate', u.username)
  } else {
    note(active ? 'reactivate' : 'deactivate', `${u.username} REFUSED ${res.status}`)
  }
}

async function deleteUser() {
  const candidates = livingUsers().filter((u) => u.id !== admin.user?.id)
  const u = pick(candidates)
  if (!u) return
  const res = await DELETE(`/api/users/${u.id}`, { token: admin.token })
  if (res.status < 300) {
    u.exists = false
    note('deleteUser', u.username)
  } else {
    note('deleteUser', `${u.username} REFUSED ${res.status}`)
    witness('an administrator', `tried to remove ${u.username}`, u.username, res)
  }
}

async function addDoorRule() {
  const door = pick(doors)
  if (!door) return
  const kind = pick(['role', 'card', 'user'])
  const value =
    kind === 'role' ? pick(ROLES)
    : kind === 'user' ? (pick(livingUsers())?.id ?? '')
    : unique('card')
  if (!value) return

  const res = await POST(`/api/admin/doors/${door}/rules`, {
    token: admin.token, body: { kind, value },
  })
  const id = res.json?.data?.id
  // `accepted` is what the server said, not what the driver hoped. The
  // invariant this feeds exists because a rule was once accepted and then not
  // listed; recording the driver's intent instead of the server's answer would
  // make that undetectable.
  if (res.status < 300 && id) {
    model.doorRules.push({ id, doorId: door, kind, value, accepted: true, deleted: false })
    note('addDoorRule', `${kind}=${value} on ${door} -> ${id}`)
  } else {
    note('addDoorRule', `${kind}=${value} REFUSED ${res.status}`)
    witness('an administrator', `tried to add a ${kind} rule to a door`, door, res)
  }
}

async function deleteDoorRule() {
  const r = pick(model.doorRules.filter((x) => !x.deleted))
  if (!r) return
  const res = await DELETE(`/api/admin/doors/${r.doorId}/rules/${r.id}`, { token: admin.token })
  if (res.status < 300) {
    r.deleted = true
    note('deleteDoorRule', `${r.kind}=${r.value}`)
  } else {
    note('deleteDoorRule', `${r.id} REFUSED ${res.status}`)
  }
}

async function writeProfileConfig() {
  const current = await GET('/api/profiles/config', { token: admin.token })
  const body = current.json?.data
  if (!body) return
  const res = await PUT('/api/profiles/config', { token: admin.token, body })
  note('writeProfileConfig', `${res.status}`)
}

async function createInviteAndRegister() {
  const invite = await POST('/api/admin/devices/invite', {
    token: admin.token, body: { expires_in_hours: 1 },
  })
  const code = invite.json?.data?.device_code
  if (invite.status >= 300 || !code) {
    note('createInvite', `REFUSED ${invite.status}`)
    witness('an administrator', 'tried to create a device invite', 'a new device', invite)
    return
  }
  note('createInvite', code)
  // Redeemed through the shipping registration path, twice on purpose some of
  // the time: `invites-are-single-use` is the invariant that catches the second
  // one succeeding, and it cannot catch it if the driver never tries.
  const attempts = chance(0.3) ? 2 : 1
  for (let i = 0; i < attempts; i += 1) {
    const res = await POST('/api/devices/register', {
      body: { device_code: code, name: unique('dev'), platform: 'linux' },
    })
    const id = res.json?.data?.id ?? res.json?.data?.device?.id
    if (res.status < 300 && id) {
      model.devices.push({ id, inviteCode: code, registered: true })
      note('registerDevice', `${code} -> ${id}`)
    } else {
      note('registerDevice', `${code} attempt ${i + 1} REFUSED ${res.status}`)
    }
  }
}

// --- nemesis ---------------------------------------------------------------
// In the same weighted pool as the ordinary actions rather than a separate
// phase. A world that only ever received well-formed requests is not the world
// the application runs in, and a nemesis that runs in its own block is easy to
// satisfy by making the block short.
async function nemesisWrongCredential() {
  const u = pick(livingUsers())
  if (!u) return
  const res = await PUT(`/api/users/${u.id}`, { body: { role: 'Admin' } })
  note('nemesis/no-token', `${u.username} ${res.status}`)
  witness('somebody not signed in', `tried to change ${u.username}'s role`, u.username, res)
  // Asserted, because a missing credential granting a role change is the one
  // outcome here that is never acceptable.
  tallied('journeys/nemesis/unauthenticated-role-change-refused', res.status === 401,
    `PUT /api/users/{id} with no token answered ${res.status}`)
}

async function nemesisDeletedUserAction() {
  const dead = pick(model.users.filter((u) => !u.exists))
  if (!dead) return
  const res = await PUT(`/api/users/${dead.id}`, { token: admin.token, body: { role: 'Member' } })
  note('nemesis/deleted-user', `${dead.username} ${res.status}`)
  witness('an administrator', `acted on ${dead.username}, who had been removed`, dead.username, res)
  tallied('journeys/nemesis/acting-on-a-deleted-user-is-not-a-5xx', res.status < 500,
    `answered ${res.status} for a user that was deleted`)
}

async function nemesisMalformedBody() {
  const u = pick(livingUsers())
  if (!u) return
  const res = await PUT(`/api/users/${u.id}`, { token: admin.token, body: { role: 'Sovereign' } })
  note('nemesis/bad-role', 'unknown role')
  witness('an administrator', 'set a role the system does not have', 'a member', res)
  tallied('journeys/nemesis/an-unknown-role-is-not-a-5xx', res.status < 500,
    `an unrecognised role answered ${res.status}`)
}

// ---------------------------------------------------------------------------
// Observation
// ---------------------------------------------------------------------------
async function observe() {
  // Paged, not fetched once. `GET /api/users` defaults to per_page=20 and
  // refuses anything over 100, so a single call returns the first page and the
  // roster invariant then reports every user past it as "created but absent" --
  // which is what the first run of this tier did, and it was the driver's fault
  // rather than a finding. An invariant fed a truncated observation is worse
  // than no invariant: it produces confident false reports.
  const roster = []
  for (let page = 1; ; page += 1) {
    const res = await GET(`/api/users?page=${page}&per_page=100`, { token: admin.token })
    const body = res.json?.data
    // `PaginatedResponse<T>` puts the rows in `items`. Guessing `users` here
    // produced an EMPTY roster, and an empty observation does not fail loudly:
    // roster-matches reported every model user as missing, while roles-match
    // and deactivations-held passed *vacuously* because they had nothing to
    // disagree with. That is the worst failure mode this tier has -- two
    // invariants reporting success for judging nothing.
    const batch = Array.isArray(body) ? body : (body?.items ?? body?.users ?? [])
    roster.push(...batch)
    if (batch.length < 100) break
    if (page > 50) break // 5000 users; the driver never makes that many
  }

  // The observation has to be real. If the roster comes back empty while the
  // driver believes it created users, the shape changed and every invariant
  // fed from it is judging nothing -- which is exactly how this read as a
  // finding on the first two runs when it was a parsing bug.
  tallied('journeys/roster-observation-is-not-empty',
    roster.length > 0 || model.users.filter((u) => u.exists).length === 0,
    `the driver believes ${model.users.filter((u) => u.exists).length} users ` +
    'exist and GET /api/users returned none; the response shape is not what ' +
    'this driver parses, so the roster invariants are judging an empty world')
  const versions = (await GET('/api/profiles/config/versions', { token: admin.token })).json?.data ?? []
  const devices = (await GET('/api/admin/devices', { token: admin.token })).json?.data ?? []

  const rules = []
  for (const door of doors) {
    const res = await GET(`/api/admin/doors/${door}/rules`, { token: admin.token })
    for (const r of res.json?.data ?? []) rules.push(r)
  }

  return {
    'roster-matches': roster,
    'roles-match': roster,
    'accepted-rules-are-present': rules,
    'versions-are-contiguous': Array.isArray(versions) ? versions : [],
    // No `inviteCode` is attached, and that is not an oversight here.
    //
    // Nothing in the schema links a device back to the invite that produced it:
    // `space_device_auth_requests` carries the `device_code`, and `space_devices`
    // has no invite or auth-request column at all. So the per-invite half of
    // `invites-are-single-use` -- "invite X produced 2 devices" -- cannot be
    // evaluated from the API, and inventing a field here would make it look
    // evaluated while it silently counted nothing.
    //
    // What still works is the invariant's second half: the driver knows how many
    // devices it registered, and a server listing fewer has lost one. That is
    // asserted; the stronger half is recorded as unreachable below.
    'invites-are-single-use': Array.isArray(devices) ? devices : [],
    'deactivations-held': roster,
  }
}

/**
 * The model as a given invariant should see it.
 *
 * One adjustment, and it pins a defect rather than hiding one.
 * `DatabaseManager::list_users` filters `is_active.eq(true)`, so a deactivated
 * user is absent from `GET /api/users` entirely -- not listed as inactive,
 * absent. `roster-matches` would therefore report every deactivated user as
 * "created but absent from the roster" forever, which is a true statement about
 * a defect and a useless thing to fail on every run.
 *
 * So deactivated users are withheld from the roster comparison, and the fact
 * that they are absent is asserted separately below -- which is what makes this
 * a pin rather than a weakening. The day the roster includes them, that
 * assertion fails and this adjustment comes out.
 */
function modelFor(name) {
  if (name !== 'roster-matches') return model
  return { ...model, users: model.users.filter((u) => !u.deactivated) }
}

/**
 * The observation as a given invariant should see it.
 *
 * `roster-matches` compares in both directions: users created and absent, and
 * users present and never created. The second direction assumes the driver is
 * the only actor, and it is not -- `journeys` runs after `contract`, `fuzz` and
 * `concurrency` against the same stack, so the roster legitimately contains
 * accounts this driver never made.
 *
 * `invariants.mjs` already reasons this way about the accumulated world:
 * `deactivationsHeld` is one-directional on purpose, because "a user the driver
 * never touched may have been changed by another actor". `rosterMatches` was
 * not, and in a shared world its second direction reports other stages' users
 * as a defect.
 *
 * So the roster is scoped to ids this driver knows about. What survives:
 * "created and absent" still fires, and so does the duplicate check, because
 * filtering keeps both copies of a duplicated id -- and the duplicate is the
 * half that catches the shape this invariant was written for, a role change
 * that inserted rather than updated.
 *
 * WHAT THIS NO LONGER PROVES: that the roster contains nothing unaccounted for.
 * In a world with several actors that is not a property any one of them can
 * assert, and claiming it would mean the tier only worked when run alone.
 */
function observationFor(name, observations) {
  const raw = observations[name] ?? []
  if (name !== 'roster-matches') return raw
  const known = new Set(model.users.map((u) => u.id))
  return raw.filter((u) => known.has(u.id))
}

function checkAll(observations) {
  const violations = []
  for (const { name, fn } of INVARIANTS) {
    const why = fn(modelFor(name), observationFor(name, observations))
    if (why) violations.push({ name, why })
  }
  return violations
}

// ---------------------------------------------------------------------------
main(async () => {
  console.log(`journey seed: ${SEED}`)
  console.log(`iterations: ${ITERATIONS}, checking every ${CHECK_EVERY}, tag: ${RUN_TAG}`)
  console.log(`cluster encoding: ${ENCODING}`)

  admin = await adminAccount('journey-admin')
  ok('journeys/admin-available', Boolean(admin?.token), 'no admin token; nothing can run')
  if (!admin?.token) return

  // The place vocabulary is read from the server, not guessed. `place_type` is
  // validated against configured values, so a hardcoded guess makes every door
  // creation fail and the tier silently exercises nothing.
  const cfg = await GET('/api/admin/places/config', { token: admin.token })
  const vocabulary = cfg.json?.data?.types ?? cfg.json?.data?.place_types ?? cfg.json?.data ?? []
  const placeType = Array.isArray(vocabulary) && vocabulary.length
    ? (typeof vocabulary[0] === 'string' ? vocabulary[0] : vocabulary[0]?.name)
    : null
  ok('journeys/place-vocabulary-readable', Boolean(placeType),
    'GET /api/admin/places/config gave no usable place type; doors cannot be created')

  if (placeType) {
    for (const which of ['from', 'to']) {
      const res = await POST('/api/admin/places', {
        token: admin.token, body: { name: unique(`place_${which}`), place_type: placeType },
      })
      const id = res.json?.data?.id
      if (id) places.push(id)
    }
  }
  if (places.length === 2) {
    const res = await POST('/api/admin/doors', {
      token: admin.token,
      body: { name: unique('door'), place_id_from: places[0], place_id_to: places[1] },
    })
    const id = res.json?.data?.id
    if (id) doors.push(id)
  }
  // Recorded honestly either way. If the places module is switched off, doors
  // cannot exist and `accepted-rules-are-present` judges an empty world -- that
  // is a real reduction in what this tier proves, and it says so rather than
  // reporting a pass.
  if (doors.length > 0) {
    record('journeys/a-door-exists', 'ok', `door ${doors[0]}`)
  } else {
    record('journeys/door-rule-invariant-not-exercised', 'skip',
      'no door could be created -- the places module is disabled or place ' +
      'creation was refused, so accepted-rules-are-present judges an empty ' +
      'world. Check [place] enabled in the stack config.')
  }

  // Device actions need an invite, and an invite code is eight emoji. On a
  // cluster that cannot store them the invite cannot be created at all, so
  // `invites-are-single-use` has nothing to judge. Recorded as a skip naming
  // the reason rather than left to look like a pass.
  // The per-invite half of invites-are-single-use cannot be reached from the
  // API on any cluster. Recorded once, unconditionally, so it is not mistaken
  // for a consequence of the encoding skip below.
  record('journeys/invite-to-device-link-is-not-observable', 'skip',
    'no column links a device to the invite that created it -- ' +
    'space_devices has no invite or auth-request id -- so "one invite produced ' +
    'two devices" cannot be checked through the API. The invariant\'s count ' +
    'half still runs. Recorded in TESTING.md as a gap in the audit trail.')

  const deviceActionsPossible = ENCODING === 'UTF8'
  if (!deviceActionsPossible) {
    record('journeys/device-actions-not-run', 'skip',
      `a device invite code is eight emoji and this cluster (${ENCODING}) can ` +
      'store none of them, so invites-are-single-use judges an empty world here. ' +
      'Exercise it with `reaper test --profile utf8`, or CSS_E2E_DB_ENCODING=UTF8 ' +
      'when invoking e2e/run.sh directly -- setting that variable on the ' +
      'workstation does not reach a reaper run.')
  }

  const pool = [
    [createUser, 5],
    [changeRole, 4],
    [() => setActive(false), 3],
    [() => setActive(true), 2],
    [deleteUser, 1],
    [addDoorRule, 4],
    [deleteDoorRule, 2],
    [writeProfileConfig, 2],
    [nemesisWrongCredential, 2],
    [nemesisDeletedUserAction, 2],
    [nemesisMalformedBody, 2],
    ...(deviceActionsPossible ? [[createInviteAndRegister, 3]] : []),
  ]
  const weighted = pool.flatMap(([fn, w]) => Array.from({ length: w }, () => fn))

  let checks = 0
  let firstViolation = null

  for (let i = 1; i <= ITERATIONS && !firstViolation; i += 1) {
    await pick(weighted)()

    if (i % CHECK_EVERY === 0 || i === ITERATIONS) {
      checks += 1
      const violations = checkAll(await observe())
      if (violations.length) {
        firstViolation = { at: i, violations }
      }
    }
  }

  // The tier has to have actually done something. A driver whose actions were
  // all refused reports no violations and looks identical to a healthy run.
  ok('journeys/the-world-accumulated', model.users.length >= 3,
    `only ${model.users.length} user(s) were created in ${ITERATIONS} actions; ` +
    'the driver is not exercising the application')
  ok('journeys/invariants-were-checked', checks > 0,
    'no invariant check ran')

  // The transcript, written whatever happened -- a crashed run's partial file is
  // still readable, which is the reason for JSONL over a single JSON document.
  try {
    const { writeFileSync } = await import('node:fs')
    const dir = process.env.CSS_STACK_DIR ?? '.'
    writeFileSync(
      `${dir}/journey-transcript.jsonl`,
      transcript.map((t) => JSON.stringify(t)).join('\n') + '\n'
    )
  } catch (e) {
    record('journeys/transcript-written', 'fail', `could not write the transcript: ${e}`)
  }

  flushTally()

  // The pin, and the honest note about what it costs this tier.
  const deactivated = model.users.filter((u) => u.exists && u.deactivated)
  if (deactivated.length) {
    const roster = (await observe())['roster-matches']
    const visible = deactivated.filter((u) => roster.some((r) => r.id === u.id))
    ok('findings/deactivated-users-vanish-from-the-admin-roster', visible.length === 0,
      `PINNED FINDING, not a passing behaviour: DatabaseManager::list_users ` +
      `filters is_active = true, so deactivating a member removes them from ` +
      `GET /api/users altogether -- not shown as inactive, absent -- and there ` +
      `is no parameter to include them. An administrator who deactivates ` +
      `somebody cannot then find them to undo it. If this assertion fails, the ` +
      `roster now lists them and modelFor() should stop withholding them. ` +
      `(${visible.length} of ${deactivated.length} were visible)`)

    record('journeys/deactivations-held-is-vacuous-here', 'skip',
      'that invariant asks whether anything the driver deactivated is reported ' +
      'active, and a deactivated user is not reported at all -- so it judges an ' +
      'empty set. It cannot mean anything until the roster lists them.')
  }

  if (!firstViolation) {
    record('journeys/no-invariant-violated', 'ok',
      `${ITERATIONS} actions, ${checks} check(s), ${model.users.length} users, ` +
      `${model.doorRules.length} door rules, ${model.devices.length} devices`)
    return
  }

  for (const v of firstViolation.violations) {
    record(`journeys/${v.name}`, 'fail', `after ${firstViolation.at} actions: ${v.why}`)
  }

  // The action log, verbatim. This is what a finding is actually made of: the
  // seed reproduces the sequence of choices but not the ids, so the log is the
  // only account that does not depend on a replay behaving identically.
  console.log('\n--- actions leading to the violation ---')
  for (const line of log) console.log(line)
  console.log(`\nreplay a similar path with CSS_JOURNEY_SEED=${SEED}`)
  if (!CONTROL) {
    console.log(
      'REAPER_CONTROL is unset, so no shrink was attempted: without a reset ' +
      'between attempts a failure to reproduce says nothing.'
    )
  }
})
