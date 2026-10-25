// Tier: metered pay-per-use tool billing, driving the server toolguard endpoints
// directly (the edge is a thin relay; its online-sync/key-carriage changes are
// exercised in the edge's own tests). This is the only tier that proves the
// whole money path end to end against a running server: hold at activation,
// settle on stop, and the gates.
//
// Every claim carries TWO oracles -- the ledger balance AND the session/hold --
// through the shared, self-tested invariant (journeys/toolbilling-invariants.mjs).
// It uses a FLAT-fee tool so the charge is deterministic; the per-time math and
// the wall-clock cap are unit-tested (tool_billing.rs) rather than raced here.
// It proves:
//   * a metered activation places a hold (available drops by the max cost);
//   * stop settles the actual charge and releases the hold -- never negative;
//   * an unrecognized credential is refused by entry auth; a device bound to
//     the tool in ANY role may start, report on and stop a metered tool
//     (#174) -- the charge goes to the session, not to the device;
//   * a metered tool with a SEPARATE reader and plug meters end to end: the
//     reader starts and stops, the plug reports with no card, and the charge is
//     the plug's figure (#174);
//   * insufficient balance and (separately) non-membership are refused;
//   * a second stop does not double-charge (idempotent);
//   * TRAINING is checked before any money moves: an untrained member is refused
//     at the training gate and NO session or charge is ever created;
//   * the session sweep judges each session against ITS tool's cap (#171): a
//     one-minute tool is swept at a minute while a ten-minute tool and a no-cap
//     tool on the same server are not;
//   * the sweep RELEASES the tool too (#173) -- metered or not, so with no
//     session row to find -- and says so with a Warning-severity audit event;
//   * a stop needs no badge (#179): a device bound to the tool in ANY role
//     ends the session with a card-less tool-off; it settles; a late stop on
//     an idle tool is accepted; an unbound credential is still refused.
//
// WHAT THIS DOES NOT PROVE: postpaid dip-and-block or the per-time charge (both
// unit-covered), nor the edge's online-sync behaviour (the edge's own tests).

import { main, ok, assertEq, GET, POST, PUT, DELETE, account, adminAccount, boundDevice } from './lib.mjs'
import { toolBillingHonored } from '../journeys/toolbilling-invariants.mjs'

// #101 slice 6: the credentials are device tokens, not keys.
//
// `power` is the binding a billable report requires. `reader` is the control: it
// is a VALID credential that entry auth accepts, but it must not be able to post a
// charge -- the role the shared global key played before it was retired, and the
// reason the two oracles below still test two different layers.
//
// Keyed by external id, because a binding is PER TOOL and this driver creates
// several. A single shared token only ever matched the last tool created, and the
// tiers section -- which drives a different one -- was refused with "not bound to
// that tool". The scoping was right; the fixture was wrong.
const powerTokens = {}
const readerTokens = {}
let macSeq = 0

function q(path, params) {
  return `${path}?${new URLSearchParams(params).toString()}`
}
async function auditCount(admin, eventType, userId) {
  const res = await GET(q('/api/admin/audit-logs', { event_type: eventType, per_page: '100' }), {
    token: admin.token,
  })
  assertEq(`toolbilling/audit-query:${eventType}`, 200, res.status)
  return (res.json?.data ?? []).filter((r) => r.user_id === userId).length
}
const toolStatus = async (admin, id) =>
  (await GET(`/api/tools/${id}`, { token: admin.token })).json?.data?.status
const toolOn = (card, tid, token) =>
  POST('/api/toolguard/tool-on', { token, body: { card, tool_id: tid } })
const toolOff = (card, tid, token) =>
  POST('/api/toolguard/tool-off', { token, body: { card, tool_id: tid } })
const toolLog = (card, tid, token, seconds) =>
  POST('/api/toolguard/tool-log', { token, body: { card, tool_id: tid, seconds } })

async function createMeteredTool(
  admin,
  { externalId, flatFee, maxMin, requiresTraining, ratePerMin },
) {
  const res = await POST('/api/tools', {
    token: admin.token,
    body: {
      name: `E2E ${externalId}`,
      category: 'other',
      requires_training: !!requiresTraining,
      external_id: externalId,
      usage_flat_fee: flatFee,
      // Defaults to null so every existing caller is unchanged; the
      // usage-basis section needs a per-minute rate, because a flat fee is
      // the same whichever duration bills and so cannot discriminate.
      usage_rate_per_min: ratePerMin ?? null,
      usage_max_session_minutes: maxMin,
    },
  })
  ok('toolbilling/tool-created', res.status === 200 || res.status === 201, res.text.slice(0, 200))

  // Two devices per tool: the `power` one that may bill, and a `reader` one that
  // may start the tool but must not be able to.
  macSeq += 1
  const seq = macSeq.toString(16).padStart(2, '0')
  const power = await boundDevice(admin.token, res.json?.data?.id, {
    role: 'power',
    name: `billing-plug-${externalId}`,
    mac: `02:00:00:00:8a:${seq}`,
  })
  const reader = await boundDevice(admin.token, res.json?.data?.id, {
    role: 'reader',
    name: `billing-reader-${externalId}`,
    mac: `02:00:00:00:8b:${seq}`,
  })
  ok('toolbilling/power-device-bound', !!power.token && power.bindStatus === 201, power.text)
  ok('toolbilling/reader-device-bound', !!reader.token && reader.bindStatus === 201, reader.text)
  powerTokens[externalId] = power.token
  readerTokens[externalId] = reader.token

  return res.json.data
}
async function setCard(member, card) {
  const res = await PUT(`/api/profiles/${member.user.id}`, {
    token: member.token,
    body: { profile: { card_id: card } },
  })
  assertEq('toolbilling/card-set', 200, res.status)
}
async function fund(admin, member, amount) {
  const res = await POST('/api/admin/membership/payments', {
    token: admin.token,
    body: { user_id: member.user.id, amount, entry_type: 'cash_payment' },
  })
  assertEq('toolbilling/funded', 200, res.status)
}
async function view(member) {
  const res = await GET('/api/tool-billing', { token: member.token })
  return res.json?.data ?? {}
}

await main(async () => {
  const admin = await adminAccount('toolbilling_admin')

  // A flat-fee metered tool: charge = 1.50 regardless of time.
  const tool = await createMeteredTool(admin, {
    externalId: 'e2e-flat-tool',
    flatFee: '1.50',
    maxMin: 5,
  })
  ok('toolbilling/tool-is-metered', tool?.usage_flat_fee != null, JSON.stringify(tool))

  // A funded member (the cash credit also enrolls them, satisfying
  // require_membership): 20.00 - 10.00 dues = 10.00 for tools.
  const member = await account('toolbilling')
  await setCard(member, 'E2E-CARD-1')
  await fund(admin, member, '20.00')
  ok(
    'toolbilling/member-funded',
    toolBillingHonored({ balance: 10, held: 0, available: 10, nonNegative: true }, await view(member)) === null,
    JSON.stringify(await view(member)),
  )

  // ── #171/#173: the sweep judges a session against ITS tool's cap, and
  // releases the tool ───────────────────────────────────────────────────────
  //
  // Four tools, one open session each, on a server whose sweep runs every 2s
  // with no grace (stack-config). Once a minute has passed, the one-minute
  // tool's session must be settled as abandoned AND the tool returned to idle;
  // the ten-minute tool's and the no-cap tool's (default 120) must still be open
  // and in use on that same sweep. The #171 defect was one global cutoff --
  // default plus grace -- for every tool, so before that fix the first case
  // fails (the one-minute tool is judged at 120 minutes); a sweep that applied
  // the SMALLEST cap to every tool fails the other two. Each claim from two
  // sides: the session row AND the ledger.
  //
  // The fourth tool is UNMETERED with a one-minute cap. It has no session row
  // at all, so a billing sweep has nothing to find -- the #173 case: the tool
  // itself must still be released, and the release audited.
  //
  // Opened here, at the top, so the minute elapses while the rest of the driver
  // runs; asserted at the bottom.
  const sweep = { startedAt: Date.now(), cases: [] }
  for (const [label, maxMin, flatFee] of [
    ['one-minute', 1, '0.50'],
    ['ten-minute', 10, '0.50'],
    ['no-cap', null, '0.50'],
    ['unmetered', 1, null],
  ]) {
    const ext = `e2e-sweep-${label}`
    const t = await createMeteredTool(admin, { externalId: ext, flatFee, maxMin })
    const m = await account(`sweep_${label}`)
    const card = `E2E-SWEEP-${label.toUpperCase()}`
    await setCard(m, card)
    await fund(admin, m, '20.00') // member, balance 10
    const on = await toolOn(card, ext, powerTokens[ext])
    ok(`sweep/${label}-activated`, on.json?.tool_on === true, JSON.stringify(on.json))
    assertEq(`sweep/${label}-is-in-use`, 'in_use', await toolStatus(admin, t.id))
    sweep.cases.push({ label, ext, m, card, id: t.id })
  }

  // A credential that is not a credential at all is rejected by entry auth
  // (authorize_toolguard) with 401 -- a stronger refusal than a tool_denied
  // body, so there is no `tool_on` field to read. Retiring the API keys is what
  // makes a bare string this, rather than something the server would compare.
  // It places no hold.
  const wrong = await toolOn('E2E-CARD-1', 'e2e-flat-tool', 'not-a-real-token')
  ok('toolbilling/unrecognized-credential-denied', wrong.status === 401,
    `${wrong.status} ${JSON.stringify(wrong.json)}`)
  ok(
    'toolbilling/no-hold-after-denied',
    toolBillingHonored({ balance: 10, held: 0, available: 10 }, await view(member)) === null,
    JSON.stringify(await view(member)),
  )

  // #174: a `reader`-bound device MAY start a metered tool -- and stop it, and
  // its stop settles. Until #174 this arm asserted the opposite ("being
  // authorized to START a tool is not being authorized to put money on it"),
  // and that rule made a tool with a separate badge reader and plug unstartable
  // by badge: the reader held no `power` binding and the plug held no card. The
  // charge is attributed to the session -- the member and the tool -- not to
  // the device that opened it, so binding to the tool is the whole
  // authorization. A member of its own, so the main member's ledger below is
  // untouched by this.
  const readerMember = await account('toolbilling_reader')
  await setCard(readerMember, 'E2E-CARD-R')
  await fund(admin, readerMember, '20.00') // member, balance 10
  const readerOn = await toolOn('E2E-CARD-R', 'e2e-flat-tool', readerTokens['e2e-flat-tool'])
  ok('toolbilling/a-reader-bound-device-may-start-a-metered-tool', readerOn.json?.tool_on === true,
    `a device bound as reader was refused a metered tool: ${JSON.stringify(readerOn.json)}`)
  ok(
    'toolbilling/the-readers-activation-placed-the-hold',
    toolBillingHonored({ balance: 10, held: 1.5, available: 8.5, nonNegative: true }, await view(readerMember)) === null,
    JSON.stringify(await view(readerMember)),
  )
  assertEq('toolbilling/the-reader-stops', 200,
    (await toolOff('E2E-CARD-R', 'e2e-flat-tool', readerTokens['e2e-flat-tool'])).status)
  ok(
    'toolbilling/the-readers-stop-settled-not-deferred',
    toolBillingHonored({ balance: 8.5, held: 0, available: 8.5, nonNegative: true }, await view(readerMember)) === null,
    `a stop from the reader used to defer the settle to the sweep: ${JSON.stringify(await view(readerMember))}`,
  )

  // Activate with the tool's `power`-bound device: a hold of the max session cost (1.50).
  const on = await toolOn('E2E-CARD-1', 'e2e-flat-tool', powerTokens['e2e-flat-tool'])
  ok('toolbilling/authorized', on.json?.tool_on === true, JSON.stringify(on.json))
  ok(
    'toolbilling/hold-placed',
    toolBillingHonored({ balance: 10, held: 1.5, available: 8.5, nonNegative: true }, await view(member)) === null,
    JSON.stringify(await view(member)),
  )

  // Stop: settle -> charge 1.50, release the hold, never negative.
  const off = await toolOff('E2E-CARD-1', 'e2e-flat-tool', powerTokens['e2e-flat-tool'])
  assertEq('toolbilling/off-accepted', 200, off.status)
  ok(
    'toolbilling/settled-never-negative',
    toolBillingHonored({ balance: 8.5, held: 0, available: 8.5, nonNegative: true }, await view(member)) === null,
    JSON.stringify(await view(member)),
  )
  // Second oracle: the session row is settled with the charge.
  const sessions = await GET(`/api/admin/tool-billing/users/${member.user.id}/sessions`, {
    token: admin.token,
  })
  const last = sessions.json?.data?.[0]
  ok('toolbilling/session-settled', last?.status === 'settled', JSON.stringify(last))
  ok(
    'toolbilling/session-charged-the-flat-fee',
    Math.abs(Number(last?.charged_amount) - 1.5) < 1e-9,
    `charged=${last?.charged_amount}`,
  )

  // Idempotency: a second stop finds no open session -> no double charge.
  const off2 = await toolOff('E2E-CARD-1', 'e2e-flat-tool', powerTokens['e2e-flat-tool'])
  assertEq('toolbilling/second-off-accepted', 200, off2.status)
  ok(
    'toolbilling/idempotent-no-double-charge',
    toolBillingHonored({ balance: 8.5, available: 8.5 }, await view(member)) === null,
    JSON.stringify(await view(member)),
  )

  // Insufficient balance: a member funded exactly to the dues has 0 for tools.
  const broke = await account('toolbilling_broke')
  await setCard(broke, 'E2E-CARD-2')
  await fund(admin, broke, '10.00') // -> member, balance 0
  const brokeOn = await toolOn('E2E-CARD-2', 'e2e-flat-tool', powerTokens['e2e-flat-tool'])
  ok('toolbilling/insufficient-balance-denied', brokeOn.json?.tool_on === false, JSON.stringify(brokeOn.json))

  // Membership required: a non-member with some balance is refused.
  const nonmember = await account('toolbilling_nonmember')
  await setCard(nonmember, 'E2E-CARD-3')
  await fund(admin, nonmember, '5.00') // < dues -> not enrolled, balance 5
  const nmOn = await toolOn('E2E-CARD-3', 'e2e-flat-tool', powerTokens['e2e-flat-tool'])
  ok('toolbilling/membership-required-denied', nmOn.json?.tool_on === false, JSON.stringify(nmOn.json))

  // Training-before-money: a metered tool with a training step; an untrained
  // (but funded, member) user is refused at the training gate, and NO session or
  // charge is ever created -- nobody loses money over a tool they couldn't use.
  const trained = await createMeteredTool(admin, {
    externalId: 'e2e-trained-tool',
    flatFee: '1.00',
    maxMin: 5,
    requiresTraining: true,
  })
  const step = await POST('/api/training/steps', {
    token: admin.token,
    body: { tool_id: trained.id, step_number: 1, step_name: 'Safety' },
  })
  ok('toolbilling/training-step-created', step.status === 200 || step.status === 201, step.text.slice(0, 200))
  const trainee = await account('toolbilling_trainee')
  await setCard(trainee, 'E2E-CARD-4')
  await fund(admin, trainee, '20.00') // member, balance 10
  const before = await view(trainee)
  const tOn = await toolOn('E2E-CARD-4', 'e2e-trained-tool', powerTokens['e2e-trained-tool'])
  ok('toolbilling/training-denied', tOn.json?.tool_on === false, JSON.stringify(tOn.json))
  ok('toolbilling/training-denied-reason', /training/i.test(tOn.json?.message ?? ''), JSON.stringify(tOn.json))
  const after = await view(trainee)
  ok(
    'toolbilling/no-charge-before-training',
    toolBillingHonored(
      { balance: Number(before.balance), held: 0, available: Number(before.available) },
      after,
    ) === null,
    `${JSON.stringify(before)} -> ${JSON.stringify(after)}`,
  )
  const trSessions = await GET(`/api/admin/tool-billing/users/${trainee.user.id}/sessions`, {
    token: admin.token,
  })
  ok(
    'toolbilling/no-session-before-training',
    (trSessions.json?.data?.length ?? 0) === 0,
    JSON.stringify(trSessions.json?.data),
  )

  // ---- #34: per-member rate tiers ----------------------------------------
  // A flat-fee tool (default 1.00) with two tiers: Comp (0.50) and Free (0).
  // Three members -- one on each tier and one unassigned -- must each settle at
  // THEIR tier's rate, proven from two sides: the ledger charge AND the tier the
  // session captured. Flat fees keep it deterministic (per-time math is unit
  // tested).
  const tierTool = await createMeteredTool(admin, {
    externalId: 'e2e-tier-tool',
    flatFee: '1.00',
    maxMin: 5,
  })
  const tid = tierTool.id
  const ext = 'e2e-tier-tool'

  const compTier = await POST(`/api/admin/tool-tiers/tools/${tid}/tiers`, {
    token: admin.token,
    body: { name: 'Comp', flat_fee: '0.50' },
  })
  assertEq('tiers/create-comp', 201, compTier.status)
  const compId = compTier.json?.data?.id
  const freeTier = await POST(`/api/admin/tool-tiers/tools/${tid}/tiers`, {
    token: admin.token,
    body: { name: 'Free', flat_fee: '0', rate_per_min: '0' },
  })
  assertEq('tiers/create-free', 201, freeTier.status)
  const freeId = freeTier.json?.data?.id

  // Self-test: a tier that belongs to another tool cannot be assigned here.
  const otherTool = await createMeteredTool(admin, {
    externalId: 'e2e-tier-other',
    flatFee: '1.00',
    maxMin: 5,
  })
  const foreign = await PUT(`/api/admin/tool-tiers/users/${admin.user.id}/tools/${otherTool.id}`, {
    token: admin.token,
    body: { tier_id: compId },
  })
  assertEq('tiers/reject-foreign-tier', 400, foreign.status)

  const lastSession = async (m) =>
    (
      await GET(`/api/admin/tool-billing/users/${m.user.id}/sessions`, { token: admin.token })
    ).json?.data?.[0]

  // Run one full session for a member and assert the settled charge + captured
  // tier match the expectation (two oracles: the ledger charge AND the tier the
  // session locked in).
  const runTierCase = async (label, member, card, expectCharge, expectTierId) => {
    const on = await toolOn(card, ext, powerTokens[ext])
    ok(`tiers/${label}-on`, on.json?.tool_on === true, JSON.stringify(on.json))
    const off = await toolOff(card, ext, powerTokens[ext])
    assertEq(`tiers/${label}-off`, 200, off.status)
    const s = await lastSession(member)
    ok(
      `tiers/${label}-charged`,
      Math.abs(Number(s?.charged_amount) - expectCharge) < 1e-9,
      `charged=${s?.charged_amount} expected=${expectCharge}`,
    )
    assertEq(`tiers/${label}-captured-tier`, expectTierId, s?.tier_id ?? null)
  }

  const tierMembers = {}
  for (const [label, card, tierId] of [
    ['comp', 'E2E-TIER-COMP', compId],
    ['free', 'E2E-TIER-FREE', freeId],
    ['default', 'E2E-TIER-DEF', null],
  ]) {
    const m = await account(`tier_${label}`)
    tierMembers[label] = m
    await setCard(m, card)
    await fund(admin, m, '20.00') // member, balance 10
    if (tierId) {
      assertEq(
        `tiers/${label}-assigned`,
        200,
        (
          await PUT(`/api/admin/tool-tiers/users/${m.user.id}/tools/${tid}`, {
            token: admin.token,
            body: { tier_id: tierId },
          })
        ).status,
      )
    }
  }

  await runTierCase('comp', tierMembers.comp, 'E2E-TIER-COMP', 0.5, compId)
  await runTierCase('free', tierMembers.free, 'E2E-TIER-FREE', 0.0, freeId)
  await runTierCase('default', tierMembers.default, 'E2E-TIER-DEF', 1.0, null)

  // The comp member's non-default assignment is listed for the tool...
  const beforeClear = await GET(`/api/admin/tool-tiers/tools/${tid}/assignments`, {
    token: admin.token,
  })
  ok(
    'tiers/assignment-listed',
    (beforeClear.json?.data ?? []).some((a) => a.user_id === tierMembers.comp.user.id),
    JSON.stringify(beforeClear.json?.data),
  )
  // ...and clearing it reverts that member to the tool default (1.00).
  const clear = await DELETE(`/api/admin/tool-tiers/users/${tierMembers.comp.user.id}/tools/${tid}`, {
    token: admin.token,
  })
  assertEq('tiers/clear-assignment', 200, clear.status)
  await runTierCase('comp-after-clear', tierMembers.comp, 'E2E-TIER-COMP', 1.0, null)

  // #120/M2: two concurrent activations of the same idle NON-metered tool must
  // not both succeed. A non-metered tool has no open-session unique index to
  // serialize on, so this drives the try_activate_tool compare-and-set directly:
  // exactly one tool-on gets tool_on:true, the other is refused. Without the CAS
  // both pass the loaded-row status check and both write InUse -> wins == 2. A
  // few rounds since a race won once may lose the next.
  const m2Rounds = Number(process.env.CSS_RACE_ROUNDS ?? 3)
  for (let r = 0; r < m2Rounds; r++) {
    const ext = `e2e-m2-race-${r}`
    await createMeteredTool(admin, {
      externalId: ext,
      flatFee: null,
      maxMin: null,
      requiresTraining: false,
    })
    const racer = await account(`tb_m2_${r}`)
    const card = `M2-RACE-${r}`
    await setCard(racer, card)
    const [a, b] = await Promise.all([
      toolOn(card, ext, powerTokens[ext]),
      toolOn(card, ext, powerTokens[ext]),
    ])
    const wins = [a, b].filter((x) => x.json?.tool_on === true).length
    assertEq(`toolbilling/m2-one-activation-wins-r${r}`, 1, wins,
      `concurrent activations returned tool_on=${JSON.stringify([a.json?.tool_on, b.json?.tool_on])}; exactly one must win`)
    await toolOff(card, ext, powerTokens[ext])
  }

  // #120/M1: a billable usage report must come from the card that activated the
  // session; a stray card must not inflate the activator's usage. tool-OFF stays
  // open to any card (anyone can cut power for safety) -- this guards *reporting*
  // only. Without the check the stranger's report is accepted (status "ok").
  {
    const ext = 'e2e-m1-tool'
    await createMeteredTool(admin, {
      externalId: ext,
      flatFee: '1.00',
      maxMin: 60,
      requiresTraining: false,
    })
    const owner = await account('tb_m1_owner')
    await setCard(owner, 'M1-OWNER')
    await fund(admin, owner, '20.00')
    const stranger = await account('tb_m1_stranger')
    await setCard(stranger, 'M1-STRANGER')

    const on = await toolOn('M1-OWNER', ext, powerTokens[ext])
    ok('toolbilling/m1-owner-activated', on.json?.tool_on === true, JSON.stringify(on.json))

    const strangerLog = await toolLog('M1-STRANGER', ext, powerTokens[ext], 30)
    assertEq('toolbilling/m1-stranger-report-refused', 'error', strangerLog.json?.status,
      `a stranger's card reported usage onto the owner's session: ${JSON.stringify(strangerLog.json)}`)

    const ownerLog = await toolLog('M1-OWNER', ext, powerTokens[ext], 30)
    assertEq('toolbilling/m1-owner-report-accepted', 'ok', ownerLog.json?.status,
      `the activating member's own report was refused: ${JSON.stringify(ownerLog.json)}`)

    await toolOff('M1-OWNER', ext, powerTokens[ext])
  }

  // ── #174: a metered tool with a SEPARATE reader and plug ─────────────────
  //
  // The shape #83 was built for -- a laser on an unmodifiable plug with a badge
  // reader beside it -- and the one nothing exercised until now, because it
  // could not work: the reader was refused for lacking the `power` binding and
  // the plug had no card to name. Now the reader starts and stops, the plug
  // reports with NO card, and the server attributes the plug's figure to the
  // member on the session. A per-minute rate, so the charge can only come from
  // a report (a flat fee is the same whether anything was reported).
  {
    const ext = 'e2e-split-tool'
    await createMeteredTool(admin, { externalId: ext, flatFee: null, ratePerMin: '60.00', maxMin: 1 })
    const m = await account('tb_split')
    await setCard(m, 'SPLIT-CARD')
    await fund(admin, m, '200.00') // member, balance 190
    const plugLog = (seconds) =>
      POST('/api/toolguard/tool-log', { token: powerTokens[ext], body: { tool_id: ext, seconds } })
    const sessionOf = async (member) =>
      (await GET(`/api/admin/tool-billing/users/${member.user.id}/sessions`, { token: admin.token }))
        .json?.data?.[0]

    // Nobody signed on: a card-less report has nobody to go to, and is refused
    // the way a card-named report with no session is.
    const orphan = await plugLog(1.0)
    assertEq('split/a-cardless-report-with-nobody-signed-on-is-refused', 'error', orphan.json?.status,
      `usage was attributed to nobody: ${JSON.stringify(orphan.json)}`)

    const on = await toolOn('SPLIT-CARD', ext, readerTokens[ext])
    ok('split/the-reader-starts-the-tool', on.json?.tool_on === true, JSON.stringify(on.json))

    const report = await plugLog(1.0)
    assertEq('split/the-plug-reports-with-no-card', 'ok', report.json?.status,
      `the plug's card-less report was refused: ${JSON.stringify(report.json)}`)

    // Bound to the tool is the authorization: a device bound to a DIFFERENT
    // tool is refused at entry auth, card or no card. Without this the
    // card-less path would let any device on the site put time on any session.
    const foreign = await POST('/api/toolguard/tool-log', {
      token: powerTokens['e2e-flat-tool'],
      body: { tool_id: ext, seconds: 100 },
    })
    ok('split/a-device-bound-elsewhere-cannot-report-here', foreign.status === 401,
      `a device bound to another tool put time on this one: ${foreign.status} ${JSON.stringify(foreign.json)}`)

    // Wall clock above the reported second, so the cap is not what decides.
    await new Promise((r) => setTimeout(r, 2000))
    assertEq('split/the-reader-stops', 200, (await toolOff('SPLIT-CARD', ext, readerTokens[ext])).status)

    // Two oracles: the session's charge AND the ledger. Rate 60/min, so the
    // charge is exactly the plug's seconds -- 1.00 -- and not the foreign 100.
    const s = await sessionOf(m)
    ok('split/the-charge-is-the-plugs-figure',
      s?.status === 'settled' && Math.abs(Number(s?.charged_amount) - 1.0) < 1e-9,
      `expected a settled session charged 1.00 from the plug's report: ${JSON.stringify(s)}`)
    ok(
      'split/the-ledger-agrees',
      toolBillingHonored({ balance: 189, held: 0, available: 189, nonNegative: true }, await view(m)) === null,
      JSON.stringify(await view(m)),
    )
  }

  // ── #169: which reported duration bills is the TOOL's setting ────────────
  //
  // A device watching a machine's working signal reports two durations, and
  // they are not interchangeable -- a laser's enable line drops on every
  // scan-line return, so the same engrave is either continuous bench time or
  // only the intervals the beam was on. `usage_basis` picks which one the
  // charge uses.
  //
  // WHY A PER-MINUTE RATE AND A SLEEP. A flat fee is identical whichever
  // duration bills, so it cannot tell the two apart; only a time charge can.
  // And `effective = min(reported, wall_clock)` -- wall clock is a cap, never a
  // source -- so both reported figures must sit *below* the session's real
  // elapsed time or the cap, not the basis, decides the charge and both cases
  // come out equal. Hence small reported values and a sleep that puts elapsed
  // comfortably above the larger of them.
  {
    const RATE = '60.00' // => charge is exactly `effective_seconds` in currency
    const REPORTED = 1.0 // billed under `occupancy`
    const ACTIVE = 0.25 // billed under `active`

    const basisTool = async (externalId, basis) => {
      // maxMin 1 keeps the prepaid hold at rate x 1 = 60, which the funding
      // below covers. The cap is then 60 billable seconds, well above both
      // figures, so `max_minutes` is not what decides either case.
      const t = await createMeteredTool(admin, {
        externalId,
        flatFee: null,
        ratePerMin: RATE,
        maxMin: 1,
      })
      const set = await PUT(`/api/tools/${t.id}`, {
        token: admin.token,
        body: { usage_basis: basis },
      })
      // Named by tool, not by basis: two of these tools are `active`, and
      // duplicate case names would collide in the report.
      assertEq(`usagebasis/${externalId}-set`, 200, set.status, set.text.slice(0, 200))
      assertEq(`usagebasis/${externalId}-stored`, basis, set.json?.data?.usage_basis)
      return t
    }

    // The validation is checked before anything depends on it: an unknown value
    // must be refused by name rather than reaching the column's CHECK and
    // surfacing as a 500 about a constraint.
    const occupancyTool = await basisTool('e2e-basis-occ', 'occupancy')
    const bad = await PUT(`/api/tools/${occupancyTool.id}`, {
      token: admin.token,
      body: { usage_basis: 'tube-hours' },
    })
    assertEq('usagebasis/rejects-an-unknown-value', 400, bad.status, bad.text.slice(0, 200))
    ok(
      'usagebasis/refusal-names-the-alternatives',
      /occupancy/.test(bad.text) && /active/.test(bad.text),
      `a 400 that does not say what is allowed just blocks somebody: ${bad.text.slice(0, 200)}`,
    )

    // Both created for their side effects: `basisTool` asserts the setting
    // stored, and registers the tool's power token under its external id.
    await basisTool('e2e-basis-act', 'active')
    await basisTool('e2e-basis-fallback', 'active')

    const lastSessionOf = async (m) =>
      (await GET(`/api/admin/tool-billing/users/${m.user.id}/sessions`, { token: admin.token }))
        .json?.data?.[0]

    // One full session, reporting whichever figures the case calls for.
    const runBasisCase = async (label, extId, card, report) => {
      const m = await account(`basis_${label}`)
      await setCard(m, card)
      await fund(admin, m, '200.00')

      const on = await toolOn(card, extId, powerTokens[extId])
      ok(`usagebasis/${label}-on`, on.json?.tool_on === true, JSON.stringify(on.json))

      const log = await POST('/api/toolguard/tool-log', {
        token: powerTokens[extId],
        body: { card, tool_id: extId, ...report },
      })
      assertEq(`usagebasis/${label}-logged`, 'ok', log.json?.status, JSON.stringify(log.json))

      // Put wall clock above both reported figures, so the cap is not the
      // thing under test.
      await new Promise((r) => setTimeout(r, 2000))

      const off = await toolOff(card, extId, powerTokens[extId])
      assertEq(`usagebasis/${label}-off`, 200, off.status)
      return await lastSessionOf(m)
    }

    const occ = await runBasisCase('occupancy', 'e2e-basis-occ', 'E2E-BASIS-OCC', {
      seconds: REPORTED,
      active_seconds: ACTIVE,
    })
    const act = await runBasisCase('active', 'e2e-basis-act', 'E2E-BASIS-ACT', {
      seconds: REPORTED,
      active_seconds: ACTIVE,
    })

    ok(
      'usagebasis/occupancy-bills-the-reported-seconds',
      Math.abs(Number(occ?.charged_amount) - REPORTED) < 1e-9,
      `charged=${occ?.charged_amount} expected=${REPORTED}. If this is lower than ` +
        `expected the wall-clock cap bit, which means the sleep above was too ` +
        `short rather than the basis being wrong.`,
    )
    ok(
      'usagebasis/active-bills-the-working-seconds',
      Math.abs(Number(act?.charged_amount) - ACTIVE) < 1e-9,
      `charged=${act?.charged_amount} expected=${ACTIVE}`,
    )

    // THE DISCRIMINATING ASSERTION. Either case alone is satisfied by a server
    // that ignores `usage_basis` and always bills one figure; only the pair
    // proves the setting is read. Asserted as a strict inequality rather than
    // by re-deriving the numbers, so it still means something if the fixtures
    // change.
    ok(
      'usagebasis/the-basis-changes-the-charge',
      Number(act?.charged_amount) < Number(occ?.charged_amount),
      `same report, two bases, same charge: occupancy=${occ?.charged_amount} ` +
        `active=${act?.charged_amount}. The tool's usage_basis is not being read.`,
    )

    // The second oracle: the figure that did NOT bill is still recorded, so an
    // administrator can tell a dense engrave from a sparse one at equal bench
    // time. Without this, "report both" is only a claim about the wire.
    ok(
      'usagebasis/the-unbilled-figure-is-still-recorded',
      Math.abs(Number(occ?.reported_active_seconds) - ACTIVE) < 1e-6,
      `the occupancy-billed session did not keep active_seconds: ` +
        `reported_active_seconds=${occ?.reported_active_seconds} expected=${ACTIVE}`,
    )

    // A tool set to `active` whose device senses nothing must bill what it does
    // report, not nothing. Absence is the fallback trigger -- a genuine zero is
    // an answer and is billed as zero.
    const fb = await runBasisCase('fallback', 'e2e-basis-fallback', 'E2E-BASIS-FB', {
      seconds: REPORTED,
    })
    ok(
      'usagebasis/active-falls-back-when-no-working-signal-is-reported',
      Math.abs(Number(fb?.charged_amount) - REPORTED) < 1e-9,
      `charged=${fb?.charged_amount} expected=${REPORTED}. A tool configured for ` +
        `the working figure and wired to a device that cannot sense one must not ` +
        `bill zero.`,
    )
    ok(
      'usagebasis/the-fallback-leaves-the-active-figure-unset',
      fb?.reported_active_seconds === null || fb?.reported_active_seconds === undefined,
      `expected no active figure at all, got ${fb?.reported_active_seconds}. A zero ` +
        `here would read as "the machine never ran" rather than "nothing measured it".`,
    )
  }

  // ── #179: a stop needs no badge ──────────────────────────────────────────
  //
  // A stop is a safety act. Any device bound to the tool may send a tool-off
  // with no card -- a plug with a stop button, a sensor, an automation -- and
  // the server ends whatever session is open, settling it against the member
  // who opened it. A member of its own, so nothing above is coupled to it.
  {
    const ext = 'e2e-flat-tool'
    const m = await account('tb_stop')
    await setCard(m, 'STOP-CARD')
    await fund(admin, m, '20.00') // member, balance 10
    const on = await toolOn('STOP-CARD', ext, powerTokens[ext])
    ok('stop/session-open', on.json?.tool_on === true, JSON.stringify(on.json))
    ok(
      'stop/hold-placed-before-the-stop',
      toolBillingHonored({ balance: 10, held: 1.5, available: 8.5 }, await view(m)) === null,
      JSON.stringify(await view(m)),
    )
    // The READER device: bound to this tool, not the one that activated, and
    // presenting no card at all.
    const cardless = await POST('/api/toolguard/tool-off', {
      token: readerTokens[ext],
      body: { tool_id: ext },
    })
    ok(
      'stop/a-cardless-stop-from-a-bound-device-is-accepted',
      cardless.status === 200 && cardless.json?.tool_off === true,
      `a stop with no card was refused: ${cardless.status} ${JSON.stringify(cardless.json)}`,
    )
    const toolRow = (await GET(`/api/tools/${tool.id}`, { token: admin.token })).json?.data
    assertEq('stop/the-tool-is-idle-again', 'idle', toolRow?.status)
    // Two oracles: the session row settled against the member who opened it,
    // AND the ledger -- fee charged, hold released -- with no card named.
    const s = (await GET(`/api/admin/tool-billing/users/${m.user.id}/sessions`, { token: admin.token }))
      .json?.data?.[0]
    ok(
      'stop/the-session-settled-against-the-member-who-opened-it',
      s?.status === 'settled' && Math.abs(Number(s?.charged_amount) - 1.5) < 1e-9,
      `expected the opener's session settled at 1.50: ${JSON.stringify(s)}`,
    )
    ok(
      'stop/the-ledger-agrees',
      toolBillingHonored({ balance: 8.5, held: 0, available: 8.5, nonNegative: true }, await view(m)) === null,
      JSON.stringify(await view(m)),
    )
    // A stop that lands late -- the tool is already idle -- is harmless, not an
    // error: a device that cannot know whether it was first must still be able
    // to send one.
    const late = await POST('/api/toolguard/tool-off', { token: powerTokens[ext], body: { tool_id: ext } })
    ok(
      'stop/a-late-cardless-stop-on-an-idle-tool-is-harmless',
      late.status === 200 && late.json?.tool_off === true,
      `${late.status} ${JSON.stringify(late.json)}`,
    )
    ok(
      'stop/a-late-stop-charges-nothing',
      toolBillingHonored({ balance: 8.5, held: 0, available: 8.5 }, await view(m)) === null,
      JSON.stringify(await view(m)),
    )
    // Bound is still the authorization: no credential, no stop.
    const nobody = await POST('/api/toolguard/tool-off', { token: 'not-a-real-token', body: { tool_id: ext } })
    assertEq('stop/an-unbound-credential-cannot-stop-anything', 401, nobody.status)
  }

  // ── #171, the other end: what the sweep did while the above ran ──────────
  {
    // A minute past the one-minute cap, plus a few sweeps to notice it.
    const remaining = sweep.startedAt + 75_000 - Date.now()
    if (remaining > 0) await new Promise((r) => setTimeout(r, remaining))

    const sessionOf = async (m) =>
      (await GET(`/api/admin/tool-billing/users/${m.user.id}/sessions`, { token: admin.token }))
        .json?.data?.[0]
    const [one, ten, none, unmetered] = sweep.cases
    const oneSession = await sessionOf(one.m)
    ok(
      'sweep/a-session-past-its-own-cap-is-settled-abandoned',
      oneSession?.status === 'abandoned',
      `the one-minute tool's session is ${oneSession?.status} after ` +
        `${Math.round((Date.now() - sweep.startedAt) / 1000)}s: the sweep is not ` +
        `reading the tool's own cap. ${JSON.stringify(oneSession)}`,
    )
    // Second oracle: the ledger -- the hold is gone and the flat fee charged.
    ok(
      'sweep/the-abandoned-sessions-hold-is-released-and-its-fee-charged',
      toolBillingHonored({ balance: 9.5, held: 0, available: 9.5, nonNegative: true }, await view(one.m)) === null,
      JSON.stringify(await view(one.m)),
    )

    // THE DISCRIMINATING PAIR. A sweep that applied the one-minute cap to every
    // tool passes the case above and fails these; the sweep as it was passes
    // these and fails the case above. Only all three prove the cap is per tool.
    const tenSession = await sessionOf(ten.m)
    const noneSession = await sessionOf(none.m)
    ok(
      'sweep/a-ten-minute-tool-is-not-swept-at-one-minute',
      tenSession?.status === 'open',
      `a member mid-job on a ten-minute tool was cut off: ${JSON.stringify(tenSession)}`,
    )
    ok(
      'sweep/a-tool-with-no-cap-of-its-own-keeps-the-default',
      noneSession?.status === 'open',
      `a tool that sets no cap must be judged at the global default, not at ` +
        `another tool's: ${JSON.stringify(noneSession)}`,
    )
    for (const c of [ten, none]) {
      ok(
        `sweep/the-${c.label}-hold-is-still-held`,
        toolBillingHonored({ balance: 10, held: 0.5, available: 9.5 }, await view(c.m)) === null,
        JSON.stringify(await view(c.m)),
      )
    }

    // ── #173: the tool comes back too, not only the money ──────────────────
    // Settling the session and freeing the tool are different writes to
    // different tables, and until #173 only the first happened: the money was
    // reconciled and the tool stayed `in_use` for everyone.
    assertEq(
      'sweep/a-released-tool-is-idle-again',
      'idle',
      await toolStatus(admin, one.id),
      'the session was settled but the tool is still in use: the sweep reconciled the money and stranded the machine',
    )
    // THE DISCRIMINATOR between "have the billing sweep clear the status" and
    // "a sweep independent of billing": an unmetered tool has no session row,
    // so a status change bolted onto the billing settle never runs for it.
    assertEq(
      'sweep/an-unmetered-tool-with-no-session-row-is-released-too',
      'idle',
      await toolStatus(admin, unmetered.id),
      'an unmetered tool stayed in use: the release is riding on the billing session, which this tool never had',
    )
    ok(
      'sweep/an-unmetered-release-moves-no-money',
      toolBillingHonored({ balance: 10, held: 0, available: 10 }, await view(unmetered.m)) === null,
      JSON.stringify(await view(unmetered.m)),
    )
    // The negatives, with the same clock: tools inside their cap keep their
    // session. A sweep that freed tools eagerly cuts a member off mid-job,
    // which is worse than the defect.
    for (const c of [ten, none]) {
      assertEq(`sweep/the-${c.label}-tool-is-still-in-use`, 'in_use', await toolStatus(admin, c.id))
    }
    // Loud: a release is a Warning-severity audit event against the member
    // whose session it was, and only against them. The event is written in the
    // same pass as the status change, so it is there by the time the status is.
    const released = await auditCount(admin, 'tool_released_by_sweep', unmetered.m.user.id)
    ok(
      'sweep/the-release-is-audited-against-the-member',
      released === 1,
      `expected exactly one tool_released_by_sweep for the unmetered member, got ${released}`,
    )
    assertEq(
      'sweep/a-tool-still-in-use-raised-no-release-event',
      0,
      await auditCount(admin, 'tool_released_by_sweep', ten.m.user.id),
    )
    // And a released tool is usable again: the next member is not told it is
    // already in use.
    const again = await toolOn(unmetered.card, unmetered.ext, powerTokens[unmetered.ext])
    ok('sweep/a-released-tool-can-be-started-again', again.json?.tool_on === true, JSON.stringify(again.json))
    assertEq('sweep/unmetered-stopped', 200, (await toolOff(unmetered.card, unmetered.ext, powerTokens[unmetered.ext])).status)

    for (const c of [ten, none]) {
      // Tidy: a clean stop, so nothing is left open for a later stage to settle.
      assertEq(`sweep/${c.label}-stopped`, 200, (await toolOff(c.card, c.ext, powerTokens[c.ext])).status)
    }
  }
})
