// The tool-billing invariant: after a metered tool event, a member's observed
// balance state (ledger balance, held, available) must honor what the event
// should have produced. Two oracles at once -- the ledger balance AND the hold
// -- because the bugs split along that line: a charge posted without releasing
// the hold, or a hold released without the charge.
//
// Kept pure and separate from the driver, exactly as the stripe/groupsio
// invariants are: toolbilling-selftest.mjs feeds this what a broken settle would
// leave behind and confirms it fires. The non-negative-available rule is
// asserted here rather than assumed -- prepaid billing exists to keep it true.

function num(x) {
  return Number(x)
}

/**
 * @param {{balance?: number|string, held?: number|string, available?: number|string,
 *          nonNegative?: boolean}} model
 * @param {{balance: number|string, held: number|string, available: number|string}} observed
 * @returns {string|null} null when satisfied, else the violation.
 */
export function toolBillingHonored(model, observed) {
  for (const key of ['balance', 'held', 'available']) {
    const want = model[key]
    if (want !== undefined && Math.abs(num(observed[key]) - num(want)) > 1e-9) {
      return `${key} should be ${want} but is ${observed[key]}`
    }
  }
  if (model.nonNegative && num(observed.available) < 0) {
    return `available must be non-negative but is ${observed.available}`
  }
  return null
}

export const TOOLBILLING_INVARIANTS = [{ name: 'tool-billing-honored', fn: toolBillingHonored }]
