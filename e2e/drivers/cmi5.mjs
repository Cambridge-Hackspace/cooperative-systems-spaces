// The cmi5 end-to-end journey: import a package, bind its AU to a training step,
// launch it as a learner, drive the embedded LRS, and prove that a valid pass
// grants physical tool access — while a forged statement does not.
//
// This is the headline test the offline tiers cannot show: accepting a
// credential and flipping an access decision both require a live database. The
// pure statement-validation logic is unit-tested in the `cmi5` crate; this
// proves the wiring.

import {
  BASE,
  GET,
  POST,
  PUT,
  account,
  adminAccount,
  assertEq,
  main,
  ok,
} from './lib.mjs'
import { buildPackageZip, minimalPackage } from './cmi5-fixture.mjs'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const CMI5_CAT = { id: 'https://w3id.org/xapi/cmi5/context/categories/cmi5' }
const MOVEON_CAT = { id: 'https://w3id.org/xapi/cmi5/context/categories/moveon' }
const V = {
  initialized: 'http://adlnet.gov/expapi/verbs/initialized',
  passed: 'http://adlnet.gov/expapi/verbs/passed',
}

async function putStatement(sessionToken, stmt) {
  return fetch(new URL(`/api/cmi5/lrs/statements?statementId=${stmt.id}`, BASE), {
    method: 'PUT',
    headers: {
      Authorization: `Bearer ${sessionToken}`,
      'Content-Type': 'application/json',
      'X-Experience-API-Version': '1.0.3',
    },
    body: JSON.stringify(stmt),
  })
}

main(async () => {
  const tag = `cmi5_${Date.now()}_${Math.floor(Math.random() * 1e6)}`
  const admin = await adminAccount('cmi5admin')

  // A learner, promoted to Member so they may launch. A fresh account is a
  // Newbie, which the launch route must refuse (checked at the end).
  const member = await account('cmi5member')
  const promote = await PUT(`/api/users/${member.user.id}`, {
    token: admin.token,
    body: { role: 'Member' },
  })
  ok('cmi5/member-promoted', promote.status === 200, `promote -> ${promote.status}`)

  // 1. A tool and a training step for the AU to gate.
  const toolRes = await POST('/api/tools', {
    token: admin.token,
    body: {
      name: `cmi5 e2e tool ${tag}`,
      category: 'safety',
      requires_training: true,
    },
  })
  const toolId = toolRes.json?.data?.id
  ok('cmi5/tool-created', !!toolId, `POST /api/tools -> ${toolRes.status}`)

  const stepRes = await POST('/api/training/steps', {
    token: admin.token,
    body: {
      tool_id: toolId,
      step_number: 1,
      step_name: 'cmi5 safety basics',
      requires_assessment: false,
    },
  })
  const stepId = stepRes.json?.data?.id
  ok('cmi5/step-created', !!stepId, `POST /api/training/steps -> ${stepRes.status}`)

  // 2. Import a package (the only multipart upload in the app).
  const { zip } = minimalPackage(tag)
  const form = new FormData()
  form.append('file', new Blob([zip], { type: 'application/zip' }), 'course.zip')
  const impRes = await fetch(new URL('/api/cmi5/courses', BASE), {
    method: 'POST',
    headers: { Authorization: `Bearer ${admin.token}` },
    body: form,
  })
  const impJson = await impRes.json().catch(() => null)
  const course = impJson?.data?.course
  const aus = impJson?.data?.aus ?? []
  ok(
    'cmi5/import',
    impRes.status === 200 && !!course && aus.length === 1,
    `POST /api/cmi5/courses -> ${impRes.status}, ${aus.length} AU(s)`,
  )
  const auId = aus[0]?.id

  // 3. Bind the AU to the training step (admin gates access here).
  const assignRes = await POST(
    `/api/cmi5/courses/${course.id}/aus/${auId}/assign`,
    { token: admin.token, body: { training_step_id: stepId } },
  )
  ok('cmi5/assign', assignRes.status === 200, `assign -> ${assignRes.status}`)

  // Baseline: the learner cannot use the tool yet.
  const before = await GET(`/api/training/access/${toolId}`, { token: member.token })
  assertEq('cmi5/no-access-before', false, before.json?.data, 'access before any pass')

  // 4. Launch (member). The launch URL carries the mandated params.
  const launchRes = await POST(`/api/cmi5/aus/${auId}/launch`, { token: member.token })
  const launchUrl = launchRes.json?.data?.launch_url
  ok('cmi5/launch', launchRes.status === 200 && !!launchUrl, `launch -> ${launchRes.status}`)

  const u = new URL(launchUrl)
  const fetchUrl = u.searchParams.get('fetch')
  const actor = JSON.parse(u.searchParams.get('actor'))
  const registration = u.searchParams.get('registration')
  const activityId = u.searchParams.get('activityId')
  ok(
    'cmi5/launch-params',
    !!fetchUrl && !!actor?.account?.name && !!registration && !!activityId,
    'launch URL must carry fetch/actor/registration/activityId',
  )

  // The content itself is served (rendered) at the launch URL — the embedded
  // player and a new-tab launch both just load this. Proves the static content
  // store is wired, not only the API.
  const contentRes = await fetch(new URL(launchUrl))
  const contentBody = await contentRes.text()
  assertEq('cmi5/content-served', 200, contentRes.status, 'the launched content is served')
  ok(
    'cmi5/content-renders',
    contentBody.includes('hello'),
    'the AU content body is returned from the content store',
  )

  // 5. Fetch: trade the one-time token for a session credential.
  const fRes = await fetch(fetchUrl, { method: 'POST' })
  const fJson = await fRes.json().catch(() => null)
  const sessionToken = fJson?.['auth-token']
  ok('cmi5/fetch', fRes.status === 200 && !!sessionToken, `fetch -> ${fRes.status}`)

  // The fetch token is single-use: a second exchange must fail.
  const f2 = await fetch(fetchUrl, { method: 'POST' })
  assertEq('cmi5/fetch-single-use', 401, f2.status, 'second fetch must be refused')

  // The LRS refuses a request with no session credential.
  const noCred = await fetch(
    new URL(`/api/cmi5/lrs/statements?statementId=${crypto.randomUUID()}`, BASE),
    { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: '{}' },
  )
  assertEq('cmi5/lrs-no-cred-401', 401, noCred.status, 'LRS without a credential')

  const stmt = (verb, extra = {}) => ({
    id: crypto.randomUUID(),
    actor,
    verb: { id: verb },
    object: { objectType: 'Activity', id: activityId },
    context: {
      registration,
      contextActivities: { category: [CMI5_CAT, ...(extra.moveon ? [MOVEON_CAT] : [])] },
    },
    ...(extra.result ? { result: extra.result } : {}),
  })

  // initialized (required first).
  const initRes = await putStatement(sessionToken, stmt(V.initialized))
  assertEq('cmi5/initialized', 204, initRes.status, 'initialized accepted')

  // Hostile: a pass forged for a different actor must be refused (403) and grant
  // nothing.
  const forged = stmt(V.passed, { moveon: true, result: { success: true, score: { scaled: 0.95 } } })
  forged.actor = {
    objectType: 'Agent',
    account: { homePage: actor.account.homePage, name: 'not-the-learner' },
  }
  const forgedRes = await putStatement(sessionToken, forged)
  assertEq('cmi5/hostile-wrong-actor', 403, forgedRes.status, 'forged actor refused')

  // Hostile: a pass about a different activity must be refused (403).
  const wrongAct = stmt(V.passed, { moveon: true, result: { success: true, score: { scaled: 0.95 } } })
  wrongAct.object = { objectType: 'Activity', id: 'https://e2e.invalid/cmi5/au/OTHER' }
  const wrongActRes = await putStatement(sessionToken, wrongAct)
  assertEq('cmi5/hostile-wrong-activity', 403, wrongActRes.status, 'wrong activity refused')

  // Hostile: a pass below masteryScore (0.8) must be refused (400) and grant
  // nothing.
  const belowMastery = stmt(V.passed, { moveon: true, result: { success: true, score: { scaled: 0.5 } } })
  const belowRes = await putStatement(sessionToken, belowMastery)
  assertEq('cmi5/hostile-below-mastery', 400, belowRes.status, 'below-mastery pass refused')

  // Still no access after the hostile attempts.
  const mid = await GET(`/api/training/access/${toolId}`, { token: member.token })
  assertEq('cmi5/no-access-after-hostile', false, mid.json?.data, 'no grant from forged statements')

  // 6. A valid pass at/above masteryScore satisfies the AU.
  const passed = stmt(V.passed, { moveon: true, result: { success: true, score: { scaled: 0.9 } } })
  const passedRes = await putStatement(sessionToken, passed)
  assertEq('cmi5/passed', 204, passedRes.status, 'valid pass accepted')

  // The headline assertion: the pass flipped physical tool access.
  const after = await GET(`/api/training/access/${toolId}`, { token: member.token })
  assertEq('cmi5/access-granted', true, after.json?.data, 'a cmi5 pass must grant tool access')

  // Replaying the same statement id is an idempotent no-op, not a second grant
  // or an error.
  const replay = await putStatement(sessionToken, passed)
  assertEq('cmi5/replay-idempotent', 204, replay.status, 'replayed statement is a no-op')

  // The State API serves the LMS.LaunchData written at launch.
  const ld = await fetch(
    new URL('/api/cmi5/lrs/activities/state?stateId=LMS.LaunchData', BASE),
    { headers: { Authorization: `Bearer ${sessionToken}` } },
  )
  assertEq('cmi5/launchdata-readable', 200, ld.status, 'LMS.LaunchData readable')

  // Export the course, re-import the result, and confirm the AU survives the
  // round trip: import(export(x)) is x at the tree level.
  const exp = await fetch(new URL(`/api/cmi5/courses/${course.id}/export`, BASE), {
    headers: { Authorization: `Bearer ${admin.token}` },
  })
  ok('cmi5/export', exp.status === 200, `export -> ${exp.status}`)
  const exported = new Uint8Array(await exp.arrayBuffer())
  const reForm = new FormData()
  reForm.append('file', new Blob([exported], { type: 'application/zip' }), 'reexport.zip')
  const reimp = await fetch(new URL('/api/cmi5/courses', BASE), {
    method: 'POST',
    headers: { Authorization: `Bearer ${admin.token}` },
    body: reForm,
  })
  const reJson = await reimp.json().catch(() => null)
  const reAus = reJson?.data?.aus ?? []
  assertEq('cmi5/reimport-au-count', 1, reAus.length, 'export then re-import yields one AU')
  assertEq(
    'cmi5/reimport-au-iri',
    aus[0]?.au_iri,
    reAus[0]?.au_iri,
    'the AU IRI survives export and re-import',
  )

  // Real ADL CATAPULT manifests, imported through the full server pipeline
  // (parse → extract → persist the tree). A stub index.html stands in for the
  // real content; what is under test here is that a genuine vendor manifest —
  // multi-AU-at-root, and a pre/post-test with blocks and objectives — imports
  // and persists every AU.
  async function importReal(name, expectedAus) {
    const manifestPath = fileURLToPath(
      new URL(`../fixtures/catapult/${name}.cmi5.xml`, import.meta.url),
    )
    const manifestXml = readFileSync(manifestPath, 'utf8')
    const realZip = buildPackageZip([
      { name: 'cmi5.xml', data: manifestXml },
      { name: 'index.html', data: '<!doctype html><title>au</title>' },
    ])
    const rf = new FormData()
    rf.append('file', new Blob([realZip], { type: 'application/zip' }), `${name}.zip`)
    const res = await fetch(new URL('/api/cmi5/courses', BASE), {
      method: 'POST',
      headers: { Authorization: `Bearer ${admin.token}` },
      body: rf,
    })
    const json = await res.json().catch(() => null)
    const importedAus = json?.data?.aus ?? []
    assertEq(
      `cmi5/real-${name}`,
      expectedAus,
      importedAus.length,
      `real ADL manifest ${name} imports with ${expectedAus} AUs`,
    )
  }
  await importReal('multi_au_framed', 8)
  await importReal('pre_post_test_framed', 6)

  // A well-formed but non-conformant package (two AUs sharing an id) is refused
  // at import, before anything is persisted.
  const badManifest =
    '<courseStructure xmlns="https://w3id.org/xapi/profiles/cmi5/v1/CourseStructure.xsd">' +
    '<course id="https://e2e.invalid/bad"/>' +
    '<au id="https://e2e.invalid/bad/au" moveOn="Passed"><url>a.html</url></au>' +
    '<au id="https://e2e.invalid/bad/au" moveOn="Completed"><url>b.html</url></au>' +
    '</courseStructure>'
  const badZip = buildPackageZip([
    { name: 'cmi5.xml', data: badManifest },
    { name: 'a.html', data: 'x' },
    { name: 'b.html', data: 'y' },
  ])
  const bf = new FormData()
  bf.append('file', new Blob([badZip], { type: 'application/zip' }), 'bad.zip')
  const badRes = await fetch(new URL('/api/cmi5/courses', BASE), {
    method: 'POST',
    headers: { Authorization: `Bearer ${admin.token}` },
    body: bf,
  })
  assertEq(
    'cmi5/non-conformant-rejected',
    400,
    badRes.status,
    'a package with duplicate AU ids is refused at import',
  )

  // The Stage 4 deferral: a Newbie (default role) must be refused the launch,
  // while the promoted Member above was accepted.
  const newbie = await account('cmi5newbie')
  const nbLaunch = await POST(`/api/cmi5/aus/${auId}/launch`, { token: newbie.token })
  assertEq('cmi5/newbie-refused', 403, nbLaunch.status, 'a Newbie must be refused launch')
})
