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
    role: 'active',
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
    role: 'active',
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
  ok('stripe/member-before-lapse', (await roleOf(member.user.id, member.token)) === 'active')
  await setNextDue(member.user.id, PAST, admin.token)
  const lapseRun = await reconcile(admin.token)
  assertEq('stripe/reconcile-accepted', 200, lapseRun.status)
  ok('stripe/reconcile-ok', lapseRun.json?.data?.ok === true, lapseRun.text.slice(0, 200))

  await assertMembership('stripe/unpaid-member-lapses', member, {
    role: 'historical',
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
    role: 'active',
    enrolled: true,
    balance: 0, // the gap is forgiven: not -10 owed from the lapsed period
    nonNegative: true,
  })

  // ---- F. an enrolled Staff who lapses returns as a plain Member ------------
  const promote = await setRole(member.user.id, 'staff', admin.token)
  assertEq('stripe/promote-to-staff-accepted', 200, promote.status)
  ok('stripe/is-staff-before-lapse', (await roleOf(member.user.id, member.token)) === 'staff')

  await setNextDue(member.user.id, PAST, admin.token)
  await reconcile(admin.token)
  await assertMembership('stripe/enrolled-staff-lapses-to-historical', member, {
    role: 'historical',
    enrolled: false,
    nonNegative: true,
  })

  await cash(member.user.id, '10.00', admin.token)
  await assertMembership('stripe/returning-staff-comes-back-as-member', member, {
    role: 'active', // NOT staff -- elevated roles are never auto-restored
    enrolled: true,
    nonNegative: true,
  })

  // ---- G. the last admin is never demoted, even owing dues ------------------
  // Enrol the admin (they keep Admin -- a grant never lowers an elevated role),
  // then lapse them: the guard must refuse the demotion.
  await cash(admin.user.id, '10.00', admin.token)
  ok('stripe/admin-still-admin-after-enrolling', (await roleOf(admin.user.id, admin.token)) === 'admin')
  await setNextDue(admin.user.id, PAST, admin.token)
  await reconcile(admin.token)
  ok(
    'stripe/last-admin-is-never-demoted',
    (await roleOf(admin.user.id, admin.token)) === 'admin',
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
    role: 'active',
    enrolled: true,
    balance: 0,
    nonNegative: true,
  })

  // ---- I. one member, two Stripe customers (#118) ------------------------------
  // A member who changed the email they gave Stripe, or who was merged out of a
  // ToolPass record and a Stripe-only record, is known to Stripe by two
  // customer ids. Money on EITHER must reach the ledger, and the member is "on
  // a subscription" while ANY of them carries one. Before #118 the second
  // checkout overwrote the first customer id and a payment on the old one found
  // no account -- the webhook answered 200 (handled: false), which stops Stripe
  // retrying, so the payment was simply never recorded.
  const member3 = await account('stripe3')
  const cusA = `cus_${member3.username}_a`
  const cusB = `cus_${member3.username}_b`
  for (const [cus, sub] of [[cusA, `sub_${member3.username}_a`], [cusB, `sub_${member3.username}_b`]]) {
    const done = await webhook('checkout.session.completed', {
      id: `cs_${cus}`,
      client_reference_id: member3.user.id,
      customer: cus,
      subscription: sub,
      mode: 'subscription',
    })
    assertEq(`stripe/second-customer-links/${cus}`, 200, done.status)
  }
  const ledgerBefore = await GET(`/api/admin/membership/users/${member3.user.id}/ledger`, { token: admin.token })
  const stripeEntries = (r) => (r.json?.data ?? []).filter((e) => e.external_reference).map((e) => e.external_reference)
  assertEq('stripe/two-customers-no-money-yet', 0, stripeEntries(ledgerBefore).length, JSON.stringify(ledgerBefore.json?.data))

  // Money on the FIRST customer, after the second was linked: this is the
  // payment that used to vanish.
  const paidOnA = await webhook('invoice.paid', { id: 'in_a', customer: cusA, amount_paid: DUES_CENTS })
  assertEq('stripe/payment-on-first-customer-accepted', 200, paidOnA.status)
  ok('stripe/payment-on-first-customer-handled', paidOnA.json?.data?.handled === true, paidOnA.text.slice(0, 200))
  const paidOnB = await webhook('invoice.paid', { id: 'in_b', customer: cusB, amount_paid: DUES_CENTS })
  ok('stripe/payment-on-second-customer-handled', paidOnB.json?.data?.handled === true, paidOnB.text.slice(0, 200))
  const ledgerAfter = await GET(`/api/admin/membership/users/${member3.user.id}/ledger`, { token: admin.token })
  const refs = stripeEntries(ledgerAfter)
  ok(
    'stripe/ledger-records-both-customers',
    refs.includes('in_a') && refs.includes('in_b'),
    `ledger references: ${JSON.stringify(refs)}`
  )
  await assertMembership('stripe/two-customers-member-is-paid-up', member3, {
    role: 'active',
    enrolled: true,
    // Two periods paid, one deducted on enrolment: one period in credit.
    balance: DUES,
    nonNegative: true,
  })

  // Only ONE subscription need be live. Cancel A: still subscribed through B.
  // Cancel B too: no longer subscribed. Asserted from both sides so a
  // has_subscription that ignored the table (always true, or always the
  // first row) fails one of the two.
  const viewBoth = await view(member3.token)
  ok('stripe/two-customers-has-subscription', viewBoth.has_subscription === true, JSON.stringify(viewBoth))
  // #150: the customers are visible, to the member and to an admin, with the
  // one checkout would use marked -- and a plain other member cannot see them.
  const customersOf = async (token) => (await GET(`/api/users/${member3.user.id}/stripe-customers`, { token })).json?.data ?? []
  const asAdmin = await GET(`/api/users/${member3.user.id}/stripe-customers`, { token: admin.token })
  assertEq('stripe/customers-visible-to-admin', 200, asAdmin.status, asAdmin.text.slice(0, 200))
  assertEq('stripe/customers-lists-both', 2, (asAdmin.json?.data ?? []).length, JSON.stringify(asAdmin.json?.data))
  const asSelf = await GET(`/api/users/${member3.user.id}/stripe-customers`, { token: member3.token })
  assertEq('stripe/customers-visible-to-self', 200, asSelf.status)
  const asOther = await GET(`/api/users/${member3.user.id}/stripe-customers`, { token: member2.token })
  assertEq('stripe/customers-hidden-from-another-member', 403, asOther.status)
  ok('stripe/exactly-one-customer-is-current', (await customersOf(admin.token)).filter((c) => c.current).length === 1, JSON.stringify(await customersOf(admin.token)))
  const cancelA = await webhook('customer.subscription.deleted', { id: `sub_${member3.username}_a`, customer: cusA })
  assertEq('stripe/cancel-first-accepted', 200, cancelA.status)
  const viewOne = await view(member3.token)
  ok('stripe/one-live-subscription-still-counts', viewOne.has_subscription === true, JSON.stringify(viewOne))
  // After A's cancellation, B (the one still subscribed) is current.
  const afterCancelA = await customersOf(admin.token)
  assertEq('stripe/current-follows-the-live-subscription', cusB, afterCancelA.find((c) => c.current)?.customer_id, JSON.stringify(afterCancelA))
  const cancelB = await webhook('customer.subscription.deleted', { id: `sub_${member3.username}_b`, customer: cusB })
  assertEq('stripe/cancel-second-accepted', 200, cancelB.status)
  const viewNone = await view(member3.token)
  ok('stripe/no-live-subscription-after-both-cancel', viewNone.has_subscription === false, JSON.stringify(viewNone))

  // A customer id already linked to someone else is never silently re-homed.
  const intruder = await account('stripe4')
  const steal = await webhook('checkout.session.completed', {
    id: `cs_steal_${intruder.username}`,
    client_reference_id: intruder.user.id,
    customer: cusA,
    subscription: `sub_steal`,
    mode: 'subscription',
  })
  ok('stripe/foreign-customer-is-not-rehomed', steal.status !== 200 || steal.json?.data?.handled !== true, `steal -> ${steal.status} ${steal.text.slice(0, 200)}`)
  const stillA = await webhook('invoice.paid', { id: 'in_a2', customer: cusA, amount_paid: DUES_CENTS })
  const ledgerFinal = await GET(`/api/admin/membership/users/${member3.user.id}/ledger`, { token: admin.token })
  ok('stripe/foreign-customer-money-stays-with-its-owner', stripeEntries(ledgerFinal).includes('in_a2'), `after steal attempt, in_a2 -> handled=${stillA.json?.data?.handled}; refs ${JSON.stringify(stripeEntries(ledgerFinal))}`)
})
