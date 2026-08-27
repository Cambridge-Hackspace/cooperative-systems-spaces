// Tier 7: seeded fuzz against the live stack.
//
// Three oracles, and the value of this tier is entirely in how weak they are.
// A strong oracle needs a model of what each endpoint should return, which is
// the contract tier's job and costs a line per case. These three need no model
// at all, apply to all 164 endpoints at once, and are still violated by real
// defects:
//
//   1. NO 5xx.  A 4xx is the server saying no. A 5xx is the server saying it
//      broke. Nothing a client sends should be able to produce the second, and
//      every input that does is either a missing validation or an error
//      conversion that lost information on the way out.
//   2. WELL-FORMED ENVELOPE.  Anything answering with a JSON content type must
//      produce JSON, and anything using the ApiResponse envelope must fill in
//      `success`. A handler returning a bare value on one path and an envelope
//      on another breaks every client that trusted the shape.
//   3. STILL ALIVE.  /status answers 200 after every batch. This is what
//      catches a panic that takes a worker down, a pool leaked to exhaustion,
//      or a deadlock -- none of which appear in the response to the request
//      that caused them.
//
// WHAT THIS DOES NOT PROVE. Nothing about whether an accepted request did the
// right thing. Every oracle here is satisfied by a server that returns
// `{"success":true}` to everything and writes nothing to the database. That is
// the contract and journey tiers' territory; this tier's job is to find the
// inputs nobody thought about, cheaply, across the whole surface at once.
//
// REPLAY HONESTY. The seed reproduces the sequence of *decisions*, not the run:
// entity ids differ between runs, so a replayed seed follows a similar path
// rather than an identical one. Stated here rather than implied, because a seed
// advertised as a reproduction and delivering a near-miss wastes more time than
// no seed at all. What it does reproduce reliably is which endpoint, which
// corpus entry and which credential each iteration chose.

import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { GET, account, adminAccount, record, assertEq, ok, main, RUN_TAG } from './lib.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const CORPUS = JSON.parse(readFileSync(join(HERE, '../corpus/hostile.json'), 'utf8'))
const INVENTORY = JSON.parse(readFileSync(join(HERE, '../corpus/endpoints.json'), 'utf8'))

const ITERATIONS = Number(process.env.CSS_FUZZ_ITERATIONS ?? 400)
const BATCH = Number(process.env.CSS_FUZZ_BATCH ?? 25)

// ---------------------------------------------------------------------------
// The generator
// ---------------------------------------------------------------------------
// mulberry32, twelve lines, because Math.random cannot be seeded and a fuzz run
// nobody can replay is a fuzz run whose findings nobody can act on.
function mulberry32(a) {
  return function () {
    a = (a + 0x6d2b79f5) | 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

const SEED = Number(process.env.CSS_FUZZ_SEED ?? Math.floor(Math.random() * 2 ** 31))
const rnd = mulberry32(SEED)
const pick = (xs) => xs[Math.floor(rnd() * xs.length)]
const chance = (p) => rnd() < p

// The tag is drawn OUTSIDE the seeded stream, on purpose. Names have to stay
// unique across runs against one cluster, and drawing them from the seeded
// stream would make a replay collide with the run it is replaying on every
// unique constraint -- turning a reproduction attempt into a wall of conflicts.
const TAG = `${RUN_TAG}_${process.hrtime.bigint().toString(36).slice(-6)}`

// ---------------------------------------------------------------------------
// Request construction
// ---------------------------------------------------------------------------
const ENCODING = process.env.CSS_DB_ENCODING ?? 'UTF8'

const ALL_STRINGS = CORPUS.strings.map((s) => s.v)
const ALL_SCALARS = CORPUS.scalars.map((s) => s.v)
const ALL_TIMES = CORPUS.timestamps.map((s) => s.v)

/**
 * The whole corpus, on every cluster. There is no longer a narrowing here.
 *
 * There was one, and this is the record of why it went. Text a cluster cannot
 * represent used to answer 500, so firing it at every route that writes text
 * reproduced one known defect over and over and buried anything new. The
 * entries were filtered out on non-UTF-8 clusters to keep the tier readable.
 *
 * That defect is fixed: `is_unrepresentable_text` classifies both Postgres
 * messages -- the NUL byte and the out-of-encoding character -- as 400, so
 * these inputs now exercise a correct rejection path instead of tripping the
 * no-5xx oracle. Nothing is buried, so nothing needs excluding, and the full
 * corpus runs everywhere.
 *
 * Worth stating plainly, because removing a narrowing usually deserves more
 * suspicion than adding one: this is not the exemption being widened until the
 * noise stops. The noise stopped because the thing making it was repaired, and
 * the tier now covers strictly more than it did.
 */
const STORABLE_STRINGS = ALL_STRINGS

/** A value from somewhere in the corpus, weighted towards text. */
function hostileValue() {
  const r = rnd()
  if (r < 0.55) return pick(STORABLE_STRINGS)
  if (r < 0.8) return pick(ALL_SCALARS)
  return pick(ALL_TIMES)
}

/**
 * Field names the API actually uses, so generated bodies are near-misses rather
 * than noise. A body of random keys is rejected by serde before any application
 * code runs, and a fuzzer that only ever produces those is measuring serde.
 */
const FIELDS = [
  'name', 'username', 'email', 'password', 'full_name', 'description', 'title',
  'value', 'kind', 'effect', 'role', 'status', 'card', 'card_id', 'tool_id',
  'door_id', 'user_id', 'device_id', 'schedule_id', 'profile', 'meta', 'config',
  'fields', 'intervals', 'starts_at', 'ends_at', 'created_at', 'enabled',
  'is_active', 'is_public', 'version', 'url', 'secret', 'events', 'theme',
]

function hostileBody() {
  const shape = rnd()
  // Sometimes not an object at all. Extractors reject this, and the question is
  // whether they reject it with a 400 or fall over.
  if (shape < 0.1) return hostileValue()
  if (shape < 0.15) return null

  const n = 1 + Math.floor(rnd() * 6)
  const body = {}
  for (let i = 0; i < n; i += 1) body[pick(FIELDS)] = hostileValue()
  // Occasionally deeply nested: serde's recursion depth and the request-size
  // limit are different failure modes and only one of them is bounded.
  if (chance(0.08)) {
    let deep = {}
    const root = deep
    for (let i = 0; i < 40; i += 1) {
      deep.next = {}
      deep = deep.next
    }
    body.profile = root
  }
  return body
}

function hostileQuery() {
  if (chance(0.5)) return ''
  const n = 1 + Math.floor(rnd() * 3)
  const params = new URLSearchParams()
  for (let i = 0; i < n; i += 1) {
    const v = hostileValue()
    params.set(pick(FIELDS), typeof v === 'object' && v !== null ? JSON.stringify(v) : String(v))
  }
  return `?${params.toString()}`
}

function fillPath(template) {
  return template.replace(/\{id\}/g, () => {
    const r = rnd()
    if (r < 0.5) return '00000000-0000-4000-8000-000000000001'
    if (r < 0.7) return crypto.randomUUID()
    const v = hostileValue()
    return encodeURIComponent(typeof v === 'object' && v !== null ? JSON.stringify(v) : String(v))
  })
}

// ---------------------------------------------------------------------------
// Findings this tier already reported, and why they are still here
// ---------------------------------------------------------------------------
// Each entry names one (method, template, status) and says why the run is
// expected to produce it. Nothing broader: an exemption on a *route* would
// cover the next real 500 on that route too, and an exemption on a *status*
// would switch the oracle off.
//
// Every one of them is also asserted to still occur, further down. An exemption
// for something that no longer happens is a claim about behaviour nobody is
// checking, and it is the reason a suppression list eventually stops describing
// anything.
const KNOWN = [
  {
    method: 'POST',
    template: '/api/admin/devices/invite',
    status: 500,
    only: (enc) => enc !== 'UTF8',
    why:
      'a device invite code is eight emoji, and this cluster cannot store one. ' +
      'The finding is pinned by the concurrency tier. TESTING.md, "Known defects".',
  },
  // The four registered-but-unimplemented routes. A 501 is honest rather than
  // broken -- but each is a route the frontend can call and that can never
  // succeed, so they are recorded rather than ignored.
  //
  // `checks/tests/unimplemented_endpoints.rs` pins the same list from the source
  // in milliseconds, which is the version that does not depend on the fuzzer
  // happening to reach the endpoint. This one exists so the oracle stays on for
  // every *other* 5xx on these routes.
  ...[
    ['POST', '/api/tools/{id}/training-types'],
    ['POST', '/api/tools/{id}/trainers'],
    ['POST', '/api/tools/user-training/{id}'],
    ['DELETE', '/api/tools/user-training/{id}'],
  ].map(([method, template]) => ({
    method,
    template,
    status: 501,
    only: () => true,
    why:
      'registered and unimplemented; the handler returns 501. See ' +
      'checks/tests/unimplemented_endpoints.rs and TESTING.md, "Known defects".',
  })),
]

function knownFor(f) {
  return KNOWN.find(
    (k) =>
      k.method === f.method &&
      k.template === f.template &&
      k.status === f.status &&
      k.only(ENCODING),
  )
}

// ---------------------------------------------------------------------------
// Findings
// ---------------------------------------------------------------------------
// Collapsed by (oracle, method, template, status). A fuzzer that reports every
// instance of one defect buries the other four, and the count is what says
// whether something is an edge case or the common path.
const findings = new Map()
function finding(oracle, req, detail) {
  const key = `${oracle} ${req.method} ${req.template} ${detail.status ?? ''}`
  const existing = findings.get(key)
  if (existing) {
    existing.count += 1
    return
  }
  findings.set(key, {
    oracle,
    method: req.method,
    template: req.template,
    count: 1,
    seed: SEED,
    iteration: req.iteration,
    ...detail,
    // The whole request, so a finding can be reproduced by hand without
    // replaying the seed at all. This is the part a seed cannot give you.
    repro: {
      method: req.method,
      path: req.path,
      credential: req.credName,
      body: req.body === undefined ? undefined : JSON.stringify(req.body).slice(0, 2000),
    },
  })
}

main(async () => {
  console.log(`fuzz seed: ${SEED}`)
  console.log(`iterations: ${ITERATIONS}, endpoints: ${INVENTORY.count}, tag: ${TAG}`)
  console.log(`cluster encoding: ${ENCODING}`)
  // Asserted rather than assumed. This tier ran a reduced corpus on non-UTF-8
  // clusters for as long as unrepresentable text answered 500; now that it
  // answers 400, the whole corpus runs everywhere. Stating it as a case means a
  // future narrowing has to remove this line, which is harder to do by accident
  // than adding a filter.
  ok('fuzz/whole-corpus-on-every-cluster', STORABLE_STRINGS.length === ALL_STRINGS.length,
    `${ALL_STRINGS.length - STORABLE_STRINGS.length} of ${ALL_STRINGS.length} ` +
    `corpus strings were withheld on a ${ENCODING} cluster`)

  // The inventory has to be complete or this tier is a sampling exercise
  // wearing a fuzzer's name. e2e/lint.sh keeps endpoints.json in step with the
  // route table; this asserts the file that arrived is the one generated.
  ok('fuzz/inventory-is-complete', INVENTORY.endpoints.length === INVENTORY.count,
    `count says ${INVENTORY.count}, array has ${INVENTORY.endpoints.length}`)
  ok('fuzz/inventory-is-large', INVENTORY.count > 150, `only ${INVENTORY.count} endpoints`)
  ok('fuzz/corpus-is-loaded',
    CORPUS.strings.length + CORPUS.scalars.length + CORPUS.timestamps.length > 40,
    'the corpus is smaller than expected; the run below would be shallow')

  // Credentials. Real ones, made through the shipping path, because most of the
  // interesting surface is behind a guard and a fuzzer holding no credential
  // spends its whole run confirming that 401 is not 500.
  const admin = await adminAccount(`fuzz_admin_${TAG}`)
  const member = await account(`fuzz_member_${TAG}`)
  const CREDS = [
    ['admin', admin.token],
    ['member', member.token],
    ['none', undefined],
    ['garbage', 'not-a-token'],
  ]

  let requests = 0
  let alive = true
  // Every status each endpoint answered, so a KNOWN entry that did not fire can
  // be told apart from one whose endpoint was never really reached.
  //
  // "Attempted" is not enough on its own. A fuzzer sends whatever credential it
  // drew, so an endpoint behind a staff guard hit with a member token answers
  // 403 and the handler never runs -- which is the fuzzer's input being
  // refused, not news about the endpoint.
  const observed = new Map()

  for (let i = 0; i < ITERATIONS && alive; i += 1) {
    const ep = pick(INVENTORY.endpoints)
    const [credName, token] = pick(CREDS)
    const path = fillPath(ep.template) + hostileQuery()
    const sendsBody = ['POST', 'PUT', 'PATCH'].includes(ep.method)
    const body = sendsBody ? hostileBody() : undefined

    const req = { method: ep.method, template: ep.template, path, credName, body, iteration: i }


    let res
    try {
      res = await rawRequest(req, token)
    } catch (e) {
      // A transport failure is the still-alive oracle firing early: the server
      // stopped answering mid-run.
      finding('transport', req, { status: null, message: String(e?.message ?? e) })
      alive = false
      break
    }
    requests += 1
    const key = `${ep.method} ${ep.template}`
    if (!observed.has(key)) observed.set(key, new Set())
    observed.get(key).add(res.status)

    // --- oracle 1: no 5xx --------------------------------------------------
    if (res.status >= 500) {
      finding('no-5xx', req, { status: res.status, message: res.body.slice(0, 400) })
    }

    // --- oracle 2: well-formed envelope ------------------------------------
    if ((res.contentType ?? '').includes('application/json')) {
      let parsed = null
      try {
        parsed = JSON.parse(res.body)
      } catch {
        finding('malformed-json', req, { status: res.status, message: res.body.slice(0, 200) })
      }
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
        const looksLikeEnvelope =
          'data' in parsed || 'error' in parsed || 'message' in parsed || 'success' in parsed
        if (looksLikeEnvelope && typeof parsed.success !== 'boolean') {
          finding('envelope-without-success', req, {
            status: res.status,
            message: `keys: ${Object.keys(parsed).join(',')}`,
          })
        }
      }
    }

    // --- oracle 3: still alive ---------------------------------------------
    if ((i + 1) % BATCH === 0) {
      const health = await GET('/status')
      if (health.status !== 200) {
        finding('not-alive', req, {
          status: health.status,
          message: `/status answered ${health.status} after ${i + 1} iterations`,
        })
        alive = false
      }
    }
  }

  // -------------------------------------------------------------------------
  // Report
  // -------------------------------------------------------------------------
  assertEq('fuzz/reached-the-iteration-count', ITERATIONS, requests)
  ok('fuzz/server-survived', alive, 'the server stopped answering before the run finished')

  const list = [...findings.values()].sort((a, b) => b.count - a.count)
  const dir = process.env.CSS_STACK_DIR
  if (dir) {
    writeFileSync(
      join(dir, 'fuzz-findings.json'),
      JSON.stringify({ seed: SEED, iterations: ITERATIONS, requests, findings: list }, null, 2) +
        '\n',
    )
  }

  const seenKnown = new Set()
  let novel = 0

  for (const f of list) {
    const known = knownFor(f)
    const name = `fuzz/${f.oracle}/${f.method} ${f.template}`
    const detail =
      `${f.count}x status=${f.status} seed=${SEED} cred=${f.repro.credential} ` +
      `path=${f.repro.path} -- ${String(f.message).replace(/\s+/g, ' ').slice(0, 200)}`

    if (known) {
      seenKnown.add(`${known.method} ${known.template} ${known.status}`)
      record(`findings/${name}`, 'ok', `${known.why} (${detail})`)
    } else {
      novel += 1
      record(name, 'fail', detail)
    }
  }

  if (novel === 0) {
    record('fuzz/no-new-oracle-violations', 'ok',
      `${requests} requests, seed ${SEED}, ${seenKnown.size} known finding(s) reproduced`)
  }

  // The other direction. A KNOWN entry that did not fire is either fixed --
  // delete it -- or the run stopped reaching that endpoint, which means the
  // fuzzer's coverage narrowed without anybody noticing.
  for (const k of KNOWN) {
    if (!k.only(ENCODING)) continue
    const key = `${k.method} ${k.template} ${k.status}`
    if (seenKnown.has(key)) continue

    // Two very different situations, and reporting them the same way is how a
    // check that should mean something becomes noise:
    //
    //   * the run never tried this endpoint -- a seeded fuzzer picks uniformly
    //     from 164 endpoints, so at 400 iterations any given one is missed
    //     about a tenth of the time. That is not news.
    //   * the run DID try it and got something else -- either the defect was
    //     fixed, or it changed shape. That is news either way.
    const statuses = observed.get(`${k.method} ${k.template}`) ?? new Set()
    const succeeded = [...statuses].some((s) => s >= 200 && s < 300)
    const onlyRefused = statuses.size > 0 && [...statuses].every((s) => s >= 400 && s < 500)

    if (succeeded) {
      record(`fuzz/known-finding-fixed: ${key}`, 'fail',
        `this endpoint answered ${[...statuses].join(',')} -- it works now. Delete ` +
        'the KNOWN entry, here and in checks/tests/unimplemented_endpoints.rs if ' +
        `it is named there. (seed ${SEED})`)
    } else if (onlyRefused || statuses.size === 0) {
      // Either never tried, or tried with a credential or a body the endpoint
      // refused before the handler ran. Neither says anything about the
      // finding, and reporting them as failures would make this check fire on
      // roughly half of all runs -- which is how a check gets deleted.
      record(`fuzz/known-finding-not-reached: ${key}`, 'skip',
        statuses.size === 0
          ? `this run never tried ${k.method} ${k.template}. At ${ITERATIONS} ` +
            'iterations across 164 endpoints that is ordinary; raise ' +
            'CSS_FUZZ_ITERATIONS to make the coverage claim stronger.'
          : `every attempt was refused before the handler ran (${[...statuses].join(',')}), ` +
            'so this run says nothing about the finding.')
    } else {
      record(`fuzz/known-finding-changed: ${key}`, 'fail',
        `this endpoint answered ${[...statuses].join(',')} rather than ${k.status}. ` +
        `It did not fail the way it is recorded as failing. (seed ${SEED})`)
    }
  }

  // The seed, restated where somebody reading only the summary will see it.
  record('fuzz/seed', 'ok',
    `${SEED} -- replay with CSS_FUZZ_SEED=${SEED}; it reproduces the sequence of ` +
      'choices rather than the entity ids, so a replay follows a similar path ' +
      'and not an identical one')
})

async function rawRequest(req, token) {
  const base = process.env.CSS_BASE_URL ?? 'http://127.0.0.1:4399'
  const headers = {}
  if (token) headers.Authorization = `Bearer ${token}`
  if (req.body !== undefined) headers['Content-Type'] = 'application/json'
  const res = await fetch(`${base}${req.path}`, {
    method: req.method,
    headers,
    body: req.body === undefined ? undefined : JSON.stringify(req.body),
    redirect: 'manual',
  })
  return {
    status: res.status,
    contentType: res.headers.get('content-type'),
    body: await res.text(),
  }
}
