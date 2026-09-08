// Tier: membership dues ledger, over a simulated Stripe API.
//
// The destination is css-stripe-sink, an in-repo fake started by stack.sh. This
// tier is the only place the whole membership lifecycle is proven end to end
// against a running server: the pure functions (plan_role_transition,
// advance_period, dues_due) and the signature verifier are unit-tested, but none
// of that shows a real payment moving a real member's role and balance.
//
// Every claim carries TWO oracles at once -- the ledger balance AND the role --
// through the shared, self-tested invariant (journeys/stripe-invariants.mjs),
// whose broken worlds are proven to fire in journeys/stripe-selftest.mjs. It
// exercises:
//   * a signed invoice.paid grants membership (balance 0 after the first dues,
//     role Member, enrolled);
//   * a redelivered invoice.paid posts nothing a second time (idempotency);
//   * a forged signature is refused (401);
//   * a missed renewal lapses the member -- balance stays non-negative, role
//     drops, and login still works (billing never touches is_active);
//   * a cash payment starts a FRESH membership with no back-charge;
//   * an enrolled Staff who lapses returns as a plain Member, never Staff;
//   * the last admin is never demoted, even owing dues;
//   * a paid invoice whose webhook was withheld is caught by the reconcile poll.
//
// WHAT THIS DOES NOT PROVE: the module-disabled behaviour (webhook 404) or
// cash-only operation with Stripe off -- both need a server with the module
// configured differently than this stack runs it; they are covered by the config
// guards and the contract matrix. The webhook wire format is the fake's, which
// accepts what this server's verifier expects.

import { createHmac } from 'node:crypto'

import { main, ok, assertEq, GET, POST, PUT, account, adminAccount, login } from './lib.mjs'
import { membershipHonored } from '../journeys/stripe-invariants.mjs'

const SINK = process.env.CSS_STRIPE_SINK_URL ?? 'http://127.0.0.1:4391'

// MUST match e2e/stack-config.toml's [stripe]/[membership] blocks.
const WEBHOOK_SECRET = 'e2e-stripe-webhook-secret'
const DUES = 10 // due_amount = "10.00"
const DUES_CENTS = DUES * 100
const PAST = '2000-01-01T00:00:00Z'

// --- the fake Stripe's control surface --------------------------------------
async function sinkReset() {
  await fetch(`${SINK}/_control/reset`, { method: 'POST' })
}
async function sinkPaidInvoice(customer, id, amountCents) {
  await fetch(`${SINK}/_control/paid-invoice`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ customer, id, amount_paid: amountCents, currency: 'usd' }),
  })
}

// --- signed Stripe webhooks -------------------------------------------------
function stripeSignature(raw) {
  const t = Math.floor(Date.now() / 1000)
  const v1 = createHmac('sha256', WEBHOOK_SECRET).update(`${t}.${raw}`).digest('hex')
  return `t=${t},v1=${v1}`
}
function webhook(type, object) {
  const event = { type, data: { object } }
  const raw = JSON.stringify(event)
  return POST('/api/stripe/webhook', {
    body: event,
    headers: { 'Stripe-Signature': stripeSignature(raw) },
  })
}

// --- reads / admin actions --------------------------------------------------
async function view(token) {
  const res = await GET('/api/membership', { token })
  return res.json?.data ?? {}
}
async function roleOf(id, token) {
  const res = await GET(`/api/users/${id}`, { token })
  return res.json?.data?.role
}
function reconcile(adminToken) {
  return POST('/api/admin/membership/reconcile', { token: adminToken })
}
function setNextDue(id, when, adminToken) {
  return POST(`/api/admin/membership/users/${id}/next-due`, {
    token: adminToken,
    body: { next_due_at: when },
  })
}
function cash(id, amount, adminToken) {
  return POST('/api/admin/membership/payments', {
    token: adminToken,
    body: { user_id: id, amount, entry_type: 'cash_payment' },
  })
}
function setRole(id, role, adminToken) {
  return PUT(`/api/admin/users/${id}/role`, { token: adminToken, body: { role } })
}

// Assert both oracles at once: the role and the ledger state must match `model`.
async function assertMembership(name, member, model) {
  const v = await view(member.token)
  const role = await roleOf(member.user.id, member.token)
  const observed = { role, enrolled: v.enrolled, balance: v.balance }
  const violation = membershipHonored(model, observed)
  ok(name, violation === null, violation ?? `observed ${JSON.stringify(observed)}`)
  return { v, role }
}

await main(async () => {
  const admin = await adminAccount('stripe_admin')
  await sinkReset()

  // ---- A. subscription lifecycle: signed invoice.paid grants membership -----
  const member = await account('stripe')
  const cus = `cus_${member.username}`
  const sub = `sub_${member.username}`

  const checkout = await POST('/api/stripe/checkout', {
    token: member.token,
    body: { mode: 'subscription' },
  })
  assertEq('stripe/checkout-accepted', 200, checkout.status)
  ok('stripe/checkout-returns-a-url', typeof checkout.json?.data?.url === 'string', checkout.text.slice(0, 200))

  // Stripe reports the session completed (links customer + subscription)...
  const completed = await webhook('checkout.session.completed', {
    id: `cs_${member.username}`,
    client_reference_id: member.user.id,
    customer: cus,
    subscription: sub,
    mode: 'subscription',
  })
  assertEq('stripe/checkout-completed-accepted', 200, completed.status)

  // ...then the first invoice is paid, which credits the ledger and, because the
  // balance now covers a period, starts the membership (first dues deducted).
  const paid = await webhook('invoice.paid', { id: 'in_1', customer: cus, amount_paid: DUES_CENTS })
  assertEq('stripe/invoice-paid-accepted', 200, paid.status)

  await assertMembership('stripe/paid-member-is-granted', member, {
    role: 'member',
    enrolled: true,
    balance: 0,
    nonNegative: true,
  })
  const afterPaid = await view(member.token)
  ok('stripe/member-has-a-subscription', afterPaid.has_subscription === true, JSON.stringify(afterPaid))

  // ---- B. a redelivered webhook posts nothing a second time -----------------
  const dup = await webhook('invoice.paid', { id: 'in_1', customer: cus, amount_paid: DUES_CENTS })
  assertEq('stripe/duplicate-invoice-accepted', 200, dup.status)
  await assertMembership('stripe/duplicate-invoice-is-idempotent', member, {
    role: 'member',
    enrolled: true,
    balance: 0, // not +10 (double credit) and not -10 (double dues)
    nonNegative: true,
  })

  // ---- C. a forged signature is refused before it can change anything -------
  const forged = await POST('/api/stripe/webhook', {
    body: { type: 'invoice.paid', data: { object: { id: 'in_x', customer: cus, amount_paid: 999999 } } },
    headers: { 'Stripe-Signature': 't=1,v1=deadbeef' },
  })
  assertEq('stripe/forged-webhook-refused', 401, forged.status)

  // ---- D. a missed renewal lapses the member, non-negative, login intact ----
  // Precondition before the success indicator: they ARE a member now, so the
  // later downgrade is proof the lapse acted, not proof it never took effect.
  ok('stripe/member-before-lapse', (await roleOf(member.user.id, member.token)) === 'Member')
  await setNextDue(member.user.id, PAST, admin.token)
  const lapseRun = await reconcile(admin.token)
  assertEq('stripe/reconcile-accepted', 200, lapseRun.status)
  ok('stripe/reconcile-ok', lapseRun.json?.data?.ok === true, lapseRun.text.slice(0, 200))

  await assertMembership('stripe/unpaid-member-lapses', member, {
    role: 'newbie',
    enrolled: false,
    balance: 0, // never driven negative
    nonNegative: true,
  })
  // Second oracle on "keep login": a lapsed member can still authenticate.
  const relogin = await login(member.username)
  assertEq('stripe/lapsed-member-can-still-log-in', 200, relogin.status)

  // ---- E. a cash payment starts a FRESH membership, no back-charge ----------
  const cashRes = await cash(member.user.id, '10.00', admin.token)
  assertEq('stripe/cash-accepted', 200, cashRes.status)
  ok('stripe/cash-posted', cashRes.json?.data?.posted === true, cashRes.text.slice(0, 200))
  await assertMembership('stripe/cash-restores-membership', member, {
    role: 'member',
    enrolled: true,
    balance: 0, // the gap is forgiven: not -10 owed from the lapsed period
    nonNegative: true,
  })

  // ---- F. an enrolled Staff who lapses returns as a plain Member ------------
  const promote = await setRole(member.user.id, 'Staff', admin.token)
  assertEq('stripe/promote-to-staff-accepted', 200, promote.status)
  ok('stripe/is-staff-before-lapse', (await roleOf(member.user.id, member.token)) === 'Staff')

  await setNextDue(member.user.id, PAST, admin.token)
  await reconcile(admin.token)
  await assertMembership('stripe/enrolled-staff-lapses-to-newbie', member, {
    role: 'newbie',
    enrolled: false,
    nonNegative: true,
  })

  await cash(member.user.id, '10.00', admin.token)
  await assertMembership('stripe/returning-staff-comes-back-as-member', member, {
    role: 'member', // NOT staff -- elevated roles are never auto-restored
    enrolled: true,
    nonNegative: true,
  })

  // ---- G. the last admin is never demoted, even owing dues ------------------
  // Enrol the admin (they keep Admin -- a grant never lowers an elevated role),
  // then lapse them: the guard must refuse the demotion.
  await cash(admin.user.id, '10.00', admin.token)
  ok('stripe/admin-still-admin-after-enrolling', (await roleOf(admin.user.id, admin.token)) === 'Admin')
  await setNextDue(admin.user.id, PAST, admin.token)
  await reconcile(admin.token)
  ok(
    'stripe/last-admin-is-never-demoted',
    (await roleOf(admin.user.id, admin.token)) === 'Admin',
    'the last admin was downgraded on lapse -- the guard did not fire',
  )

  // ---- H. a withheld webhook is caught by the reconcile poll ----------------
  const member2 = await account('stripe2')
  const cus2 = `cus_${member2.username}`
  // Link the customer via checkout completion, but WITHHOLD the invoice.paid
  // webhook; seed the payment only into Stripe, so only the poll can find it.
  await webhook('checkout.session.completed', {
    id: `cs_${member2.username}`,
    client_reference_id: member2.user.id,
    customer: cus2,
    subscription: `sub_${member2.username}`,
    mode: 'subscription',
  })
  await sinkPaidInvoice(cus2, 'in_withheld', DUES_CENTS)
  // Precondition: with no webhook and no reconcile yet, they are not a member.
  await assertMembership('stripe/withheld-payment-not-yet-a-member', member2, {
    enrolled: false,
  })
  const pollRun = await reconcile(admin.token)
  assertEq('stripe/poll-reconcile-accepted', 200, pollRun.status)
  await assertMembership('stripe/poll-backbone-credits-withheld-payment', member2, {
    role: 'member',
    enrolled: true,
    balance: 0,
    nonNegative: true,
  })
})
