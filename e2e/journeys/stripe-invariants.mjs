// The membership invariant: after a billing event, a member's observed state
// (role, ledger balance, enrollment) must honor what the event should have
// produced. Two oracles live here at once -- role AND balance -- because the
// bugs this feature can have split along exactly that line: a role change with
// the wrong balance, or the right balance with the role left wrong.
//
// Kept pure and separate from the driver, exactly as groupsio-invariants.mjs is:
// stripe-selftest.mjs feeds this what a broken lifecycle would leave behind and
// confirms it fires. An invariant never seen to fire is indistinguishable from
// one that cannot, and the whole stripe stage rests on this being right.
//
// The non-negative-balance rule is asserted here rather than assumed: the whole
// point of the model is that dues are deducted only when covered, so a negative
// balance is a defect the invariant must catch on its own.

function normRole(role) {
  return String(role).trim().toLowerCase()
}

/**
 * @param {{role?: string, enrolled?: boolean, balance?: number|string,
 *          nonNegative?: boolean}} model  what the state should be.
 * @param {{role: string, enrolled: boolean, balance: number|string}} observed
 *   the state read back from the server.
 * @returns {string|null} null when satisfied, else the violation.
 */
export function membershipHonored(model, observed) {
  if (model.role !== undefined && normRole(observed.role) !== normRole(model.role)) {
    return `role should be ${model.role} but is ${observed.role}`
  }
  if (model.enrolled !== undefined && Boolean(observed.enrolled) !== Boolean(model.enrolled)) {
    return `enrolled should be ${model.enrolled} but is ${observed.enrolled}`
  }
  const bal = Number(observed.balance)
  if (Number.isNaN(bal)) {
    return `balance ${observed.balance} is not a number`
  }
  if (model.nonNegative && bal < 0) {
    return `balance must be non-negative but is ${bal}`
  }
  if (model.balance !== undefined && Math.abs(bal - Number(model.balance)) > 1e-9) {
    return `balance should be ${model.balance} but is ${bal}`
  }
  return null
}

export const STRIPE_INVARIANTS = [{ name: 'membership-honored', fn: membershipHonored }]
