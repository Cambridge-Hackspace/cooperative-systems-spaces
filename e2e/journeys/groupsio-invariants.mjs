// The Groups.io reconcile invariant: after a reconciliation pass, the group
// roster the fake Groups.io reports must honor the platform's intent -- the
// members the driver made intended are present, and the strangers it seeded are
// gone (the platform owns the whole list), while protected addresses survive.
//
// Kept pure and separate from the driver on purpose, exactly as
// journeys/invariants.mjs is separated from journeys.mjs: groupsio-selftest.mjs
// feeds this what a broken reconcile would leave behind and confirms it fires.
// An invariant that has never been seen to fire is indistinguishable from one
// that cannot, and the whole stack stage rests on this being right.
//
// Deliberately per-address, not full-set equality: the driver cannot enumerate
// every address the server might legitimately have on the list (a pre-verified
// admin, say), so it asserts only about the addresses it controls. WHAT THIS
// DOES NOT PROVE: that the roster contains *only* these -- only that the members
// it names are present and the strangers it names are absent. The "removes every
// stranger" claim in full is the reconcile_plan unit test's; this proves the
// mechanism fires end to end against a real Groups.io conversation.

function norm(email) {
  return String(email).trim().toLowerCase()
}

/**
 * @param {{present?: string[], absent?: string[]}} model  addresses that must
 *   be on the group after reconcile, and addresses that must not be.
 * @param {string[]} observed  the roster the fake Groups.io reports.
 * @returns {string|null} null when satisfied, else the violation.
 */
export function groupsioReconcileHonored(model, observed) {
  const seen = new Set(observed.map(norm))

  const missing = (model.present ?? []).map(norm).filter((e) => !seen.has(e))
  if (missing.length) {
    return `should be on the group and are not: ${missing.join(', ')}`
  }

  const lingering = (model.absent ?? []).map(norm).filter((e) => seen.has(e))
  if (lingering.length) {
    return `should have been removed and are still present: ${lingering.join(', ')}`
  }

  return null
}

export const GROUPSIO_INVARIANTS = [{ name: 'groupsio-reconcile-honored', fn: groupsioReconcileHonored }]
