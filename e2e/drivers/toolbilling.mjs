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
//   * the tool's own key is required (a wrong key and the shared global key are
//     both refused);
//   * insufficient balance and (separately) non-membership are refused;
//   * a second stop does not double-charge (idempotent);
//   * TRAINING is checked before any money moves: an untrained member is refused
//     at the training gate and NO session or charge is ever created.
//
// WHAT THIS DOES NOT PROVE: postpaid dip-and-block or the per-time charge (both
// unit-covered), nor the edge's online-sync behaviour (the edge's own tests).

import { main, ok, assertEq, GET, POST, PUT, account, adminAccount } from './lib.mjs'
import { toolBillingHonored } from '../journeys/toolbilling-invariants.mjs'

// MUST match e2e/stack-config.toml.
const TOOL_KEY = 'e2e-tool-key'
const GLOBAL_KEY = 'e2e-global-key' // [toolguard].global_api_key

function q(path, params) {
  return `${path}?${new URLSearchParams(params).toString()}`
}
const toolOn = (card, tid, key) =>
  GET(q('/api/toolguard/tool-on', { card, tool_id: tid, api_key: key }))
const toolOff = (card, tid, key) =>
  GET(q('/api/toolguard/tool-off', { card, tool_id: tid, api_key: key }))

async function createMeteredTool(admin, { externalId, flatFee, maxMin, requiresTraining }) {
  const res = await POST('/api/tools', {
    token: admin.token,
    body: {
      name: `E2E ${externalId}`,
      category: 'other',
      requires_training: !!requiresTraining,
      external_id: externalId,
      external_api_key: TOOL_KEY,
      usage_flat_fee: flatFee,
      usage_rate_per_min: null,
      usage_max_session_minutes: maxMin,
    },
  })
  ok('toolbilling/tool-created', res.status === 200 || res.status === 201, res.text.slice(0, 200))
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

  // The tool's own key is required: a wrong key and the shared global key are
  // both refused, and neither places a hold -- but by DIFFERENT layers, and the
  // two oracles below pin each one.
  //   * An unrecognized key ('not-the-key') is neither this tool's key, nor the
  //     global key, nor a device token, so entry auth (authorize_toolguard)
  //     rejects it with 401 BEFORE the metered gate is reached -- a stronger
  //     refusal than a tool_denied body, so there is no `tool_on` field to read.
  //   * The global key DOES pass entry auth (it is a valid credential) and is
  //     then refused by the per-tool metered gate (metered_key_ok) with
  //     tool_on:false. This is the check that actually pins the per-tool-key
  //     hardening: a valid-but-not-this-tool's key cannot post a charge.
  const wrong = await toolOn('E2E-CARD-1', 'e2e-flat-tool', 'not-the-key')
  ok('toolbilling/wrong-key-denied', wrong.status === 401, `${wrong.status} ${JSON.stringify(wrong.json)}`)
  const glob = await toolOn('E2E-CARD-1', 'e2e-flat-tool', GLOBAL_KEY)
  ok('toolbilling/global-key-denied', glob.json?.tool_on === false, JSON.stringify(glob.json))
  ok(
    'toolbilling/no-hold-after-denied',
    toolBillingHonored({ balance: 10, held: 0, available: 10 }, await view(member)) === null,
    JSON.stringify(await view(member)),
  )

  // Activate with the tool's own key: a hold of the max session cost (1.50).
  const on = await toolOn('E2E-CARD-1', 'e2e-flat-tool', TOOL_KEY)
  ok('toolbilling/authorized', on.json?.tool_on === true, JSON.stringify(on.json))
  ok(
    'toolbilling/hold-placed',
    toolBillingHonored({ balance: 10, held: 1.5, available: 8.5, nonNegative: true }, await view(member)) === null,
    JSON.stringify(await view(member)),
  )

  // Stop: settle -> charge 1.50, release the hold, never negative.
  const off = await toolOff('E2E-CARD-1', 'e2e-flat-tool', TOOL_KEY)
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
  const off2 = await toolOff('E2E-CARD-1', 'e2e-flat-tool', TOOL_KEY)
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
  const brokeOn = await toolOn('E2E-CARD-2', 'e2e-flat-tool', TOOL_KEY)
  ok('toolbilling/insufficient-balance-denied', brokeOn.json?.tool_on === false, JSON.stringify(brokeOn.json))

  // Membership required: a non-member with some balance is refused.
  const nonmember = await account('toolbilling_nonmember')
  await setCard(nonmember, 'E2E-CARD-3')
  await fund(admin, nonmember, '5.00') // < dues -> not enrolled, balance 5
  const nmOn = await toolOn('E2E-CARD-3', 'e2e-flat-tool', TOOL_KEY)
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
  const tOn = await toolOn('E2E-CARD-4', 'e2e-trained-tool', TOOL_KEY)
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
})
